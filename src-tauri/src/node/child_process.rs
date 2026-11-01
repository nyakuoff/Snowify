//! Node 20 `child_process.execFile(file, args, { timeout }, callback)` on Linux, as the source
//! calls it: utf8 output, `maxBuffer` 1 MiB, `SIGTERM` on timeout, the same error objects and
//! the split between errors thrown synchronously and errors passed to the callback.

use crate::{
    ipc::{JsValue, to_js_string},
    js,
    net::JsErr,
};
use std::{future::Future, process::Stdio};
use tokio::{io::AsyncReadExt, process::Command, sync::mpsc};

const MAX_BUFFER: usize = 1024 * 1024;

/// What the callback receives: `(err, stdout, stderr)`.
#[derive(Debug)]
pub struct Output {
    pub err: Option<JsErr>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    /// `err.killed` (undefined on spawn and maxBuffer errors, which reads as false).
    pub fn killed(&self) -> bool {
        self.prop("killed").is_some_and(JsValue::truthy)
    }

    /// `err.signal`.
    pub fn signal(&self) -> Option<String> {
        self.prop("signal").and_then(JsValue::as_str)
    }

    fn prop(&self, k: &str) -> Option<&JsValue> {
        self.err.as_ref()?.props.iter().find(|(n, _)| n == k).map(|(_, v)| v)
    }
}

/// `child.kill('SIGTERM')` from outside the call (`cache:cancelDownload`).
#[derive(Clone, Debug)]
pub struct Killer(mpsc::UnboundedSender<()>);

impl Killer {
    pub fn kill(&self) {
        let _ = self.0.send(());
    }
}

/// Node's `ERR_INVALID_ARG_VALUE` (`The argument '<name>' <reason>. Received <inspect>`).
pub fn invalid_arg_value(name: &str, value: &JsValue, reason: &str) -> JsErr {
    let mut shown = match value {
        JsValue::String(s) => js::inspect_string(&s.0),
        v => to_js_string(v).encode_utf16().collect(),
    };
    if shown.len() > 128 {
        shown.truncate(128);
        shown.extend("...".encode_utf16());
    }
    let kind = if name.contains('.') { "property" } else { "argument" };
    JsErr {
        name: "TypeError [ERR_INVALID_ARG_VALUE]".into(),
        message: format!("The {kind} '{name}' {reason}. Received {}", String::from_utf16_lossy(&shown)).into(),
        props: vec![("code".into(), "ERR_INVALID_ARG_VALUE".into())],
    }
}

fn null_check(v: &JsValue, name: &str) -> Result<(), JsErr> {
    match v {
        JsValue::String(s) if s.0.contains(&0) => Err(invalid_arg_value(name, v, "must be a string without null bytes")),
        _ => Ok(()),
    }
}

/// `strsignal`-style names libuv reports (`signo_string`); "" for real-time signals.
fn signal_name(sig: i32) -> &'static str {
    const NAMES: [&str; 32] = [
        "", "SIGHUP", "SIGINT", "SIGQUIT", "SIGILL", "SIGTRAP", "SIGABRT", "SIGBUS", "SIGFPE", "SIGKILL", "SIGUSR1",
        "SIGSEGV", "SIGUSR2", "SIGPIPE", "SIGALRM", "SIGTERM", "SIGSTKFLT", "SIGCHLD", "SIGCONT", "SIGSTOP", "SIGTSTP",
        "SIGTTIN", "SIGTTOU", "SIGURG", "SIGXCPU", "SIGXFSZ", "SIGVTALRM", "SIGPROF", "SIGWINCH", "SIGIO", "SIGPWR",
        "SIGSYS",
    ];
    usize::try_from(sig).ok().and_then(|i| NAMES.get(i)).copied().unwrap_or("")
}

fn uv(errno: i32) -> (&'static str, i32) {
    let code = crate::uv_errors::UV_ERRORS.iter().find(|r| r.0 == errno).map_or("UNKNOWN", |r| r.1);
    (code, -errno)
}

