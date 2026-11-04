//! Differential test: tough-cookie 4.1.4 (the jar ytmusic-api uses, via `parity/node/
//! ytm_cookies.js`) vs `snowify::ytm::cookies::Jar`, over the same Set-Cookie sequences with
//! explicit URLs and timestamps. Compares each step's set outcome and the cookie header.

use serde_json::{Value, json};
use snowify::ytm::cookies::{self, Jar};

mod common;
use common::Diff;

const ROOT: &str = "https://music.youtube.com/";
const T: i64 = 1_700_000_000_000;

fn set(ops: &mut Vec<Value>, cookie: &str, now: i64) {
    ops.push(json!(["set", cookie, ROOT, now]));
}

fn get(ops: &mut Vec<Value>, now: i64) {
    ops.push(json!(["get", Value::Null, ROOT, now]));
}

#[test]
fn ytm_cookies_match_tough_cookie() {
    let mut ops = Vec::new();
    set(&mut ops, "a=1; Path=/", T);
    get(&mut ops, T);
    set(&mut ops, "b=2; Path=/deep", T);
    get(&mut ops, T);
    set(&mut ops, "c=3; Domain=.music.youtube.com; Path=/", T);
    get(&mut ops, T);
    set(&mut ops, "d=4; Domain=music.youtube.com; Path=/", T);
    get(&mut ops, T);
    set(&mut ops, "e=5; Domain=.example.org; Path=/", T);
    set(&mut ops, "f=6; Domain=com; Path=/", T);
    // `Domain=.`: stripping the leading dot leaves nothing, so the attribute is dropped and
    // the cookie is stored host-only (tough-cookie does the same).
    set(&mut ops, "dot=1; Domain=.", T);
    get(&mut ops, T);
    set(&mut ops, "g=7; Path=/; Secure", T);
    set(&mut ops, "__Secure-bad=1; Path=/", T);
    set(&mut ops, "__Secure-good=1; Path=/; Secure", T);
    set(&mut ops, "__Host-bad=1; Path=/", T);
    set(&mut ops, "__Host-good=1; Path=/; Secure", T);
    get(&mut ops, T);

    // Max-Age edges: zero is already expired at creation, negative likewise, positive expires
    // exactly at creation + seconds*1000.
    set(&mut ops, "maxzero=1; Max-Age=0; Path=/", T + 5000);
    get(&mut ops, T + 4999);
    get(&mut ops, T + 5000);
    set(&mut ops, "maxneg=1; Max-Age=-5; Path=/", T + 6000);
    get(&mut ops, T + 5999);
    set(&mut ops, "maxpos=1; Max-Age=10; Path=/", T + 10000);
    get(&mut ops, T + 19999);
    get(&mut ops, T + 20000);

    // Ordering: descending path length, then creation time, then parse index.
    set(&mut ops, "orderLate=1; Path=/", T + 30020);
    set(&mut ops, "orderEarly=1; Path=/", T + 30010);
    get(&mut ops, T + 30020);
    // Replacing a cookie keeps its creation time, so it still sorts first.
    set(&mut ops, "a=9; Path=/", T + 30030);
    get(&mut ops, T + 30030);
    set(&mut ops, "z=1; Path=/", T + 30040);
    get(&mut ops, T + 30040);

    let source = common::run_source("ytm_cookies.js", &Value::Array(ops.clone()));

    let mut jar = Jar::default();
    let mut d = Diff::default();
    for (op, expected) in ops.iter().zip(source.as_array().unwrap()) {
        let kind = op[0].as_str().unwrap();
        let now = op[3].as_f64().unwrap();
        let rust = if kind == "set" {
            jar.created += 1;
            match cookies::parse(op[1].as_str().unwrap(), jar.created) {
                None => json!(["null"]),
                Some(c) => match jar.set(c, now) {
                    Ok(()) => json!(["ok"]),
                    Err(_) => json!(["error"]),
                },
            }
        } else {
            json!(["header", jar.header(now)])
        };
        d.check(op, expected, &rust);
    }
    d.finish("ytm_cookies");
}
