//! Registry snapshots + getter results against the real Electron app (not Node mode).
use super::common;
use common::Diff;
use serde_json::{Value, json};
use snowify::login_items::{APPROVED, LoginItems, RUN, Settings};
use std::{
    path::Path,
    process::{Command, Stdio},
    ptr,
};
use windows_sys::Win32::{Foundation::ERROR_SUCCESS, System::Registry::*};
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
fn key(path: &str) -> Key {
    let mut h = ptr::null_mut();
    assert_eq!(
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(path).as_ptr(),
                0,
                ptr::null(),
                0,
                KEY_ALL_ACCESS,
                ptr::null(),
                &mut h,
                ptr::null_mut(),
            )
        },
        ERROR_SUCCESS
    );
    Key(h)
}
fn raw(path: &str, name: &str) -> Value {
    let key = key(path);
    let (mut ty, mut size) = (0, 0);
    if unsafe {
        RegQueryValueExW(
            key.0,
            wide(name).as_ptr(),
            ptr::null(),
            &mut ty,
            ptr::null_mut(),
            &mut size,
        )
    } != ERROR_SUCCESS
    {
        return Value::Null;
    }
    let mut data = vec![0; size as usize];
    assert_eq!(
        unsafe {
            RegQueryValueExW(
                key.0,
                wide(name).as_ptr(),
                ptr::null(),
                &mut ty,
                data.as_mut_ptr(),
                &mut size,
            )
        },
        ERROR_SUCCESS
    );
    json!({"type":ty,"data":data})
}
fn write(path: &str, name: &str, ty: u32, data: &[u8]) {
    let key = key(path);
    assert_eq!(
        unsafe {
            RegSetValueExW(
                key.0,
                wide(name).as_ptr(),
                0,
                ty,
                data.as_ptr(),
                data.len() as u32,
            )
        },
        ERROR_SUCCESS
    );
}
fn clear(names: &[String]) {
    for path in [RUN, APPROVED] {
        let key = key(path);
        for name in names {
            unsafe {
                RegDeleteValueW(key.0, wide(name).as_ptr());
            }
        }
    }
}
struct Cleanup(Vec<String>);
impl Drop for Cleanup {
    fn drop(&mut self) {
        clear(&self.0)
    }
}
fn settings(v: &Value) -> Settings {
    Settings {
        open_at_login: v["openAtLogin"].as_bool().unwrap_or(false),
        enabled: v["enabled"].as_bool().unwrap_or(true),
        name: v["name"].as_str().unwrap_or("").into(),
        path: v["path"].as_str().unwrap_or("").into(),
        args: v["args"].as_array().map_or(vec![], |a| {
            a.iter().map(|s| s.as_str().unwrap().into()).collect()
        }),
    }
}
fn seed(names: &[String], value: &Value) {
    clear(names);
    for record in value.as_array().unwrap() {
        let path = if record["key"] == "run" {
            RUN
        } else {
            APPROVED
        };
        let name = &names[record["name"].as_u64().unwrap() as usize];
        let ty = record["type"].as_u64().unwrap() as u32;
        let data: Vec<u8> = if ty == REG_BINARY {
            record["data"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n.as_u64().unwrap() as u8)
                .collect()
        } else {
            wide(record["data"].as_str().unwrap())
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect()
        };
        write(path, name, ty, &data);
    }
}
fn snapshot(names: &[String]) -> Value {
    json!(
        names
            .iter()
            .map(|name| json!({"name":name,"run":raw(RUN,name),"approved":raw(APPROVED,name)}))
            .collect::<Vec<_>>()
    )
}
fn electron(case: &Value, app_id: &str, dir: &Path) -> Value {
    let input = dir.join("input.json");
    let output = dir.join("output.json");
    let _ = std::fs::remove_file(&output);
    let mut case = case.clone();
    case["appId"] = app_id.into();
    case["userData"] = dir.join("profile").to_string_lossy().into_owned().into();
    std::fs::write(&input, case.to_string()).unwrap();
    let mut child = Command::new(
        std::env::var_os("SNOWIFY_PARITY_ELECTRON").expect("use parity/windows/run.sh"),
    )
    .arg(common::root().join("parity/electron/login-items/main.js"))
    .arg(input)
    .arg(&output)
    .env_remove("ELECTRON_RUN_AS_NODE")
    .stdin(Stdio::null())
    .spawn()
    .expect("start Electron app");
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "Electron app failed: {status}");
            break;
        }
        if start.elapsed() > std::time::Duration::from_secs(30) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("Electron login-items runner timed out");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap()
}
#[test]
fn login_items_match_windows_electron_registry_and_getter() {
    let id = format!(
        "Snowify-parity-login-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let names = vec![id.clone(), format!("{id}-alternate"), format!("{id}-keep")];
    for path in [RUN, APPROVED] {
        for name in &names {
            assert_eq!(raw(path, name), Value::Null, "test value collision");
        }
    }
    let _cleanup = Cleanup(names.clone());
    let exe = std::env::var("SNOWIFY_PARITY_ELECTRON").unwrap();
    let quoted = r#""C:\Program Files\Snowify\Snowify.exe""#;
    let args = json!(["--flag=x", "plain", "\"two words\"", "--", "/kept", "--"]);
    let pre = |key: &str, name: u32, ty: u32, data: Value| json!({"key":key,"name":name,"type":ty,"data":data});
    let mut cases = vec![
        json!({"sets":[{"openAtLogin":true}],"gets":[{}]}),
        json!({"sets":[{"openAtLogin":true,"enabled":false}],"gets":[{}]}),
        json!({"sets":[{"openAtLogin":true,"enabled":false},{"openAtLogin":true}],"gets":[{}]}),
        json!({"sets":[{"openAtLogin":true},{"openAtLogin":false}],"gets":[{}]}),
        json!({"sets":[{"openAtLogin":false}],"gets":[{}]}),
        json!({"sets":[{"openAtLogin":true,"path":quoted,"args":args}],"gets":[{"path":quoted,"args":args},{"path":quoted},{"path":r#""c:\program files\snowify\snowify.exe""#,"args":args}]}),
        json!({"sets":[{"openAtLogin":true,"path":r"C:\Program Files\Snowify\Snowify.exe","args":["a b","", "tail"]}],"gets":[{"path":r"C:\Program Files\Snowify\Snowify.exe","args":["a b","", "tail"]}]}),
        json!({"sets":[{"openAtLogin":true,"path":quoted,"args":["--single-argument","raw \"quoted\" spaces"]}],"gets":[{"path":quoted}]}),
        json!({"sets":[{"openAtLogin":true,"name":names[1]}],"gets":[{}, {"name":names[1]}]}),
        json!({"sets":[{"openAtLogin":true,"name":names[1],"enabled":false},{"openAtLogin":true}],"gets":[{}]}),
        json!({"sets":[{"openAtLogin":true},{"openAtLogin":true,"name":names[1]},{"openAtLogin":false,"name":names[1]}],"gets":[{}]}),
        json!({"sets":[{"openAtLogin":true,"args":["", "x", ""]}],"gets":[{"args":["", "x", ""]},{}]}),
    ];
    // Seeded StartupApproved data pins Electron's NUL-terminated-byte comparison quirk.
    for data in [
        json!([0, 99, 0, 0]),
        json!([2, 0, 99, 0]),
        json!([2, 9, 0, 0]),
        json!([3, 0, 0, 0]),
    ] {
        cases.push(json!({"seed":[pre("run",0,REG_SZ,json!(exe)),pre("approved",0,REG_BINARY,data)],"sets":[],"gets":[{}]}));
    }
    cases.push(json!({"seed":[pre("run",2,REG_SZ,json!(exe)),pre("approved",2,REG_BINARY,json!([3,0,0,0]))],"sets":[{"openAtLogin":true},{"openAtLogin":false}],"gets":[{}]}));
    let dir = std::env::temp_dir().join(&id);
    std::fs::create_dir_all(&dir).unwrap();
    let mut diff = Diff::default();
    for (i, case) in cases.iter_mut().enumerate() {
        if case.get("seed").is_none() {
            case["seed"] = json!([]);
        }
        seed(&names, &case["seed"]);
        let source = electron(case, &id, &dir);
        let source_registry = snapshot(&names);
        // Inject the oracle's actual execPath, including any loader normalization.
        // The executable beneath <tmp> differs between the two runtimes by contract.
        let items = LoginItems::new(id.clone(), source["exe"].as_str().unwrap().into());
        seed(&names, &case["seed"]);
        for set in case["sets"].as_array().unwrap() {
            items.set(&settings(set));
        }
        let rust = case["gets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|get| items.get(&settings(get)))
            .collect::<Vec<_>>();
        diff.check(
            format!("case {i} getter"),
            &source["settings"],
            &json!(rust),
        );
        diff.check(
            format!("case {i} registry type/bytes"),
            &source_registry,
            &snapshot(&names),
        );
    }
    clear(&names);
    let _ = std::fs::remove_dir_all(dir);
    diff.finish("Windows login items");
}
