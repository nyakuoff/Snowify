//! JavaScript language semantics the source relies on implicitly: `Number#toString`,
//! `JSON.stringify` and `Date#toISOString`, as V8 (Electron 33) implements them.

use serde_json::Value;

/// JS `Number::toString(10)`: shortest round-trip digits, exponent form outside 1e-7..1e21.
pub fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n == 0.0 {
        return "0".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if n < 0.0 {
        return format!("-{}", number_to_string(-n));
    }
    // `{:e}` is Rust's shortest round-trip form, e.g. "1.2345e3".
    let sci = format!("{n:e}");
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let point = exp.parse::<i32>().unwrap_or(0) + 1; // the spec's `n`
    let digits = even_on_tie(n, mantissa.replace('.', ""), point);
    let k = digits.len() as i32;
    if k <= point && point <= 21 {
        digits + &"0".repeat((point - k) as usize)
    } else if 0 < point && point <= 21 {
        let (int, frac) = digits.split_at(point as usize);
        format!("{int}.{frac}")
    } else if -6 < point && point <= 0 {
        format!("0.{}{digits}", "0".repeat(-point as usize))
    } else {
        let e = point - 1;
        let sign = if e >= 0 { '+' } else { '-' };
        let (first, rest) = digits.split_at(1);
        let mantissa = if rest.is_empty() {
            first.to_owned()
        } else {
            format!("{first}.{rest}")
        };
        format!("{mantissa}e{sign}{}", e.abs())
    }
}

/// When `n` lies exactly halfway between two shortest candidates, the spec picks the even
/// one; Rust's formatter may pick the other. `digits`/`point` are Rust's shortest form.
fn even_on_tie(n: f64, digits: String, point: i32) -> String {
    let k = digits.len();
    // A double's exact decimal expansion has at most 767 significant digits.
    let exact = format!("{n:.800e}");
    let Some((m, e)) = exact.split_once('e') else {
        return digits;
    };
    let exact_digits = m.replace('.', "");
    let exact_digits = exact_digits.trim_end_matches('0');
    if exact_digits.len() != k + 1
        || !exact_digits.ends_with('5')
        || e.parse::<i32>().map(|e| e + 1) != Ok(point)
    {
        return digits;
    }
    let low = &exact_digits[..k];
    let candidate = if low.as_bytes()[k - 1] % 2 == 0 {
        low.to_owned()
    } else {
        // low + 1 in the last place; an odd last digit never carries.
        let mut b = low.as_bytes().to_vec();
        b[k - 1] += 1;
        String::from_utf8(b).unwrap_or_default()
    };
    let round_trips = format!("{candidate}e{}", point - k as i32).parse::<f64>() == Ok(n);
    if round_trips { candidate } else { digits }
}

/// JS `String(v)` for a JSON value.
pub fn to_string(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => s.clone(),
        // Array.prototype.join: null/undefined elements become "".
        Value::Array(items) => items
            .iter()
            .map(|i| {
                if i.is_null() {
                    String::new()
                } else {
                    to_string(i)
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// JS `JSON.stringify(v)` (no replacer, no indent). Object keys follow JS property order:
/// array-index keys ascending, then the rest in insertion order.
pub fn json_stringify(v: &Value) -> String {
    let mut out = String::new();
    write_json(v, &mut out);
    out
}

fn write_json(v: &Value, out: &mut String) {
    match v {
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.is_finite() => out.push_str(&number_to_string(f)),
            _ => out.push_str("null"),
        },
        // JSON.stringify escapes exactly what serde_json escapes for a valid Rust string.
        Value::String(s) => out.push_str(&Value::from(s.as_str()).to_string()),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            // Stable sort: index keys first, ascending; others keep insertion order.
            keys.sort_by_key(|k| array_index(k).map_or((1, 0), |i| (0, i)));
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(&Value::from(k.as_str()), out);
                out.push(':');
                write_json(&map[k], out);
            }
            out.push('}');
        }
        Value::Null | Value::Bool(_) => out.push_str(&v.to_string()),
    }
}

/// A canonical array index: "0", or digits without a leading zero, below 2^32 - 1.
fn array_index(k: &str) -> Option<u32> {
    let valid = k == "0" || (!k.starts_with('0') && !k.is_empty());
    let i: u32 = k
        .parse()
        .ok()
        .filter(|_| valid && k.bytes().all(|b| b.is_ascii_digit()))?;
    (i != u32::MAX).then_some(i)
}

/// JS `new Date(ms).toISOString()`. `ms` is a valid time value (|ms| <= 8.64e15).
pub fn iso_string(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let rem = ms.rem_euclid(86_400_000);
    // Days since 1970-01-01 to civil date (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let year = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else {
        format!("{}{:06}", if year < 0 { '-' } else { '+' }, year.abs())
    };
    format!(
        "{year}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3_600_000,
        rem / 60_000 % 60,
        rem / 1000 % 60,
        rem % 1000
    )
}

