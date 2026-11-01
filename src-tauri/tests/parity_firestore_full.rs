//! Replay genuine full-SDK observations through the production FirebaseBackend.
use serde_json::{Value, json};
use snowify::{cloud_auth::Backend, cloud_backend::FirebaseBackend, ipc, ipc::JsValue};
use std::{path::Path, sync::Arc, time::Duration};
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
    // REST batchGet can carry an application error in its response stream.
    // Preserve the RPC status inside that envelope for target REMOVE causes.
    // A non-2xx HTTP response models a failed Listen transport instead.
    let error = json!({"status":if s["targetError"] == true {200} else {http},"body":json!({"error":{"status":status,"message":s["message"]}}).to_string()});
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

#[tokio::test(start_paused = true)]
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
        let mut fetch = auth_fetch();
        fetch["https://firestore.googleapis.com"] = json!(responses(s));
        // Heartbeat dates are real UTC dates in the recording. Set the replay
        // clock from that observation rather than pinning today's date.
        let auth_calls = oracle["authCalls"].as_array().unwrap();
        let base = auth_calls
            .first()
            .map(|c| {
                let heartbeat = c["headers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|h| h[0] == "x-firebase-client")
                    .unwrap()[1]
                    .as_str()
                    .unwrap();
                let decoded: Value = serde_json::from_slice(&host::base64_decode(
                    &heartbeat.replace('-', "+").replace('_', "/"),
                ))
                .unwrap();
                let date = decoded["heartbeats"][0]["dates"][0].as_str().unwrap();
                snowify::js::date_parse_iso(&format!("{date}T00:00:00.000Z")).unwrap() as i64
            })
            .unwrap_or(1700000000000i64);
        let h = Arc::new(host::MockHost::new(
            &json!({"userData":"/tmp/firebase-full-unused","now":base,"fetch":fetch}),
            "",
        ));
        let backend = FirebaseBackend::new(h.clone());
        if s["auth"].is_string() {
            backend
                .sign_in(&"oracle@example.invalid".into(), &"fixture-password".into())
                .await
                .unwrap();
        }
        if s["auth"] == "refresh" {
            h.now
                .store(base + 3600001, std::sync::atomic::Ordering::SeqCst);
        }
        let op = async {
            if s["op"] == "get" {
                backend.get_doc("users", &"u".into()).await.map(|data| {
                    let exists = data.is_some();
                    let data = data.unwrap_or_default();
                    if s["reference"] == true {
                        let JsValue::FirestoreReference(path) = data.get("value") else { panic!("reference marker missing") };
                        json!({"exists":exists,"reference":{
                            "constructor":"DocumentReference","type":"document","path":path,
                            "id":path.rsplit('/').next().unwrap(),"parentPath":path.rsplit_once('/').unwrap().0,
                            "ownKeys":["converter","_key","type","firestore"]
                        }, "structuredClone":ipc::structured_clone_error(&data).unwrap()})
                    } else { json!({"exists":exists,"data":data.encode()}) }
                })
            } else {
                let data = if s["op"] == "set" {
                    JsValue::from(&json!({"profile":{"bio":"fixture 雪"}}))
                } else {
                    JsValue::from(&json!({"profile.bio":"fixture 雪"}))
                };
                let r = if s["op"] == "set" {
                    backend.set_doc_merge("users", &"u".into(), &data).await
                } else {
                    backend.update_doc("users", &"u".into(), &data).await
                };
                r.map(|_| Value::Null)
            }
        };
        let budget = corpus["clock"]["budgetMs"].as_u64().unwrap();
        let started = tokio::time::Instant::now();
        let outcome = match tokio::time::timeout(Duration::from_millis(budget), op).await {
            Err(_) => json!({"status":"pending","observedForMs":budget}),
            Ok(Ok(value)) => json!({"status":"fulfilled","value":value}),
            Ok(Err(e)) => {
                json!({"status":"rejected","message":ipc::to_js_string(&e.0),"code":if e.1.is_empty(){Value::Null}else{json!(e.1)}})
            }
        };
        diff.check(format!("{id} outcome"), &oracle["outcome"], &outcome);
        // This checks the SDK policy's jitter support, not a ±25% match to the
        // recording's independent random draw (see FIREBASE-11A-REPORT.md).
        if s["failFor"] == 2 && s["op"] != "get" && oracle["outcome"]["status"] == "fulfilled" {
            let elapsed = started.elapsed().as_millis();
            assert!(
                (500..=1502).contains(&elapsed),
                "{id}: recovery backoff {elapsed} ms"
            );
        }

        let requests = h.fetch_requests.lock().unwrap();
        let rust_auth = requests
            .iter()
            .filter(|r| {
                !r[1]
                    .as_str()
                    .unwrap()
                    .starts_with("https://firestore.googleapis.com")
            })
            .map(|r| json!({"method":r[0],"url":r[1],"headers":r[2],"body":r[3]}))
            .collect::<Vec<_>>();
        let source_auth = auth_calls.iter().map(|c| json!({"method":c["method"],"url":c["url"],"headers":c["headers"],"body":c["body"]})).collect::<Vec<_>>();
        diff.check(
            format!("{id} auth wire/heartbeat"),
            &json!(source_auth),
            &json!(rust_auth),
        );
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
        // Pending retry counts also depend on the oracle's unseeded jitter.
        // Recovery/permanent paths still compare every document attempt.
        if s["offline"] != true && oracle["outcome"]["status"] != "pending" {
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

#[tokio::test(start_paused = true)]
async fn offline_cloud_save_remains_pending() {
    use snowify::{cloud_auth::CloudAuth, i18n::Translations};
    let mut fetch = auth_fetch();
    fetch["https://firestore.googleapis.com"] = json!([{"error":true}]);
    let h = Arc::new(host::MockHost::new(
        &json!({
            "userData":"/tmp/firebase-full-unused","now":1700000000000i64,"fetch":fetch
        }),
        "",
    ));
    let backend = FirebaseBackend::new(h);
    backend
        .sign_in(&"oracle@example.invalid".into(), &"fixture-password".into())
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(12000),
            CloudAuth::default().save(
                &backend,
                &Translations::default(),
                &JsValue::from(&json!({"x":1}))
            )
        )
        .await
        .is_err()
    );
}

