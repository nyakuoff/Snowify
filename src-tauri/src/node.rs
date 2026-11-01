//! Node.js 20.18 (Electron 33) standard-library behaviour the source depends on: `path`,
//! `os.tmpdir`, and the `fs` calls it makes, including Node's error messages
//! (`ENOENT: no such file or directory, rmdir '/x'`), which reach the UI and the log panel.
//!
//! Ported from the JS embedded in the Electron 33 binary and from Node 20's `node_file.cc`.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub mod child_process;

pub mod path {
    /// Node's `normalizeString`. Byte indices stand in for UTF-16 ones: every comparison is
    /// against ASCII separators or dots, so the result is the same.
    fn normalize_string(
        path: &str,
        allow_above_root: bool,
        sep: char,
        is_sep: fn(u8) -> bool,
    ) -> String {
        let bytes = path.as_bytes();
        let mut res = String::new();
        let mut last_segment_length = 0usize;
        let mut last_slash: isize = -1;
        let mut dots: i32 = 0;
        let mut code = 0u8;
        for i in 0..=bytes.len() {
            if i < bytes.len() {
                code = bytes[i];
            } else if is_sep(code) {
                break;
            } else {
                code = b'/';
            }
            if is_sep(code) {
                if last_slash == i as isize - 1 || dots == 1 {
                    // no-op
                } else if dots == 2 {
                    if res.len() < 2 || last_segment_length != 2 || !res.ends_with("..") {
                        if res.len() > 2 {
                            match res.rfind(sep) {
                                None => {
                                    res.clear();
                                    last_segment_length = 0;
                                }
                                Some(idx) => {
                                    res.truncate(idx);
                                    last_segment_length = (res.len() as isize
                                        - 1
                                        - res.rfind(sep).map_or(-1, |j| j as isize))
                                        as usize;
                                }
                            }
                            last_slash = i as isize;
                            dots = 0;
                            continue;
                        } else if !res.is_empty() {
                            res.clear();
                            last_segment_length = 0;
                            last_slash = i as isize;
                            dots = 0;
                            continue;
                        }
                    }
                    if allow_above_root {
                        if res.is_empty() {
                            res.push_str("..");
                        } else {
                            res.push(sep);
                            res.push_str("..");
                        }
                        last_segment_length = 2;
                    }
                } else {
                    let seg = &path[(last_slash + 1) as usize..i];
                    if !res.is_empty() {
                        res.push(sep);
                    }
                    res.push_str(seg);
                    last_segment_length = (i as isize - last_slash - 1) as usize;
                }
                last_slash = i as isize;
                dots = 0;
            } else if code == b'.' && dots != -1 {
                dots += 1;
            } else {
                dots = -1;
            }
        }
        res
    }

    pub mod posix {
        fn is_sep(b: u8) -> bool {
            b == b'/'
        }

        /// `path.posix.normalize`.
        pub fn normalize(path: &str) -> String {
            if path.is_empty() {
                return ".".into();
            }
            let is_absolute = path.starts_with('/');
            let trailing = path.ends_with('/');
            let mut p = super::normalize_string(path, !is_absolute, '/', is_sep);
            if p.is_empty() {
                if is_absolute {
                    return "/".into();
                }
                return if trailing { "./" } else { "." }.into();
            }
            if trailing {
                p.push('/');
            }
            if is_absolute { format!("/{p}") } else { p }
        }

        /// `path.posix.join`.
        pub fn join(parts: &[&str]) -> String {
            let parts: Vec<&str> = parts.iter().copied().filter(|p| !p.is_empty()).collect();
            if parts.is_empty() {
                return ".".into();
            }
            normalize(&parts.join("/"))
        }
    }

    pub mod win32 {
        fn is_sep(b: u8) -> bool {
            b == b'/' || b == b'\\'
        }

        fn is_device_root(b: u8) -> bool {
            b.is_ascii_alphabetic()
        }

