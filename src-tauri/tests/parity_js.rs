//! Differential test: `snowify::js` vs V8 (Electron 33) for number formatting, `String(v)`,
//! `JSON.stringify` and `Date#toISOString`.

use serde_json::{Value, json};
use snowify::js;

mod common;
use common::{Diff, Rng};

fn random_value(rng: &mut Rng, depth: u32) -> String {
    const KEYS: [&str; 14] = [
        "a",
        "b",
        "0",
        "1",
        "10",
        "2",
        "01",
        "-1",
        "4294967294",
        "4294967295",
        "1.5",
        "",
        "日本",
        "\\u0000",
    ];
    const STRS: [&str; 8] = [
        "",
        "x",
        "\\\"q\\\"",
        "\\n\\t\\u0001\\u001f\\u007f",
        "🎵",
        "\\u2028",
        "a b",
        "\\\\",
    ];
    let leaf = |rng: &mut Rng| -> String {
        match rng.below(7) {
            0 => "null".into(),
            1 => "true".into(),
            2 => "false".into(),
            3 => rng
                .pick(&[
                    "0",
                    "-0",
                    "1e21",
                    "1e-7",
                    "123.456",
                    "-5",
                    "12345678901234567890",
                    "5e-324",
                    "0.1",
                ])
                .to_string(),
            4 => format!("{}", rng.next() as i64),
            5 => format!("{:e}", f64::from_bits(rng.next() & 0x7fef_ffff_ffff_ffff)),
            _ => format!("\"{}\"", rng.pick(&STRS)),
        }
    };
    if depth == 0 || rng.below(3) == 0 {
        return leaf(rng);
    }
    let n = rng.below(5);
    if rng.below(2) == 0 {
        let items: Vec<String> = (0..n).map(|_| random_value(rng, depth - 1)).collect();
        format!("[{}]", items.join(","))
    } else {
        let items: Vec<String> = (0..n)
            .map(|_| format!("\"{}\":{}", rng.pick(&KEYS), random_value(rng, depth - 1)))
            .collect();
        format!("{{{}}}", items.join(","))
    }
}

