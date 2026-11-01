use serde_json::{Value, json};
use snowify::{firebase::Auth, ipc::JsValue};
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
    let mut success = Value::Null;
    for signup in [false, true] {
        // "30" sits exactly on the SDK's refresh buffer boundary (`now > expires - 30000`).
        for expires in ["3600", "10", "30"] {
            scenarios.push(json!({"userData":"/tmp/firebase-parity","now":1700000000000i64,"fetch":{
 format!("{base}signInWithPassword"):[response(json!({"localId":"u","idToken":"t","refreshToken":"r +/","expiresIn":expires}))],
 format!("{base}signUp"):[response(json!({"localId":"u","idToken":"t","refreshToken":"r +/","expiresIn":expires}))],
 format!("{base}lookup"):[response(json!({"users":[{"localId":"u","email":"e@example.com","displayName":"Name","photoUrl":"pic"}]}))],
 format!("{base}update"):[response(json!({"displayName":"New","photoUrl":"newpic"}))],
 "https://securetoken.googleapis.com/v1/token":[response(json!({"access_token":"fresh","refresh_token":"r","expires_in":"3600"}))]
 },"ops":[[if signup{"signUp"}else{"signIn"},"e@example.com","password"],["profile",{"displayName":"New","photoURL":"newpic"}],["token",true],["current"],["out"],["out"]]}));
            if signup && expires == "3600" {
                success = scenarios.last().unwrap().clone();
            }
        }
    }
    // A sign-in response that selects a second factor is rejected before the user is set.
    for body in [
        json!({"localId":"u","idToken":"t","refreshToken":"r","expiresIn":"3600","mfaPendingCredential":"m"}),
        json!({"needConfirmation":true,"email":"e@example.com"}),
    ] {
        scenarios.push(json!({"userData":"/tmp/firebase-parity","now":1700000000000i64,"fetch":{
            format!("{base}signInWithPassword"):[response(body)],
        },"ops":[["signIn","e@example.com","password"]]}));
    }
    // updateProfile's response replaces the stored id token without a refresh request.
    scenarios.push(json!({"userData":"/tmp/firebase-parity","now":1700000000000i64,"fetch":{
        format!("{base}signInWithPassword"):[response(json!({"localId":"u","idToken":"old","refreshToken":"r","expiresIn":"3600"}))],
        format!("{base}lookup"):[response(json!({"users":[{"localId":"u"}]}))],
        format!("{base}update"):[response(json!({"displayName":"N","idToken":"newtok","refreshToken":"newref","expiresIn":"7200"}))],
        "https://securetoken.googleapis.com/v1/token":[response(json!({"access_token":"refreshed","refresh_token":"r","expires_in":"3600"}))]
    },"ops":[["signIn","e@example.com","password"],["profile",{"displayName":"N"}],["token",false]]}));
    // Credentials over the whole IPC value domain: the SDK has no client-side checks.
    let odd = |v: Value| JsValue::decode(&v).unwrap().encode();
    for (email, password) in [
        (json!(""), json!("p")),
        (json!("e@x.com"), json!("")),
        (json!(5), json!(null)),
        (json!({ "$": "u" }), json!({ "$": "u" })),
        (json!({ "$": "s", "v": [0xD800, 0x61] }), json!(true)),
        (json!({ "$": "o", "v": [["a", 1]] }), json!([1, { "$": "u" }])),
        (json!({ "$": "b", "v": "7" }), json!("p")),
        (json!({ "$": "n", "v": "NaN" }), json!({ "$": "n", "v": "-0" })),
    ] {
        for op in ["signIn", "signUp"] {
            scenarios.push(json!({"userData":"/tmp/firebase-parity","now":1700000000000i64,"fetch":{
                format!("{base}signInWithPassword"):[{"status":400,"body":json!({"error":{"message":"MISSING_EMAIL"}}).to_string()}],
                format!("{base}signUp"):[{"status":400,"body":json!({"error":{"message":"INVALID_EMAIL"}}).to_string()}],
            },"ops":[[op, odd(email.clone()), odd(password.clone())]]}));
        }
        scenarios.push(json!({"userData":"/tmp/firebase-parity","now":1700000000000i64,"fetch":{
            format!("{base}sendOobCode"):[{"status":400,"body":json!({"error":{"message":"MISSING_EMAIL"}}).to_string()}],
        },"ops":[["reset", odd(email)]]}));
    }
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
    // Plain JSON args go over in the wire format (wire-form args are already encoded).
    for s in scenarios.iter_mut() {
        for op in s["ops"].as_array_mut().unwrap() {
            for a in op.as_array_mut().unwrap().iter_mut().skip(1) {
                if !a.get("$").is_some_and(Value::is_string) {
                    *a = JsValue::from(&*a).encode();
                }
            }
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
            let arg = |i: usize| op.get(i).map_or(JsValue::Undefined, |a| JsValue::decode(a).unwrap());
            let r = match op[0].as_str().unwrap() {
                "signIn" => a.sign_in_with_email_and_password(&h, &arg(1), &arg(2)).await.map(|u| u.fields()),
                "signUp" => a.create_user_with_email_and_password(&h, &arg(1), &arg(2)).await.map(|u| u.fields()),
                "reset" => a.send_password_reset_email(&h, &arg(1)).await.map(|_| Value::Null),
                "profile" => a.update_profile(&h, &arg(1)).await.map(|_| a.fields()),
                "token" => a.get_id_token(&h, arg(1).truthy()).await.map(Value::String),
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
