//! Port of `src/main/discord.js` over `@xhayper/discord-rpc` 1.3.0's IPC transport:
//! socket discovery, the handshake, one-frame-per-`readable` parsing (coalesced or partial
//! frames are dropped as malformed), PING/PONG, CLOSE, nonce'd requests that wait for
//! Discord's reply, the 10 s connect timeout, and the module's two globals, which any
//! client's `disconnected` event clears, including an orphaned or already-replaced one.

use crate::{
    ipc::{self, HandlerResult, JsValue},
    js,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    sync::{mpsc, oneshot},
};

#[cfg(unix)]
use tokio::net::UnixStream as IpcStream;
#[cfg(windows)]
use tokio::net::windows::named_pipe::{NamedPipeClient as IpcStream, ClientOptions};

#[cfg(unix)]
async fn connect_socket(path: &str) -> std::io::Result<IpcStream> { IpcStream::connect(path).await }
#[cfg(windows)]
async fn connect_socket(path: &str) -> std::io::Result<IpcStream> { ClientOptions::new().open(path) }

const CLIENT_ID: &str = "1473620585832517644";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;
const OP_PING: u32 = 3;
const OP_PONG: u32 = 4;

/// `rpcClient` / `rpcReady`.
#[derive(Default)]
struct Globals {
    client: Option<Arc<Client>>,
    ready: bool,
}

/// The module state.
#[derive(Default)]
pub struct Discord {
    globals: Arc<Mutex<Globals>>,
}

/// A request's outcome: Discord's reply, or a rejection (ERROR reply, connection closed).
type Reply = Result<JsValue, ()>;

