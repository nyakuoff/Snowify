//! libuv 1.46 src/win/process.c@node-v20.18.3. Use CreateProcessW directly:
//! std::process::Command changes argv[0], extension search and .cmd/.bat handling.
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle},
    sync::{Arc, OnceLock},
};
use windows_sys::Win32::{
    Foundation::{
        ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
        WAIT_FAILED,
    },
    Security::SECURITY_ATTRIBUTES,
    Storage::FileSystem::{
        CreateFileW, FILE_FLAG_OVERLAPPED, FILE_READ_ATTRIBUTES, FILE_WRITE_ATTRIBUTES,
        OPEN_EXISTING, PIPE_ACCESS_DUPLEX, PIPE_ACCESS_INBOUND, WRITE_DAC,
    },
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
            JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectExtendedLimitInformation, SetInformationJobObject,
        },
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
        },
        Threading::{
            CREATE_UNICODE_ENVIRONMENT, CreateProcessW, GetExitCodeProcess, INFINITE,
            PROCESS_INFORMATION, STARTF_USESHOWWINDOW, STARTF_USESTDHANDLES, STARTUPINFOW,
            TerminateProcess, WaitForSingleObject,
        },
    },
};

use tokio::net::windows::named_pipe::NamedPipeServer;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// `quote_cmd_arg`, including empty arguments and trailing backslashes inside quotes.
fn quote(s: &str) -> String {
    if s.is_empty() {
        return "\"\"".into();
    }
    if !s.contains([' ', '\t', '"']) {
        return s.into();
    }
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in s.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        out.extend(std::iter::repeat_n(
            '\\',
            slashes * if c == '"' { 2 } else { 1 },
        ));
        slashes = 0;
        if c == '"' {
            out.push('\\');
        }
        out.push(c);
    }
    out.extend(std::iter::repeat_n('\\', slashes * 2));
    out.push('"');
    out
}

fn join_test(dir: &str, name: &str, cwd: &str) -> Option<String> {
    // search_path_join_test: notably a different drive's drive-relative path is left alone.
    let bytes = dir.as_bytes();
    let sep = |c| c == b'/' || c == b'\\';
    let (base, dir) = if bytes.len() > 2 && sep(bytes[0]) && sep(bytes[1]) {
        ("", dir)
    } else if !bytes.is_empty() && sep(bytes[0]) {
        (&cwd[..2], dir)
    } else if bytes.len() >= 2 && bytes[1] == b':' && (bytes.len() < 3 || !sep(bytes[2])) {
        if dir[..2].eq_ignore_ascii_case(&cwd[..2]) {
            (cwd, &dir[2..])
        } else {
            ("", dir)
        }
    } else if bytes.len() > 2 && bytes[1] == b':' {
        ("", dir)
    } else {
        (cwd, dir)
    };
    let mut p = base.to_owned();
    for part in [dir, name] {
        if !p.is_empty() && !p.ends_with(['/', '\\', ':']) {
            p.push('\\');
        }
        p.push_str(part);
    }
    std::fs::metadata(&p)
        .ok()
        .filter(|m| !m.is_dir())
        .map(|_| p)
}

fn walk_ext(dir: &str, name: &str, cwd: &str) -> Option<String> {
    // libuv tests the first dot, not the last one.
    if name.find('.').is_some_and(|i| i + 1 < name.len()) {
        if let Some(p) = join_test(dir, name, cwd) {
            return Some(p);
        }
    }
    let dot = if name.ends_with('.') { "" } else { "." };
    ["com", "exe"]
        .iter()
        .find_map(|ext| join_test(dir, &format!("{name}{dot}{ext}"), cwd))
}

