//! The parts of tough-cookie 4.1.4 that ytmusic-api uses: `Cookie.parse` (strict mode),
//! `setCookieSync(cookie, baseURL)` and `getCookieStringSync(baseURL)` on a memory store.
//! The jar only ever sees `https://music.youtube.com/`.

/// One stored cookie.
#[derive(Clone, Debug)]
pub struct Cookie {
    pub key: String,
    pub value: String,
    /// `expires` as ms since the epoch; `None` = Infinity.
    expires: Option<f64>,
    max_age: Option<f64>,
    domain: Option<String>,
    path: Option<String>,
    secure: bool,
    host_only: bool,
    creation: f64,
    /// `Cookie.cookiesCreated` at construction (parse order).
    creation_index: u64,
}

const DATE_DELIM: fn(char) -> bool =
    |c| matches!(c as u32, 0x09 | 0x20..=0x2F | 0x3B..=0x40 | 0x5B..=0x60 | 0x7B..=0x7E);

/// `parseDigits(token, min, max, trailingOK)`.
fn parse_digits(token: &str, min: usize, max: usize, trailing_ok: bool) -> Option<u32> {
    let count = token.bytes().take_while(u8::is_ascii_digit).count();
    // Non-ASCII stops the count too (UTF-16 code units ≥ 0x3A are non-digits).
    if count < min || count > max || (!trailing_ok && count != token.chars().count()) {
        return None;
    }
    token[..count].parse().ok()
}

fn parse_time(token: &str) -> Option<[u32; 3]> {
    let parts: Vec<&str> = token.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut r = [0; 3];
    for i in 0..3 {
        r[i] = parse_digits(parts[i], 1, 2, i == 2)?;
    }
    Some(r)
}

fn parse_month(token: &str) -> Option<u32> {
    let t: String = token.chars().take(3).collect::<String>().to_lowercase();
    ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"]
        .iter()
        .position(|m| *m == t)
        .map(|i| i as u32)
}

/// RFC 6265 §5.1.1 `parseDate`, as ms since the epoch.
fn parse_date(s: &str) -> Option<f64> {
    if s.is_empty() {
        return None;
    }
    let (mut hms, mut day, mut month, mut year) = (None, None, None, None);
    for token in s.split(DATE_DELIM) {
        let token = crate::js::trim(token);
        if token.is_empty() {
            continue;
        }
        if hms.is_none()
            && let Some(t) = parse_time(token)
        {
            hms = Some(t);
            continue;
        }
        if day.is_none()
            && let Some(d) = parse_digits(token, 1, 2, true)
        {
            day = Some(d);
            continue;
        }
        if month.is_none()
            && let Some(m) = parse_month(token)
        {
            month = Some(m);
            continue;
        }
        if year.is_none()
            && let Some(mut y) = parse_digits(token, 2, 4, true)
        {
            if (70..=99).contains(&y) {
                y += 1900;
            } else if y <= 69 {
                y += 2000;
            }
            year = Some(y);
        }
    }
    let ([h, mi, se], d, mo, y) = (hms?, day?, month?, year?);
    if !(1..=31).contains(&d) || y < 1601 || h > 23 || mi > 59 || se > 59 {
        return None;
    }
    Some(utc_ms(i64::from(y), i64::from(mo), i64::from(d), i64::from(h), i64::from(mi), i64::from(se)))
}

/// `Date.UTC(y, m, d, h, mi, s)` (month 0-based; day overflow rolls over as in JS).
fn utc_ms(y: i64, m: i64, d: i64, h: i64, mi: i64, s: i64) -> f64 {
    let (y, m) = (y + m.div_euclid(12), m.rem_euclid(12) + 1);
    let (yy, mm) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * mm + 2) / 5;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468 + d - 1;
    ((days * 86_400 + h * 3600 + mi * 60 + s) * 1000) as f64
}

impl Cookie {
    /// `expiryTime(now)`.
    fn expiry_time(&self) -> f64 {
        if let Some(age) = self.max_age {
            return self.creation + if age <= 0.0 { f64::NEG_INFINITY } else { age * 1000.0 };
        }
        self.expires.unwrap_or(f64::INFINITY)
    }
}

