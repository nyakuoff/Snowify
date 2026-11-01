//! Differential test: `src/main/plugins.js` (unmodified, mocked Electron/https) vs
//! `snowify::plugins`, scenario by scenario on identical trees. Dev-mode scenarios read the
//! real repository's `plugins/` directory, read-only, on both sides.

use serde_json::{Value, json};
use snowify::{ipc::JsValue, plugins};

mod common;
#[path = "common/host.rs"]
mod host;
#[path = "common/tree.rs"]
mod tree;
use common::{Diff, root};
use host::MockHost;
use tree::{Entry, dir, file, link};

const RAW: &str = "https://raw.githubusercontent.com";
const REG: &str = "https://raw.githubusercontent.com/nyakuoff/Snowify/main/plugins/registry.json";

fn w(v: Value) -> Value {
    JsValue::from(&v).encode()
}

fn op(ch: &str, args: Vec<Value>) -> Value {
    json!([ch, args])
}

struct Scenario {
    name: &'static str,
    tree: Vec<Entry>,
    contents: Vec<(&'static str, Vec<u8>)>,
    packaged: bool,
    http: Value,
    ops: Vec<Value>,
}

fn txt(p: &'static str, s: &str) -> (&'static str, Vec<u8>) {
    (p, s.as_bytes().to_vec())
}

const PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

fn scenarios() -> Vec<Scenario> {
    let manifest = |id: &str| {
        json!({ "id": id, "name": "P", "renderer": "main.js", "styles": "css/style.css", "logoUrl": "logo.png" }).to_string()
    };
    vec![
        Scenario {
            name: "registry: remote",
            tree: vec![],
            contents: vec![],
            packaged: true,
            http: json!({ REG: [{ "status": 200, "body": json!({
                "version": 1,
                "plugins": [
                    { "id": "a", "repo": "me/a", "logoUrl": "logo.png" },
                    { "id": "b", "repo": "me/b", "branch": "dev", "path": "sub", "logoUrl": "img/l.svg" },
                    { "id": "c", "logoUrl": "https://x/l.png" },
                    { "id": "d", "logoUrl": "data:image/png;base64,AA" },
                    { "id": "e" },
                ],
                "themes": [{ "id": "t" }],
            }).to_string() }] }),
            ops: vec![op("plugins:getRegistry", vec![])],
        },
        Scenario {
            name: "registry: remote failures",
            tree: vec![],
            contents: vec![],
            packaged: true,
            http: json!({ REG: [
                { "status": 200, "body": "<!DOCTYPE html><html><body>rate limited</body></html>" },
                { "status": 500, "body": "" },
                { "status": 200, "body": "{\"plugins\": {\"not\": \"array\"}}" },
                { "status": 200, "body": "{\"plugins\": [null]}" },
                { "status": 200, "body": "{\"plugins\": [{\"logoUrl\": 5}]}" },
                { "status": 200, "body": "null" },
                { "status": 200, "body": "7" },
                { "timeout": true },
            ] }),
            ops: (0..8).map(|_| op("plugins:getRegistry", vec![])).collect(),
        },
        Scenario {
            name: "registry: dev reads the repo",
            tree: vec![],
            contents: vec![],
            packaged: false,
            http: json!({}),
            ops: vec![op("plugins:getRegistry", vec![])],
        },
        Scenario {
            name: "install from remote, then files, then uninstall",
            tree: vec![],
            contents: vec![],
            packaged: true,
            http: json!({
                format!("{RAW}/me/repo/main/sub/snowify-plugin.json"): [{ "status": 200, "body": manifest("cool.plugin") }],
                format!("{RAW}/me/repo/main/sub/main.js"): [{ "status": 200, "body": "console.log('ü');" }],
                format!("{RAW}/me/repo/main/sub/css/style.css"): [{ "status": 200, "body": "a{}" }],
                format!("{RAW}/me/repo/main/sub/logo.png"): [{ "status": 200, "b64": PNG_B64 }],
            }),
            ops: vec![
                op(
                    "plugins:install",
                    vec![w(
                        json!({ "id": "cool.plugin", "repo": "me/repo", "path": "sub" }),
                    )],
                ),
                op("plugins:getInstalled", vec![]),
                op("plugins:getFiles", vec![w(json!("cool.plugin"))]),
                op("plugins:getFiles", vec![w(json!("missing"))]),
                op("plugins:getFiles", vec![w(json!("../x"))]),
                op("plugins:uninstall", vec![w(json!("cool.plugin"))]),
                op("plugins:uninstall", vec![w(json!("cool.plugin"))]),
                op("plugins:uninstall", vec![w(json!("bad id!"))]),
                op("plugins:uninstall", vec![w(json!(null))]),
                op("plugins:getInstalled", vec![]),
            ],
        },
        Scenario {
            name: "install: odd entries and manifests",
            tree: vec![],
            contents: vec![],
            packaged: true,
            http: json!({
                format!("{RAW}/me/html/main/snowify-plugin.json"): [{ "status": 200, "body": "<html>not json</html>" }],
                format!("{RAW}/me/nullm/main/snowify-plugin.json"): [{ "status": 200, "body": "null" }],
                format!("{RAW}/me/badid/main/snowify-plugin.json"): [{ "status": 200, "body": "{\"id\": \"no/slash\"}" }],
                format!("{RAW}/me/escape/main/snowify-plugin.json"): [{ "status": 200, "body": "{\"id\": \"esc\", \"renderer\": \"../../evil.js\"}" }],
                format!("{RAW}/me/abs/main/snowify-plugin.json"): [{ "status": 200, "body": "{\"id\": \"abs\", \"renderer\": \"/etc/passwd\"}" }],
                format!("{RAW}/me/numlogo/main/snowify-plugin.json"): [{ "status": 200, "body": "{\"id\": \"numlogo\", \"logoUrl\": 5}" }],
                format!("{RAW}/me/norenderer/main/snowify-plugin.json"): [{ "status": 200, "body": "{\"id\": \"nr\", \"styles\": \"s.css\", \"logoUrl\": \"l.png\"}" }],
                format!("{RAW}/me/rend404/main/snowify-plugin.json"): [{ "status": 200, "body": "{\"id\": \"r404\", \"renderer\": \"x/y z.js\"}" }],
                format!("{RAW}/me/fallback/main/snowify-plugin.json"): [{ "status": 200, "body": "{\"name\": \"no id\"}" }],
            }),
            ops: vec![
                op(
                    "plugins:install",
                    vec![w(json!({ "id": "html", "repo": "me/html" }))],
                ),
                op(
                    "plugins:install",
                    vec![w(json!({ "id": "nullm", "repo": "me/nullm" }))],
                ),
                op(
                    "plugins:install",
                    vec![w(json!({ "id": "badid", "repo": "me/badid" }))],
                ),
                op(
                    "plugins:install",
                    vec![w(json!({ "id": "escape", "repo": "me/escape" }))],
                ),
                op(
                    "plugins:install",
                    vec![w(json!({ "id": "abs", "repo": "me/abs" }))],
                ),
                op(
                    "plugins:install",
                    vec![w(json!({ "id": "numlogo", "repo": "me/numlogo" }))],
                ),
                op(
                    "plugins:install",
                    vec![w(json!({ "id": "norenderer", "repo": "me/norenderer" }))],
                ),
                op(
                    "plugins:install",
                    vec![w(json!({ "id": "rend404", "repo": "me/rend404" }))],
                ),
                op(
                    "plugins:install",
                    vec![w(json!({ "id": "fallback", "repo": "me/fallback" }))],
                ),
                op(
                    "plugins:install",
                    vec![w(json!({ "id": "../up", "repo": "me/x" }))],
                ),
                op(
                    "plugins:install",
                    vec![w(json!({ "id": "  spaced  ", "repo": "me/x" }))],
                ),
                op(
                    "plugins:install",
                    vec![w(json!({ "id": 42, "repo": "me/x" }))],
                ),
                op("plugins:install", vec![w(json!(null))]),
                op("plugins:install", vec![json!({ "$": "u" })]),
                op("plugins:install", vec![w(json!("string entry"))]),
                op("plugins:getInstalled", vec![]),
            ],
        },
        Scenario {
            name: "install from the repo in dev",
            tree: vec![],
            contents: vec![],
            packaged: false,
            http: json!({}),
            ops: vec![
                op(
                    "plugins:install",
                    vec![w(
                        json!({ "id": "spotify-metadata", "repo": "nyakuoff/Snowify" }),
                    )],
                ),
                op(
                    "plugins:install",
                    vec![w(
                        json!({ "id": "soundcloud-source", "repo": "nyakuoff/Snowify" }),
                    )],
                ),
                op("plugins:getFiles", vec![w(json!("spotify-metadata"))]),
                op("plugins:getInstalled", vec![]),
            ],
        },
        Scenario {
            name: "installed scan: odd entries",
            tree: vec![
                dir("ud/plugins/b-no-manifest"),
                file("ud/plugins/a-file"),
                link("ud/plugins/c-broken", "{root}/nowhere"),
                dir("ud/plugins/d"),
            ],
            contents: vec![
                txt(
                    "ud/plugins/aa/snowify-plugin.json",
                    "{\"id\": \"aa\", \"logoUrl\": \"logo.png\"}",
                ),
                (
                    "ud/plugins/aa/logo.png",
                    vec![0x89, b'P', b'N', b'G', 0, 1, 2, 255],
                ),
                txt(
                    "ud/plugins/ab/snowify-plugin.json",
                    "{\"id\": \"ab\", \"logoUrl\": \"pic.JPG\"}",
                ),
                txt("ud/plugins/ab/pic.JPG", "jpg"),
                txt("ud/plugins/ac/snowify-plugin.json", "not json"),
                txt("ud/plugins/ad/snowify-plugin.json", "\"just a string\""),
                txt("ud/plugins/ae/snowify-plugin.json", "{\"logoUrl\": [1]}"),
                txt("ud/plugins/d/snowify-plugin.json", "{\"id\": \"d\"}"),
            ],
            packaged: true,
            http: json!({}),
            ops: vec![
                op("plugins:getInstalled", vec![]),
                op("plugins:getFiles", vec![w(json!("ac"))]),
                op("plugins:getFiles", vec![w(json!("aa"))]),
            ],
        },
        Scenario {
            name: "userData is a file",
            tree: vec![file("ud")],
            contents: vec![],
            packaged: true,
            http: json!({ format!("{RAW}/me/x/main/snowify-plugin.json"): [{ "status": 200, "body": "{\"id\": \"x\"}" }] }),
            ops: vec![
                op("plugins:getInstalled", vec![]),
                op(
                    "plugins:install",
                    vec![w(json!({ "id": "x", "repo": "me/x" }))],
                ),
                op("plugins:uninstall", vec![w(json!("x"))]),
                op("plugins:getFiles", vec![w(json!("x"))]),
            ],
        },
    ]
}

fn build(root: &std::path::Path, s: &Scenario) {
    tree::build(root, &s.tree);
    for (p, body) in &s.contents {
        let full = root.join(p);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, body).unwrap();
    }
    tree::build_modes(root, &s.tree);
}