/// V8's `JSON.parse` over UTF-16 code units, including its exact error messages
/// (`Expected ',' or '}' after property value in JSON at position 6 (line 1 column 7)`,
/// `Unexpected token '<', "<html><bod"... is not valid JSON`, …). Objects keep JS property
/// order (array-index keys ascending, then insertion order; a repeated key keeps its slot).
pub fn json_parse(src: &[u16]) -> Result<crate::ipc::JsValue, crate::ipc::JsString> {
    let mut p = JsonParser { s: src, i: 0 };
    let parsed = (|| {
        p.ws();
        let v = p.value()?;
        p.ws();
        if p.i < src.len() {
            return Err(p.position("Unexpected non-whitespace character after JSON at position"));
        }
        Ok(v)
    })();
    parsed.map_err(crate::ipc::JsString)
}

struct JsonParser<'a> {
    s: &'a [u16],
    i: usize,
}

const MAX_CONTEXT: usize = 10;

impl JsonParser<'_> {
    fn peek(&self) -> Option<u16> {
        self.s.get(self.i).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(0x20 | 0x09 | 0x0A | 0x0D)) {
            self.i += 1;
        }
    }

    /// `<msg> in JSON at position N (line L column C)`; `\n`, `\r` and `\r\n` end lines.
    fn at(&self, msg: &str) -> Vec<u16> {
        self.position(&format!("{msg} in JSON at position"))
    }

    fn position(&self, prefix: &str) -> Vec<u16> {
        let (mut line, mut start, mut k) = (1, 0, 0);
        while k < self.i {
            match self.s[k] {
                0x0D if self.s.get(k + 1) == Some(&0x0A) && k + 1 < self.i => {
                    line += 1;
                    k += 2;
                    start = k;
                    continue;
                }
                0x0A | 0x0D => {
                    line += 1;
                    start = k + 1;
                }
                _ => {}
            }
            k += 1;
        }
        format!(
            "{prefix} {} (line {line} column {})",
            self.i,
            self.i - start + 1
        )
        .encode_utf16()
        .collect()
    }

    /// V8's `ReportUnexpectedToken` for the token at the cursor, with no specific message. The
    /// token and context are raw UTF-16 units, so they can hold half a surrogate pair, as in V8.
    fn unexpected(&self) -> Vec<u16> {
        let u = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        let Some(c) = self.peek() else {
            return u("Unexpected end of JSON input");
        };
        match c {
            0x30..=0x39 | 0x2D => return self.at("Unexpected number"),
            0x22 => return self.at("Unexpected string"),
            _ => {}
        }
        let src = String::from_utf16_lossy(self.s);
        if ["NaN", "Infinity", "undefined", "[object Object]"].contains(&src.as_str()) {
            return u(&format!("\"{src}\" is not valid JSON"));
        }
        let (len, pos) = (self.s.len(), self.i);
        let (ctx, open, close): (&[u16], &str, &str) = if len < MAX_CONTEXT * 2 + 1 {
            (self.s, "\"", "\"")
        } else if pos < MAX_CONTEXT {
            (&self.s[..(pos + MAX_CONTEXT).min(len)], "\"", "\"...")
        } else if pos < len - MAX_CONTEXT {
            (
                &self.s[pos - MAX_CONTEXT..pos + MAX_CONTEXT],
                "...\"",
                "\"...",
            )
        } else {
            (&self.s[pos - MAX_CONTEXT..], "...\"", "\"")
        };
        let mut out = u("Unexpected token '");
        out.push(c);
        out.extend(u(&format!("', {open}")));
        out.extend_from_slice(ctx);
        out.extend(u(&format!("{close} is not valid JSON")));
        out
    }

    fn value(&mut self) -> Result<crate::ipc::JsValue, Vec<u16>> {
        use crate::ipc::JsValue;
        match self.peek() {
            Some(0x22) => self
                .string()
                .map(|s| JsValue::String(crate::ipc::JsString(s))),
            Some(0x2D | 0x30..=0x39) => self.number().map(JsValue::Number),
            Some(0x7B) => self.object(),
            Some(0x5B) => self.array(),
            Some(0x74) => self.literal("true").map(|()| JsValue::Bool(true)),
            Some(0x66) => self.literal("false").map(|()| JsValue::Bool(false)),
            Some(0x6E) => self.literal("null").map(|()| JsValue::Null),
            _ => Err(self.unexpected()),
        }
    }

    fn literal(&mut self, word: &str) -> Result<(), Vec<u16>> {
        for u in word.encode_utf16() {
            if self.peek() != Some(u) {
                return Err(self.unexpected());
            }
            self.i += 1;
        }
        Ok(())
    }

    fn digits(&mut self) -> usize {
        let start = self.i;
        while matches!(self.peek(), Some(0x30..=0x39)) {
            self.i += 1;
        }
        self.i - start
    }

    fn number(&mut self) -> Result<f64, Vec<u16>> {
        let start = self.i;
        if self.peek() == Some(0x2D) {
            self.i += 1;
            if !matches!(self.peek(), Some(0x30..=0x39)) {
                return Err(self.at("No number after minus sign"));
            }
        }
        if self.peek() == Some(0x30) {
            self.i += 1;
            if matches!(self.peek(), Some(0x30..=0x39)) {
                return Err(self.unexpected());
            }
        } else {
            self.digits();
        }
        if self.peek() == Some(0x2E) {
            self.i += 1;
            if self.digits() == 0 {
                return Err(self.at("Unterminated fractional number"));
            }
        }
        if matches!(self.peek(), Some(0x65 | 0x45)) {
            self.i += 1;
            if matches!(self.peek(), Some(0x2B | 0x2D)) {
                self.i += 1;
            }
            if self.digits() == 0 {
                return Err(self.at("Exponent part is missing a number"));
            }
        }
        let text = String::from_utf16_lossy(&self.s[start..self.i]);
        Ok(text.parse().unwrap_or(f64::NAN))
    }

    fn string(&mut self) -> Result<Vec<u16>, Vec<u16>> {
        self.i += 1; // opening quote
        let mut out = Vec::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(self.at("Unterminated string"));
            };
            match c {
                0x22 => {
                    self.i += 1;
                    return Ok(out);
                }
                0x00..=0x1F => return Err(self.at("Bad control character in string literal")),
                0x5C => {
                    self.i += 1;
                    let Some(e) = self.peek() else {
                        return Err("Unexpected end of JSON input".encode_utf16().collect());
                    };
                    let simple = match e {
                        0x22 => Some(0x22),
                        0x5C => Some(0x5C),
                        0x2F => Some(0x2F),
                        0x62 => Some(0x08),
                        0x66 => Some(0x0C),
                        0x6E => Some(0x0A),
                        0x72 => Some(0x0D),
                        0x74 => Some(0x09),
                        _ => None,
                    };
                    if let Some(u) = simple {
                        out.push(u);
                        self.i += 1;
                    } else if e == 0x75 {
                        self.i += 1;
                        let mut v: u16 = 0;
                        for _ in 0..4 {
                            let d = self
                                .peek()
                                .and_then(|h| char::from_u32(u32::from(h)))
                                .and_then(|h| h.to_digit(16));
                            let Some(d) = d else {
                                return Err(self.at("Bad Unicode escape"));
                            };
                            v = v * 16 + d as u16;
                            self.i += 1;
                        }
                        out.push(v);
                    } else if e > 0xFF {
                        // V8's escape table covers Latin-1 only; beyond it the token is "illegal".
                        return Err(self.unexpected());
                    } else {
                        return Err(self.at("Bad escaped character"));
                    }
                }
                _ => {
                    out.push(c);
                    self.i += 1;
                }
            }
        }
    }

    fn array(&mut self) -> Result<crate::ipc::JsValue, Vec<u16>> {
        self.i += 1;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(0x5D) {
            self.i += 1;
            return Ok(crate::ipc::JsValue::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.ws();
            match self.peek() {
                Some(0x2C) => {
                    self.i += 1;
                    self.ws();
                }
                Some(0x5D) => {
                    self.i += 1;
                    return Ok(crate::ipc::JsValue::Array(items));
                }
                _ => return Err(self.at("Expected ',' or ']' after array element")),
            }
        }
    }

    fn object(&mut self) -> Result<crate::ipc::JsValue, Vec<u16>> {
        self.i += 1;
        let mut obj = crate::ipc::JsValue::Object(Vec::new());
        self.ws();
        if self.peek() == Some(0x7D) {
            self.i += 1;
            return Ok(obj);
        }
        let mut first = true;
        loop {
            if self.peek() != Some(0x22) {
                return Err(self.at(if first {
                    "Expected property name or '}'"
                } else {
                    "Expected double-quoted property name"
                }));
            }
            first = false;
            let key = self.string()?;
            self.ws();
            if self.peek() != Some(0x3A) {
                return Err(self.at("Expected ':' after property name"));
            }
            self.i += 1;
            self.ws();
            let v = self.value()?;
            crate::ipc::define(&mut obj, crate::ipc::JsString(key), v);
            self.ws();
            match self.peek() {
                Some(0x2C) => {
                    self.i += 1;
                    self.ws();
                }
                Some(0x7D) => {
                    self.i += 1;
                    return Ok(obj);
                }
                _ => return Err(self.at("Expected ',' or '}' after property value")),
            }
        }
    }
}

