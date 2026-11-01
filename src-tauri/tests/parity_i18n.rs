//! Differential test: `src/main/i18n.js` (via Node) vs `snowify::i18n`, over the same cases.
//! See `parity/node/i18n.js` for the case format.

use serde_json::{Value, json};
use snowify::i18n::{Param, Translated, Translations, resolve_locale};
use std::{collections::BTreeSet, fs, path::Path};

mod common;
use common::root;

fn echo(m: &str, p: usize, s: &str) -> String {
    format!("{m}|{p}|{s}")
}

fn param(v: &Value) -> Param<'_> {
    match v {
        Value::String(s) => Param::Str(s),
        Value::Number(n) => Param::Num(n.as_f64().unwrap()),
        Value::Bool(b) => Param::Bool(*b),
        Value::Null => Param::Null,
        Value::Object(o) if o.contains_key("undefined") => Param::Undefined,
        Value::Object(o) if o.contains_key("num") => {
            Param::Num(o["num"].as_str().unwrap().parse().unwrap())
        }
        Value::Object(o) if o["fn"] == "echo" => Param::Fn(&echo),
        _ => panic!("bad param {v}"),
    }
}

fn type_of(v: &Translated) -> &'static str {
    match v {
        Translated::Str(_) => "string",
        Translated::Json(Value::Number(_)) => "number",
        Translated::Json(Value::Bool(_)) => "boolean",
        Translated::Json(_) | Translated::ObjectProto("__proto__") => "object",
        Translated::ObjectProto(_) => "function",
    }
}

/// Runs the cases in order, carrying state across them like the source module does.
fn rust_outputs(locales: &Path, cases: &[Value]) -> Vec<Value> {
    let mut t = Translations::default();
    cases
        .iter()
        .enumerate()
        .map(|(i, case)| {
            let dir = match case.get("files") {
                Some(files) => {
                    let dir = std::env::temp_dir()
                        .join(format!("snowify-i18n-{}-{i}", std::process::id()));
                    let _ = fs::remove_dir_all(&dir);
                    fs::create_dir_all(&dir).unwrap();
                    for (name, text) in files.as_object().unwrap() {
                        if let Some(text) = text.as_str() {
                            fs::write(dir.join(format!("{name}.json")), text).unwrap();
                        }
                    }
                    dir
                }
                None => locales.to_path_buf(),
            };
            let loaded = resolve_locale(
                &(&case["override"]).into(),
                case["system"].as_str().unwrap(),
            )
            .ok()
            .and_then(|l| Translations::load(&dir, l).ok());
            let threw = loaded.is_none();
            if let Some(new) = loaded {
                t = new;
            }
            if case.get("files").is_some() {
                fs::remove_dir_all(&dir).unwrap();
            }
            let calls: Vec<Value> = case["calls"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| {
                    let params: Vec<(&str, Param)> = c[1]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|p| (p[0].as_str().unwrap(), param(&p[1])))
                        .collect();
                    match t.mt(c[0].as_str().unwrap(), &params) {
                        Ok(v) => json!({ "t": type_of(&v), "s": v.to_string() }),
                        Err(_) => json!({ "t": "throw", "s": "TypeError" }),
                    }
                })
                .collect();
            json!({ "threw": threw, "calls": calls })
        })
        .collect()
}

#[test]
fn i18n_matches_source() {
    let locales = root().join("../src/renderer/locales");
    let corpus: Value =
        serde_json::from_str(&fs::read_to_string(root().join("parity/corpus/i18n.json")).unwrap())
            .unwrap();

    // Shipped locale files must be flat objects of strings: the Rust loader rejects other
    // top-level shapes that the JS would accept (see `Translations::load`).
    let mut keys = BTreeSet::new();
    for entry in fs::read_dir(&locales).unwrap() {
        let path = entry.unwrap().path();
        let file: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let map = file
            .as_object()
            .unwrap_or_else(|| panic!("{path:?} is not an object"));
        assert!(
            map.values().all(Value::is_string),
            "{path:?} has non-string values"
        );
        keys.extend(map.keys().cloned());
    }
    let mut calls: Vec<Value> = keys.iter().map(|k| json!([k, null])).collect();
    calls.extend(corpus["extra_calls"].as_array().unwrap().iter().cloned());

    let golden = corpus["golden"].as_array().unwrap();
    let mut cases: Vec<Value> = corpus["locales"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| json!({ "override": l[0], "system": l[1], "calls": calls }))
        .collect();
    cases.extend(corpus["fixtures"].as_array().unwrap().iter().cloned());
    cases.extend(golden.iter().map(
        |g| json!({ "override": g["locale"][0], "system": g["locale"][1], "calls": [g["call"]] }),
    ));

    let node = common::run_source("i18n.js", &Value::Array(cases.clone()));
    let node = node.as_array().unwrap();

    // Characterization: the golden expectations describe what the source actually does.
    for (g, out) in golden.iter().zip(&node[cases.len() - golden.len()..]) {
        assert_eq!(
            out["calls"][0], g["expect"],
            "source behaviour differs from golden {g}"
        );
    }

    let (mut compared, mut diffs) = (0, Vec::new());
    for ((case, n), r) in cases.iter().zip(node).zip(rust_outputs(&locales, &cases)) {
        compared += 1;
        if n["threw"] != r["threw"] {
            diffs.push(format!(
                "load {} {}: node={} rust={}",
                case["override"], case["system"], n["threw"], r["threw"]
            ));
        }
        for (i, (nc, rc)) in n["calls"]
            .as_array()
            .unwrap()
            .iter()
            .zip(r["calls"].as_array().unwrap())
            .enumerate()
        {
            compared += 1;
            if nc != rc {
                diffs.push(format!(
                    "{} {}: node={nc} rust={rc}",
                    case["system"], case["calls"][i]
                ));
            }
        }
    }
    println!(
        "i18n parity: compared {compared}, differences {}",
        diffs.len()
    );
    assert!(
        diffs.is_empty(),
        "{}",
        diffs[..diffs.len().min(20)].join("\n")
    );
}
