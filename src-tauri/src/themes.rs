//! Port of `src/main/themes.js`: custom CSS themes in `<userData>/themes` and marketplace
//! themes (`_marketplace.json`), plus `_sources.json` mapping theme files to where they were
//! added from.

use crate::{
    host::{Host, OpenDialog},
    ipc::{self, HandlerResult, JsValue, Lookup, Thrown, to_js_string},
    logger::{self, LogArg},
    net::{self, Flavor, JsErr},
    node::{self, path::posix as path, path::posix_more as path2},
};
use regex::Regex;
use std::sync::LazyLock;

/// `console.error(label, err)` with Node's `JSON.stringify(err)` rendering.
fn log_error(label: &str, err: &JsErr) {
    logger::error(&[label.into(), LogArg::from_js(&err.log_json())]);
}

fn type_error(message: impl Into<crate::ipc::JsString>) -> JsErr {
    JsErr {
        name: "TypeError".into(),
        message: message.into(),
        props: Vec::new(),
    }
}

impl From<Thrown> for JsErr {
    fn from(t: Thrown) -> Self {
        match t {
            Thrown::Error { name, message } => JsErr {
                name,
                message,
                props: Vec::new(),
            },
            Thrown::Value(v) => JsErr::error(to_js_string(&v)),
        }
    }
}

impl From<JsErr> for Thrown {
    fn from(e: JsErr) -> Self {
        Thrown::Error {
            name: e.name,
            message: e.message,
        }
    }
}

/// Node's `ERR_INVALID_ARG_TYPE` for a non-string `path` argument.
pub fn invalid_path_arg(name: &str, v: &JsValue) -> JsErr {
    invalid_arg_type(name, "of type string", v)
}

/// Node's `ERR_INVALID_ARG_TYPE`: `The "<name>" argument must be <expected>. Received <…>`.
pub fn invalid_arg_type(name: &str, expected: &str, v: &JsValue) -> JsErr {
    let received = match v {
        JsValue::Undefined => "undefined".into(),
        JsValue::Null => "null".into(),
        JsValue::Number(n) => {
            let shown = if *n == 0.0 && n.is_sign_negative() { "-0".into() } else { to_js_string(v) };
            format!("type number ({shown})")
        }
        JsValue::Bool(b) => format!("type boolean ({b})"),
        JsValue::BigInt(b) => format!("type bigint ({b}n)"),
        JsValue::String(s) => {
            let mut shown = crate::js::inspect_string(&s.0);
            if shown.len() > 25 {
                shown.truncate(25);
                shown.extend("...".encode_utf16());
            }
            format!("type string ({})", String::from_utf16_lossy(&shown))
        }
        JsValue::Array(_) => "an instance of Array".into(),
        JsValue::Date(_) => "an instance of Date".into(),
        JsValue::RegExp { .. } => "an instance of RegExp".into(),
        JsValue::Map(_) => "an instance of Map".into(),
        JsValue::Set(_) => "an instance of Set".into(),
        JsValue::ArrayBuffer(_) => "an instance of ArrayBuffer".into(),
        JsValue::View { kind, .. } => format!("an instance of {kind}"),
        JsValue::Error { .. } => "an instance of Error".into(),
        JsValue::Boxed(b) => match **b {
            JsValue::Number(_) => "an instance of Number".into(),
            JsValue::Bool(_) => "an instance of Boolean".into(),
            _ => "an instance of String".into(),
        },
        JsValue::Shared(_, inner) => return invalid_arg_type(name, expected, inner),
        _ => "an instance of Object".into(),
    };
    JsErr {
        // Node's internal errors stringify as `TypeError [CODE]: message`.
        name: "TypeError [ERR_INVALID_ARG_TYPE]".into(),
        message: format!("The \"{name}\" argument must be {expected}. Received {received}").into(),
        props: vec![("code".into(), "ERR_INVALID_ARG_TYPE".into())],
    }
}

/// A `path` argument: a string, or Node's `ERR_INVALID_ARG_TYPE`.
pub fn path_arg(v: &JsValue) -> Result<String, JsErr> {
    match v {
        JsValue::String(s) => Ok(s.to_string_lossy()),
        JsValue::Shared(_, inner) => path_arg(inner),
        v => Err(invalid_path_arg("path", v)),
    }
}

/// `getThemesDir()`.
fn themes_dir<H: Host>(host: &H) -> Result<String, JsErr> {
    let dir = path::join(&[&host.user_data(), "themes"]);
    if !node::exists_sync(&dir) {
        node::mkdir_recursive_sync(&dir)?;
    }
    Ok(dir)
}