/// JS `parseInt(s, 10)`: leading whitespace, optional sign, then decimal digits; `NaN` if none.
pub fn parse_int(s: &str) -> f64 {
    let t = s.trim_start_matches(|c: char| matches!(c, '\t' | '\n' | '\u{B}' | '\u{C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'));
    let (neg, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let end = digits.bytes().position(|b| !b.is_ascii_digit()).unwrap_or(digits.len());
    if end == 0 {
        return f64::NAN;
    }
    let v: f64 = digits[..end].parse().unwrap_or(f64::NAN);
    if neg { -v } else { v }
}

/// JS `Math.round`: halves round toward +∞.
pub fn math_round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let f = x.floor();
    if x - f >= 0.5 { f + 1.0 } else { f }
}

/// JS `StringToNumber` (what `Number(s)` / arithmetic coercion does with a string).
pub fn string_to_number(s: &str) -> f64 {
    let t = s.trim_matches(|c: char| matches!(c, '\t' | '\n' | '\u{B}' | '\u{C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'));
    if t.is_empty() {
        return 0.0;
    }
    let radix = |p: &str, r: u32| -> Option<f64> {
        let d = t.strip_prefix(p).or_else(|| t.strip_prefix(&p.to_uppercase()))?;
        (!d.is_empty() && d.chars().all(|c| c.is_digit(r))).then(|| d.chars().fold(0.0, |acc, c| acc * f64::from(r) + f64::from(c.to_digit(r).unwrap_or(0))))
    };
    if let Some(v) = radix("0x", 16).or_else(|| radix("0o", 8)).or_else(|| radix("0b", 2)) {
        return v;
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    let ok = t.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
        && !t.contains("inf")
        && t.bytes().any(|b| b.is_ascii_digit());
    if !ok {
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// JS `Math.floor`.
pub fn math_floor(x: f64) -> f64 {
    x.floor()
}

/// JS `String#trim` (WhiteSpace + LineTerminator).
pub fn trim(s: &str) -> &str {
    s.trim_matches(|c: char| {
        matches!(c, '\t' | '\n' | '\u{B}' | '\u{C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}')
    })
}

/// Node 20 `util.inspect(string)` with default options at depth 0 (`formatPrimitive`):
/// quote choice, `strEscape`, the 10,000-unit cap and the newline split past 76 units (breakLength 80).
pub fn inspect_string(s: &[u16]) -> Vec<u16> {
    const MAX: usize = 10_000;
    let mut trailer: Vec<u16> = Vec::new();
    let mut s = s;
    if s.len() > MAX {
        let remaining = s.len() - MAX;
        s = &s[..MAX];
        trailer = format!("... {remaining} more character{}", if remaining > 1 { "s" } else { "" }).encode_utf16().collect();
    }
    let mut out = if s.len() > 16 && s.len() > 76 {
        // `/(?<=\n)/` split, each line escaped and quoted on its own.
        let mut lines: Vec<&[u16]> = s.split_inclusive(|&u| u == 10).collect();
        if lines.is_empty() {
            lines.push(&[]);
        }
        let mut out = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            if i > 0 {
                out.extend(" +\n  ".encode_utf16());
            }
            out.extend(str_escape(l));
        }
        out
    } else {
        str_escape(s)
    };
    out.extend(trailer);
    out
}

/// Node 20 `strEscape`.
fn str_escape(s: &[u16]) -> Vec<u16> {
    let has = |c: u16| s.contains(&c);
    let has_dollar_brace = s.windows(2).any(|w| w == [b'$' as u16, b'{' as u16]);
    // 39 = single quote, -2 = double, -1 = backtick.
    let quote: i32 = if has(39) {
        if !has(34) {
            -2
        } else if !has(96) && !has_dollar_brace {
            -1
        } else {
            39
        }
    } else {
        39
    };
    let q: u16 = match quote {
        -2 => 34,
        -1 => 96,
        _ => 39,
    };
    let mut out = vec![q];
    let mut i = 0;
    while i < s.len() {
        let p = s[i];
        if (quote == 39 && p == 39) || p == 92 || p < 32 || (p > 126 && p < 160) {
            let meta = match p {
                8 => "\\b".into(),
                9 => "\\t".into(),
                10 => "\\n".into(),
                11 => "\\x0B".into(),
                12 => "\\f".into(),
                13 => "\\r".into(),
                39 => "\\'".into(),
                92 => "\\\\".into(),
                _ => format!("\\x{p:02X}"),
            };
            out.extend(meta.encode_utf16());
        } else if (0xD800..=0xDFFF).contains(&p) {
            if p <= 0xDBFF && i + 1 < s.len() && (0xDC00..=0xDFFF).contains(&s[i + 1]) {
                out.extend([p, s[i + 1]]);
                i += 2;
                continue;
            }
            out.extend(format!("\\u{p:x}").encode_utf16());
        } else {
            out.push(p);
        }
        i += 1;
    }
    out.push(q);
    out
}


/// `Date.parse` for the ECMAScript date-time string format only (`YYYY`, `YYYY-MM`,
/// `YYYY-MM-DD`, optional `THH:mm[:ss[.sss]]`, optional `Z`/`±HH:mm`; `±YYYYYY` years).
/// Date-only forms are UTC; date-time forms without an offset are local time. `None` for
/// anything else (V8 then tries its legacy parser, which isn't ported).
pub fn date_parse_iso(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let mut i = 0;
    let num = |i: &mut usize, n: usize| -> Option<i64> {
        let d = b.get(*i..*i + n)?;
        if !d.iter().all(u8::is_ascii_digit) {
            return None;
        }
        *i += n;
        std::str::from_utf8(d).ok()?.parse().ok()
    };
    let year = match b.first()? {
        b'+' | b'-' => {
            let neg = b[0] == b'-';
            i = 1;
            let y = num(&mut i, 6)?;
            if neg && y == 0 {
                return None;
            }
            if neg { -y } else { y }
        }
        _ => num(&mut i, 4)?,
    };
    let (mut month, mut day) = (1, 1);
    if b.get(i) == Some(&b'-') {
        i += 1;
        month = num(&mut i, 2)?;
        if b.get(i) == Some(&b'-') {
            i += 1;
            day = num(&mut i, 2)?;
        }
    }
    let (mut h, mut m, mut sec, mut ms) = (0, 0, 0, 0);
    let mut has_time = false;
    let mut offset: Option<i64> = None;
    if b.get(i) == Some(&b'T') {
        has_time = true;
        i += 1;
        h = num(&mut i, 2)?;
        if b.get(i) != Some(&b':') {
            return None;
        }
        i += 1;
        m = num(&mut i, 2)?;
        if b.get(i) == Some(&b':') {
            i += 1;
            sec = num(&mut i, 2)?;
            if b.get(i) == Some(&b'.') {
                i += 1;
                let start = i;
                while b.get(i).is_some_and(u8::is_ascii_digit) {
                    i += 1;
                }
                if i == start {
                    return None;
                }
                let frac = &s[start..i];
                ms = format!("{:0<3}", &frac[..frac.len().min(3)]).parse().ok()?;
            }
        }
        match b.get(i) {
            Some(b'Z') => {
                offset = Some(0);
                i += 1;
            }
            Some(&c @ (b'+' | b'-')) => {
                i += 1;
                let oh = num(&mut i, 2)?;
                if b.get(i) != Some(&b':') {
                    return None;
                }
                i += 1;
                let om = num(&mut i, 2)?;
                if oh > 23 || om > 59 {
                    return None;
                }
                offset = Some(if c == b'+' { 1 } else { -1 } * (oh * 60 + om));
            }
            _ => {}
        }
    }
    if i != b.len() || !(1..=12).contains(&month) || h > 24 || m > 59 || sec > 59 || (h == 24 && (m, sec, ms) != (0, 0, 0)) {
        return None;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days_in = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if day < 1 || day > days_in[(month - 1) as usize] {
        return None;
    }
    // Days from civil (Howard Hinnant).
    let (y, mo) = if month <= 2 { (year - 1, month + 9) } else { (year, month - 3) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * mo + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let local = (days * 86_400 + h * 3600 + m * 60 + sec) * 1000 + ms;
    let t = match (has_time, offset) {
        (false, _) => local,
        (true, Some(off)) => local - off * 60_000,
        (true, None) => local - local_offset_ms(local),
    };
    let t = t as f64;
    (t.abs() <= 8.64e15).then_some(t)
}

/// The local zone's UTC offset for a local wall-clock time, via `mktime`.
fn local_offset_ms(local_ms: i64) -> i64 {
    let secs = local_ms.div_euclid(1000);
    // SAFETY: plain libc calls on caller-owned structs.
    unsafe {
        let t = secs as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::gmtime_r(&t, &mut tm).is_null() {
            return 0;
        }
        tm.tm_isdst = -1;
        let as_utc = libc::mktime(&mut tm);
        if as_utc == -1 {
            return 0;
        }
        (secs - as_utc as i64) * 1000
    }
}