        /// `path.win32.normalize` (Node 20.18).
        pub fn normalize(path: &str) -> String {
            let b = path.as_bytes();
            let len = b.len();
            if len == 0 {
                return ".".into();
            }
            let mut root_end = 0;
            let mut device: Option<String> = None;
            let mut is_absolute = false;
            let code = b[0];
            if path.encode_utf16().count() == 1 {
                return if code == b'/' {
                    "\\".into()
                } else {
                    path.into()
                };
            }
            if is_sep(code) {
                is_absolute = true;
                if is_sep(b[1]) {
                    let mut j = 2;
                    let mut last = j;
                    while j < len && !is_sep(b[j]) {
                        j += 1;
                    }
                    if j < len && j != last {
                        let first_part = &path[last..j];
                        last = j;
                        while j < len && is_sep(b[j]) {
                            j += 1;
                        }
                        if j < len && j != last {
                            last = j;
                            while j < len && !is_sep(b[j]) {
                                j += 1;
                            }
                            if j == len {
                                return format!("\\\\{first_part}\\{}\\", &path[last..]);
                            }
                            if j != last {
                                device = Some(format!("\\\\{first_part}\\{}", &path[last..j]));
                                root_end = j;
                            }
                        }
                    }
                } else {
                    root_end = 1;
                }
            } else if is_device_root(code) && b[1] == b':' {
                device = Some(path[..2].to_owned());
                root_end = 2;
                if len > 2 && is_sep(b[2]) {
                    is_absolute = true;
                    root_end = 3;
                }
            }

            let mut tail = if root_end < len {
                super::normalize_string(&path[root_end..], !is_absolute, '\\', is_sep)
            } else {
                String::new()
            };
            if tail.is_empty() && !is_absolute {
                tail = ".".into();
            }
            if !tail.is_empty() && is_sep(b[len - 1]) {
                tail.push('\\');
            }
            if !is_absolute && device.is_none() && path.contains(':') {
                // CVE-2024-36139: keep a relative result from looking like a drive path.
                let t = tail.as_bytes();
                if t.len() >= 2 && is_device_root(t[0]) && t[1] == b':' {
                    return format!(".\\{tail}");
                }
                for (index, _) in path.match_indices(':') {
                    if index == len - 1 || is_sep(b[index + 1]) {
                        return format!(".\\{tail}");
                    }
                }
            }
            match device {
                None if is_absolute => format!("\\{tail}"),
                None => tail,
                Some(d) if is_absolute => format!("{d}\\{tail}"),
                Some(d) => format!("{d}{tail}"),
            }
        }

        /// `path.win32.join` (Node 20.18).
        pub fn join(parts: &[&str]) -> String {
            let parts: Vec<&str> = parts.iter().copied().filter(|p| !p.is_empty()).collect();
            let Some(first) = parts.first() else {
                return ".".into();
            };
            let mut joined = parts.join("\\");
            // Don't let normalize() mistake a leading `\\` for a UNC root unless the first part
            // clearly is one (exactly two slashes then a non-slash).
            let f = first.as_bytes();
            let mut needs_replace = true;
            let mut slash_count = 0;
            if is_sep(f[0]) {
                slash_count += 1;
                if f.len() > 1 && is_sep(f[1]) {
                    slash_count += 1;
                    if f.len() > 2 {
                        if is_sep(f[2]) {
                            slash_count += 1;
                        } else {
                            needs_replace = false;
                        }
                    }
                }
            }
            if needs_replace {
                let jb = joined.as_bytes();
                while slash_count < jb.len() && is_sep(jb[slash_count]) {
                    slash_count += 1;
                }
                if slash_count >= 2 {
                    joined = format!("\\{}", &joined[slash_count..]);
                }
            }
            normalize(&joined)
        }
    }