fn search(file: &str) -> io::Result<String> {
    let cwd = std::env::current_dir()?.to_string_lossy().into_owned();
    if file != "." {
        if let Some(at) = file.rfind(['/', '\\', ':']) {
            if let Some(p) = walk_ext(&file[..=at], &file[at + 1..], &cwd) {
                return Ok(p);
            }
        } else {
            if let Some(p) = walk_ext("", file, &cwd) {
                return Ok(p);
            }
            // Quotes can enclose semicolons. Leading/trailing spaces are significant.
            let path = std::env::var("PATH").unwrap_or_default();
            let mut rest = path.as_str();
            while !rest.is_empty() {
                let quoted = rest.starts_with(['\'', '"']);
                let after_quote = if quoted {
                    rest[1..]
                        .find(rest.chars().next().unwrap())
                        .map_or(rest.len(), |n| n + 2)
                } else {
                    0
                };
                let end = rest[after_quote..]
                    .find(';')
                    .map_or(rest.len(), |n| n + after_quote);
                let mut dir = &rest[..end];
                rest = if end == rest.len() {
                    ""
                } else {
                    &rest[end + 1..]
                };
                if dir.is_empty() {
                    continue;
                }
                if quoted {
                    dir = &dir[1..];
                }
                if dir.ends_with(['\'', '"']) {
                    dir = &dir[..dir.len() - 1];
                }
                if let Some(p) = walk_ext(dir, file, &cwd) {
                    return Ok(p);
                }
            }
        }
    }
    Err(io::Error::from_raw_os_error(2))
}

// libuv src/win/pipe.c uv__create_stdio_pipe_pair: overlapped parent, synchronous
// inheritable child. Anonymous blocking pipes would leave uncancellable read threads
// behind when a timeout closes stdout but a grandchild still holds the write end.
fn pipe(parent_reads: bool) -> io::Result<(NamedPipeServer, OwnedHandle)> {
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(|e| io::Error::other(e.to_string()))?;
    let suffix: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let name = wide(&format!(
        r"\\.\pipe\snowify-stdio-{}-{suffix}",
        std::process::id()
    ));
    unsafe {
        let server_access = if parent_reads {
            PIPE_ACCESS_INBOUND
        } else {
            PIPE_ACCESS_DUPLEX
        };
        let h = CreateNamedPipeW(
            name.as_ptr(),
            server_access | FILE_FLAG_OVERLAPPED | WRITE_DAC,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            1,
            65536,
            65536,
            0,
            std::ptr::null(),
        );
        if h == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let server = OwnedHandle::from_raw_handle(h);
        let sa = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: 1,
        };
        let client_access = if parent_reads {
            GENERIC_WRITE | FILE_READ_ATTRIBUTES
        } else {
            GENERIC_READ | FILE_WRITE_ATTRIBUTES
        };
        let h = CreateFileW(
            name.as_ptr(),
            client_access | WRITE_DAC,
            0,
            &sa,
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if h == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let child = OwnedHandle::from_raw_handle(h);
        // Both ends already exist: like libuv, this call cannot block.
        if ConnectNamedPipe(server.as_raw_handle(), std::ptr::null_mut()) == 0
            && io::Error::last_os_error().raw_os_error() != Some(ERROR_PIPE_CONNECTED as i32)
        {
            return Err(io::Error::last_os_error());
        }
        let server = NamedPipeServer::from_raw_handle(server.into_raw_handle())?;
        Ok((server, child))
    }
}

fn job() -> io::Result<&'static OwnedHandle> {
    static JOB: OnceLock<Result<OwnedHandle, i32>> = OnceLock::new();
    JOB.get_or_init(|| unsafe {
        let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if h.is_null() {
            return Err(io::Error::last_os_error().raw_os_error().unwrap_or(0));
        }
        let h = OwnedHandle::from_raw_handle(h);
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_BREAKAWAY_OK
            | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK
            | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION
            | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            h.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            &info as *const _ as _,
            size_of_val(&info) as u32,
        ) == 0
        {
            return Err(io::Error::last_os_error().raw_os_error().unwrap_or(0));
        }
        Ok(h)
    })
    .as_ref()
    .map_err(|e| io::Error::from_raw_os_error(*e))
}

