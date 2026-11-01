//! Port of `src/main/plugins.js`: the plugin marketplace registry, installing plugins into
//! `<userData>/plugins/<id>`, and serving their files to the renderer.

use crate::{
    host::Host,
    ipc::{self, HandlerResult, JsString, JsValue, to_js_string},
    logger::{self, LogArg},
    net::{self, Flavor, JsErr},
    node::{self, path::posix as path, path::posix_more as path2},
    themes::invalid_path_arg,
};

const MARKETPLACE_REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/nyakuoff/Snowify/main/plugins/registry.json";

fn log_error(label: &str, err: &JsErr) {
    logger::error(&[label.into(), LogArg::from_js(&err.log_json())]);
}

fn type_error(message: impl Into<JsString>) -> JsErr {
    JsErr {
        name: "TypeError".into(),
        message: message.into(),
        props: Vec::new(),
    }
}

fn path_arg(v: &JsValue) -> Result<String, JsErr> {
    match v {
        JsValue::String(s) => Ok(s.to_string_lossy()),
        JsValue::Shared(_, inner) => path_arg(inner),
        v => Err(invalid_path_arg("path", v)),
    }
}

/// `value.startsWith(prefix)` on a property that must be a string (else V8 throws).
fn starts_with(v: &JsValue, prefix: &str, expr: &str) -> Result<bool, JsErr> {
    match v {
        JsValue::String(s) => Ok(s.to_string_lossy().starts_with(prefix)),
        _ => Err(type_error(format!("{expr}.startsWith is not a function"))),
    }
}

/// `getPluginsDir()`.
fn plugins_dir<H: Host>(host: &H) -> Result<String, JsErr> {
    let dir = path::join(&[&host.user_data(), "plugins"]);
    if !node::exists_sync(&dir) {
        node::mkdir_recursive_sync(&dir)?;
    }
    Ok(dir)
}

/// JS `String.prototype.trim`.
fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| {
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
    })
}