    /// `path.posix` functions beyond join/normalize, as Node 20.18 implements them. Indices
    /// are bytes; every comparison is against ASCII '/' or '.', so results match JS.
    pub mod posix_more {
        /// `path.posix.basename(path)` (no suffix argument).
        pub fn basename(path: &str) -> String {
            let b = path.as_bytes();
            let (mut start, mut end, mut matched_slash) = (0usize, None::<usize>, true);
            for i in (0..b.len()).rev() {
                if b[i] == b'/' {
                    if !matched_slash {
                        start = i + 1;
                        break;
                    }
                } else if end.is_none() {
                    matched_slash = false;
                    end = Some(i + 1);
                }
            }
            end.map_or_else(String::new, |e| path[start..e].to_owned())
        }

        /// `path.posix.basename(path, suffix)`.
        pub fn basename_ext(path: &str, suffix: &str) -> String {
            let (p, sfx) = (path.as_bytes(), suffix.as_bytes());
            if sfx.is_empty() || sfx.len() > p.len() {
                return basename(path);
            }
            if suffix == path {
                return String::new();
            }
            let (mut start, mut end) = (0usize, None::<usize>);
            let mut matched_slash = true;
            let mut ext_idx = sfx.len() as isize - 1;
            let mut first_non_slash_end = None::<usize>;
            for i in (0..p.len()).rev() {
                let code = p[i];
                if code == b'/' {
                    if !matched_slash {
                        start = i + 1;
                        break;
                    }
                } else {
                    if first_non_slash_end.is_none() {
                        matched_slash = false;
                        first_non_slash_end = Some(i + 1);
                    }
                    if ext_idx >= 0 {
                        if code == sfx[ext_idx as usize] {
                            ext_idx -= 1;
                            if ext_idx == -1 {
                                end = Some(i);
                            }
                        } else {
                            ext_idx = -1;
                            end = first_non_slash_end;
                        }
                    }
                }
            }
            let end = match end {
                Some(e) if e == start => first_non_slash_end.unwrap_or(0),
                Some(e) => e,
                None => p.len(),
            };
            path.get(start..end.max(start)).unwrap_or_default().to_owned()
        }

        /// `path.posix.dirname(path)`.
        pub fn dirname(path: &str) -> String {
            let b = path.as_bytes();
            if b.is_empty() {
                return ".".into();
            }
            let has_root = b[0] == b'/';
            let (mut end, mut matched_slash) = (None::<usize>, true);
            for i in (1..b.len()).rev() {
                if b[i] == b'/' {
                    if !matched_slash {
                        end = Some(i);
                        break;
                    }
                } else {
                    matched_slash = false;
                }
            }
            match end {
                None if has_root => "/".into(),
                None => ".".into(),
                Some(1) if has_root => "//".into(),
                Some(e) => path[..e].to_owned(),
            }
        }

        /// `path.posix.extname(path)`.
        pub fn extname(path: &str) -> String {
            let b = path.as_bytes();
            let (mut start_dot, mut start_part, mut end) = (None::<usize>, 0usize, None::<usize>);
            let (mut matched_slash, mut pre_dot_state) = (true, 0i8);
            for i in (0..b.len()).rev() {
                let c = b[i];
                if c == b'/' {
                    if !matched_slash {
                        start_part = i + 1;
                        break;
                    }
                    continue;
                }
                if end.is_none() {
                    matched_slash = false;
                    end = Some(i + 1);
                }
                if c == b'.' {
                    if start_dot.is_none() {
                        start_dot = Some(i);
                    } else if pre_dot_state != 1 {
                        pre_dot_state = 1;
                    }
                } else if start_dot.is_some() {
                    pre_dot_state = -1;
                }
            }
            match (start_dot, end) {
                (Some(d), Some(e))
                    if pre_dot_state != 0
                        && !(pre_dot_state == 1 && d + 1 == e && d == start_part + 1) =>
                {
                    path[d..e].to_owned()
                }
                _ => String::new(),
            }
        }