fn dispatch<'a>(
    h: &'a MockHost,
    ch: String,
    args: Vec<JsValue>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<snowify::ipc::HandlerResult>> + 'a>>
{
    Box::pin(async move {
        let a = |i: usize| args.get(i).cloned().unwrap_or(JsValue::Undefined);
        Some(match ch.as_str() {
            "plugins:getRegistry" => plugins::get_registry(h).await,
            "plugins:getInstalled" => plugins::get_installed(h).await,
            "plugins:install" => plugins::install(h, &a(0)).await,
            "plugins:uninstall" => plugins::uninstall(h, &a(0)).await,
            "plugins:getFiles" => plugins::get_files(h, &a(0)).await,
            _ => return None,
        })
    })
}

#[tokio::test(flavor = "current_thread")]
async fn plugins_match_source() {
    let base = std::env::temp_dir().join(format!("snowify-parity-plugins-{}", std::process::id()));
    let repo_root = root()
        .join("..")
        .canonicalize()
        .unwrap()
        .display()
        .to_string();
    let probe_root = base.join("case");
    let ud = format!("{}/ud", probe_root.display());
    let mut d = Diff::default();
    for s in scenarios() {
        let scenario = json!({
            "module": "plugins", "userData": ud, "isPackaged": s.packaged,
            "dialogs": [], "http": s.http, "ops": s.ops,
        });
        build(&probe_root, &s);
        let src = common::run_source("main_module.js", &scenario);
        let src_tree = tree::snapshot(&probe_root);

        build(&probe_root, &s);
        let host = MockHost::new(&scenario, &repo_root);
        let rust = host::run_ops(&host, &scenario, &dispatch).await;
        let rust_tree = tree::snapshot(&probe_root);

        for key in ["results", "logs", "httpCalls"] {
            let (a, b) = (&src[key], &rust[key]);
            let n = a
                .as_array()
                .map_or(0, Vec::len)
                .max(b.as_array().map_or(0, Vec::len));
            for i in 0..n {
                d.check(
                    format_args!("{} {key}[{i}]", s.name),
                    a.get(i).unwrap_or(&Value::Null),
                    b.get(i).unwrap_or(&Value::Null),
                );
            }
        }
        d.check(
            format_args!("{} tree", s.name),
            &json!(src_tree),
            &json!(rust_tree),
        );
    }
    tree::remove(&base);
    d.finish("plugins");
}
