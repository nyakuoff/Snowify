//! Port of `src/main/logger.js`: the last 500 `console.log/warn/error` calls, served to the
//! in-app debug panel by `app:getLogs`.
//!
//! Terminal output prints the captured `msg`; Node's `util.inspect` formatting of the raw
//! arguments is not reproduced (log wording on the terminal is an accepted difference).

use crate::ipc::{self, JsValue};
use crate::js;
use std::{
    collections::VecDeque,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

const LOG_MAX: usize = 500;

/// One `{ ts, level, msg }` entry. `level` is any JS value because `app:appendLog` stores
/// whatever truthy level the renderer sends.
#[derive(Debug, Clone, PartialEq)]
pub struct LogEntry {
    pub ts: String,
    pub level: JsValue,
    pub msg: String,
}

impl LogEntry {
    /// The `{ ts, level, msg }` object `app:getLogs` returns.
    pub fn to_js(&self) -> JsValue {
        JsValue::object([
            ("ts", JsValue::from(self.ts.as_str())),
            ("level", self.level.clone()),
            ("msg", JsValue::from(self.msg.as_str())),
        ])
    }
}

/// One `console.*` argument, as `_captureLog` stringifies it.
#[derive(Debug, Clone)]
pub enum LogArg {
    /// Joins as `""`: `JSON.stringify(undefined)` (also functions, symbols) is `undefined`.
    Undefined,
    /// A JS string: used as-is.
    Str(String),
    /// Any other JS value, given as the JSON value `JSON.stringify` sees (after `toJSON`):
    /// a Date is `Json("2026-…Z")` and prints quoted, an Error is `Json({})`, NaN is
    /// `Json(null)`.
    Json(serde_json::Value),
    /// A value `JSON.stringify` throws on (BigInt, circular); holds its `String(a)`.
    Unserializable(String),
}

impl LogArg {
    /// A value received over IPC, stringified as `_captureLog` would.
    pub fn from_js(v: &JsValue) -> Self {
        match v {
            JsValue::String(s) => Self::Str(s.to_string_lossy()),
            v => match ipc::json_stringify(v) {
                Ok(None) => Self::Undefined,
                Ok(Some(s)) => Self::Unserializable(s),
                Err(ipc::StringifyThrows) => Self::Unserializable(ipc::to_js_string(v)),
            },
        }
    }
}

impl From<&str> for LogArg {
    fn from(s: &str) -> Self {
        Self::Str(s.into())
    }
}

impl From<String> for LogArg {
    fn from(s: String) -> Self {
        Self::Str(s)
    }
}

#[derive(Debug, Default)]
pub struct LogBuffer(VecDeque<LogEntry>);

impl LogBuffer {
    pub const fn new() -> Self {
        Self(VecDeque::new())
    }

    /// `_captureLog(level, args)` at time `now` (`Date.now()` ms).
    pub fn capture(&mut self, level: JsValue, args: &[LogArg], now: i64) {
        let ts = js::iso_string(now).chars().skip(11).take(12).collect();
        let msg = args
            .iter()
            .map(|a| match a {
                LogArg::Undefined => String::new(),
                LogArg::Str(s) | LogArg::Unserializable(s) => s.clone(),
                LogArg::Json(v) => js::json_stringify(v),
            })
            .collect::<Vec<_>>()
            .join(" ");
        self.0.push_back(LogEntry { ts, level, msg });
        if self.0.len() > LOG_MAX {
            self.0.pop_front();
        }
    }

    /// `[..._logBuffer]`.
    pub fn entries(&self) -> Vec<LogEntry> {
        self.0.iter().cloned().collect()
    }
}

/// The process-wide buffer (the source's module-level `_logBuffer`).
pub static LOGS: Mutex<LogBuffer> = Mutex::new(LogBuffer::new());

/// `Date.now()`.
pub fn now_ms() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_millis() as i64,
        Err(e) => -(e.duration().as_millis() as i64),
    }
}

fn emit(level: &str, args: &[LogArg]) {
    let mut logs = LOGS.lock().unwrap_or_else(|e| e.into_inner());
    logs.capture(JsValue::from(level), args, now_ms());
    let msg = &logs.0.back().map(|e| e.msg.clone()).unwrap_or_default();
    if level == "log" {
        println!("{msg}");
    } else {
        eprintln!("{msg}");
    }
}

/// `console.log(...args)`.
pub fn log(args: &[LogArg]) {
    emit("log", args);
}

/// `console.warn(...args)`.
pub fn warn(args: &[LogArg]) {
    emit("warn", args);
}

/// `console.error(...args)`.
pub fn error(args: &[LogArg]) {
    emit("error", args);
}