        /// `path.posix.resolve(...paths)` with `cwd` standing in for `process.cwd()`.
        pub fn resolve(paths: &[&str], cwd: &str) -> String {
            let mut resolved = String::new();
            let mut absolute = false;
            for p in paths.iter().rev() {
                if absolute {
                    break;
                }
                if p.is_empty() {
                    continue;
                }
                resolved = format!("{p}/{resolved}");
                absolute = p.starts_with('/');
            }
            if !absolute {
                resolved = format!("{cwd}/{resolved}");
                absolute = cwd.starts_with('/');
            }
            let resolved = super::normalize_string(&resolved, !absolute, '/', |b| b == b'/');
            if absolute {
                format!("/{resolved}")
            } else if resolved.is_empty() {
                ".".into()
            } else {
                resolved
            }
        }
    }

    #[cfg(windows)]
    pub use win32::join;

    #[cfg(not(windows))]
    pub use posix::join;
}

/// `os.tmpdir()` (Node 20.18), given an env lookup. Node treats an empty variable as unset.
pub fn tmpdir_from(windows: bool, var: impl Fn(&str) -> Option<String>) -> String {
    let var = |k: &str| var(k).filter(|v| !v.is_empty());
    if windows {
        let p = var("TEMP").or_else(|| var("TMP")).unwrap_or_else(|| {
            let root = var("SystemRoot").or_else(|| var("windir"));
            format!("{}\\temp", root.as_deref().unwrap_or("undefined"))
        });
        if p.encode_utf16().count() > 1 && p.ends_with('\\') && !p.ends_with(":\\") {
            return p[..p.len() - 1].to_owned();
        }
        p
    } else {
        let p = var("TMPDIR")
            .or_else(|| var("TMP"))
            .or_else(|| var("TEMP"))
            .unwrap_or_else(|| "/tmp".into());
        if p.encode_utf16().count() > 1 && p.ends_with('/') {
            return p[..p.len() - 1].to_owned();
        }
        p
    }
}

/// `os.tmpdir()` for this process. Assumes the binary is not setuid/setgid (Node ignores
/// TMPDIR etc. then); Snowify is never installed that way.
pub fn tmpdir() -> String {
    tmpdir_from(cfg!(windows), |k| {
        std::env::var_os(k).map(|v| v.to_string_lossy().into_owned())
    })
}

/// A Node `fs` error: libuv code (`ENOENT`), the full `err.message`, and the own enumerable
/// properties Node puts on it (`errno`, `code`, `syscall`, `path`), which is what
/// `console.error(..., err)` writes into the log panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsError {
    pub code: &'static str,
    pub message: String,
    pub errno: i32,
    pub syscall: String,
    pub path: Option<String>,
    pub dest: Option<String>,
}

impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FsError {}

impl FsError {
    /// Node's `uvException`: `${code}: ${description}, ${syscall} '${path}'`.
    pub fn uv(err: &io::Error, syscall: &str, path: &Path) -> Self {
        #[cfg(windows)]
        let owned = windows_fs::error(err, syscall, path);
        #[cfg(windows)]
        let err = &owned;
        let (code, desc, errno) = Self::describe(err);
        #[cfg(windows)]
        let path = windows_fs::error_path(path);
        Self::with(
            code,
            desc,
            errno,
            syscall,
            Some(path.to_string_lossy().into_owned()),
            None,
        )
    }

    fn describe(err: &io::Error) -> (&'static str, &'static str, i32) {
        let errno = crate::uv_errors::errno(err.raw_os_error().unwrap_or(0));
        let (code, desc) = crate::uv_errors::UV_ERRORS
            .iter()
            .find(|r| r.0 == errno)
            .map_or(("UNKNOWN", "unknown error"), |r| (r.1, r.2));
        (code, desc, errno)
    }

    /// An error with a `dest` (`copyfile 'src' -> 'dest'`).
    pub fn uv_dest(err: &io::Error, syscall: &str, path: &Path, dest: &Path) -> Self {
        let mut e = Self::uv(err, syscall, path);
        #[cfg(windows)]
        let dest = windows_fs::error_path(dest);
        let dest = dest.to_string_lossy().into_owned();
        e.message = format!("{} -> '{dest}'", e.message);
        e.dest = Some(dest);
        e
    }