enum Cmd {
    Request { cmd: &'static str, args: JsValue, reply: oneshot::Sender<Reply> },
    Destroy(oneshot::Sender<()>),
}

/// One `Client`: an actor owning the socket once there is one.
struct Client {
    tx: mpsc::UnboundedSender<Cmd>,
    /// `client.user` is set by a READY that carries `data.user`.
    has_user: Mutex<bool>,
    /// `transport.socket` exists (`destroy()` before that closes nothing).
    has_socket: std::sync::atomic::AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// `getTempDir()`: `XDG_RUNTIME_DIR ?? TMPDIR ?? TMP ?? TEMP ?? "/tmp"` through
/// `realpathSync` (an empty or missing directory throws).
#[cfg(unix)]
fn temp_dir() -> Option<String> {
    let v = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"].iter().find_map(|k| std::env::var_os(k));
    let dir = v.map_or_else(|| "/tmp".into(), |v| v.to_string_lossy().into_owned());
    std::fs::canonicalize(dir).ok().map(|p| p.to_string_lossy().into_owned())
}

/// `getSocket()`'s candidate list, or `None` when `getTempDir()` throws (the promise then
/// never settles).
#[cfg(unix)]
fn candidates() -> Option<Vec<String>> {
    let mut out = Vec::new();
    for sub in [&[][..], &["snap.discord"][..], &["app", "com.discordapp.Discord"][..]] {
        for id in 0..10 {
            let tmp = temp_dir()?;
            let mut parts: Vec<&str> = vec![&tmp];
            parts.extend_from_slice(sub);
            let name = format!("discord-ipc-{id}");
            parts.push(&name);
            let p = crate::node::path::posix::join(&parts);
            if crate::node::exists_sync(&p) {
                out.push(p);
            }
        }
    }
    Some(out)
}

#[cfg(windows)]
fn candidates() -> Option<Vec<String>> {
    // Unlike Unix, @xhayper/discord-rpc never existsSync's a pipe path.
    Some((0..10).map(|id| format!(r"\\?\pipe\discord-ipc-{id}")).collect())
}

fn frame(op: u32, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend(op.to_le_bytes());
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

/// `transport.send(message, op)`: `JSON.stringify`, or a throw (BigInt) that rejects the
/// request before anything is written.
fn encode(op: u32, message: &JsValue) -> Result<Vec<u8>, ()> {
    let json = ipc::json_stringify(message).map_err(|_| ())?;
    Ok(frame(op, json.unwrap_or_default().as_bytes()))
}

/// What one `readable` event yields: everything buffered must be exactly one frame.
fn parse_one(data: &[u8]) -> Option<(u32, JsValue)> {
    if data.len() < 8 {
        return None;
    }
    let op = u32::from_le_bytes(data[0..4].try_into().ok()?);
    let len = u32::from_le_bytes(data[4..8].try_into().ok()?) as usize;
    if data.len() != len + 8 {
        return None;
    }
    // `data.subarray(8, length + 8)`.
    let text: Vec<u16> = String::from_utf8_lossy(&data[8..8 + len]).encode_utf16().collect();
    js::json_parse(&text).ok().map(|v| (op, v))
}

/// The client actor. `connected` fires once on the first READY; `on_disconnect` runs when the
/// transport closes after that.
async fn run_client(
    socket: IpcStream,
    mut rx: mpsc::UnboundedReceiver<Cmd>,
    client: Arc<Client>,
    connected: oneshot::Sender<()>,
    globals: Arc<Mutex<Globals>>,
) {
    #[cfg(unix)]
    let (read, mut write) = socket.into_split();
    // NamedPipeClient has readiness + try_read but no owned read half.
    #[cfg(windows)]
    let read = Arc::new(socket);
    #[cfg(windows)]
    let mut write = PipeWrite(read.clone());
    #[cfg(unix)]
    let mut write_open = true;
    #[cfg(windows)]
    let write_open = true;

    if let Ok(b) = encode(OP_HANDSHAKE, &JsValue::object([("v", JsValue::Number(1.0)), ("client_id", CLIENT_ID.into())])) {
        let _ = write.write_all(&b).await;
    }
    let mut connected = Some(connected);
    let mut is_connected = false;
    let mut pending: HashMap<String, oneshot::Sender<Reply>> = HashMap::new();
    let mut closed = false;
    let mut destroy_waiters: Vec<oneshot::Sender<()>> = Vec::new();
    let mut buf = vec![0u8; 65536];
    // The transport `close` event, after READY: reject every pending request, then
    // `disconnected` clears the module's globals (whichever client is current).
    let on_close = |pending: &mut HashMap<String, oneshot::Sender<Reply>>, is_connected: &mut bool| {
        if std::mem::replace(is_connected, false) {
            for (_, tx) in pending.drain() {
                let _ = tx.send(Err(()));
            }
            let mut g = lock(&globals);
            g.ready = false;
            g.client = None;
        }
    };
    loop {
        tokio::select! {
            cmd = rx.recv() => match cmd {
                None => return,
                Some(Cmd::Request { cmd, args, reply }) => {
                    let nonce = uuid();
                    let msg = JsValue::object([("cmd", JsValue::from(cmd)), ("args", args), ("nonce", JsValue::from(nonce.as_str()))]);
                    match encode(OP_FRAME, &msg) {
                        Ok(b) => {
                            if !closed && write_open {
                                let _ = write.write_all(&b).await;
                            }
                            pending.insert(nonce, reply);
                        }
                        Err(()) => {
                            let _ = reply.send(Err(()));
                        }
                    }
                }
                Some(Cmd::Destroy(done)) => {
                    if closed {
                        let _ = done.send(());
                    } else {
                        let _ = write.shutdown().await;
                        #[cfg(unix)]
                        {
                            write_open = false;
                            closed = true;
                        }
                        on_close(&mut pending, &mut is_connected);
                        let _ = done.send(());
                        destroy_waiters.clear();
                        #[cfg(windows)]
                        return;
                    }
                }
            },
            n = read.readable(), if !closed => {
                if n.is_err() {
                    closed = true;
                    on_close(&mut pending, &mut is_connected);
                    continue;
                }
                // Drain everything buffered, as the `readable` handler does.
                let mut data = Vec::new();
                let mut eof = false;
                loop {
                    match read.try_read(&mut buf) {
                        Ok(0) => {
                            eof = true;
                            break;
                        }
                        Ok(n) => data.extend_from_slice(&buf[..n]),
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(_) => {
                            eof = true;
                            break;
                        }
                    }
                }
                if let Some((op, msg)) = parse_one(&data) {
                    match op {
                        OP_FRAME => {
                            let cmd = ipc::get_prop(&msg, "cmd");
                            let evt = ipc::get_prop(&msg, "evt");
                            // A non-object message throws inside the listener (uncaught).
                            if let (Ok(cmd), Ok(evt)) = (cmd, evt) {
                                if cmd == JsValue::from("DISPATCH") && evt == JsValue::from("READY") {
                                    let data = ipc::get_prop(&msg, "data").unwrap_or_default();
                                    if let Ok(user) = ipc::get_prop(&data, "user") {
                                        if user.truthy() {
                                            *lock(&client.has_user) = true;
                                        }
                                        if let Some(tx) = connected.take() {
                                            is_connected = true;
                                            let _ = tx.send(());
                                        }
                                    }
                                } else {
                                    let nonce = ipc::get_prop(&msg, "nonce").unwrap_or_default();
                                    let mut threw = false;
                                    if nonce.truthy()
                                        && let JsValue::String(n) = &nonce
                                        && let Some(tx) = pending.remove(&n.to_string_lossy())
                                    {
                                        if evt == JsValue::from("ERROR") {
                                            // `nonceObj.error.code = message.data.code` throws on a
                                            // null/undefined `data`: the request stays pending.
                                            let data = ipc::get_prop(&msg, "data").unwrap_or_default();
                                            if ipc::get_prop(&data, "code").is_ok() {
                                                let _ = tx.send(Err(()));
                                            } else {
                                                pending.insert(n.to_string_lossy(), tx);
                                                threw = true;
                                            }
                                        } else {
                                            let _ = tx.send(Ok(msg.clone()));
                                        }
                                    }
                                    // `this.emit(message.evt, message.data)`: the client's own
                                    // listeners answer to these names.
                                    if !threw && let JsValue::String(e) = &evt {
                                        match e.to_string_lossy().as_str() {
                                            "connected" => {
                                                if let Some(tx) = connected.take() {
                                                    is_connected = true;
                                                    let _ = tx.send(());
                                                }
                                            }
                                            "ready" => lock(&globals).ready = true,
                                            "disconnected" => {
                                                let mut g = lock(&globals);
                                                g.ready = false;
                                                g.client = None;
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                        OP_CLOSE => on_close(&mut pending, &mut is_connected),
                        OP_PING => {
                            if let Ok(b) = encode(OP_PONG, &msg) && write_open {
                                let _ = write.write_all(&b).await;
                            }
                        }
                        _ => {}
                    }
                }
                if eof {
                    closed = true;
                    on_close(&mut pending, &mut is_connected);
                }
            }
        }
    }
}

/// `crypto.randomUUID()`.
fn uuid() -> String {
    let mut b = [0u8; 16];
    let _ = getrandom::fill(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// `client.login()` (no scopes): connect, then `ready`. `Err` is a rejection.
async fn login(client: &Arc<Client>, rx: mpsc::UnboundedReceiver<Cmd>, globals: &Arc<Mutex<Globals>>) -> Result<(), ()> {
    let (ctx, crx) = oneshot::channel();
    let connect = async {
        let paths = match candidates() {
            Some(p) => p,
            // The executor threw: nothing settles until the timeout.
            None => std::future::pending().await,
        };
        for p in paths {
            if let Ok(socket) = connect_socket(&p).await {
                client.has_socket.store(true, std::sync::atomic::Ordering::SeqCst);
                tokio::spawn(run_client(socket, rx, client.clone(), ctx, globals.clone()));
                return crx.await.map_err(|_| ());
            }
        }
        Err(())
    };
    tokio::time::timeout(CONNECT_TIMEOUT, connect).await.map_err(|_| ())?
}

impl Discord {
    /// `connectDiscordRPC()`.
    async fn connect(&self) {
        let (client, rx) = {
            let mut g = lock(&self.globals);
            if g.client.is_some() {
                return;
            }
            let (tx, rx) = mpsc::unbounded_channel();
            let c = Arc::new(Client { tx, has_user: Mutex::new(false), has_socket: Default::default() });
            g.client = Some(c.clone());
            (c, rx)
        };
        match login(&client, rx, &self.globals).await {
            // The `ready` listener.
            Ok(()) => lock(&self.globals).ready = true,
            Err(()) => {
                let mut g = lock(&self.globals);
                g.ready = false;
                g.client = None;
            }
        }
    }

    /// `discord:connect`.
    pub async fn handle_connect(&self) -> HandlerResult {
        self.connect().await;
        Ok(JsValue::Bool(lock(&self.globals).ready))
    }

    /// `discord:disconnect` (`disconnectDiscordRPC()`).
    pub async fn handle_disconnect(&self) -> HandlerResult {
        let mut g = lock(&self.globals);
        if let Some(c) = g.client.take() {
            if c.has_socket.load(std::sync::atomic::Ordering::SeqCst) {
                let (tx, _rx) = oneshot::channel();
                let _ = c.tx.send(Cmd::Destroy(tx));
            }
            g.ready = false;
        }
        Ok(JsValue::Undefined)
    }

    fn current(&self) -> Option<Arc<Client>> {
        let g = lock(&self.globals);
        if g.ready { g.client.clone() } else { None }
    }

    async fn request(client: &Client, cmd: &'static str, args: JsValue) -> Reply {
        let (tx, rx) = oneshot::channel();
        if client.tx.send(Cmd::Request { cmd, args, reply: tx }).is_err() {
            return std::future::pending().await;
        }
        match rx.await {
            Ok(r) => r,
            // The actor is gone with the request unanswered: it never settles.
            Err(_) => std::future::pending().await,
        }
    }

    /// `discord:updatePresence`.
    pub async fn handle_update(&self, data: &JsValue) -> HandlerResult {
        let Some(client) = self.current() else { return Ok(JsValue::Undefined) };
        let Ok(activity) = activity(data) else { return Ok(JsValue::Undefined) };
        if !*lock(&client.has_user) {
            return Ok(JsValue::Undefined);
        }
        let args = JsValue::object([("pid", JsValue::Number(f64::from(std::process::id()))), ("activity", activity)]);
        let _ = Self::request(&client, "SET_ACTIVITY", args).await;
        Ok(JsValue::Undefined)
    }

    /// `discord:clearPresence`.
    pub async fn handle_clear(&self) -> HandlerResult {
        let Some(client) = self.current() else { return Ok(JsValue::Undefined) };
        if !*lock(&client.has_user) {
            return Ok(JsValue::Undefined);
        }
        let args = JsValue::object([("pid", JsValue::Number(f64::from(std::process::id())))]);
        let _ = Self::request(&client, "SET_ACTIVITY", args).await;
        Ok(JsValue::Undefined)
    }
}

/// `data.x` (throws on null/undefined `data`).
fn prop(data: &JsValue, k: &str) -> Result<JsValue, ()> {
    ipc::get_prop(data, k).map_err(|_| ())
}

/// `new Date(v).getTime()` for the values the renderer can send. Strings other than the
/// ECMAScript date-time format need V8's legacy parser and come out NaN here (Coverage gaps).
fn date_time(v: &JsValue) -> Result<f64, ()> {
    let clip = |n: f64| if !n.is_finite() || n.abs() > 8.64e15 { f64::NAN } else { n.trunc() + 0.0 };
    match v {
        JsValue::Date(t) => Ok(*t),
        JsValue::String(s) => Ok(js::date_parse_iso(&s.to_string_lossy()).unwrap_or(f64::NAN)),
        JsValue::BigInt(_) => Err(()),
        JsValue::Boxed(b) | JsValue::Shared(_, b) => date_time(b),
        JsValue::Object(_) | JsValue::Map(_) | JsValue::Set(_) | JsValue::RegExp { .. } | JsValue::Error { .. } | JsValue::Array(_) => {
            Ok(js::date_parse_iso(&ipc::to_js_string(v)).unwrap_or(f64::NAN))
        }
        v => Ok(clip(ipc::to_number(v).map_err(|_| ())?)),
    }
}

/// The object `discord:updatePresence` passes to `setActivity`, formatted as `setActivity`
/// sends it (`created_at` is `Date.now()`).
fn activity(data: &JsValue) -> Result<JsValue, ()> {
    let or = |v: JsValue, d: &str| if v.truthy() { v } else { JsValue::from(d) };
    let title = prop(data, "title")?;
    let details = or(title.clone(), "Unknown");
    let state = or(prop(data, "artist")?, "Unknown Artist");
    let large_key = or(prop(data, "thumbnail")?, "logo");
    let large_text = or(title, "Snowify");
    let start = prop(data, "startTimestamp")?;
    let start = if start.truthy() { Some(date_time(&start)?) } else { None };
    let end = prop(data, "endTimestamp")?;
    let end = if end.truthy() { Some(date_time(&end)?) } else { None };
    let video = prop(data, "videoId")?;
    let mut buttons = vec![JsValue::object([("label", JsValue::from("Get Snowify")), ("url", "https://snowify.cc".into())])];
    if video.truthy() {
        let url = format!("https://snowify.cc/track/{}", ipc::to_js_string(&video));
        buttons.push(JsValue::object([("label", JsValue::from("Listen on Snowify")), ("url", url.into())]));
    }
    let mut props: Vec<(&str, JsValue)> = vec![
        ("name", JsValue::Undefined),
        ("type", JsValue::Number(2.0)),
        ("created_at", JsValue::Number(crate::logger::now_ms() as f64)),
        ("instance", JsValue::Bool(false)),
        ("details", details),
        ("state", state),
    ];
    if start.is_some() || end.is_some() {
        let mut ts = Vec::new();
        if let Some(s) = start {
            ts.push(("start", JsValue::Number(s)));
        }
        if let Some(e) = end {
            ts.push(("end", JsValue::Number(e)));
        }
        props.push(("timestamps", JsValue::object(ts)));
    }
    props.push((
        "assets",
        JsValue::object([
            ("large_image", large_key),
            ("small_image", JsValue::from("logo")),
            ("large_text", large_text),
            ("small_text", JsValue::from("Snowify")),
        ]),
    ));
    props.push(("buttons", JsValue::Array(buttons)));
    Ok(JsValue::object(props))
}

#[cfg(windows)]
struct PipeWrite(Arc<IpcStream>);
#[cfg(windows)]
impl tokio::io::AsyncWrite for PipeWrite {
    fn poll_write(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, data: &[u8]) -> std::task::Poll<std::io::Result<usize>> {
        loop {
            std::task::ready!(self.0.poll_write_ready(cx))?;
            match self.0.try_write(data) {
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                r => return std::task::Poll::Ready(r),
            }
        }
    }
    fn poll_flush(self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> { std::task::Poll::Ready(Ok(())) }
    fn poll_shutdown(self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> { std::task::Poll::Ready(Ok(())) }
}
