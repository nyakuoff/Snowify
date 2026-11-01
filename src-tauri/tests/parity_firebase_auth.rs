use serde_json::{Value, json};
use snowify::firebase::Auth;
mod common;
#[path = "common/host.rs"]
mod host;
fn response(v: Value) -> Value {
    json!({"body":v.to_string()})
}
#[tokio::test]
async fn parity() {
    let base = "https://identitytoolkit.googleapis.com/v1/accounts:";
    let mut scenarios = Vec::new();
    let errors: Value =
        serde_json::from_str(include_str!("../src/firebase/auth_errors.json")).unwrap();
    for code in errors
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .chain([
            "USER_DISABLED",
            "WEAK_PASSWORD",
            "WEAK_PASSWORD : Password should be at least 6 characters",
            "SOMETHING_NEW",
            "UNKNOWN : detailed : more",
        ])
    {
        scenarios.push(json!({"userData":"/tmp/firebase-parity","now":1700000000000i64,"fetch":{format!("{base}signInWithPassword"):[{"status":400,"body":json!({"error":{"message":code}}).to_string()}]},"ops":[["signIn","e@example.com","p"]]}));
    }
    for r in [
        json!({"error":true}),
        json!({"status":500,"body":"<html>"}),
        response(json!({})),
        json!({"status":400,"body":"{}"}),
    ] {
        scenarios.push(json!({"userData":"/tmp/firebase-parity","now":1700000000000i64,"fetch":{format!("{base}sendOobCode"):[r]},"ops":[["reset","e@example.com"]]}));
    }
    for signup in [false, true] {
        for expires in ["3600", "10"] {
            scenarios.push(json!({"userData":"/tmp/firebase-parity","now":1700000000000i64,"fetch":{
 format!("{base}signInWithPassword"):[response(json!({"localId":"u","idToken":"t","refreshToken":"r +/","expiresIn":expires}))],
 format!("{base}signUp"):[response(json!({"localId":"u","idToken":"t","refreshToken":"r +/","expiresIn":expires}))],
 format!("{base}lookup"):[response(json!({"users":[{"localId":"u","email":"e@example.com","displayName":"Name","photoUrl":"pic"}]}))],
 format!("{base}update"):[response(json!({"displayName":"New","photoUrl":"newpic"}))],
 "https://securetoken.googleapis.com/v1/token":[response(json!({"access_token":"fresh","refresh_token":"r","expires_in":"3600"}))]
 },"ops":[[if signup{"signUp"}else{"signIn"},"e@example.com","password"],["profile",{"displayName":"New","photoURL":"newpic"}],["token",true],["current"],["out"],["out"]]}));
        }
    }
    let success = scenarios[scenarios.len() - 2].clone();
    for endpoint in ["update", "token"] {
        for code in [
            "USER_DISABLED",
            "TOKEN_EXPIRED",
            "INVALID_ID_TOKEN",
            "INVALID_LOGIN_CREDENTIALS",
        ] {
            let mut s = success.clone();
            s["fetch"][if endpoint == "token" {
                "https://securetoken.googleapis.com/v1/token".into()
            } else {
                format!("{base}update")
            }] = json!([{"status":400,"body":json!({"error":{"message":code}}).to_string()}]);
            s["ops"] = json!([
                ["signIn", "e@example.com", "password"],
                if endpoint == "token" {
                    json!(["token", true])
                } else {
                    json!(["profile",{"displayName":"x"}])
                },
                ["current"]
            ]);
            scenarios.push(s);
        }
    }
    let source = common::run_source("firebase_auth.js", &json!(scenarios));
    let mut diff = common::Diff::default();
    for (i, s) in scenarios.iter().enumerate() {
        let h = host::MockHost::new(s, "");
        let mut a = Auth::default();
        a.on_auth_state_changed();
        let mut results = Vec::new();
        for op in s["ops"].as_array().unwrap() {
            let r = match op[0].as_str().unwrap() {
                "signIn" => a
                    .sign_in_with_email_and_password(
                        &h,
                        op[1].as_str().unwrap(),
                        op[2].as_str().unwrap(),
                    )
                    .await
                    .map(|u| u.fields()),
                "signUp" => a
                    .create_user_with_email_and_password(
                        &h,
                        op[1].as_str().unwrap(),
                        op[2].as_str().unwrap(),
                    )
                    .await
                    .map(|u| u.fields()),
                "reset" => a
                    .send_password_reset_email(&h, op[1].as_str().unwrap())
                    .await
                    .map(|_| Value::Null),
                "profile" => a.update_profile(&h, &op[1]).await.map(|_| a.fields()),
                "token" => a
                    .get_id_token(&h, op[1].as_bool().unwrap())
                    .await
                    .map(Value::String),
                "current" => Ok(a.fields()),
                "out" => {
                    a.sign_out();
                    Ok(Value::Null)
                }
                _ => unreachable!(),
            };
            results.push(match r {
                Ok(v) => json!({"ok":v}),
                Err(e) => json!({"error":e.message,"code":e.code}),
            });
        }
        diff.check(i,&source[i],&json!({"calls":*h.fetch_requests.lock().unwrap(),"events":a.take_events(),"results":results}));
    }
    diff.finish("firebase_auth");
}