    /// An error with no path in its message (`read`, `write` on an open descriptor).
    pub fn uv_nopath(err: &io::Error, syscall: &str) -> Self {
        #[cfg(windows)]
        let owned = windows_fs::error(err, syscall, Path::new(""));
        #[cfg(windows)]
        let err = &owned;
        let (code, desc, errno) = Self::describe(err);
        Self::with(code, desc, errno, syscall, None, None)
    }

    fn with(
        code: &'static str,
        desc: &str,
        errno: i32,
        syscall: &str,
        path: Option<String>,
        dest: Option<String>,
    ) -> Self {
        let message = match &path {
            Some(p) => format!("{code}: {desc}, {syscall} '{p}'"),
            None => format!("{code}: {desc}, {syscall}"),
        };
        Self {
            code,
            message,
            errno: -errno,
            syscall: syscall.into(),
            path,
            dest,
        }
    }

    /// `JSON.stringify(err)`: `{"errno":-2,"code":"ENOENT","syscall":"open","path":"/x"}`.
    pub fn to_json(&self) -> serde_json::Value {
        let mut m = serde_json::Map::new();
        m.insert("errno".into(), self.errno.into());
        m.insert("code".into(), self.code.into());
        m.insert("syscall".into(), self.syscall.as_str().into());
        if let Some(p) = &self.path {
            m.insert("path".into(), p.as_str().into());
        }
        if let Some(d) = &self.dest {
            m.insert("dest".into(), d.as_str().into());
        }
        serde_json::Value::Object(m)
    }
}

/// Electron's `app.getPath('userData')` for app name `snowify` (package.json `name`).
pub fn user_data_dir() -> String {
    let var = |k: &str| {
        std::env::var_os(k)
            .map(|v| v.to_string_lossy().into_owned())
            .filter(|v| !v.is_empty())
    };
    let home = || var("HOME").unwrap_or_default();
    let app_data = if cfg!(windows) {
        var("APPDATA").unwrap_or_default()
    } else {
        var("XDG_CONFIG_HOME").unwrap_or_else(|| format!("{}/.config", home()))
    };
    path::join(&[&app_data, "snowify"])
}

/// `fs.existsSync`: true if the path resolves (following symlinks), false on any error.
pub fn exists_sync(path: &str) -> bool {
    fs::metadata(path).is_ok()
}

/// `fs.mkdirSync(path, { recursive: true })`: Node 20's `MKDirpSync`. Errors name the
/// original `path` and syscall `mkdir`.
pub fn mkdir_recursive_sync(path: &str) -> Result<(), FsError> {
    #[cfg(unix)]
    let sep = if cfg!(windows) { '\\' } else { '/' };
    let fail = |e: io::Error| FsError::uv(&e, "mkdir", Path::new(path));
    let errno = |code: &str| {
        let n = crate::uv_errors::UV_ERRORS
            .iter()
            .find(|r| r.1 == code)
            .map_or(0, |r| r.0);
        #[cfg(unix)]
        let raw = n;
        #[cfg(windows)]
        let raw = -n;
        io::Error::from_raw_os_error(raw)
    };
    let mut stack = vec![path.to_owned()];
    while let Some(next) = stack.pop() {
        let mut err = fs::create_dir(&next).err();
        #[cfg(windows)]
        { err = err.map(|e| windows_fs::mkdir_error(&e)); }
        loop {
            match err.as_ref().map(|e| crate::uv_errors::errno(e.raw_os_error().unwrap_or(0))) {
                None => {
                    if stack.is_empty() {
                        return Ok(());
                    }
                }
                Some(e)
                    if [
                        libc_errno::EACCES,
                        libc_errno::ENOSPC,
                        libc_errno::ENOTDIR,
                        libc_errno::EPERM,
                    ]
                    .contains(&e) =>
                {
                    return Err(fail(err.unwrap()));
                }
                Some(libc_errno::ENOENT) => {
                    #[cfg(unix)]
                    let dirname = next
                        .rfind(sep)
                        .map_or(next.as_str(), |i| &next[..i])
                        .to_owned();
                    #[cfg(windows)]
                    let dirname = Path::new(&next).parent().filter(|p| !p.as_os_str().is_empty())
                        .map_or_else(|| next.clone(), |p| p.to_string_lossy().into_owned());
                    if dirname != next {
                        stack.push(next.clone());
                        stack.push(dirname);
                    } else if stack.is_empty() {
                        err = Some(errno("EEXIST"));
                        continue;
                    }
                }
                Some(orig) => match fs::metadata(&next) {
                    Ok(m) if !m.is_dir() => {
                        let code = if orig == libc_errno::EEXIST && !stack.is_empty() {
                            "ENOTDIR"
                        } else {
                            "EEXIST"
                        };
                        return Err(fail(errno(code)));
                    }
                    Ok(_) => {}
                    Err(e) => return Err(fail(e)),
                },
            }
            break;
        }
    }
    Ok(())
}

