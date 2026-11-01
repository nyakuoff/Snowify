//! Differential test: `src/main/themes.js` (unmodified, mocked Electron/https) vs
//! `snowify::themes`, scenario by scenario on identical trees.

use serde_json::{Value, json};
use snowify::{ipc::JsValue, themes};

mod common;
#[path = "common/host.rs"]
mod host;
#[path = "common/tree.rs"]
mod tree;
use common::{Diff, root};
use host::MockHost;
use tree::{Entry, dir, file, link, raw_file};

const RAW: &str = "https://raw.githubusercontent.com";

fn w(v: Value) -> Value {
    JsValue::from(&v).encode()
}

fn undef() -> Value {
    json!({ "$": "u" })
}

struct Scenario {
    name: &'static str,
    tree: Vec<Entry>,
    contents: Vec<(&'static str, &'static str)>,
    packaged: bool,
    dialogs: Value,
    http: Value,
    ops: Vec<Value>,
}

fn op(ch: &str, args: Vec<Value>) -> Value {
    json!([ch, args])
}

fn scenarios(ud: &dyn Fn(&str) -> String) -> Vec<Scenario> {
    let css = |name: &'static str, body: &'static str| (name, body);
    vec![
        Scenario {
            name: "empty userData",
            tree: vec![],
            contents: vec![],
            packaged: true,
            dialogs: json!([]),
            http: json!({}),
            ops: vec![
                op("theme:scan", vec![]),
                op("theme:load", vec![w(json!("x.css"))]),
                op("themes:getInstalled", vec![]),
                op("theme:openFolder", vec![]),
            ],
        },
        Scenario {
            name: "scan and load",
            tree: vec![
                dir("ud/themes/x.css"),
                file("ud/themes/locked.css").mode(0o000),
                raw_file(b"ud/themes/\xffbad.css"),
                file("ud/themes/upper.CSS"),
            ],
            contents: vec![
                css("ud/themes/a.css", "/* @name   Midnight Blue  */ body{}"),
                css("ud/themes/b-theme_x.css", "body{}"),
                css("ud/themes/Z.css", "/*@NAME Shouty*/"),
                css("ud/themes/ä-umlaut.css", "/* @name\u{FEFF}Bom\u{FEFF}*/"),
                css("ud/themes/edge.css", "/* @name    */"),
                css(
                    "ud/themes/multi.css",
                    "/* @name first\nline */ /* @name second */",
                ),
                css("ud/themes/nel.css", "/* @name\u{85}x */"),
                css("ud/themes/emoji_🎵-x.css", "x"),
                css("ud/themes/_sources.json", "{}"),
            ],
            packaged: true,
            dialogs: json!([]),
            http: json!({}),
            ops: vec![
                op("theme:scan", vec![]),
                op("theme:load", vec![w(json!("a.css"))]),
                op("theme:load", vec![w(json!("../../etc/a.css"))]),
                op("theme:load", vec![w(json!("x.css"))]),
                op("theme:load", vec![w(json!("missing.css"))]),
                op("theme:load", vec![w(json!(5))]),
                op("theme:load", vec![w(json!(null))]),
                op("theme:load", vec![undef()]),
                op("theme:load", vec![w(json!({ "a": 1 }))]),
                op("theme:load", vec![w(json!([1]))]),
                op("theme:load", vec![w(json!(true))]),
                op("theme:load", vec![json!({ "$": "n", "v": "-0" })]),
            ],
        },
        Scenario {
            name: "add, reload, remove",
            tree: vec![dir("ext/dir.css"), dir("ud/themes")],
            contents: vec![
                css("ext/one.css", "/* @name One */"),
                css("ext/two-two.css", "two"),
            ],
            packaged: true,
            dialogs: json!([
                { "canceled": false, "filePaths": [ud("ext/one.css"), ud("ext/missing.css"), ud("ext/dir.css"), ud("ext/two-two.css")] },
                { "canceled": true, "filePaths": [] },
                { "canceled": false, "filePaths": [] },
                { "canceled": false, "filePaths": [ud("ext/missing.css")] },
            ]),
            http: json!({}),
            ops: vec![
                op("theme:add", vec![]),
                op("theme:add", vec![]),
                op("theme:add", vec![]),
                op("theme:add", vec![]),
                op("theme:reload", vec![w(json!("one.css"))]),
                op("theme:reload", vec![w(json!("nope.css"))]),
                op("theme:reload", vec![w(json!(7))]),
                op("theme:remove", vec![w(json!("one.css"))]),
                op("theme:remove", vec![w(json!("toString"))]),
                op("theme:remove", vec![w(json!("missing.css"))]),
                op("theme:remove", vec![w(json!(null))]),
                op("theme:scan", vec![]),
            ],
        },
        Scenario {
            name: "odd _sources.json shapes",
            tree: vec![dir("ud/themes/sub")],
            contents: vec![
                css("ext/one.css", "x"),
                css("ud/themes/_sources.json", "[1, 2]"),
            ],
            packaged: true,
            dialogs: json!([{ "canceled": false, "filePaths": [ud("ext/one.css")] }]),
            http: json!({}),
            ops: vec![
                op("theme:add", vec![]),
                op("theme:remove", vec![w(json!("one.css"))]),
                op("theme:remove", vec![w(json!("0"))]),
            ],
        },
        Scenario {
            name: "null _sources.json",
            tree: vec![],
            contents: vec![
                css("ext/one.css", "x"),
                css("ud/themes/_sources.json", "null"),
            ],
            packaged: true,
            dialogs: json!([{ "canceled": false, "filePaths": [ud("ext/one.css")] }]),
            http: json!({}),
            ops: vec![
                op("theme:add", vec![]),
                op("theme:reload", vec![w(json!("one.css"))]),
                op("theme:remove", vec![w(json!("one.css"))]),
            ],
        },
        Scenario {
            name: "sources pointing elsewhere",
            tree: vec![],
            contents: vec![
                css("ext/live.css", "/* @name Live */ v2"),
                css("ud/themes/live.css", "v1"),
                css("ud/themes/num.css", "n"),
                css(
                    "ud/themes/_sources.json",
                    "{\"live.css\": \"__EXT__\", \"num.css\": 5, \"gone.css\": \"/nonexistent/x.css\"}",
                ),
            ],
            packaged: true,
            dialogs: json!([]),
            http: json!({}),
            ops: vec![
                op("theme:reload", vec![w(json!("live.css"))]),
                op("theme:reload", vec![w(json!("num.css"))]),
                op("theme:reload", vec![w(json!("gone.css"))]),
                op("theme:remove", vec![w(json!("num.css"))]),
            ],
        },
        Scenario {
            name: "marketplace install",
            tree: vec![],
            contents: vec![],
            packaged: true,
            dialogs: json!([]),
            http: json!({
                format!("{RAW}/me/repo/main/theme.css"): [{ "status": 200, "body": "/* @name Market */ ü" }],
                format!("{RAW}/me/repo/dev/sub/dir/custom.css"): [{ "status": 200, "body": "custom" }],
                format!("{RAW}/me/redir/main/theme.css"): [{ "status": 302, "headers": { "location": format!("{RAW}/me/repo/main/theme.css") } }],
                format!("{RAW}/me/loop/main/theme.css"): [{ "status": 301, "headers": { "location": format!("{RAW}/me/loop/main/theme.css") } }],
                format!("{RAW}/me/slow/main/theme.css"): [{ "timeout": true }],
                format!("{RAW}/me/dns/main/theme.css"): [{ "error": { "name": "Error", "message": "getaddrinfo ENOTFOUND raw.githubusercontent.com", "props": { "errno": -3008, "code": "ENOTFOUND", "syscall": "getaddrinfo", "hostname": "raw.githubusercontent.com" } } }],
                format!("{RAW}/me/nolocation/main/theme.css"): [{ "status": 302, "headers": {} }],
                format!("{RAW}/me/badredirect/main/theme.css"): [{ "status": 302, "headers": { "location": "http://plain.example/x" } }],
            }),
            ops: vec![
                op(
                    "themes:install",
                    vec![w(
                        json!({ "repo": "me/repo", "id": "market", "name": "Market", "version": "2.1.0", "author": "Me" }),
                    )],
                ),
                op(
                    "themes:install",
                    vec![w(
                        json!({ "repo": "me/repo", "id": "custom", "branch": "dev", "path": "sub/dir", "file": "custom.css", "official": true }),
                    )],
                ),
                op(
                    "themes:install",
                    vec![w(json!({ "repo": "me/redir", "id": "redir" }))],
                ),
                op(
                    "themes:install",
                    vec![w(json!({ "repo": "me/loop", "id": "loop" }))],
                ),
                op(
                    "themes:install",
                    vec![w(json!({ "repo": "me/slow", "id": "slow" }))],
                ),
                op(
                    "themes:install",
                    vec![w(json!({ "repo": "me/dns", "id": "dns" }))],
                ),
                op(
                    "themes:install",
                    vec![w(json!({ "repo": "me/nolocation", "id": "nl" }))],
                ),
                op(
                    "themes:install",
                    vec![w(json!({ "repo": "me/missing", "id": "missing" }))],
                ),
                op(
                    "themes:install",
                    vec![w(json!({ "repo": "me/badredirect", "id": "badredirect" }))],
                ),
                op(
                    "themes:install",
                    vec![w(json!({ "repo": "me/repo", "id": 5 }))],
                ),
                op(
                    "themes:install",
                    vec![w(json!({ "repo": "me/repo", "id": "f", "file": 7 }))],
                ),
                op(
                    "themes:install",
                    vec![w(json!({ "repo": "bad url", "id": "space" }))],
                ),
                op("themes:install", vec![w(json!(null))]),
                op("themes:install", vec![undef()]),
                op("themes:install", vec![w(json!(5))]),
                op("themes:install", vec![w(json!({}))]),
                op("themes:getInstalled", vec![]),
                op("themes:uninstallMarketplace", vec![w(json!("market"))]),
                op("themes:uninstallMarketplace", vec![w(json!("nope"))]),
                op("themes:uninstallMarketplace", vec![w(json!("toString"))]),
                op("themes:uninstallMarketplace", vec![undef()]),
                op("themes:getInstalled", vec![]),
                op("theme:scan", vec![]),
            ],
        },
        Scenario {
            name: "dev install reads repo plugins/themes",
            tree: vec![],
            contents: vec![],
            packaged: false,
            dialogs: json!([]),
            http: json!({ format!("{RAW}/me/repo/main/theme.css"): [{ "status": 200, "body": "remote" }] }),
            ops: vec![
                op(
                    "themes:install",
                    vec![w(json!({ "repo": "me/repo", "id": "not-local" }))],
                ),
                op(
                    "themes:install",
                    vec![w(json!({ "repo": "me/repo", "id": "f", "file": 7 }))],
                ),
            ],
        },
        Scenario {
            name: "odd _marketplace.json shapes",
            tree: vec![],
            contents: vec![
                css(
                    "ud/themes/_marketplace.json",
                    "{\"a\": {\"filename\": 3}, \"b\": \"str\", \"c\": {\"filename\": \"../c.css\"}}",
                ),
                css("ud/c.css", "c"),
            ],
            packaged: true,
            dialogs: json!([]),
            http: json!({ format!("{RAW}/me/repo/main/theme.css"): [{ "status": 200, "body": "x" }] }),
            ops: vec![
                op("themes:uninstallMarketplace", vec![w(json!("a"))]),
                op("themes:uninstallMarketplace", vec![w(json!("b"))]),
                op("themes:uninstallMarketplace", vec![w(json!("c"))]),
                op(
                    "themes:install",
                    vec![w(json!({ "repo": "me/repo", "id": "new" }))],
                ),
            ],
        },
        Scenario {
            name: "null _marketplace.json",
            tree: vec![],
            contents: vec![css("ud/themes/_marketplace.json", "null")],
            packaged: true,
            dialogs: json!([]),
            http: json!({ format!("{RAW}/me/repo/main/theme.css"): [{ "status": 200, "body": "x" }] }),
            ops: vec![
                op(
                    "themes:install",
                    vec![w(json!({ "repo": "me/repo", "id": "new" }))],
                ),
                op("themes:uninstallMarketplace", vec![w(json!("new"))]),
                op("themes:getInstalled", vec![]),
            ],
        },
        Scenario {
            name: "userData is a file",
            tree: vec![file("ud")],
            contents: vec![css("ext/one.css", "x")],
            packaged: true,
            dialogs: json!([{ "canceled": false, "filePaths": [ud("ext/one.css")] }]),
            http: json!({ format!("{RAW}/me/repo/main/theme.css"): [{ "status": 200, "body": "x" }] }),
            ops: vec![
                op("theme:scan", vec![]),
                op("theme:load", vec![w(json!("a.css"))]),
                op("theme:add", vec![]),
                op("theme:reload", vec![w(json!("a.css"))]),
                op("theme:remove", vec![w(json!("a.css"))]),
                op("theme:openFolder", vec![]),
                op("themes:getInstalled", vec![]),
                op(
                    "themes:install",
                    vec![w(json!({ "repo": "me/repo", "id": "new" }))],
                ),
                op("themes:uninstallMarketplace", vec![w(json!("new"))]),
            ],
        },
        Scenario {
            name: "themes is a symlink to a directory",
            tree: vec![dir("real"), link("ud/themes", "{root}/real")],
            contents: vec![css("real/s.css", "/* @name Linked */")],
            packaged: true,
            dialogs: json!([]),
            http: json!({}),
            ops: vec![
                op("theme:scan", vec![]),
                op("theme:remove", vec![w(json!("s.css"))]),
            ],
        },
    ]
}