#[test]
fn js_semantics_match_v8() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);

    let mut nums: Vec<f64> = vec![
        0.0,
        -0.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        1e21,
        1e20,
        1.2345678901234568e20,
        999999999999999900000.0,
        1e-7,
        1e-6,
        1.5e-7,
        123e-20,
        0.1 + 0.2,
        5e-324,
        f64::MAX,
        f64::MIN_POSITIVE,
        100.0,
        1.0,
        -1.5,
        2f64.powi(53),
        2f64.powi(53) + 2.0,
    ];
    for _ in 0..4000 {
        nums.push(f64::from_bits(rng.next()));
    }
    // 17-significant-digit decimals: where shortest-form ties between candidates show up.
    for _ in 0..20000 {
        let text = format!(
            "{}.{:016}e{}",
            1 + rng.below(9),
            rng.next() % 10_000_000_000_000_000,
            rng.below(40) as i32 - 20
        );
        nums.push(text.parse().unwrap());
    }
    for _ in 0..2000 {
        let digits = (rng.next() % 10_000_000_000) as f64;
        nums.push(digits * 10f64.powi(rng.below(50) as i32 - 25));
    }

    let mut values: Vec<String> = [
        "{}",
        "[]",
        "[null,[null,[]],{}]",
        "{\"a\":1,\"a\":2,\"0\":3}",
        "\"s\"",
        "{\"__proto__\":1}",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for _ in 0..3000 {
        values.push(random_value(&mut rng, 3));
    }

    let max = 8_640_000_000_000_000i64;
    let mut dates: Vec<i64> = vec![
        0,
        -1,
        1,
        253_402_300_799_999,
        253_402_300_800_000,
        -62_167_219_200_000,
        -62_167_219_200_001,
        max,
        -max,
        951_782_400_000,
        1_709_164_800_000,
        4_107_542_400_000,
    ];
    for _ in 0..3000 {
        dates.push((rng.next() % (2 * max as u64 + 1)) as i64 - max);
    }

    // JSON.parse fuzz: token soup and single-character mutations of valid documents.
    const FRAGS: [&str; 34] = [
        "{", "}", "[", "]", ":", ",", "\"", "\"k\"", "\"v a\"", "1", "-", "0", "01", "1.5", ".",
        "e", "E+", "-0", "true", "tru", "false", "null", "nul", " ", "\n", "\r", "\r\n", "\t", "x",
        "\\", "\\u00e9", "\\ud83c", "日", "🎵",
    ];
    const VALID: [&str; 6] = [
        "{\"a\": [1, 2.5e3, -0, true, null], \"b\": {\"c\": \"d\\n\"}}",
        "[\"long string value here\", {\"nested\": {\"deeper\": [false]}}, 12345678901234567890]",
        "{\"2\": \"b\", \"1\": \"a\", \"z\": 0, \"1\": \"again\", \"__proto__\": 1}",
        "\"\\ud800 lone and \\udc00 and \\u0041\"",
        "<!DOCTYPE html><html><head><title>404 Not Found</title></head></html>",
        "{\"id\": \"plugin\", \"renderer\": \"main.js\",\r\n  \"version\": \"1.0.0\"}",
    ];
    let mut parse: Vec<Vec<u16>> = Vec::new();
    for _ in 0..6000 {
        let n = 1 + rng.below(12);
        let s: String = (0..n).map(|_| *rng.pick(&FRAGS)).collect();
        parse.push(s.encode_utf16().collect());
    }
    for _ in 0..6000 {
        let mut u: Vec<u16> = rng.pick(&VALID).encode_utf16().collect();
        for _ in 0..1 + rng.below(2) {
            let at = rng.below(u.len() + 1);
            match rng.below(3) {
                0 if at < u.len() => {
                    u.remove(at);
                }
                1 => u.insert(
                    at,
                    *rng.pick(&[
                        0x22u16, 0x2C, 0x7D, 0x5D, 0x3A, 0x20, 0x31, 0x5C, 0xD83C, 0x0A, 0x00,
                    ]),
                ),
                _ if at < u.len() => u[at] = *rng.pick(&[0x78u16, 0x2E, 0x2D, 0x7B, 0x5B, 0x0D]),
                _ => {}
            }
        }
        parse.push(u);
    }
    for t in [
        "",
        " ",
        "NaN",
        "Infinity",
        "undefined",
        "[object Object]",
        "\"\\",
        "\"\\u12\"",
        "0x1",
    ] {
        parse.push(t.encode_utf16().collect());
    }

    // util.inspect(string): quote choice, escapes, lone surrogates, the newline split and cap.
    const UNITS: [u16; 22] = [
        0x61, 0x20, 0x27, 0x22, 0x60, 0x24, 0x7B, 0x5C, 0x0A, 0x0D, 0x09, 0x00, 0x1F, 0x7F, 0x85, 0x9F, 0xA0, 0xE9,
        0xD83C, 0xDFB5, 0x0B, 0x2028,
    ];
    let mut inspect: Vec<Vec<u16>> = vec![vec![], "'\"`".encode_utf16().collect(), "'\"${".encode_utf16().collect()];
    for _ in 0..4000 {
        let n = *rng.pick(&[0usize, 1, 3, 8, 16, 17, 60, 76, 77, 100, 101, 124, 125, 200, 5000, 10_000, 10_001, 10_002, 12_000]);
        let dense = rng.below(4) == 0;
        inspect.push(
            (0..n)
                .map(|_| if dense || rng.below(20) == 0 { *rng.pick(&UNITS) } else { 0x61 + rng.below(3) as u16 })
                .collect(),
        );
    }

    let input = json!({
        "inspect": inspect,
        "parse": parse,
        "nums": nums.iter().map(|n| format!("{:016x}", n.to_bits())).collect::<Vec<_>>(),
        "values": values,
        "dates": dates,
    });
    let out = common::run_source("js.js", &input);

    let mut d = Diff::default();
    for (n, s) in nums.iter().zip(out["nums"].as_array().unwrap()) {
        d.check(
            format_args!("String({n:e})"),
            s,
            &Value::from(js::number_to_string(*n)),
        );
    }
    for ((text, s), j) in values
        .iter()
        .zip(out["strings"].as_array().unwrap())
        .zip(out["json"].as_array().unwrap())
    {
        let v: Value = serde_json::from_str(text).unwrap();
        d.check(
            format_args!("String({text})"),
            s,
            &Value::from(js::to_string(&v)),
        );
        d.check(
            format_args!("JSON.stringify({text})"),
            j,
            &Value::from(js::json_stringify(&v)),
        );
    }
    for (ms, s) in dates.iter().zip(out["dates"].as_array().unwrap()) {
        d.check(
            format_args!("toISOString({ms})"),
            s,
            &Value::from(js::iso_string(*ms)),
        );
    }
    for (units, s) in parse.iter().zip(out["parse"].as_array().unwrap()) {
        let r = match js::json_parse(units) {
            Ok(v) => json!({ "ok": v.encode() }),
            Err(m) => json!({ "err": snowify::ipc::JsValue::String(m).encode() }),
        };
        d.check(
            format_args!("JSON.parse({:?})", String::from_utf16_lossy(units)),
            s,
            &r,
        );
    }
    for (units, s) in inspect.iter().zip(out["inspect"].as_array().unwrap()) {
        let r: Vec<u16> = js::inspect_string(units);
        d.check(format_args!("inspect({:?})", String::from_utf16_lossy(&units[..units.len().min(40)])), s, &json!(r));
    }
    d.finish("js");
}
