//! Media origin parity. In Electron the renderer was a `file://` page whose
//! `onHeadersReceived` hook (`src/main/window.js`) added `Access-Control-Allow-Origin: *` to
//! every response whose URL contains `.googlevideo.com/`. So `<audio crossorigin>` + Web Audio
//! worked for YouTube streams and for local files (same origin), and `<img src="file://…">`
//! covers loaded. Platform webviews have neither the hook nor a file origin, so the shim routes
//! those element URLs through this loopback server, which serves `file:` URLs from disk and
//! forwards `http(s):` ones, adding the CORS header under the same rules.
//!
//! Requests: `GET|HEAD /<token>/m?a=<percent-encoded original src attribute>`; the shim maps
//! the URL back to the original for `src`/`currentSrc`/`getAttribute`. The random token keeps
//! other local processes out.

use futures_util::StreamExt;
use std::{io::SeekFrom, sync::OnceLock};
use tokio::{
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

pub struct MediaProxy {
    pub port: u16,
    pub token: String,
}

/// Electron's rule, verbatim: `details.url.includes('.googlevideo.com/')`.
pub fn needs_cors_header(url: &str) -> bool {
    url.contains(".googlevideo.com/")
}

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// Binds 127.0.0.1 on a free port and serves on the Tauri async runtime.
pub fn start() -> std::io::Result<MediaProxy> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let prefix = format!("/{token}/m?a=");
    tauri::async_runtime::spawn(async move {
        let Ok(listener) = TcpListener::from_std(listener) else {
            return;
        };
        while let Ok((sock, _)) = listener.accept().await {
            let prefix = prefix.clone();
            tauri::async_runtime::spawn(async move {
                let _ = serve(sock, &prefix).await;
            });
        }
    });
    Ok(MediaProxy { port, token })
}

const CLOSE: &str = "Connection: close\r\n";

async fn status(sock: &mut TcpStream, line: &str) -> std::io::Result<()> {
    sock.write_all(
        format!(
            "HTTP/1.1 {line}\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: 0\r\n{CLOSE}\r\n"
        )
        .as_bytes(),
    )
    .await
}

async fn serve(mut sock: TcpStream, prefix: &str) -> std::io::Result<()> {
    let mut head = Vec::new();
    let mut buf = [0u8; 4096];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = sock.read(&mut buf).await?;
        if n == 0 || head.len() > 64 * 1024 {
            return Ok(());
        }
        head.extend_from_slice(&buf[..n]);
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next().unwrap_or_default().split(' ');
    let method = request_line.next().unwrap_or_default().to_owned();
    let target = request_line.next().unwrap_or_default().to_owned();
    let headers: Vec<(String, String)> = lines
        .take_while(|l| !l.is_empty())
        .filter_map(|l| {
            l.split_once(':')
                .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        })
        .collect();

    if method == "OPTIONS" {
        let resp = format!(
            "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: *\r\nContent-Length: 0\r\n{CLOSE}\r\n"
        );
        return sock.write_all(resp.as_bytes()).await;
    }
    let (Some(attr), true) = (
        target.strip_prefix(prefix).map(percent_decode),
        method == "GET" || method == "HEAD",
    ) else {
        return status(&mut sock, "404 Not Found").await;
    };
    // Media URLs routed here are absolute (file: paths or googlevideo).
    let Ok(url) = url::Url::parse(&attr) else {
        return status(&mut sock, "404 Not Found").await;
    };
    let range = headers
        .iter()
        .find(|(k, _)| k == "range")
        .map(|(_, v)| v.as_str());
    if url.scheme() == "file" {
        let Ok(path) = url.to_file_path() else {
            return status(&mut sock, "404 Not Found").await;
        };
        return serve_file(&mut sock, &path, range, method == "HEAD").await;
    }
    forward(&mut sock, &url, &headers, method == "HEAD").await
}

/// MIME type by extension, as Chromium's file:// loader reports the common media types.
fn mime(path: &std::path::Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "mp3" => "audio/mpeg",
        "m4a" => "audio/x-m4a",
        "mp4" => "video/mp4",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "webm" => "video/webm",
        "weba" => "audio/webm",
        "png" => "image/png",
        "jpg" | "jpeg" | "jfif" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        _ => "application/octet-stream",
    }
}