fn build(root: &std::path::Path, s: &Scenario) {
    tree::build(root, &s.tree);
    for (p, body) in &s.contents {
        let full = root.join(p);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(
            &full,
            body.replace("__EXT__", &root.join("ext/live.css").display().to_string()),
        )
        .unwrap();
    }
    // Modes from the tree spec apply after contents exist.
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
            "theme:scan" => themes::scan(h).await,
            "theme:load" => themes::load(h, &a(0)).await,
            "theme:add" => themes::add(h).await,
            "theme:reload" => themes::reload(h, &a(0)).await,
            "theme:remove" => themes::remove(h, &a(0)).await,
            "theme:openFolder" => themes::open_folder(h).await,
            "themes:getInstalled" => themes::get_installed(h).await,
            "themes:install" => themes::install(h, &a(0)).await,
            "themes:uninstallMarketplace" => themes::uninstall_marketplace(h, &a(0)).await,
            _ => return None,
        })
    })
}

#[tokio::test(flavor = "current_thread")]
async fn themes_match_source() {
    let base = std::env::temp_dir().join(format!("snowify-parity-themes-{}", std::process::id()));
    let repo_root = root()
        .join("..")
        .canonicalize()
        .unwrap()
        .display()
        .to_string();
    let mut d = Diff::default();
    let probe_root = base.join("case");
    let ud = |rel: &str| format!("{}/{rel}", probe_root.display());
    for s in scenarios(&ud) {
        let scenario = json!({
            "module": "themes", "userData": ud("ud"), "isPackaged": s.packaged,
            "dialogs": s.dialogs, "http": s.http, "ops": s.ops,
        });
        build(&probe_root, &s);
        let src = common::run_source("main_module.js", &scenario);
        let src_tree = tree::snapshot(&probe_root);

        build(&probe_root, &s);
        let host = MockHost::new(&scenario, &repo_root);
        let rust = host::run_ops(&host, &scenario, &dispatch).await;
        let rust_tree = tree::snapshot(&probe_root);

        for key in [
            "results",
            "logs",
            "dialogCalls",
            "openPathCalls",
            "httpCalls",
        ] {
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
    d.finish("themes");
}
