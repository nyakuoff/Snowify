use serde_json::json;
use snowify::{firebase::firestore, ipc::JsValue};
mod common;
#[path = "common/host.rs"]
mod host;
#[tokio::test]
async fn parity() {
    let mut cases = Vec::new();
    let values = vec![
        JsValue::Null,
        JsValue::Bool(true),
        JsValue::from("雪 café 😀"),
        JsValue::Number(0.0),
        JsValue::Number(-0.0),
        JsValue::Number(f64::NAN),
        JsValue::Number(f64::INFINITY),
        JsValue::Number(f64::NEG_INFINITY),
        JsValue::Number(9007199254740992.0),
        JsValue::Undefined,
        JsValue::Date(1700000000123.0),
        JsValue::from(&json!({})),
        JsValue::from(&json!([1,null,{"a":[true]}])),
        JsValue::from(&json!([[1]])),
        JsValue::from(&json!({"a.b":{"c":3}})),
        JsValue::View {
            kind: "Uint8Array".into(),
            buffer: vec![0, 255],
            offset: 0,
            length: 2,
        },
    ];
    for update in [false, true] {
        for key in ["a", "a.b", "a b", "`\\", "雪", "", "__bad__", "a..b", "a/b"] {
            for v in &values {
                cases.push(json!({"op":if update{"update"}else{"set"},"value":JsValue::Object(vec![(key.into(),v.clone())]).encode()}));
            }
        }
    }
    for v in &values {
        for op in ["set", "update"] {
            cases.push(json!({"op":op,"value":v.encode()}));
        }
    }
    for i in 0..200 {
        let data = json!({format!("field{i}"):{"n":i as f64*0.125,"nested":{},"a":[i,"x",null]}});
        cases.push(json!({"op":"set","value":JsValue::from(&data).encode()}));
    }
    for value in [
        json!({"mapValue":{"fields":{"__type__":{"stringValue":"__vector__"},"value":{"arrayValue":{"values":[{"doubleValue":1.5},{"doubleValue":2.5}]}}}}}),
        json!({"integerValue":"9223372036854775807"}),
        json!({"timestampValue":"2023-11-14T22:13:20.123456789Z"}),
        json!({"bytesValue":"AP9h"}),
        json!({"geoPointValue":{"latitude":1.25,"longitude":-35.5}}),
        json!({"arrayValue":{"values":[{"doubleValue":"NaN"},{"doubleValue":"-0"}]}}),
        json!({"mapValue":{"fields":{"x":{"nullValue":"NULL_VALUE"}}}}),
    ] {
        cases.push(json!({"op":"get","reply":[{"found":{"name":"projects/snowify-dcda0/databases/(default)/documents/users/u","fields":{"value":value},"updateTime":"2023-01-01T00:00:00Z"}}]}));
    }
    let source = common::run_source("firestore.js", &json!(cases));
    let mut diff = common::Diff::default();
    let doc = firestore::doc("users", "u").unwrap();
    for (i, c) in cases.iter().enumerate() {
        if c["op"] == "get" {
            let h = host::MockHost::new(
                &json!({"userData":"/tmp/firebase-parity","fetch":{"https://firestore.googleapis.com":[{"body":c["reply"][0]["found"].to_string()}]}}),
                "",
            );
            let mut auth = snowify::firebase::Auth::default();
            let snap = firestore::get_doc(&h, &mut auth, &doc).await.unwrap();
            let data = snap.data();
            // Lite uses batchGet, while the required production transport uses GET. Assert
            // each transport independently; only equivalent document responses are compared.
            assert_eq!(
                h.fetch_requests.lock().unwrap()[0],
                json!([
                    "GET",
                    "https://firestore.googleapis.com/v1/projects/snowify-dcda0/databases/(default)/documents/users/u",
                    [["content-type", "application/json"]],
                    null
                ])
            );
            diff.check(i,&source[i],&json!({"calls":[{"documents":["projects/snowify-dcda0/databases/(default)/documents/users/u"]}],"ok":{"exists":true,"data":data.encode()}}));
            continue;
        }
        let h = host::MockHost::new(
            &json!({"userData":"/tmp/firebase-parity","fetch":{"https://firestore.googleapis.com":[{"body":"{}"}]}}),
            "",
        );
        let mut auth = snowify::firebase::Auth::default();
        let data = JsValue::decode(&c["value"]).unwrap();
        let r = if c["op"] == "update" {
            firestore::update_doc(&h, &mut auth, &doc, &data).await
        } else {
            firestore::set_doc(&h, &mut auth, &doc, &data).await
        };
        let calls: Vec<serde_json::Value> = h
            .fetch_requests
            .lock()
            .unwrap()
            .iter()
            .map(|v| serde_json::from_str(v[3].as_str().unwrap()).unwrap())
            .collect();
        let result = match r {
            Ok(()) => json!({"calls":calls,"ok":null}),
            Err(e) => json!({"calls":calls,"error":e.message,"code":e.code}),
        };
        diff.check(i, &source[i], &result);
    }
    diff.finish("firestore");
}