pub(super) struct Child {
    pub process: Arc<OwnedHandle>,
    pub stdin: Option<NamedPipeServer>,
    pub stdout: Option<NamedPipeServer>,
    pub stderr: Option<NamedPipeServer>,
    waiter: tokio::task::JoinHandle<io::Result<std::process::ExitStatus>>,
}
impl Child {
    pub async fn wait(&mut self) -> io::Result<std::process::ExitStatus> {
        (&mut self.waiter).await.map_err(io::Error::other)?
    }
}

pub(super) fn spawn(file: &str, args: &[String]) -> io::Result<Child> {
    // Node src/process_wrap.cc + util-inl.h rejects Windows batch files before uv_spawn,
    // including case variants and trailing spaces/dots, even if the file is missing.
    static BATCH: OnceLock<regex::Regex> = OnceLock::new();
    let batch = BATCH.get_or_init(|| {
        regex::Regex::new(r"(?i)\.([a-z0-9]+)[ \t\r\n\x0B\x0C]*[. \t\r\n\x0B\x0C]*$").unwrap()
    });
    if batch
        .captures(file)
        .is_some_and(|m| matches!(&m[1].to_ascii_lowercase()[..], "bat" | "cmd"))
    {
        return Err(io::Error::from_raw_os_error(-4071)); // UV_EINVAL
    }
    let app = wide(&search(file)?);
    let mut command = wide(
        &std::iter::once(file)
            .chain(args.iter().map(String::as_str))
            .map(quote)
            .collect::<Vec<_>>()
            .join(" "),
    );
    let (stdin, child_in) = pipe(false)?;
    let (stdout, child_out) = pipe(true)?;
    let (stderr, child_err) = pipe(true)?;
    let job = job()?;
    unsafe {
        let mut startup: STARTUPINFOW = std::mem::zeroed();
        startup.cb = size_of_val(&startup) as u32;
        startup.dwFlags = STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW;
        startup.wShowWindow = 10; // SW_SHOWDEFAULT, matching libuv's windowsHide=false.
        startup.hStdInput = child_in.as_raw_handle();
        startup.hStdOutput = child_out.as_raw_handle();
        startup.hStdError = child_err.as_raw_handle();
        let mut info: PROCESS_INFORMATION = std::mem::zeroed();
        if CreateProcessW(
            app.as_ptr(),
            command.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            CREATE_UNICODE_ENVIRONMENT,
            std::ptr::null(),
            std::ptr::null(),
            &startup,
            &mut info,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let process = Arc::new(OwnedHandle::from_raw_handle(info.hProcess));
        drop(OwnedHandle::from_raw_handle(info.hThread));
        // As in libuv, older/nested jobs denying assignment are tolerated.
        AssignProcessToJobObject(job.as_raw_handle(), process.as_raw_handle());
        drop((child_in, child_out, child_err));
        let wait_process = process.clone();
        let waiter = tokio::task::spawn_blocking(move || {
            use std::os::windows::process::ExitStatusExt;
            if WaitForSingleObject(wait_process.as_raw_handle(), INFINITE) == WAIT_FAILED {
                return Err(io::Error::last_os_error());
            }
            let mut code = 0;
            if GetExitCodeProcess(wait_process.as_raw_handle(), &mut code) == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(std::process::ExitStatus::from_raw(code))
        });
        Ok(Child {
            process,
            stdin: Some(stdin),
            stdout: Some(stdout),
            stderr: Some(stderr),
            waiter,
        })
    }
}

pub(super) fn terminate(process: &OwnedHandle) -> bool {
    // Only a successful TerminateProcess records exit_signal=SIGTERM in libuv.
    unsafe { TerminateProcess(process.as_raw_handle() as HANDLE, 1) != 0 }
}
