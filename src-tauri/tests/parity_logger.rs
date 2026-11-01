//! Differential test: `src/main/logger.js` vs `snowify::logger`: message stringification of
//! every kind of JS argument, timestamps across the whole Date range, levels from
//! `app:appendLog`, and the 500-entry cap.

use serde_json::{Map, Value, json};
use snowify::{
    js,
    logger::{LogArg, LogBuffer},
};

mod common;
use common::{Diff, Rng};

/// What `JSON.stringify` sees for an encoded runner argument: `Ok(None)` for values it maps
/// to `undefined`, `Err` where it throws.
fn json_equiv(v: &Value) -> Result<Option<Value>, ()> {
    Ok(Some(match v {
        Value::Object(o) if o.contains_key("$") => match o["$"].as_str().unwrap() {
            "undefined" | "fn" | "symbol" => return Ok(None),
            "nan" | "inf" => Value::Null,
            _ => json!(0),
        },
        Value::Object(o) if o.contains_key("$bigint") || o.contains_key("$circular") => {
            return Err(());
        }
        Value::Object(o) if o.contains_key("$date") => {
            js::iso_string(o["$date"].as_i64().unwrap()).into()
        }
        Value::Object(o) if o.contains_key("$error") => json!({}),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|i| json_equiv(i).map(|j| j.unwrap_or(Value::Null)))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(o) => {
            let mut m = Map::new();
            for (k, x) in o {
                if let Some(j) = json_equiv(x)? {
                    m.insert(k.clone(), j);
                }
            }
            Value::Object(m)
        }
        other => other.clone(),
    }))
}

/// JS `String(v)` for the encoded values the corpus puts inside a throwing argument.
fn js_string(v: &Value) -> String {
    match v {
        Value::Object(o) if o.contains_key("$bigint") => o["$bigint"].as_str().unwrap().into(),
        Value::Array(items) => items
            .iter()
            .map(|i| {
                if i.is_null() {
                    String::new()
                } else {
                    js_string(i)
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        other => js::to_string(other),
    }
}

fn arg(v: &Value) -> LogArg {
    match (v, json_equiv(v)) {
        (Value::String(s), _) => LogArg::Str(s.clone()),
        (_, Ok(None)) => LogArg::Undefined,
        (_, Ok(Some(j))) => LogArg::Json(j),
        (_, Err(())) => LogArg::Unserializable(js_string(v)),
    }
}

fn random_arg(rng: &mut Rng) -> Value {
    match rng.below(18) {
        0 => json!("plain text"),
        1 => json!(""),
        2 => json!("日本 🎵 \n\t"),
        3 => json!(rng.next() as i64),
        4 => json!(0.1),
        5 => json!(null),
        6 => json!(true),
        7 => json!({ "$": *rng.pick(&["undefined", "fn", "symbol", "nan", "inf", "negzero"]) }),
        8 => json!({ "$bigint": "12345678901234567890" }),
        9 => json!({ "$date": rng.below(1 << 40) as i64 }),
        10 => json!({ "$error": "boom" }),
        11 => json!({ "$circular": 1 }),
        12 => {
            json!({ "b": 1, "a": [1, { "$": "undefined" }, { "$": "fn" }], "2": "x", "1": { "$": "nan" }, "u": { "$": "undefined" } })
        }
        13 => json!([1, { "$bigint": "10" }, null, "s"]),
        14 => json!({ "k": { "$bigint": "1" } }),
        15 => json!(["a", ["b", null], { "$date": 0 }]),
        16 => json!({ "quote": "\"\\", "ctl": "\u{1}\u{7f}" }),
        _ => json!(1e21),
    }
}

#[test]
fn logger_matches_source() {
    let mut rng = Rng(0x1029_3847_5665_7483);
    let max = 8_640_000_000_000_000u64;
    let mut ops = Vec::new();
    for i in 0..1500 {
        let now = match i % 7 {
            0 => (rng.next() % (2 * max + 1)) as i64 - max as i64,
            1 => 253_402_300_800_000 + rng.below(1 << 30) as i64,
            2 => -1 - rng.below(1 << 40) as i64,
            _ => 1_700_000_000_000 + rng.below(1 << 30) as i64,
        };
        let args: Vec<Value> = (0..rng.below(4)).map(|_| random_arg(&mut rng)).collect();
        if rng.below(4) == 0 {
            let level = rng
                .pick(&[
                    json!("log"),
                    json!("warn"),
                    json!("info"),
                    json!(3),
                    json!(true),
                    json!({ "x": 1 }),
                ])
                .clone();
            ops.push(json!(["capture", level, args, now]));
        } else {
            ops.push(json!([
                "console",
                rng.pick(&["log", "warn", "error"]),
                args,
                now
            ]));
        }
    }
    let out = common::run_source("logger.js", &Value::Array(ops.clone()));

    let entry = |e: &snowify::logger::LogEntry| json!({ "ts": e.ts, "level": e.level.encode(), "msg": e.msg });
    // The source's `level` is plain JSON; ours is a JsValue, compared in wire form.
    let norm = |e: &Value| {
        let mut e = e.clone();
        e["level"] = snowify::ipc::JsValue::from(&e["level"]).encode();
        e
    };
    let mut buf = LogBuffer::new();
    let mut d = Diff::default();
    for (op, s) in ops.iter().zip(out["each"].as_array().unwrap()) {
        let args: Vec<LogArg> = op[2].as_array().unwrap().iter().map(arg).collect();
        buf.capture((&op[1]).into(), &args, op[3].as_i64().unwrap());
        d.check(op, &norm(s), &entry(buf.entries().last().unwrap()));
    }
    let rust: Vec<Value> = buf.entries().iter().map(entry).collect();
    let source = out["final"].as_array().unwrap();
    d.check("buffer length", &json!(source.len()), &json!(rust.len()));
    for (i, (s, r)) in source.iter().zip(&rust).enumerate() {
        d.check(format_args!("entry {i}"), &norm(s), r);
    }
    d.finish("logger");
}