/// `fs.readFileSync(path, 'utf-8')`: invalid UTF-8 becomes U+FFFD.
pub fn read_file_utf8(path: &str) -> Result<String, FsError> {
    read_file(path).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// `fs.readFileSync(path)`: the raw bytes.
#[cfg(unix)]
pub fn read_file(path: &str) -> Result<Vec<u8>, FsError> {
    use std::io::Read;
    let mut f = fs::File::open(path).map_err(|e| FsError::uv(&e, "open", Path::new(path)))?;
    let mut out = Vec::new();
    f.read_to_end(&mut out)
        .map_err(|e| FsError::uv_nopath(&e, "read"))?;
    Ok(out)
}

/// `fs.writeFileSync(path, data)` (creates with mode 0o666 & ~umask, truncates).
pub fn write_file(path: &str, data: &[u8]) -> Result<(), FsError> {
    use std::io::Write;
    let mut f = fs::File::create(path).map_err(|e| FsError::uv(&e, "open", Path::new(path)))?;
    f.write_all(data)
        .map_err(|e| FsError::uv_nopath(&e, "write"))
}

/// `fs.unlinkSync(path)`.
pub fn unlink_sync(path: &str) -> Result<(), FsError> {
    #[cfg(unix)]
    let result = fs::remove_file(path);
    #[cfg(windows)]
    let result = windows_fs::unlink(Path::new(path));
    result.map_err(|e| FsError::uv(&e, "unlink", Path::new(path)))
}

/// `fs.readdirSync(path)`: names (lossy UTF-8) in libuv's platform order.
pub fn readdir_sync(path: &str) -> Result<Vec<String>, FsError> {
    let names: Vec<std::ffi::OsString> = fs::read_dir(path)
        .and_then(|rd| rd.map(|d| d.map(|d| d.file_name())).collect())
        .map_err(|e| FsError::uv(&e, "scandir", Path::new(path)))?;
    #[cfg(unix)]
    let names = { let mut names = names; names.sort(); names };
    Ok(names
        .into_iter()
        .map(|n| n.to_string_lossy().into_owned())
        .collect())
}

/// `fs.statSync(path).isDirectory()` (follows symlinks; throws on error).
pub fn stat_is_dir(path: &str) -> Result<bool, FsError> {
    fs::metadata(path)
        .map(|m| m.is_dir())
        .map_err(|e| FsError::uv(&e, "stat", Path::new(path)))
}

/// `fs.copyFileSync(src, dest)`: libuv's `uv_fs_copyfile` (dest gets src's mode; copying a
/// file onto itself is a no-op). Every failure is reported as `copyfile 'src' -> 'dest'`.
#[cfg(unix)]
pub fn copy_file_sync(src: &str, dest: &str) -> Result<(), FsError> {
    use std::io::{Read, Write};
    #[cfg(unix)]
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    let fail = |e: io::Error| FsError::uv_dest(&e, "copyfile", Path::new(src), Path::new(dest));
    let mut input = fs::File::open(src).map_err(fail)?;
    let src_meta = input.metadata().map_err(fail)?;
    // A directory source fails before the destination is created.
    if src_meta.is_dir() {
        return Err(fail(io::Error::from_raw_os_error(21)));
    }
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true);
    #[cfg(unix)]
    opts.mode(src_meta.mode());
    let mut output = opts.open(dest).map_err(fail)?;
    let dst_meta = output.metadata().map_err(fail)?;
    #[cfg(unix)]
    if src_meta.dev() == dst_meta.dev() && src_meta.ino() == dst_meta.ino() {
        return Ok(());
    }
    output.set_len(0).map_err(fail)?;
    #[cfg(unix)]
    output
        .set_permissions(fs::Permissions::from_mode(src_meta.mode()))
        .map_err(fail)?;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = input.read(&mut buf).map_err(fail)?;
        if n == 0 {
            return Ok(());
        }
        output.write_all(&buf[..n]).map_err(fail)?;
    }
}