#[test]
fn reference_rejected_at_reply_boundary() {
    let reference = JsValue::FirestoreReference("artists/a".into());
    let data = JsValue::object([("nested", JsValue::Array(vec![reference]))]);
    let e = ipc::clone_reply(&data).unwrap_err();
    assert_eq!(
        e.rejection_message("cloud:load").to_string_lossy(),
        "Error invoking remote method 'cloud:load': Error: An object could not be cloned."
    );
    assert!(ipc::clone_reply(&JsValue::from(&json!({"nested":["artists/a"]}))).is_ok());
}

#[tokio::test(start_paused = true)]
async fn cloud_load_retains_reference_until_ipc_reply() {
    use snowify::cloud_auth::CloudAuth;
    let mut fetch = auth_fetch();
    fetch["https://firestore.googleapis.com"] =
        json!(responses(&json!({"op":"get","reference":true})));
    let h = Arc::new(host::MockHost::new(
        &json!({
            "userData":"/tmp/firebase-full-unused","now":1700000000000i64,"fetch":fetch
        }),
        "",
    ));
    let backend = FirebaseBackend::new(h);
    backend
        .sign_in(&"oracle@example.invalid".into(), &"fixture-password".into())
        .await
        .unwrap();
    let result = CloudAuth::default().load(&backend).await.unwrap();
    assert!(
        matches!(result.get("value"), JsValue::FirestoreReference(path) if path == "artists/a")
    );
    assert!(ipc::clone_reply(&result).is_err());
}
