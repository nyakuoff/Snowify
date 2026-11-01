//! Replay actual full-SDK observations. Deliberately ignored until the AF_UNIX
//! generator has been run outside the restricted sandbox. Never infer SDK
//! observations from Lite or from the Rust implementation.
use serde_json::{Value, json};
use snowify::{
    firebase::{Auth, firestore},
    ipc::JsValue,
};
use std::{path::Path, time::Duration};
mod common;
#[path = "common/host.rs"]
mod host;

const HEADERS: &[&str] = &[
    "authorization",
    "x-goog-api-client",
    "google-cloud-resource-prefix",
    "x-goog-request-params",
    "x-firebase-gmpid",
    "x-firebase-client",
];
fn reply(value: Value) -> Value {
    json!({"body":value.to_string()})
}
fn auth_fetch() -> Value {
    json!({
        "https://identitytoolkit.googleapis.com/v1/accounts:signInWithPassword": [reply(json!({
            "localId":"u","idToken":"oracle-token","refreshToken":"oracle-refresh +/","expiresIn":"3600"
        }))],
        "https://identitytoolkit.googleapis.com/v1/accounts:lookup": [reply(json!({
            "users":[{"localId":"u","email":"oracle@example.invalid","displayName":"Oracle"}]
        }))],
        "https://securetoken.googleapis.com/v1/token": [reply(json!({
            "access_token":"oracle-fresh-token","refresh_token":"oracle-refresh +/","expires_in":"3600"
        }))],
    })
}
fn responses(s: &Value) -> Vec<Value> {
    if s["offline"] == true {
        return vec![json!({"error":true})];
    }
    let name = "projects/snowify-dcda0/databases/(default)/documents/users/u";
    let success = if s["op"] == "get" {
        if s["missing"] == true {
            reply(json!([{"missing":name,"readTime":"2023-11-14T22:13:20Z"}]))
        } else {
            let value = if s["reference"] == true {
                json!({"referenceValue":"projects/snowify-dcda0/databases/(default)/documents/artists/a"})
            } else {
                json!({"stringValue":"fixture 雪"})
            };
            reply(
                json!([{"found":{"name":name,"fields":{"value":value},"updateTime":"2023-11-14T22:13:20Z"}}]),
            )
        }
    } else {
        reply(
            json!({"commitTime":"2023-11-14T22:13:20Z","writeResults":[{"updateTime":"2023-11-14T22:13:20Z"}]}),
        )
    };
    let Some(status) = s["status"].as_str() else {
        return vec![success];
    };
    // Transport differs. Feed the equivalent REST status/error, not gRPC bytes.
    let http = match status {
        "NOT_FOUND" => 404,
        "PERMISSION_DENIED" => 403,
        "UNAUTHENTICATED" => 401,
        "UNAVAILABLE" => 503,
        "RESOURCE_EXHAUSTED" => 429,
        "INVALID_ARGUMENT" => 400,
        "FAILED_PRECONDITION" => 400,
        "INTERNAL" => 500,
        "DEADLINE_EXCEEDED" => 504,
        _ => panic!("unknown corpus status {status}"),
    };
    let error = json!({"status":http,"body":json!({"error":{"status":status,"message":s["message"]}}).to_string()});
    if let Some(n) = s["failFor"].as_u64() {
        let mut out = vec![error; n as usize];
        out.push(success);
        out
    } else {
        vec![error]
    }
}
fn selected_headers(headers: &Value) -> Value {
    json!(
        HEADERS
            .iter()
            .map(|name| ((*name).to_owned(), headers[*name].clone()))
            .collect::<serde_json::Map<_, _>>()
    )
}