/// `fs.rmSync(path, { recursive: true })` (no `force`): a missing path throws from the
/// options check's `lstat`, then the same rimraf as [`rm_recursive_force_sync`].
pub fn rm_recursive_sync(path: &str) -> Result<(), FsError> {
    fs::symlink_metadata(path).map_err(|e| FsError::uv(&e, "lstat", Path::new(path)))?;
    rimraf(Path::new(path))
}

/// Linux errno values used by the ports of Node's fs algorithms.
#[cfg(unix)]
mod libc_errno {
    pub const EPERM: i32 = 1;
    pub const ENOENT: i32 = 2;
    pub const EACCES: i32 = 13;
    pub const EEXIST: i32 = 17;
    pub const ENOTDIR: i32 = 20;
    pub const ENOSPC: i32 = 28;
}

#[cfg(windows)]
mod libc_errno {
    pub const EPERM: i32 = 4048;
    pub const ENOENT: i32 = 4058;
    pub const EACCES: i32 = 4092;
    pub const EEXIST: i32 = 4075;
    pub const ENOTDIR: i32 = 4052;
    pub const ENOSPC: i32 = 4055;
}

/// `fs.rmSync(path, { recursive: true, force: true })`: Node 20's JS `rimrafSync` with
/// `maxRetries: 0`, on Linux.
pub fn rm_recursive_force_sync(path: &str) -> Result<(), FsError> {
    rimraf(Path::new(path))
}

fn rimraf(path: &Path) -> Result<(), FsError> {
    let stats = match fs::symlink_metadata(path) {
        Ok(s) => Some(s),
        Err(e) if FsError::uv(&e, "lstat", path).code == "ENOENT" => return Ok(()),
        #[cfg(windows)]
        Err(e) if FsError::uv(&e, "lstat", path).code == "EPERM" => {
            windows_fs::fix_eperm(path, FsError::uv(&e, "lstat", path))?;
            None
        }
        // Other lstat errors are ignored on non-Windows.
        Err(_) => None,
    };
    let result = if stats.is_some_and(|s| s.is_dir()) {
        rmdir(path, None)
    } else {
        unlink(path)
    };
    match result {
        Err(e) if e.code == "ENOENT" => Ok(()),
        #[cfg(windows)]
        Err(e) if e.code == "EPERM" => windows_fs::fix_eperm(path, e),
        Err(e) if e.code == "EPERM" || e.code == "EISDIR" => rmdir(path, Some(e)),
        other => other,
    }
}

fn unlink(path: &Path) -> Result<(), FsError> {
    #[cfg(unix)]
    let result = fs::remove_file(path);
    #[cfg(windows)]
    let result = windows_fs::unlink(path);
    match result {
        Ok(()) => Ok(()),
        Err(e) => match FsError::uv(&e, "unlink", path) {
            e if e.code == "ENOENT" => Ok(()),
            e => Err(e),
        },
    }
}