/// `Cookie.parse(str)` (strict mode); `None` for a failed parse. `index` is the next
/// `Cookie.cookiesCreated` value.
pub fn parse(s: &str, index: u64) -> Option<Cookie> {
    let s = crate::js::trim(s);
    if s.is_empty() {
        return None;
    }
    let (pair, rest) = match s.find(';') {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    // `trimTerminator`: cut at the first \n, then \r, then \0.
    let mut pair = pair;
    for t in ['\n', '\r', '\0'] {
        if let Some(i) = pair.find(t) {
            pair = &pair[..i];
        }
    }
    let eq = pair.find('=').filter(|&i| i > 0)?;
    let key = crate::js::trim(&pair[..eq]).to_owned();
    let value = crate::js::trim(&pair[eq + 1..]).to_owned();
    if key.chars().chain(value.chars()).any(|c| (c as u32) < 0x20) {
        return None;
    }
    let mut c = Cookie {
        key,
        value,
        expires: None,
        max_age: None,
        domain: None,
        path: None,
        secure: false,
        host_only: false,
        creation: 0.0,
        creation_index: index,
    };
    let Some(rest) = rest else { return Some(c) };
    for av in crate::js::trim(rest).split(';') {
        let av = crate::js::trim(av);
        if av.is_empty() {
            continue;
        }
        let (k, v) = match av.find('=') {
            Some(i) => (&av[..i], Some(&av[i + 1..])),
            None => (av, None),
        };
        let v = v.filter(|v| !v.is_empty()).map(crate::js::trim);
        match crate::js::trim(k).to_lowercase().as_str() {
            "expires" => {
                if let Some(v) = v
                    && let Some(t) = parse_date(v)
                {
                    c.expires = Some(t);
                }
            }
            "max-age" => {
                if let Some(v) = v {
                    let digits = v.strip_prefix('-').unwrap_or(v);
                    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                        c.max_age = Some(crate::js::parse_int(v));
                    }
                }
            }
            "domain" => {
                if let Some(v) = v {
                    let d = crate::js::trim(v);
                    let d = d.strip_prefix('.').unwrap_or(d);
                    if !d.is_empty() {
                        c.domain = Some(d.to_lowercase());
                    }
                }
            }
            "path" => c.path = v.filter(|v| v.starts_with('/')).map(str::to_owned),
            "secure" => c.secure = true,
            _ => {}
        }
    }
    Some(c)
}

/// `domainMatch(str, domStr, false)` for host names (the jar never sees IP addresses).
fn domain_match(host: &str, dom: &str) -> bool {
    host == dom || (host.len() > dom.len() && host.ends_with(dom) && host.as_bytes()[host.len() - dom.len() - 1] == b'.')
}

/// A memory cookie store for one origin.
#[derive(Default, Debug)]
pub struct Jar {
    cookies: Vec<Cookie>,
    /// `Cookie.cookiesCreated`.
    pub created: u64,
}

const HOST: &str = "music.youtube.com";

/// Why `setCookieSync` threw (the axios response interceptor then rejects).
#[derive(Debug, Clone, PartialEq)]
pub struct SetCookieError(pub String);

impl Jar {
    /// `setCookieSync(cookie, "https://music.youtube.com/")`.
    pub fn set(&mut self, mut c: Cookie, now: f64) -> Result<(), SetCookieError> {
        if let Some(d) = &c.domain {
            // The only domains that can also match the host: the public suffix "com" fails here.
            let cd = d.trim().trim_start_matches('.').to_lowercase();
            if !cd.contains('.') {
                return Err(SetCookieError("Cookie has domain set to a public suffix".into()));
            }
            if !domain_match(HOST, &cd) {
                return Err(SetCookieError(format!("Cookie not in this host's domain. Cookie:{cd} Request:{HOST}")));
            }
            c.host_only = false;
            c.domain = Some(cd);
        } else {
            c.host_only = true;
            c.domain = Some(HOST.into());
        }
        if !c.path.as_ref().is_some_and(|p| p.starts_with('/')) {
            c.path = Some("/".into());
        }
        // Prefix security is "silent": a failing cookie is dropped without an error.
        if c.key.starts_with("__Secure-") && !c.secure {
            return Ok(());
        }
        if c.key.starts_with("__Host-") && !(c.secure && c.host_only && c.path.as_deref() == Some("/")) {
            return Ok(());
        }
        match self.cookies.iter_mut().find(|o| o.domain == c.domain && o.path == c.path && o.key == c.key) {
            Some(old) => {
                c.creation = old.creation;
                c.creation_index = old.creation_index;
                *old = c;
            }
            None => {
                c.creation = now;
                self.cookies.push(c);
            }
        }
        Ok(())
    }

    /// `getCookieStringSync("https://music.youtube.com/")` at `now`: expired cookies are
    /// removed, the rest sorted by path length then creation.
    pub fn header(&mut self, now: f64) -> String {
        self.cookies.retain(|c| c.expiry_time() > now);
        let mut matching: Vec<&Cookie> = self
            .cookies
            .iter()
            .filter(|c| {
                let d = c.domain.as_deref().unwrap_or_default();
                let domain_ok = if c.host_only { d == HOST } else { domain_match(HOST, d) };
                let path = c.path.as_deref().unwrap_or("/");
                domain_ok && path_match("/", path)
            })
            .collect();
        matching.sort_by(|a, b| {
            let (al, bl) = (a.path.as_ref().map_or(0, |p| p.encode_utf16().count()), b.path.as_ref().map_or(0, |p| p.encode_utf16().count()));
            bl.cmp(&al).then(a.creation.total_cmp(&b.creation)).then(a.creation_index.cmp(&b.creation_index))
        });
        matching.iter().map(|c| format!("{}={}", c.key, c.value)).collect::<Vec<_>>().join("; ")
    }
}

/// RFC 6265 §5.1.4 path-match.
fn path_match(req: &str, cookie: &str) -> bool {
    req == cookie || (req.starts_with(cookie) && (cookie.ends_with('/') || req.as_bytes().get(cookie.len()) == Some(&b'/')))
}