/// `sanitizePluginId(id)`: `String(id || '').trim()`, kept only if it matches `^[a-z0-9._-]+$`i.
pub fn sanitize_plugin_id(id: &JsValue) -> Option<String> {
    let s = if id.truthy() {
        to_js_string(id)
    } else {
        String::new()
    };
    let v = js_trim(&s);
    (!v.is_empty()
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-'))
    .then(|| v.to_owned())
}

/// `safeJoinUnder(baseDir, relativePath)` with `cwd` for `path.resolve`.
fn safe_join_under(base: &str, rel: &JsValue) -> Result<String, JsErr> {
    let raw = if rel.truthy() {
        to_js_string(rel)
    } else {
        String::new()
    };
    let normalized = path::normalize(&raw);
    if normalized.is_empty()
        || normalized == "."
        || normalized.starts_with("../")
        || normalized.contains("/../")
    {
        return Err(JsErr::error("Invalid path"));
    }
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let target = path2::resolve(&[base, &normalized], &cwd);
    let root = format!("{}/", path2::resolve(&[base], &cwd));
    if !target.starts_with(&root) {
        return Err(JsErr::error("Invalid path"));
    }
    Ok(target)
}

/// JS `encodeURIComponent` (input has no lone surrogates: it comes from a lossy string).
fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `encodePathForUrl(relPath)`.
fn encode_path_for_url(rel: &str) -> String {
    rel.split('/')
        .map(encode_uri_component)
        .collect::<Vec<_>>()
        .join("/")
}

fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `{ ...obj, key: value }` for a plain object (other values spread to `{}`).
fn spread_with(obj: &JsValue, key: &str, value: JsValue) -> JsValue {
    let mut out = match obj {
        JsValue::Object(props) => JsValue::Object(props.clone()),
        JsValue::Shared(_, v) => return spread_with(v, key, value),
        JsValue::Array(items) => JsValue::Object(
            items
                .iter()
                .enumerate()
                .map(|(i, v)| (JsString::from(i.to_string()), v.clone()))
                .collect(),
        ),
        JsValue::String(s) => JsValue::Object(
            String::from_utf16_lossy(&s.0)
                .encode_utf16()
                .enumerate()
                .map(|(i, u)| {
                    (
                        JsString::from(i.to_string()),
                        JsValue::String(JsString(vec![u])),
                    )
                })
                .collect(),
        ),
        _ => JsValue::Object(Vec::new()),
    };
    ipc::define(&mut out, key.into(), value);
    out
}

/// `resolveLogoUrl(manifest, pluginDir)`: a relative `logoUrl` becomes a data URL.
fn resolve_logo_url(manifest: &JsValue, plugin_dir: &str) -> Result<JsValue, JsErr> {
    let logo = ipc::get_prop(manifest, "logoUrl")?;
    if !logo.truthy() {
        return Ok(manifest.clone());
    }
    if starts_with(&logo, "http://", "logo")?
        || starts_with(&logo, "https://", "logo")?
        || starts_with(&logo, "data:", "logo")?
    {
        return Ok(manifest.clone());
    }
    let logo = to_js_string(&logo);
    let logo_path = path::join(&[plugin_dir, &logo]);
    if !node::exists_sync(&logo_path) {
        return Ok(manifest.clone());
    }
    let ext = path2::extname(&logo)
        .get(1..)
        .unwrap_or_default()
        .to_lowercase();
    let mime = match ext.as_str() {
        "svg" => "image/svg+xml",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    };
    Ok(match node::read_file(&logo_path) {
        Ok(data) => spread_with(
            manifest,
            "logoUrl",
            format!("data:{mime};base64,{}", base64(&data)).into(),
        ),
        Err(_) => manifest.clone(),
    })
}

/// `getInstalledPlugins()`: a failing `statSync` ends the scan with what was collected.
fn installed_plugins<H: Host>(host: &H) -> JsValue {
    let mut plugins = Vec::new();
    let _ = (|| -> Result<(), JsErr> {
        let dir = plugins_dir(host)?;
        for name in node::readdir_sync(&dir)? {
            let plugin_dir = path::join(&[&plugins_dir(host)?, &name]);
            if !node::stat_is_dir(&plugin_dir)? {
                continue;
            }
            let manifest_path = path::join(&[&plugin_dir, "snowify-plugin.json"]);
            if node::exists_sync(&manifest_path) {
                let entry = (|| -> Result<JsValue, JsErr> {
                    let text = node::read_file_utf8(&manifest_path)?;
                    let manifest = ipc::json_parse(&text).map_err(|m| JsErr {
                        name: "SyntaxError".into(),
                        message: m,
                        props: Vec::new(),
                    })?;
                    resolve_logo_url(&manifest, &plugin_dir)
                })();
                if let Ok(m) = entry {
                    plugins.push(m);
                }
            }
        }
        Ok(())
    })();
    JsValue::Array(plugins)
}

fn syntax(m: JsString) -> JsErr {
    JsErr {
        name: "SyntaxError".into(),
        message: m,
        props: Vec::new(),
    }
}

/// `p.logoUrl` is absent, remote, or a data URL.
fn logo_is_absolute(p: &JsValue) -> Result<bool, JsErr> {
    let logo = ipc::get_prop(p, "logoUrl")?;
    Ok(!logo.truthy()
        || starts_with(&logo, "http", "p.logoUrl")?
        || starts_with(&logo, "data:", "p.logoUrl")?)
}

/// `registry.plugins = registry.plugins.map(f)`; `if (registry.plugins)` first.
fn map_registry_plugins(
    registry: &mut JsValue,
    f: impl Fn(&JsValue) -> Result<JsValue, JsErr>,
) -> Result<(), JsErr> {
    let plugins = ipc::get_prop(registry, "plugins")?;
    if !plugins.truthy() {
        return Ok(());
    }
    let JsValue::Array(items) = plugins else {
        return Err(type_error("registry.plugins.map is not a function"));
    };
    let mapped = items.iter().map(f).collect::<Result<Vec<_>, _>>()?;
    ipc::set(registry, "plugins", JsValue::Array(mapped))?;
    Ok(())
}

pub async fn get_registry<H: Host>(host: &H) -> HandlerResult {
    if !host.is_packaged() {
        let local = (|| -> Result<Option<JsValue>, JsErr> {
            let local_path = path::join(&[&host.repo_root(), "plugins", "registry.json"]);
            if !node::exists_sync(&local_path) {
                return Ok(None);
            }
            let mut registry =
                ipc::json_parse(&node::read_file_utf8(&local_path)?).map_err(syntax)?;
            map_registry_plugins(&mut registry, |p| {
                if logo_is_absolute(p)? {
                    return Ok(p.clone());
                }
                let path_or_id = {
                    let v = ipc::get_prop(p, "path")?;
                    if v.truthy() {
                        v
                    } else {
                        ipc::get_prop(p, "id")?
                    }
                };
                let dir = path::join(&[&host.repo_root(), "plugins", &path_arg(&path_or_id)?]);
                resolve_logo_url(p, &dir)
            })?;
            Ok(Some(registry))
        })();
        if let Ok(Some(registry)) = local {
            return Ok(registry);
        }
    }
    let remote = async {
        let body = net::https_get(host, MARKETPLACE_REGISTRY_URL, Flavor::Plugins).await?;
        let mut registry = ipc::json_parse(&String::from_utf8_lossy(&body)).map_err(syntax)?;
        map_registry_plugins(&mut registry, |p| {
            if logo_is_absolute(p)? {
                return Ok(p.clone());
            }
            let sub = ipc::get_prop(p, "path")?;
            let repo_path = if sub.truthy() {
                format!("/{}", to_js_string(&sub))
            } else {
                String::new()
            };
            let branch = ipc::get_prop(p, "branch")?;
            let branch = if branch.truthy() {
                to_js_string(&branch)
            } else {
                "main".into()
            };
            let url = format!(
                "https://raw.githubusercontent.com/{}/{branch}{repo_path}/{}",
                to_js_string(&ipc::get_prop(p, "repo")?),
                to_js_string(&ipc::get_prop(p, "logoUrl")?)
            );
            Ok(spread_with(p, "logoUrl", url.into()))
        })?;
        Ok::<_, JsErr>(registry)
    }
    .await;
    Ok(remote.unwrap_or_else(|e| {
        log_error("Failed to fetch plugin registry:", &e);
        JsValue::object([
            ("version", JsValue::Number(1.0)),
            ("plugins", JsValue::Array(Vec::new())),
            ("themes", JsValue::Array(Vec::new())),
        ])
    }))
}

pub async fn get_installed<H: Host>(host: &H) -> HandlerResult {
    Ok(installed_plugins(host))
}

fn result_error(message: impl Into<JsString>) -> JsValue {
    JsValue::object([("error", JsValue::String(message.into()))])
}

/// `fs.writeFileSync(dst, data, 'utf-8')` for a downloaded (string) body.
async fn fetch_text<H: Host>(host: &H, url: &str) -> Result<String, JsErr> {
    let body = net::https_get(host, url, Flavor::Plugins).await?;
    Ok(String::from_utf8_lossy(&body).into_owned())
}

pub async fn install<H: Host>(host: &H, entry: &JsValue) -> HandlerResult {
    let r = async {
        if matches!(entry, JsValue::Undefined | JsValue::Null) {
            let what = if *entry == JsValue::Undefined {
                "undefined"
            } else {
                "null"
            };
            return Err(type_error(format!(
                "Cannot destructure property 'repo' of 'registryEntry' as it is {what}."
            )));
        }
        let repo = ipc::get_prop(entry, "repo")?;
        let Some(safe_entry_id) = sanitize_plugin_id(&ipc::get_prop(entry, "id")?) else {
            return Ok(result_error("Invalid plugin id"));
        };
        let branch = ipc::get_prop(entry, "branch")?;
        let branch = if branch.truthy() {
            to_js_string(&branch)
        } else {
            "main".into()
        };
        let sub_path = ipc::get_prop(entry, "path")?;
        let local_src = path::join(&[&host.repo_root(), "plugins", &safe_entry_id]);
        let use_local = !host.is_packaged()
            && node::exists_sync(&path::join(&[&local_src, "snowify-plugin.json"]));
        let sub = if sub_path.truthy() {
            format!("/{}", to_js_string(&sub_path))
        } else {
            String::new()
        };
        let raw_base = format!(
            "https://raw.githubusercontent.com/{}/{branch}{sub}",
            to_js_string(&repo)
        );

        let manifest_raw = if use_local {
            node::read_file_utf8(&path::join(&[&local_src, "snowify-plugin.json"]))?
        } else {
            fetch_text(host, &format!("{raw_base}/snowify-plugin.json")).await?
        };
        let manifest = ipc::json_parse(&manifest_raw).map_err(syntax)?;

        let manifest_id = ipc::get_prop(&manifest, "id")?;
        let id_or = if manifest_id.truthy() {
            manifest_id
        } else {
            JsValue::from(safe_entry_id.as_str())
        };
        let Some(safe_manifest_id) = sanitize_plugin_id(&id_or) else {
            return Ok(result_error("Invalid plugin manifest id"));
        };
        let plugin_dir = safe_join_under(
            &plugins_dir(host)?,
            &JsValue::from(safe_manifest_id.as_str()),
        )?;
        if node::exists_sync(&plugin_dir) {
            node::rm_recursive_sync(&plugin_dir)?;
        }
        node::mkdir_recursive_sync(&plugin_dir)?;
        node::write_file(
            &path::join(&[&plugin_dir, "snowify-plugin.json"]),
            manifest_raw.as_bytes(),
        )?;

        let renderer = ipc::get_prop(&manifest, "renderer")?;
        if renderer.truthy() {
            let renderer_path = to_js_string(&renderer);
            let dst = safe_join_under(&plugin_dir, &JsValue::from(renderer_path.as_str()))?;
            node::mkdir_recursive_sync(&path2::dirname(&dst))?;
            if use_local {
                let src = safe_join_under(&local_src, &JsValue::from(renderer_path.as_str()))?;
                node::copy_file_sync(&src, &dst)?;
            } else {
                let body = fetch_text(
                    host,
                    &format!("{raw_base}/{}", encode_path_for_url(&renderer_path)),
                )
                .await?;
                node::write_file(&dst, body.as_bytes())?;
            }
        }
        let styles = ipc::get_prop(&manifest, "styles")?;
        if styles.truthy() {
            let _ = async {
                let styles_path = to_js_string(&styles);
                let dst = safe_join_under(&plugin_dir, &JsValue::from(styles_path.as_str()))?;
                node::mkdir_recursive_sync(&path2::dirname(&dst))?;
                if use_local {
                    let src = safe_join_under(&local_src, &JsValue::from(styles_path.as_str()))?;
                    node::copy_file_sync(&src, &dst)?;
                } else {
                    let body = fetch_text(
                        host,
                        &format!("{raw_base}/{}", encode_path_for_url(&styles_path)),
                    )
                    .await?;
                    node::write_file(&dst, body.as_bytes())?;
                }
                Ok::<_, JsErr>(())
            }
            .await;
        }
        let logo = ipc::get_prop(&manifest, "logoUrl")?;
        if logo.truthy()
            && !starts_with(&logo, "http", "manifest.logoUrl")?
            && !starts_with(&logo, "data:", "manifest.logoUrl")?
        {
            let _ = async {
                let logo_path = to_js_string(&logo);
                let dst = safe_join_under(&plugin_dir, &JsValue::from(logo_path.as_str()))?;
                node::mkdir_recursive_sync(&path2::dirname(&dst))?;
                if use_local {
                    let src = safe_join_under(&local_src, &JsValue::from(logo_path.as_str()))?;
                    if node::exists_sync(&src) {
                        node::copy_file_sync(&src, &dst)?;
                    }
                } else {
                    let data = net::https_get(
                        host,
                        &format!("{raw_base}/{}", encode_path_for_url(&logo_path)),
                        Flavor::Plugins,
                    )
                    .await?;
                    node::write_file(&dst, &data)?;
                }
                Ok::<_, JsErr>(())
            }
            .await;
        }
        Ok(JsValue::object([
            ("success", JsValue::Bool(true)),
            ("plugin", manifest),
        ]))
    }
    .await;
    Ok(r.unwrap_or_else(|e| {
        log_error("Plugin install error:", &e);
        result_error(if e.message.0.is_empty() {
            "Install failed".into()
        } else {
            e.message
        })
    }))
}

pub async fn uninstall<H: Host>(host: &H, plugin_id: &JsValue) -> HandlerResult {
    let r = (|| -> Result<JsValue, JsErr> {
        let Some(safe_id) = sanitize_plugin_id(plugin_id) else {
            return Ok(result_error("Invalid plugin id"));
        };
        let dir = safe_join_under(&plugins_dir(host)?, &JsValue::from(safe_id.as_str()))?;
        if node::exists_sync(&dir) {
            node::rm_recursive_sync(&dir)?;
        }
        Ok(JsValue::object([("success", JsValue::Bool(true))]))
    })();
    Ok(r.unwrap_or_else(|e| result_error(e.message)))
}

pub async fn get_files<H: Host>(host: &H, plugin_id: &JsValue) -> HandlerResult {
    let r = (|| -> Result<JsValue, JsErr> {
        let Some(safe_id) = sanitize_plugin_id(plugin_id) else {
            return Ok(JsValue::Null);
        };
        let id = JsValue::from(safe_id.as_str());
        let mut dir = safe_join_under(&plugins_dir(host)?, &id)?;
        if !host.is_packaged() {
            let local_dir = safe_join_under(&path::join(&[&host.repo_root(), "plugins"]), &id)?;
            if node::exists_sync(&path::join(&[&local_dir, "snowify-plugin.json"])) {
                dir = local_dir;
            }
        }
        let manifest_path = path::join(&[&dir, "snowify-plugin.json"]);
        if !node::exists_sync(&manifest_path) {
            return Ok(JsValue::Null);
        }
        let manifest = ipc::json_parse(&node::read_file_utf8(&manifest_path)?).map_err(syntax)?;
        let mut result = JsValue::object([("manifest", manifest.clone())]);
        let renderer = ipc::get_prop(&manifest, "renderer")?;
        if renderer.truthy() {
            let js_path = safe_join_under(&dir, &renderer)?;
            if node::exists_sync(&js_path) {
                ipc::set(&mut result, "js", node::read_file_utf8(&js_path)?.into())?;
            }
        }
        let styles = ipc::get_prop(&manifest, "styles")?;
        if styles.truthy() {
            let css_path = safe_join_under(&dir, &styles)?;
            if node::exists_sync(&css_path) {
                ipc::set(&mut result, "css", node::read_file_utf8(&css_path)?.into())?;
            }
        }
        Ok(result)
    })();
    Ok(r.unwrap_or_else(|e| {
        log_error("Get plugin files error:", &e);
        JsValue::Null
    }))
}