fn sources_path<H: Host>(host: &H) -> Result<String, JsErr> {
    Ok(path::join(&[&themes_dir(host)?, "_sources.json"]))
}

/// `readThemeSources()`: the parsed JSON, whatever its shape, or `{}` on any failure.
fn read_sources<H: Host>(host: &H) -> JsValue {
    (|| -> Option<JsValue> {
        let p = sources_path(host).ok()?;
        if !node::exists_sync(&p) {
            return None;
        }
        ipc::json_parse(&node::read_file_utf8(&p).ok()?).ok()
    })()
    .unwrap_or(JsValue::Object(Vec::new()))
}

/// `JSON.stringify(v, null, 2)`, which writes nothing when `v` stringifies to `undefined`.
fn pretty(v: &JsValue) -> Result<String, JsErr> {
    match ipc::json_stringify_indent(v, "  ") {
        Ok(Some(s)) => Ok(s),
        // writeFileSync(path, undefined) throws ERR_INVALID_ARG_TYPE for `data`.
        Ok(None) => Err(type_error(
            "The \"data\" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received undefined",
        )),
        Err(_) => Err(type_error("Converting circular structure to JSON")),
    }
}

/// `writeThemeSources(map)`.
fn write_sources<H: Host>(host: &H, map: &JsValue) {
    let r = (|| -> Result<(), JsErr> {
        let text = pretty(map)?;
        let p = sources_path(host)?;
        node::write_file(&p, text.as_bytes())?;
        Ok(())
    })();
    if let Err(e) = r {
        log_error("Failed to write theme sources:", &e);
    }
}

static NAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    // JS `\s` and `.` (no `u` flag), spelled out: Rust's classes differ (U+0085, U+FEFF).
    const S: &str = r"[\t\n\x0B\x0C\r \x{00A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}]";
    Regex::new(&format!(
        r"/\*{S}*(?i:@name){S}+([^\n\r\x{{2028}}\x{{2029}}]+?){S}*\*/"
    ))
    .expect("valid regex")
});

/// JS `String.prototype.trim`.
fn js_trim(s: &str) -> &str {
    let ws = |c: char| {
        matches!(
            c,
            '\t' | '\n' | '\u{B}' | '\u{C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
                ..='\u{200A}'
                    | '\u{2028}'
                    | '\u{2029}'
                    | '\u{202F}'
                    | '\u{205F}'
                    | '\u{3000}'
                    | '\u{FEFF}'
        )
    };
    s.trim_matches(ws)
}

/// `f.replace(/\.css$/i, '')`.
fn strip_css(f: &str) -> &str {
    let n = f.len();
    if n >= 4 && f.is_char_boundary(n - 4) && f[n - 4..].eq_ignore_ascii_case(".css") {
        &f[..n - 4]
    } else {
        f
    }
}

/// `parseThemeName(css, filename)`.
pub fn parse_theme_name(css: &str, filename: &str) -> String {
    if let Some(m) = NAME_RE.captures(css) {
        return js_trim(&m[1]).to_owned();
    }
    // .replace(/[-_]/g, ' ').replace(/\b\w/g, c => c.toUpperCase())
    let spaced: String = strip_css(filename)
        .chars()
        .map(|c| if c == '-' || c == '_' { ' ' } else { c })
        .collect();
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out = String::with_capacity(spaced.len());
    let mut prev_word = false;
    for c in spaced.chars() {
        out.push(if word(c) && !prev_word {
            c.to_ascii_uppercase()
        } else {
            c
        });
        prev_word = word(c);
    }
    out
}

fn theme_entry(id: &str, name: &str) -> JsValue {
    JsValue::object([("id", JsValue::from(id)), ("name", JsValue::from(name))])
}

/// JS default `Array.prototype.sort` for strings: UTF-16 code unit order.
fn sort_utf16(v: &mut [String]) {
    v.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
}

pub async fn scan<H: Host>(host: &H) -> HandlerResult {
    let r = (|| -> Result<JsValue, JsErr> {
        let dir = themes_dir(host)?;
        let mut files: Vec<String> = node::readdir_sync(&dir)?
            .into_iter()
            .filter(|f| f.ends_with(".css"))
            .collect();
        sort_utf16(&mut files);
        Ok(JsValue::Array(
            files
                .iter()
                .map(|f| match node::read_file_utf8(&path::join(&[&dir, f])) {
                    Ok(css) => theme_entry(f, &parse_theme_name(&css, f)),
                    Err(_) => theme_entry(f, strip_css(f)),
                })
                .collect(),
        ))
    })();
    Ok(r.unwrap_or_else(|e| {
        log_error("Theme scan error:", &e);
        JsValue::Array(Vec::new())
    }))
}

