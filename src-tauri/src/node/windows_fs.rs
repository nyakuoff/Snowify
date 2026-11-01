//! Node v20.18.3's fs wrappers + libuv src/win/fs.c exceptions to error.c.
use super::*;
use std::os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, GetFileInformationByHandle,
};

pub(super) fn error_path(path: &Path) -> PathBuf {
    if path.as_os_str().is_empty() {
        return path.to_owned();
    }
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    let s = abs.to_string_lossy();
    // Node src/api/exceptions.cc StringFromPath removes the namespace in error objects.
    if let Some(unc) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{unc}"))
    } else if let Some(p) = s.strip_prefix(r"\\?\") {
        PathBuf::from(p)
    } else {
        abs
    }
}

pub(super) fn error(err: &io::Error, syscall: &str, path: &Path) -> io::Error {
    let raw = err.raw_os_error().unwrap_or(0);
    let translated = match (syscall, raw) {
        ("scandir", 3 | 87 | 267) if path.is_file() => -4052, // NtQueryDirectoryFile -> UV_ENOTDIR
        ("open", 5 | 80 | 183) if path.is_dir() => -4068,     // create/truncate directory -> EISDIR
        ("read", 5) => -4083, // fs__read: ERROR_ACCESS_DENIED -> ERROR_INVALID_FLAGS -> EBADF
        _ => raw,
    };
    io::Error::from_raw_os_error(translated)
}

pub(super) fn mkdir_error(err: &io::Error) -> io::Error {
    // Apply this only to CreateDirectoryW, not MKDirpSync's subsequent stat failure.
    match err.raw_os_error().unwrap_or(0) {
        123 | 267 => io::Error::from_raw_os_error(-4071), // UV_EINVAL
        raw => io::Error::from_raw_os_error(raw),
    }
}

pub(super) fn read_file(path: &str) -> Result<Vec<u8>, FsError> {
    use std::io::Read;
    // libuv can open directories; their failure comes from read, with no path property.
    let mut f = fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .map_err(|e| FsError::uv(&e, "open", Path::new(path)))?;
    let mut out = Vec::new();
    f.read_to_end(&mut out)
        .map_err(|e| FsError::uv_nopath(&e, "read"))?;
    Ok(out)
}

pub(super) fn same_file(a: &str, b: &str) -> bool {
    fn identity(path: &str) -> io::Result<(u32, u32, u32)> {
        let f = fs::File::open(path)?;
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(f.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((
            info.dwVolumeSerialNumber,
            info.nFileIndexHigh,
            info.nFileIndexLow,
        ))
    }
    identity(a)
        .ok()
        .is_some_and(|a| identity(b).ok() == Some(a))
}

pub(super) fn unlink(path: &Path) -> io::Result<()> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, DELETE, FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_BASIC_INFO,
        FILE_DISPOSITION_INFO, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES, FileBasicInfo,
        FileDispositionInfo, OPEN_EXISTING, SetFileInformationByHandle,
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let h = CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | DELETE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        );
        if h == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let handle = OwnedHandle::from_raw_handle(h);
        let mut info: BY_HANDLE_FILE_INFORMATION = std::mem::zeroed();
        if GetFileInformationByHandle(handle.as_raw_handle(), &mut info) == 0 {
            return Err(io::Error::last_os_error());
        }
        if info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
            // libuv permits directory symlinks/junctions but rejects real directories.
            if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0
                || fs::read_link(path).is_err()
            {
                return Err(io::Error::from_raw_os_error(5));
            }
        }
        if info.dwFileAttributes & FILE_ATTRIBUTE_READONLY != 0 {
            let mut basic: FILE_BASIC_INFO = std::mem::zeroed();
            basic.FileAttributes =
                (info.dwFileAttributes & !FILE_ATTRIBUTE_READONLY) | FILE_ATTRIBUTE_ARCHIVE;
            if SetFileInformationByHandle(
                handle.as_raw_handle(),
                FileBasicInfo,
                &basic as *const _ as _,
                size_of_val(&basic) as u32,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        if SetFileInformationByHandle(
            handle.as_raw_handle(),
            FileDispositionInfo,
            &disposition as *const _ as _,
            size_of_val(&disposition) as u32,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

pub(super) fn fix_eperm(path: &Path, original: FsError) -> Result<(), FsError> {
    // Node lib/internal/fs/rimraf.js fixWinEPERMSync, maxRetries=0.
    if let Err(e) = chmod_sync(&path.to_string_lossy(), 0o666) {
        return if e.code == "ENOENT" {
            Ok(())
        } else {
            Err(original)
        };
    }
    match fs::metadata(path) {
        Ok(m) if m.is_dir() => rmdir(path, Some(original)),
        Ok(_) => super::unlink(path),
        Err(e) if FsError::uv(&e, "stat", path).code == "ENOENT" => Ok(()),
        Err(_) => Err(original),
    }
}
