//! Electron 33's `app.getLocale()` on Linux: which UI locale Chromium picks from the
//! environment. Derived from recorded runs of real Electron (`parity/corpus/electron-locale.tsv`):
//!
//! - A non-empty `LANGUAGE` is a `:`-separated priority list; entries are resolved through
//!   the system `locale.alias` (as GLib does), and nothing else is consulted.
//! - Otherwise the first non-empty of `LC_ALL`, `LC_MESSAGES`, `LANG` (no alias lookup), or "C".
//! - Each candidate is canonicalized ICU-style, then resolved against the locales Electron
//!   ships (Chromium's `CheckAndResolveLocale`); the fallback is `en-US`.

use std::collections::HashMap;

/// Locales Electron 33 ships (`dist/locales/*.pak`).
const AVAILABLE: [&str; 55] = [
    "af", "am", "ar", "bg", "bn", "ca", "cs", "da", "de", "el", "en-GB", "en-US", "es-419", "es",
    "et", "fa", "fil", "fi", "fr", "gu", "he", "hi", "hr", "hu", "id", "it", "ja", "kn", "ko",
    "lt", "lv", "ml", "mr", "ms", "nb", "nl", "pl", "pt-BR", "pt-PT", "ro", "ru", "sk", "sl", "sr",
    "sv", "sw", "ta", "te", "th", "tr", "uk", "ur", "vi", "zh-CN", "zh-TW",
];

fn available(l: &str) -> bool {
    AVAILABLE.contains(&l)
}

/// Parses `locale.alias` (`alias value` per line, `#` comments), as GLib does.
pub fn parse_locale_alias(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((it.next()?.to_owned(), it.next()?.to_owned()))
        })
        .collect()
}

/// GLib's `unalias_lang`: follows aliases up to 31 times.
fn unalias(lang: &str, aliases: &HashMap<String, String>) -> String {
    let mut cur = lang.to_owned();
    for _ in 0..31 {
        match aliases.get(&cur) {
            Some(next) if *next != cur => cur = next.clone(),
            _ => break,
        }
    }
    cur
}

/// ICU-style canonical form `lang[-Script][-REGION]` of a POSIX/BCP47-ish id; the codeset and
/// any variant/modifier are dropped. On the `LC_ALL`/`LANG` path (`posix`), ICU turns
/// "C"/"POSIX" into `en_US_POSIX`; in `LANGUAGE` they are just unknown languages.
fn canonical(raw: &str, posix: bool) -> String {
    let base = raw.split(['.', '@']).next().unwrap_or_default();
    if posix && (base == "C" || base == "POSIX") {
        return "en-US".into();
    }
    let mut parts = base.split(['_', '-']);
    let lang = parts.next().unwrap_or_default().to_ascii_lowercase();
    let lang = match lang.as_str() {
        "" => "und",
        "iw" => "he",
        "in" => "id",
        "ji" => "yi",
        "jw" => "jv",
        "mo" => "ro",
        "sh" => "sr",
        "tl" => "fil",
        "no" => "nb",
        l => l,
    }
    .to_owned();
    let mut out = lang;
    let mut rest: Vec<&str> = parts.collect();
    if let Some(s) = rest.first()
        && s.len() == 4
        && s.chars().all(|c| c.is_ascii_alphabetic())
    {
        let mut script = s.to_ascii_lowercase();
        script[..1].make_ascii_uppercase();
        out = format!("{out}-{script}");
        rest.remove(0);
    }
    if let Some(r) = rest.first()
        && ((r.len() == 2 && r.chars().all(|c| c.is_ascii_alphabetic()))
            || (r.len() == 3 && r.chars().all(|c| c.is_ascii_digit())))
    {
        out = format!("{out}-{}", r.to_ascii_uppercase());
    }
    out
}

/// Chromium's `CheckAndResolveLocale` against Electron's shipped locales.
fn resolve(locale: &str) -> Option<String> {
    if available(locale) {
        return Some(locale.into());
    }
    let (lang, region) = match locale.split_once('-') {
        Some((l, r)) if !l.is_empty() => (l, Some(r.to_ascii_lowercase())),
        _ => (locale, None),
    };
    if let Some(region) = region {
        let mapped = match lang {
            "es" if region != "es" => Some("es-419"),
            "pt" => Some("pt-PT"),
            "zh" if region == "hk" || region == "mo" => Some("zh-TW"),
            "zh" => Some("zh-CN"),
            "en" if ["us", "ph", "lr"].contains(&region.as_str()) => Some("en-US"),
            "en" => Some("en-GB"),
            _ => None,
        };
        if let Some(m) = mapped.filter(|m| available(m)) {
            return Some(m.into());
        }
    }
    let alias = match lang {
        "en" => "en-US",
        "pt" => "pt-BR",
        "zh" => "zh-CN",
        l => l,
    };
    available(alias).then(|| alias.into())
}

/// `app.getLocale()` for the given environment and `locale.alias` table.
pub fn app_locale(
    var: impl Fn(&str) -> Option<String>,
    aliases: &HashMap<String, String>,
) -> String {
    let var = |k: &str| var(k).filter(|v| !v.is_empty());
    let posix = var("LANGUAGE").is_none();
    let candidates: Vec<String> = match var("LANGUAGE") {
        Some(list) => list
            .split(':')
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| unalias(l, aliases))
            .collect(),
        None => vec![
            var("LC_ALL")
                .or_else(|| var("LC_MESSAGES"))
                .or_else(|| var("LANG"))
                .unwrap_or_else(|| "C".into()),
        ],
    };
    candidates
        .iter()
        .find_map(|c| resolve(&canonical(c, posix)))
        .unwrap_or_else(|| "en-US".into())
}

/// `app.getLocale()` for this process.
pub fn current() -> String {
    let aliases = std::fs::read_to_string("/usr/share/locale/locale.alias")
        .map(|t| parse_locale_alias(&t))
        .unwrap_or_default();
    app_locale(
        |k| std::env::var_os(k).map(|v| v.to_string_lossy().into_owned()),
        &aliases,
    )
}
