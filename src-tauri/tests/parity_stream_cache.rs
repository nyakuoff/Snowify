//! Differential test: `src/main/stream-cache.js` vs `snowify::stream_cache`, over random
//! set/get sequences around the TTL boundary and the 200-entry sweep.

use serde_json::{Value, json};
use snowify::stream_cache::{STREAM_CACHE_TTL_MS, StreamCache};

mod common;
use common::{Diff, Rng};

#[test]
fn stream_cache_matches_source() {
    let mut rng = Rng(0x5eed_1234_abcd_ef01);
    let mut ops = Vec::new();
    let mut now: i64 = 1_700_000_000_000;
    for i in 0..20_000 {
        // Bursts of distinct keys so the cache passes 200 entries and the sweep runs.
        if i % 5000 < 300 {
            now += rng.below(1000) as i64;
            ops.push(json!(["set", format!("burst{i}"), "u", now]));
            continue;
        }
        // Mostly small steps; sometimes jump to exactly the TTL edge or past it, or backwards.
        now += match rng.below(20) {
            0 => STREAM_CACHE_TTL_MS,
            1 => STREAM_CACHE_TTL_MS + 1,
            2 => -1000,
            3 => STREAM_CACHE_TTL_MS / 3,
            _ => rng.below(60_000) as i64,
        };
        let pool = if rng.below(4) == 0 { 400 } else { 40 };
        let key = format!("k{}", rng.below(pool));
        if rng.below(2) == 0 {
            let value = match rng.below(4) {
                0 => json!(format!("https://x/{}", rng.next())),
                1 => json!({ "url": "u", "n": rng.below(9) }),
                2 => Value::Null,
                _ => json!(""),
            };
            ops.push(json!(["set", key, value, now]));
        } else {
            ops.push(json!(["get", key, now]));
        }
    }
    let out = common::run_source("stream_cache.js", &Value::Array(ops.clone()));

    let mut cache = StreamCache::default();
    let mut d = Diff::default();
    let mut max_size = 0;
    for (op, s) in ops.iter().zip(out.as_array().unwrap()) {
        let key = op[1].as_str().unwrap();
        let r = if op[0] == "set" {
            cache.set(key.into(), op[2].clone(), op[3].as_i64().unwrap());
            Value::Null
        } else {
            cache
                .get(key, op[2].as_i64().unwrap())
                .unwrap_or(Value::Null)
        };
        max_size = max_size.max(cache.len());
        d.check(op, s, &json!([r, cache.len()]));
    }
    assert!(max_size > 200, "corpus never reached the 200-entry sweep");
    d.finish("stream_cache");
}