#[tokio::test]
#[ignore = "Requires complete AF_UNIX full-SDK corpus; see parity/node/firestore_full.js"]
async fn parity() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("parity/corpus/firestore_full.json");
    let corpus: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(
        corpus["status"], "complete",
        "No full-SDK observations yet. Run ELECTRON_RUN_AS_NODE=1 node_modules/.bin/electron src-tauri/parity/node/firestore_full.js outside the sandbox. Blocker: {}",
        corpus["blocker"]
    );
    assert_eq!(corpus["sdk"], "firebase/firestore");
    let scenarios = corpus["scenarios"].as_array().unwrap();
    let observed = corpus["results"].as_array().unwrap();
    assert_eq!(scenarios.len(), 77, "Partial corpus is not acceptable");
    assert_eq!(observed.len(), scenarios.len());
    let mut diff = common::Diff::default();
    for (s, oracle) in scenarios.iter().zip(observed) {
        assert_eq!(s["id"], oracle["id"]);
        let id = s["id"].as_str().unwrap();
        let mut auth = Auth::default();
        if s["auth"].is_string() {
            let h = host::MockHost::new(
                &json!({"userData":"/tmp/firebase-full-unused","now":1700000000000i64,"fetch":auth_fetch()}),
                "",
            );
            auth.sign_in_with_email_and_password(
                &h,
                &"oracle@example.invalid".into(),
                &"fixture-password".into(),
            )
            .await
            .unwrap();
        }
        let mut fetch = auth_fetch();
        fetch["https://firestore.googleapis.com"] = json!(responses(s));
        let now = 1700000000000i64 + if s["auth"] == "refresh" { 3600001 } else { 0 };
        let h = host::MockHost::new(
            &json!({"userData":"/tmp/firebase-full-unused","now":now,"fetch":fetch}),
            "",
        );
        let doc = firestore::doc("users", "u").unwrap();
        let op = async {
            if s["op"] == "get" {
                firestore::get_doc(&h, &mut auth, &doc)
                    .await
                    .map(|snap| json!({"exists":snap.exists(),"data":snap.data().encode()}))
            } else {
                let result = if s["op"] == "set" {
                    firestore::set_doc(
                        &h,
                        &mut auth,
                        &doc,
                        &JsValue::from(&json!({"profile":{"bio":"fixture 雪"}})),
                    )
                    .await
                } else {
                    firestore::update_doc(
                        &h,
                        &mut auth,
                        &doc,
                        &JsValue::from(&json!({"profile.bio":"fixture 雪"})),
                    )
                    .await
                };
                result.map(|_| Value::Null)
            }
        };
        let budget = corpus["clock"]["budgetMs"].as_u64().unwrap();
        let outcome = match tokio::time::timeout(Duration::from_millis(budget), op).await {
            Err(_) => json!({"status":"pending","observedForMs":budget}),
            Ok(Ok(value)) => json!({"status":"fulfilled","value":value}),
            Ok(Err(e)) => {
                json!({"status":"rejected","message":e.message,"code":if e.code.is_empty(){Value::Null}else{json!(e.code)}})
            }
        };
        diff.check(format!("{id} outcome"), &oracle["outcome"], &outcome);
        let requests = h.fetch_requests.lock().unwrap();
        let rest = requests
            .iter()
            .filter(|r| {
                r[1].as_str()
                    .unwrap()
                    .starts_with("https://firestore.googleapis.com")
            })
            .collect::<Vec<_>>();
        // Count document attempts, not stream handshakes or teardown frames.
        let attempts = oracle["trace"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| match t["kind"].as_str().unwrap() {
                "Listen.request" => t["request"]["addTarget"].is_object(),
                "Write.request" => t["request"]["writes"]
                    .as_array()
                    .is_some_and(|a| !a.is_empty()),
                "BatchGetDocuments.request" | "Commit.request" => true,
                _ => false,
            })
            .count();
        // Offline SDK failures never reach a server. Compare outcomes there;
        // REST fetch attempts and server-received attempts cannot be equated.
        if s["offline"] != true {
            diff.check(
                format!("{id} document attempts"),
                &json!(attempts),
                &json!(rest.len()),
            );
        }
        if s["auth"].is_string() {
            let grpc = oracle["trace"]
                .as_array()
                .unwrap()
                .iter()
                .find_map(|t| t.get("metadata"))
                .expect("authenticated RPC metadata");
            let headers = rest
                .first()
                .map(|r| {
                    r[2].as_array()
                        .unwrap()
                        .iter()
                        .map(|pair| (pair[0].as_str().unwrap().to_owned(), pair[1].clone()))
                        .collect::<serde_json::Map<_, _>>()
                })
                .unwrap_or_default();
            diff.check(
                format!("{id} metadata"),
                &selected_headers(grpc),
                &selected_headers(&json!(headers)),
            );
            let rust_refresh = requests.iter().position(|r| {
                r[1].as_str()
                    .unwrap()
                    .starts_with("https://securetoken.googleapis.com")
            });
            let rust_doc = requests.iter().position(|r| {
                r[1].as_str()
                    .unwrap()
                    .starts_with("https://firestore.googleapis.com")
            });
            let refreshed_before = matches!((rust_refresh,rust_doc), (Some(a),Some(b)) if a < b);
            let sdk_refreshed = oracle["authCalls"].as_array().unwrap().iter().any(|c| {
                c["stage"] == "firestore"
                    && c["url"]
                        .as_str()
                        .unwrap()
                        .starts_with("https://securetoken.googleapis.com")
            });
            diff.check(
                format!("{id} refresh before call"),
                &json!(sdk_refreshed),
                &json!(refreshed_before),
            );
        }
    }
    diff.finish("firestore_full");
}