/// Parses `bytes=start-end` / `bytes=start-` / `bytes=-suffix` against `len`.
fn parse_range(range: &str, len: u64) -> Option<(u64, u64)> {
    let spec = range.strip_prefix("bytes=")?.split(',').next()?.trim();
    let (a, b) = spec.split_once('-')?;
    let last = len.checked_sub(1)?;
    let (start, end) = match (a.trim(), b.trim()) {
        ("", suffix) => (len.saturating_sub(suffix.parse().ok()?), last),
        (s, "") => (s.parse().ok()?, last),
        (s, e) => (s.parse().ok()?, e.parse::<u64>().ok()?.min(last)),
    };
    (start <= end).then_some((start, end))
}

async fn serve_file(
    sock: &mut TcpStream,
    path: &std::path::Path,
    range: Option<&str>,
    head_only: bool,
) -> std::io::Result<()> {
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return status(sock, "404 Not Found").await;
    };
    let len = match file.metadata().await {
        Ok(m) if m.is_file() => m.len(),
        _ => return status(sock, "404 Not Found").await,
    };
    let ctype = mime(path);
    let (code, start, end) = match range {
        Some(r) => match parse_range(r, len) {
            Some((s, e)) => ("206 Partial Content", s, e),
            None => {
                let resp = format!(
                    "HTTP/1.1 416 Range Not Satisfiable\r\nAccess-Control-Allow-Origin: *\r\nContent-Range: bytes */{len}\r\nContent-Length: 0\r\n{CLOSE}\r\n"
                );
                return sock.write_all(resp.as_bytes()).await;
            }
        },
        None => ("200 OK", 0, len.saturating_sub(1)),
    };
    let body_len = if len == 0 { 0 } else { end - start + 1 };
    let mut out = format!(
        "HTTP/1.1 {code}\r\nAccess-Control-Allow-Origin: *\r\nContent-Type: {ctype}\r\nAccept-Ranges: bytes\r\nContent-Length: {body_len}\r\n"
    );
    if code.starts_with("206") {
        out.push_str(&format!("Content-Range: bytes {start}-{end}/{len}\r\n"));
    }
    out.push_str(CLOSE);
    out.push_str("\r\n");
    sock.write_all(out.as_bytes()).await?;
    if head_only || body_len == 0 {
        return sock.shutdown().await;
    }
    file.seek(SeekFrom::Start(start)).await?;
    let mut remaining = body_len;
    let mut chunk = vec![0u8; 64 * 1024];
    while remaining > 0 {
        let n = file
            .read(&mut chunk[..remaining.min(64 * 1024) as usize])
            .await?;
        if n == 0 {
            break;
        }
        sock.write_all(&chunk[..n]).await?;
        remaining -= n as u64;
    }
    sock.shutdown().await
}

async fn forward(
    sock: &mut TcpStream,
    url: &url::Url,
    headers: &[(String, String)],
    head_only: bool,
) -> std::io::Result<()> {
    let client = CLIENT.get_or_init(reqwest::Client::new);
    let method = if head_only {
        reqwest::Method::HEAD
    } else {
        reqwest::Method::GET
    };
    let mut req = client.request(method, url.as_str());
    for (k, v) in headers {
        if [
            "range",
            "accept",
            "accept-language",
            "user-agent",
            "if-range",
        ]
        .contains(&k.as_str())
        {
            req = req.header(k, v);
        }
    }
    let Ok(upstream) = req.send().await else {
        return status(sock, "502 Bad Gateway").await;
    };
    let mut out = format!("HTTP/1.1 {}\r\n", upstream.status());
    for (k, v) in upstream.headers() {
        let name = k.as_str();
        if [
            "connection",
            "transfer-encoding",
            "keep-alive",
            "access-control-allow-origin",
        ]
        .contains(&name)
        {
            continue;
        }
        if let Ok(v) = v.to_str() {
            out.push_str(&format!("{name}: {v}\r\n"));
        }
    }
    if needs_cors_header(upstream.url().as_str()) || needs_cors_header(url.as_str()) {
        out.push_str("Access-Control-Allow-Origin: *\r\n");
    }
    out.push_str(CLOSE);
    out.push_str("\r\n");
    sock.write_all(out.as_bytes()).await?;
    let mut body = upstream.bytes_stream();
    while let Some(chunk) = body.next().await {
        match chunk {
            Ok(c) => sock.write_all(&c).await?,
            Err(_) => break,
        }
    }
    sock.shutdown().await
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (
            b[i],
            b.get(i + 1).copied().and_then(hex),
            b.get(i + 2).copied().and_then(hex),
        ) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (c, _, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