pub async fn load<H: Host>(host: &H, id: &JsValue) -> HandlerResult {
    let r = (|| -> Result<JsValue, JsErr> {
        let dir = themes_dir(host)?;
        let p = path::join(&[&dir, &path2::basename(&path_arg(id)?)]);
        Ok(if node::exists_sync(&p) {
            node::read_file_utf8(&p)?.into()
        } else {
            JsValue::Null
        })
    })();
    Ok(r.unwrap_or_else(|e| {
        log_error("Theme load error:", &e);
        JsValue::Null
    }))
}

pub async fn add<H: Host>(host: &H) -> HandlerResult {
    let picked = host
        .show_open_dialog(OpenDialog {
            title: "Add custom theme (.css)".into(),
            filters: vec![("CSS Files".into(), vec!["css".into()])],
            multi: true,
            directory: false,
        })
        .await;
    let Some(paths) = picked.filter(|p| !p.is_empty()) else {
        return Ok(JsValue::Null);
    };
    let dir = themes_dir(host).map_err(Thrown::from)?;
    let mut sources = read_sources(host);
    let mut added = Vec::new();
    for src in &paths {
        let r = (|| -> Result<(), JsErr> {
            let filename = path2::basename(src);
            let dest = path::join(&[&dir, &filename]);
            node::copy_file_sync(src, &dest)?;
            ipc::set(&mut sources, &filename, src.as_str().into())?;
            let css = node::read_file_utf8(&dest)?;
            added.push(theme_entry(&filename, &parse_theme_name(&css, &filename)));
            Ok(())
        })();
        if let Err(e) = r {
            log_error("Theme add error:", &e);
        }
    }
    write_sources(host, &sources);
    Ok(if added.is_empty() {
        JsValue::Null
    } else {
        JsValue::Array(added)
    })
}

pub async fn reload<H: Host>(host: &H, id: &JsValue) -> HandlerResult {
    let r = (|| -> Result<JsValue, JsErr> {
        let filename = path2::basename(&path_arg(id)?);
        let dest = path::join(&[&themes_dir(host)?, &filename]);
        let sources = read_sources(host);
        // `if (srcPath && fs.existsSync(srcPath))`: existsSync is false for non-strings.
        if let Lookup::Own(JsValue::String(src)) = ipc::lookup(&sources, &filename)? {
            let src = src.to_string_lossy();
            if !src.is_empty() && node::exists_sync(&src) {
                node::copy_file_sync(&src, &dest)?;
            }
        }
        Ok(if node::exists_sync(&dest) {
            node::read_file_utf8(&dest)?.into()
        } else {
            JsValue::Null
        })
    })();
    Ok(r.unwrap_or_else(|e| {
        log_error("Theme reload error:", &e);
        JsValue::Null
    }))
}

pub async fn remove<H: Host>(host: &H, id: &JsValue) -> HandlerResult {
    let r = (|| -> Result<(), JsErr> {
        let filename = path2::basename(&path_arg(id)?);
        let p = path::join(&[&themes_dir(host)?, &filename]);
        if node::exists_sync(&p) {
            node::unlink_sync(&p)?;
        }
        let mut sources = read_sources(host);
        if ipc::lookup(&sources, &filename)?.truthy() {
            ipc::delete(&mut sources, &filename)?;
            write_sources(host, &sources);
        }
        Ok(())
    })();
    Ok(JsValue::Bool(match r {
        Ok(()) => true,
        Err(e) => {
            log_error("Theme remove error:", &e);
            false
        }
    }))
}

pub async fn open_folder<H: Host>(host: &H) -> HandlerResult {
    let dir = themes_dir(host).map_err(Thrown::from)?;
    host.open_path(&dir);
    Ok(JsValue::Bool(true))
}

/// `getMarketplaceThemeMeta()`: parsed `_marketplace.json`, or `{}` if unreadable. Creating
/// the themes dir happens before its `try`, so that failure propagates.
fn marketplace_meta<H: Host>(host: &H) -> Result<JsValue, JsErr> {
    let p = path::join(&[&themes_dir(host)?, "_marketplace.json"]);
    Ok(node::read_file_utf8(&p)
        .ok()
        .and_then(|t| ipc::json_parse(&t).ok())
        .unwrap_or(JsValue::Object(Vec::new())))
}

fn save_marketplace_meta<H: Host>(host: &H, meta: &JsValue) -> Result<(), JsErr> {
    let p = path::join(&[&themes_dir(host)?, "_marketplace.json"]);
    let text = pretty(meta)?;
    node::write_file(&p, text.as_bytes())?;
    Ok(())
}