fn rmdir(path: &Path, original: Option<FsError>) -> Result<(), FsError> {
    let Err(e) = fs::remove_dir(path) else {
        return Ok(());
    };
    let err = FsError::uv(&e, "rmdir", path);
    match err.code {
        "ENOENT" => Ok(()),
        "ENOTDIR" => Err(original.unwrap_or(err)),
        "ENOTEMPTY" | "EEXIST" | "EPERM" => {
            // Unix scandir sorts by byte value; Windows preserves native order. Children use
            // `path + sep + name`, so a trailing separator on `path` doubles, as in Node.
            let names: Vec<std::ffi::OsString> = fs::read_dir(path)
                .and_then(|rd| rd.map(|d| d.map(|d| d.file_name())).collect())
                .map_err(|e| FsError::uv(&e, "scandir", path))?;
            #[cfg(unix)]
            let names = { let mut names = names; names.sort(); names };
            for name in names {
                let mut child = path.as_os_str().to_owned();
                child.push(std::path::MAIN_SEPARATOR_STR);
                child.push(name);
                rimraf(&PathBuf::from(child))?;
            }
            match fs::remove_dir(path) {
                Ok(()) => Ok(()),
                Err(e) => match FsError::uv(&e, "rmdir", path) {
                    e if e.code == "ENOENT" => Ok(()),
                    e => Err(e),
                },
            }
        }
        _ => Err(original.unwrap_or(err)),
    }
}

/// `fs.chmodSync(path, mode)`.
#[cfg(unix)]
pub fn chmod_sync(path: &str, mode: u32) -> Result<(), FsError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(|e| FsError::uv(&e, "chmod", Path::new(path)))
}


/// libuv fs__chmod uses _wchmod: only _S_IWRITE (the owner write bit) matters.
/// Group/other bits are ignored by the Windows CRT, as in Node 20.
#[cfg(windows)]
pub fn chmod_sync(path: &str, mode: u32) -> Result<(), FsError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetFileAttributesW, SetFileAttributesW, FILE_ATTRIBUTE_READONLY, INVALID_FILE_ATTRIBUTES};
    let pathw: Vec<u16> = std::ffi::OsStr::new(path).encode_wide().chain(Some(0)).collect();
    let fail = || FsError::uv(&io::Error::last_os_error(), "chmod", Path::new(path));
    // SAFETY: NUL-terminated caller-owned path, no retained pointers.
    unsafe {
        let attrs = GetFileAttributesW(pathw.as_ptr());
        if attrs == INVALID_FILE_ATTRIBUTES { return Err(fail()); }
        let attrs = if mode & 0o200 != 0 { attrs & !FILE_ATTRIBUTE_READONLY } else { attrs | FILE_ATTRIBUTE_READONLY };
        if SetFileAttributesW(pathw.as_ptr(), attrs) == 0 { return Err(fail()); }
    }
    Ok(())
}

#[cfg(windows)]
pub fn copy_file_sync(src: &str, dest: &str) -> Result<(), FsError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::CopyFileW;
    let wide = |p: &str| std::ffi::OsStr::new(p).encode_wide().chain(Some(0)).collect::<Vec<_>>();
    let (srcw, dstw) = (wide(src), wide(dest));
    if unsafe { CopyFileW(srcw.as_ptr(), dstw.as_ptr(), 0) } == 0 {
        let err = io::Error::last_os_error();
        if crate::uv_errors::errno(err.raw_os_error().unwrap_or(0)) == 4082 && windows_fs::same_file(src, dest) {
            return Ok(());
        }
        return Err(FsError::uv_dest(&err, "copyfile", Path::new(src), Path::new(dest)));
    }
    Ok(())
}

#[cfg(windows)]
mod windows_fs;
#[cfg(windows)]
pub fn read_file(path: &str) -> Result<Vec<u8>, FsError> { windows_fs::read_file(path) }