/// Appends a chunk under the `maxBuffer` cap; true when the cap was crossed (the chunk is
/// then cut to fit, as Node does).
// ponytail: counts raw bytes and cuts on a byte boundary; Node counts the decoded chunk's
// UTF-8 length and cuts by UTF-16 units, which only differs for non-UTF-8 or non-ASCII
// output at the 1 MiB edge (and Node's own chunking there is timing-dependent).
fn take(buf: &mut Vec<u8>, chunk: &[u8]) -> bool {
    if buf.len() + chunk.len() > MAX_BUFFER {
        buf.extend_from_slice(&chunk[..MAX_BUFFER - buf.len()]);
        return true;
    }
    buf.extend_from_slice(chunk);
    false
}

/// `execFile(file, args, { timeout: timeout_ms }, cb)`. `Err` is what `execFile` throws
/// synchronously; otherwise the future resolves with what the callback gets.
pub fn exec_file(file: &str, args: &[JsValue], timeout_ms: u64) -> Result<(Killer, impl Future<Output = Output> + Send + use<>), JsErr> {
    let file_v = JsValue::from(file);
    null_check(&file_v, "file")?;
    if file.is_empty() {
        return Err(invalid_arg_value("file", &file_v, "cannot be empty"));
    }
    for (i, a) in args.iter().enumerate() {
        null_check(a, &format!("args[{i}]"))?;
    }
    // `Utf8Value` of `String(arg)`, which libuv then reads as a C string.
    let argv: Vec<String> = args
        .iter()
        .map(|a| {
            let s = to_js_string(a);
            s.split('\0').next().unwrap_or_default().to_owned()
        })
        .collect();
    let mut cmd_line = file.to_owned();
    if !args.is_empty() {
        let joined: Vec<String> =
            args.iter().map(|a| if matches!(a, JsValue::Undefined | JsValue::Null) { String::new() } else { to_js_string(a) }).collect();
        cmd_line = format!("{cmd_line} {}", joined.join(" "));
    }

    let mut command = Command::new(file);
    command.args(&argv).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // A pre-exec hook keeps std on fork + execvp, i.e. libuv's PATH search and its
    // `/bin/sh` fallback for ENOEXEC (posix_spawnp has neither).
    unsafe {
        command.pre_exec(|| Ok(()));
    }
    let (tx, mut rx) = mpsc::unbounded_channel();
    let spawned = match command.spawn() {
        Ok(c) => Ok(c),
        Err(e) => {
            let (code, errno) = uv(e.raw_os_error().unwrap_or(0));
            // Run-time failures go to the callback; anything else is thrown from execFile.
            if !matches!(code, "EACCES" | "EAGAIN" | "EMFILE" | "ENFILE" | "ENOENT") {
                return Err(JsErr {
                    name: "Error".into(),
                    message: format!("spawn {code}").into(),
                    props: vec![("errno".into(), JsValue::Number(f64::from(errno))), ("code".into(), code.into()), ("syscall".into(), "spawn".into())],
                });
            }
            let syscall = format!("spawn {file}");
            Err(JsErr {
                name: "Error".into(),
                message: format!("{syscall} {code}").into(),
                props: vec![
                    ("errno".into(), JsValue::Number(f64::from(errno))),
                    ("code".into(), code.into()),
                    ("syscall".into(), syscall.as_str().into()),
                    ("path".into(), file.into()),
                    ("spawnargs".into(), JsValue::Array(args.to_vec())),
                ],
            })
        }
    };
    let fut = async move {
        let cmd_prop = ("cmd".to_owned(), JsValue::from(cmd_line.as_str()));
        let mut child = match spawned {
            Ok(c) => c,
            Err(mut e) => {
                e.props.push(cmd_prop);
                return Output { err: Some(e), stdout: String::new(), stderr: String::new() };
            }
        };
        let _stdin = child.stdin.take();
        let mut out = child.stdout.take();
        let mut err = child.stderr.take();
        let pid = child.id().map(|p| p as i32);
        let (mut out_buf, mut err_buf) = (Vec::new(), Vec::new());
        let (mut chunk_o, mut chunk_e) = (vec![0u8; 65536], vec![0u8; 65536]);
        let mut exited: Option<(JsValue, JsValue)> = None;
        let (mut killed, mut child_killed) = (false, false);
        let mut ex: Option<JsErr> = None;
        let deadline = (timeout_ms > 0).then(|| tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms));
        let mut timer_armed = deadline.is_some();
        let signal = |exited: &Option<(JsValue, JsValue)>| -> bool {
            exited.is_none() && pid.is_some_and(|p| unsafe { libc::kill(p, libc::SIGTERM) } == 0)
        };
        loop {
            if exited.is_some() && out.is_none() && err.is_none() {
                break;
            }
            enum Ev {
                Exit(std::io::Result<std::process::ExitStatus>),
                Out(std::io::Result<usize>),
                Err(std::io::Result<usize>),
                Timeout,
                Kill,
            }
            let ev = tokio::select! {
                s = child.wait(), if exited.is_none() => Ev::Exit(s),
                n = async { out.as_mut().unwrap().read(&mut chunk_o).await }, if out.is_some() => Ev::Out(n),
                n = async { err.as_mut().unwrap().read(&mut chunk_e).await }, if err.is_some() => Ev::Err(n),
                () = async { tokio::time::sleep_until(deadline.unwrap()).await }, if timer_armed => Ev::Timeout,
                Some(()) = rx.recv() => Ev::Kill,
            };
            let mut overflow = None;
            match ev {
                Ev::Exit(s) => {
                    use std::os::unix::process::ExitStatusExt;
                    let s = s.ok();
                    let name = s.and_then(|s| s.signal()).map(signal_name).unwrap_or("");
                    exited = Some(if name.is_empty() {
                        (JsValue::Number(f64::from(s.and_then(|s| s.code()).unwrap_or(0))), JsValue::Null)
                    } else {
                        (JsValue::Null, JsValue::from(name))
                    });
                }
                Ev::Out(Ok(0) | Err(_)) => out = None,
                Ev::Err(Ok(0) | Err(_)) => err = None,
                Ev::Out(Ok(n)) => overflow = take(&mut out_buf, &chunk_o[..n]).then_some("stdout"),
                Ev::Err(Ok(n)) => overflow = take(&mut err_buf, &chunk_e[..n]).then_some("stderr"),
                Ev::Timeout => {
                    timer_armed = false;
                    out = None;
                    err = None;
                    killed = true;
                    child_killed |= signal(&exited);
                }
                Ev::Kill => child_killed |= signal(&exited),
            }
            if let Some(stream) = overflow {
                ex = Some(JsErr {
                    name: "RangeError [ERR_CHILD_PROCESS_STDIO_MAXBUFFER]".into(),
                    message: format!("{stream} maxBuffer length exceeded").into(),
                    props: vec![("code".into(), "ERR_CHILD_PROCESS_STDIO_MAXBUFFER".into())],
                });
                out = None;
                err = None;
                killed = true;
                child_killed |= signal(&exited);
            }
        }
        let (code, sig) = exited.unwrap_or((JsValue::Null, JsValue::Null));
        let stdout = String::from_utf8_lossy(&out_buf).into_owned();
        let stderr = String::from_utf8_lossy(&err_buf).into_owned();
        if ex.is_none() && code == JsValue::Number(0.0) && sig == JsValue::Null {
            return Output { err: None, stdout, stderr };
        }
        let mut e = ex.unwrap_or_else(|| JsErr {
            name: "Error".into(),
            message: format!("Command failed: {cmd_line}\n{stderr}").into(),
            props: vec![("code".into(), code), ("killed".into(), JsValue::Bool(child_killed || killed)), ("signal".into(), sig)],
        });
        e.props.push(cmd_prop);
        Output { err: Some(e), stdout, stderr }
    };
    Ok((Killer(tx), fut))
}