pub async fn get_installed<H: Host>(host: &H) -> HandlerResult {
    marketplace_meta(host).map_err(Thrown::from)
}

/// `const { repo, id } = registryEntry`.
fn destructure(entry: &JsValue, first: &str) -> Result<(), JsErr> {
    match entry {
        JsValue::Undefined | JsValue::Null => {
            let what = if *entry == JsValue::Undefined {
                "undefined"
            } else {
                "null"
            };
            Err(type_error(format!(
                "Cannot destructure property '{first}' of 'registryEntry' as it is {what}."
            )))
        }
        _ => Ok(()),
    }
}

/// `value || fallback` for a property read.
fn or(entry: &JsValue, key: &str, fallback: JsValue) -> JsValue {
    let v = ipc::get_prop(entry, key).unwrap_or(JsValue::Undefined);
    if v.truthy() { v } else { fallback }
}

pub async fn install<H: Host>(host: &H, entry: &JsValue) -> HandlerResult {
    let r = async {
        destructure(entry, "repo")?;
        let repo = ipc::get_prop(entry, "repo")?;
        let id = ipc::get_prop(entry, "id")?;
        let branch = to_js_string(&or(entry, "branch", "main".into()));
        let sub_path = or(entry, "path", "".into());
        let file = or(entry, "file", "theme.css".into());
        let id_s = path_arg(&id)?;
        let local_src = path::join(&[&host.repo_root(), "plugins", "themes", &id_s]);
        // `!app.isPackaged && fs.existsSync(path.join(localSrc, file))`: only dev joins `file`.
        let local_file = if host.is_packaged() {
            None
        } else {
            Some(path::join(&[&local_src, &path_arg(&file)?]))
        };
        let use_local = local_file.as_deref().is_some_and(node::exists_sync);
        let sub = if sub_path.truthy() {
            format!("/{}", to_js_string(&sub_path))
        } else {
            String::new()
        };
        let raw_base = format!(
            "https://raw.githubusercontent.com/{}/{branch}{sub}",
            to_js_string(&repo)
        );
        let css = if let (true, Some(f)) = (use_local, &local_file) {
            node::read_file_utf8(f)?
        } else {
            let body = net::https_get(
                host,
                &format!("{raw_base}/{}", to_js_string(&file)),
                Flavor::Themes,
            )
            .await?;
            String::from_utf8_lossy(&body).into_owned()
        };
        let filename = format!("marketplace-{id_s}.css");
        node::write_file(
            &path::join(&[&themes_dir(host)?, &filename]),
            css.as_bytes(),
        )?;
        let mut meta = marketplace_meta(host)?;
        let record = JsValue::object([
            ("name", ipc::get_prop(entry, "name")?),
            ("version", or(entry, "version", "1.0.0".into())),
            ("author", or(entry, "author", "".into())),
            ("filename", JsValue::from(filename.as_str())),
            ("official", or(entry, "official", JsValue::Bool(false))),
        ]);
        ipc::set(&mut meta, &id_s, record)?;
        save_marketplace_meta(host, &meta)?;
        Ok::<_, JsErr>(JsValue::object([
            ("success", JsValue::Bool(true)),
            ("themeId", JsValue::from(format!("custom:{filename}"))),
        ]))
    }
    .await;
    Ok(r.unwrap_or_else(|e| {
        log_error("Theme install error:", &e);
        let msg = if e.message.0.is_empty() {
            "Install failed".into()
        } else {
            e.message
        };
        JsValue::object([("error", JsValue::String(msg))])
    }))
}

pub async fn uninstall_marketplace<H: Host>(host: &H, theme_id: &JsValue) -> HandlerResult {
    let r = (|| -> Result<(), JsErr> {
        let mut meta = marketplace_meta(host)?;
        let key = to_js_string(theme_id);
        let entry = ipc::lookup(&meta, &key)?;
        if entry.truthy() {
            let dir = themes_dir(host)?;
            let filename =
                path_arg(&ipc::get_prop(&entry.value(), "filename").unwrap_or(JsValue::Undefined))?;
            let file_path = path::join(&[&dir, &filename]);
            if node::exists_sync(&file_path) {
                node::unlink_sync(&file_path)?;
            }
            ipc::delete(&mut meta, &key)?;
            save_marketplace_meta(host, &meta)?;
        }
        Ok(())
    })();
    Ok(match r {
        Ok(()) => JsValue::object([("success", JsValue::Bool(true))]),
        Err(e) => {
            log_error("Theme uninstall error:", &e);
            JsValue::object([("error", JsValue::String(e.message))])
        }
    })
}
