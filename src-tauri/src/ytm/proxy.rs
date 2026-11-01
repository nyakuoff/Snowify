//! `session.defaultSession.resolveProxy('https://music.youtube.com/')` for manual proxy
//! configurations, following Chromium's Linux `ProxyConfigServiceLinux`: GNOME-family
//! desktops read `org.gnome.system.proxy`, KDE reads `kioslaverc`, anything else reads the
//! environment. PAC and auto-detect resolve to `DIRECT` (owner decision 2026-10-30).

const HOST: &str = "music.youtube.com";

enum Desktop {
    Gnome,
    Kde,
    Other,
}

/// `base::nix::GetDesktopEnvironment`, reduced to which settings source Chromium uses.
fn desktop() -> Desktop {
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    let current = var("XDG_CURRENT_DESKTOP");
    for part in current.split(':') {
        match part {
            "Unity" | "GNOME" | "X-Cinnamon" | "Pantheon" | "Budgie" | "Deepin" | "UKUI" => return Desktop::Gnome,
            "KDE" => return Desktop::Kde,
            "XFCE" | "LXQt" | "" => {}
            _ => {}
        }
    }
    let session = var("DESKTOP_SESSION");
    if session == "gnome" || session == "mate" {
        return Desktop::Gnome;
    }
    if session.starts_with("kde") || !var("KDE_FULL_SESSION").is_empty() {
        return Desktop::Kde;
    }
    Desktop::Other
}

/// A manual proxy as Chromium's PAC string writes it.
fn pac(scheme: &str, host: &str, port: u16) -> String {
    let kind = match scheme {
        "socks4" => "SOCKS",
        "socks5" | "socks" => "SOCKS5",
        "https" => "HTTPS",
        _ => "PROXY",
    };
    format!("{kind} {host}:{port}")
}

/// `ProxyUriToProxyServer(uri, default_scheme)`: `[scheme://]host[:port]`.
fn parse_uri(uri: &str, default_scheme: &str) -> Option<(String, String, u16)> {
    let uri = uri.trim();
    if uri.is_empty() {
        return None;
    }
    let (scheme, rest) = match uri.split_once("://") {
        Some((s, r)) => (s.to_ascii_lowercase(), r),
        None => (default_scheme.to_owned(), uri),
    };
    let rest = rest.trim_end_matches('/');
    let (host, port) = match rest.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() => (h.to_owned(), p.parse().ok()?),
        _ => (rest.to_owned(), match scheme.as_str() {
            "socks4" | "socks5" | "socks" => 1080,
            "https" => 443,
            _ => 80,
        }),
    };
    (!host.is_empty()).then_some((scheme, host, port))
}

/// `ProxyBypassRules` with suffix matching (environment and KDE) for the one host here.
fn bypassed(list: &str, suffix_matching: bool) -> bool {
    list.split([',', ' ', ';']).map(str::trim).filter(|e| !e.is_empty()).any(|e| {
        let e = e.split(':').next().unwrap_or(e).to_ascii_lowercase();
        if e == "*" || e == "<local>" && !HOST.contains('.') {
            return true;
        }
        let e = e.trim_start_matches('*');
        if let Some(sfx) = e.strip_prefix('.') {
            return HOST.ends_with(&format!(".{sfx}")) || HOST == sfx;
        }
        if suffix_matching { HOST.ends_with(e) } else { HOST == e }
    })
}

fn from_env() -> String {
    let get = |k: &str| std::env::var(k).ok().or_else(|| std::env::var(k.to_uppercase()).ok()).filter(|v| !v.is_empty());
    if get("no_proxy").is_some_and(|n| bypassed(&n, true)) {
        return "DIRECT".into();
    }
    let server = get("all_proxy").or_else(|| get("https_proxy"));
    if let Some((s, h, p)) = server.and_then(|v| parse_uri(&v, "http")) {
        return pac(&s, &h, p);
    }
    if let Some(socks) = get("SOCKS_SERVER") {
        let v = if std::env::var("SOCKS_VERSION").as_deref() == Ok("4") { "socks4" } else { "socks5" };
        if let Some((_, h, p)) = parse_uri(&socks, v) {
            return pac(v, &h, p);
        }
    }
    "DIRECT".into()
}

fn gsettings(key: &str, schema: &str) -> Option<String> {
    let out = std::process::Command::new("gsettings").args(["get", schema, key]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().trim_matches('\'').to_owned())
}

fn from_gnome() -> Option<String> {
    let mode = gsettings("mode", "org.gnome.system.proxy")?;
    if mode != "manual" {
        return Some("DIRECT".into());
    }
    let ignore = gsettings("ignore-hosts", "org.gnome.system.proxy").unwrap_or_default();
    if bypassed(&ignore.replace(['[', ']', '\''], ""), false) {
        return Some("DIRECT".into());
    }
    for (schema, kind) in [("org.gnome.system.proxy.https", "http"), ("org.gnome.system.proxy.socks", "socks5")] {
        let host = gsettings("host", schema).unwrap_or_default();
        let port: u16 = gsettings("port", schema).and_then(|p| p.parse().ok()).unwrap_or(0);
        if !host.is_empty() && port != 0 {
            return Some(pac(kind, &host, port));
        }
    }
    Some("DIRECT".into())
}

fn from_kde() -> Option<String> {
    let home = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
    let text = std::fs::read_to_string(format!("{home}/kioslaverc")).ok()?;
    let mut in_section = false;
    let (mut kind, mut https, mut socks, mut no) = (String::new(), String::new(), String::new(), String::new());
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line == "[Proxy Settings]";
            continue;
        }
        if let (true, Some((k, v))) = (in_section, line.split_once('=')) {
            match k.trim() {
                "ProxyType" => kind = v.trim().into(),
                "httpsProxy" => https = v.trim().replace(' ', ":"),
                "socksProxy" => socks = v.trim().replace(' ', ":"),
                "NoProxyFor" => no = v.trim().into(),
                _ => {}
            }
        }
    }
    match kind.as_str() {
        "1" => {}
        // 4: use the environment.
        "4" => return Some(from_env()),
        _ => return Some("DIRECT".into()),
    }
    if bypassed(&no, true) {
        return Some("DIRECT".into());
    }
    if let Some((s, h, p)) = parse_uri(&https, "http") {
        return Some(pac(&s, &h, p));
    }
    if let Some((_, h, p)) = parse_uri(&socks, "socks5") {
        return Some(pac("socks5", &h, p));
    }
    Some("DIRECT".into())
}

/// The PAC-style result for `https://music.youtube.com/`.
pub fn resolve(_url: &str) -> String {
    match desktop() {
        Desktop::Gnome => from_gnome().unwrap_or_else(from_env),
        Desktop::Kde => from_kde().unwrap_or_else(from_env),
        Desktop::Other => from_env(),
    }
}
