//! Small JS-semantics helpers for handlers that walk untrusted JSON with optional chaining,
//! `for…of`, array methods and `||` fallbacks. `Err(())` is a thrown TypeError (or similar)
//! that the handler's `try` turns into its error result.

use crate::ipc::{self, JsString, JsValue};

pub type J<T> = Result<T, ()>;

/// `v.key` (throws on null/undefined).
pub fn get(v: &JsValue, key: &str) -> J<JsValue> {
    ipc::get_prop(v, key).map_err(|_| ())
}

/// `v?.key`.
pub fn opt(v: &JsValue, key: &str) -> JsValue {
    match v {
        JsValue::Undefined | JsValue::Null => JsValue::Undefined,
        v => ipc::get_prop(v, key).unwrap_or_default(),
    }
}

/// `v?.a?.b?.c` (array indices as `"0"`).
pub fn chain(v: &JsValue, keys: &[&str]) -> JsValue {
    keys.iter().fold(v.clone(), |acc, k| opt(&acc, k))
}

/// `a || b`.
pub fn or(a: JsValue, b: JsValue) -> JsValue {
    if a.truthy() { a } else { b }
}

/// `a ?? b`.
pub fn nullish_or(a: JsValue, b: JsValue) -> JsValue {
    if matches!(a, JsValue::Undefined | JsValue::Null) { b } else { a }
}

pub fn empty_array() -> JsValue {
    JsValue::Array(Vec::new())
}

/// `for (const x of v)`: arrays by element, strings by code point; anything else throws.
pub fn iter(v: &JsValue) -> J<Vec<JsValue>> {
    match v {
        JsValue::Array(items) => Ok(items.clone()),
        JsValue::String(s) => Ok(char::decode_utf16(s.0.iter().copied())
            .map(|c| match c {
                Ok(c) => JsValue::from(c.to_string()),
                Err(e) => JsValue::String(JsString(vec![e.unpaired_surrogate()])),
            })
            .collect()),
        JsValue::Shared(_, inner) => iter(inner),
        _ => Err(()),
    }
}

/// An array method on `v` (`.map`, `.filter`, `.slice`…): only arrays have them.
pub fn arr(v: &JsValue) -> J<Vec<JsValue>> {
    match v {
        JsValue::Array(items) => Ok(items.clone()),
        JsValue::Shared(_, inner) => arr(inner),
        _ => Err(()),
    }
}

/// `String(v)` keeping UTF-16.
pub fn units(v: &JsValue) -> Vec<u16> {
    match v {
        JsValue::String(s) => s.0.clone(),
        JsValue::Shared(_, inner) => units(inner),
        v => ipc::to_js_string(v).encode_utf16().collect(),
    }
}

/// `list.map(r => r.text).join(sep)`: `null`/`undefined` join as `""`.
pub fn join_texts(list: &[JsValue], sep: &str) -> J<JsValue> {
    let mut out: Vec<u16> = Vec::new();
    for (i, r) in list.iter().enumerate() {
        if i > 0 {
            out.extend(sep.encode_utf16());
        }
        let t = get(r, "text")?;
        if !matches!(t, JsValue::Undefined | JsValue::Null) {
            out.extend(units(&t));
        }
    }
    Ok(JsValue::String(JsString(out)))
}

/// `list.join(sep)` of plain values.
pub fn join(list: &[JsValue], sep: &str) -> JsValue {
    let mut out: Vec<u16> = Vec::new();
    for (i, v) in list.iter().enumerate() {
        if i > 0 {
            out.extend(sep.encode_utf16());
        }
        if !matches!(v, JsValue::Undefined | JsValue::Null) {
            out.extend(units(v));
        }
    }
    JsValue::String(JsString(out))
}

/// A string method (`.toLowerCase()`, `.includes()`…) on `v`: strings only.
pub fn string(v: &JsValue) -> J<String> {
    match v {
        JsValue::String(s) => Ok(s.to_string_lossy()),
        JsValue::Shared(_, inner) => string(inner),
        _ => Err(()),
    }
}

/// `===` for the values these handlers compare.
pub fn strict_eq(a: &JsValue, b: &str) -> bool {
    matches!(a, JsValue::String(s) if *s == JsString::from(b))
}

/// `a - b` as JS numbers (`NaN` when either side isn't numeric).
pub fn minus(a: &JsValue, b: &JsValue) -> f64 {
    let n = |v: &JsValue| match v {
        JsValue::Object(_) | JsValue::Map(_) | JsValue::Set(_) => f64::NAN,
        v => ipc::to_number(v).unwrap_or(f64::NAN),
    };
    n(a) - n(b)
}

/// `Array.prototype.sort(cmp)` as V8 does it: `undefined`s moved to the end uncompared; for
/// fewer than 64 elements V8's TimSort is one binary insertion sort over the first run, which
/// matters when the comparator is inconsistent (NaN results compare as "not less").
// ponytail: 64+ elements fall back to a stable merge sort, which equals V8 for consistent
// comparators; port TimSort's merges if an inconsistent one ever sorts that many.
pub fn v8_sort(items: Vec<JsValue>, cmp: &dyn Fn(&JsValue, &JsValue) -> J<f64>) -> J<Vec<JsValue>> {
    let undefined = items.iter().filter(|v| **v == JsValue::Undefined).count();
    let mut a: Vec<JsValue> = items.into_iter().filter(|v| *v != JsValue::Undefined).collect();
    let n = a.len();
    if n >= 2 && n < 64 {
        // CountAndMakeRun(0, n)
        let mut run = 2;
        let descending = cmp(&a[1], &a[0])? < 0.0;
        let mut prev = 1;
        for idx in 2..n {
            let order = cmp(&a[idx], &a[prev])?;
            // NaN breaks neither kind of run.
            if if descending { order >= 0.0 } else { order < 0.0 } {
                break;
            }
            prev = idx;
            run += 1;
        }
        if descending {
            a[..run].reverse();
        }
        // BinaryInsertionSort(0, run, n)
        for start in run..n {
            let pivot = a[start].clone();
            let (mut left, mut right) = (0, start);
            while left < right {
                let mid = left + ((right - left) >> 1);
                if cmp(&pivot, &a[mid])? < 0.0 {
                    right = mid;
                } else {
                    left = mid + 1;
                }
            }
            a[left..=start].rotate_right(1);
        }
    } else if n >= 64 {
        let mut err = false;
        a.sort_by(|x, y| match cmp(x, y) {
            Ok(o) if o < 0.0 => std::cmp::Ordering::Less,
            Ok(o) if o > 0.0 => std::cmp::Ordering::Greater,
            Ok(_) => std::cmp::Ordering::Equal,
            Err(()) => {
                err = true;
                std::cmp::Ordering::Equal
            }
        });
        if err {
            return Err(());
        }
    }
    a.extend(std::iter::repeat_n(JsValue::Undefined, undefined));
    Ok(a)
}
