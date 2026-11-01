//! Electron v33.4.11 browser_win.cc's desktop Run/StartupApproved behavior.
//! `name` defaults to the Electron AUMID, not app.getName(). Command lines are
//! joined verbatim: callers supply any quotes they need around paths/arguments.
use serde_json::{Value, json};
use std::{io, ptr};
use windows_sys::Win32::{
    Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS, LocalFree},
    Globalization::{CSTR_EQUAL, CompareStringOrdinal},
    System::{Environment::ExpandEnvironmentStringsW, Registry::*},
    UI::Shell::CommandLineToArgvW,
};
pub const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const APPROVED: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn text(data: &[u8]) -> String {
    let units: Vec<u16> = data
        .chunks(2)
        .map(|b| u16::from_le_bytes([b[0], *b.get(1).unwrap_or(&0)]))
        .take_while(|u| *u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
impl Key {
    fn open(root: HKEY, path: &str, create: bool) -> Option<Self> {
        let mut h = ptr::null_mut();
        let path = wide(path);
        let status = unsafe {
            if create {
                RegCreateKeyExW(
                    root,
                    path.as_ptr(),
                    0,
                    ptr::null(),
                    0,
                    KEY_ALL_ACCESS,
                    ptr::null(),
                    &mut h,
                    ptr::null_mut(),
                )
            } else {
                RegOpenKeyExW(root, path.as_ptr(), 0, KEY_READ, &mut h)
            }
        };
        (status == ERROR_SUCCESS).then(|| Self(h))
    }
    fn read(&self, name: &str) -> Option<(u32, Vec<u8>)> {
        let name = wide(name);
        let (mut ty, mut len) = (0, 0);
        if unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                ptr::null(),
                &mut ty,
                ptr::null_mut(),
                &mut len,
            )
        } != ERROR_SUCCESS
        {
            return None;
        }
        let mut data = vec![0; len as usize];
        if unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                ptr::null(),
                &mut ty,
                data.as_mut_ptr(),
                &mut len,
            )
        } != ERROR_SUCCESS
        {
            return None;
        }
        data.truncate(len as usize);
        Some((ty, data))
    }
    fn write(&self, name: &str, ty: u32, data: &[u8]) {
        unsafe {
            RegSetValueExW(
                self.0,
                wide(name).as_ptr(),
                0,
                ty,
                data.as_ptr(),
                data.len() as u32,
            );
        }
    }
    fn delete(&self, name: &str) {
        unsafe {
            RegDeleteValueW(self.0, wide(name).as_ptr());
        }
    }
    fn entries(&self) -> Vec<(String, Vec<u8>)> {
        let mut count = 0;
        let result = unsafe {
            RegQueryInfoKeyW(
                self.0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut count,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if result != ERROR_SUCCESS {
            return vec![];
        }
        let mut out = vec![];
        // Chromium's RegistryValueIterator starts at count-1, then decrements.
        for index in (0..count).rev() {
            let mut name = vec![0u16; 16384];
            let mut data = vec![0u8; 4096];
            loop {
                let (mut names, mut size) = (name.len() as u32, data.len() as u32);
                let result = unsafe {
                    RegEnumValueW(
                        self.0,
                        index,
                        name.as_mut_ptr(),
                        &mut names,
                        ptr::null(),
                        ptr::null_mut(),
                        data.as_mut_ptr(),
                        &mut size,
                    )
                };
                if result == ERROR_MORE_DATA {
                    data.resize(size as usize + 2, 0);
                    continue;
                }
                if result == ERROR_SUCCESS {
                    data.truncate(size as usize);
                    out.push((String::from_utf16_lossy(&name[..names as usize]), data));
                }
                break;
            }
        }
        out
    }
}
#[derive(Clone, Debug)]
pub struct Settings {
    pub open_at_login: bool,
    pub enabled: bool,
    pub name: String,
    pub path: String,
    pub args: Vec<String>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            open_at_login: false,
            enabled: true,
            name: String::new(),
            path: String::new(),
            args: vec![],
        }
    }
}
pub struct LoginItems {
    pub app_id: String,
    pub exe: String,
}
impl LoginItems {
    pub fn new(app_id: String, exe: String) -> Self {
        Self { app_id, exe }
    }
    pub fn current(product_name: &str) -> io::Result<Self> {
        // Snowify's NSIS Electron build has no setAppUserModelId or Squirrel update.exe:
        // application_info_win.cc falls back to electron.app.<executable ProductName>.
        // Preserve that value name across the replacement, independently of Tauri's AUMID.
        Ok(Self::new(
            format!("electron.app.{product_name}"),
            std::env::current_exe()?.to_string_lossy().into_owned(),
        ))
    }
    fn command(&self, options: &Settings) -> String {
        let mut exe = if options.path.is_empty() {
            self.exe.clone()
        } else {
            options.path.clone()
        };
        if !options.args.is_empty() {
            exe.push(' ');
            exe.push_str(&options.args.join(" "));
        }
        exe
    }
    pub fn set(&self, options: &Settings) {
        // RegKey(KEY_ALL_ACCESS) creates these keys even for openAtLogin=false.
        let run = Key::open(HKEY_CURRENT_USER, RUN, true);
        let approved = Key::open(HKEY_CURRENT_USER, APPROVED, true);
        let name = if options.name.is_empty() {
            &self.app_id
        } else {
            &options.name
        };
        if options.open_at_login {
            if let Some(run) = run {
                let data: Vec<u8> =
                    wide(self.command(options).split('\0').next().unwrap_or_default())
                        .into_iter()
                        .flat_map(u16::to_le_bytes)
                        .collect();
                run.write(name, REG_SZ, &data);
            }
            if let Some(approved) = approved {
                if options.enabled {
                    approved.delete(name);
                } else {
                    approved.write(name, REG_BINARY, &[3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
                }
            }
        } else {
            if let Some(approved) = approved {
                approved.delete(name);
            }
            if let Some(run) = run {
                run.delete(name);
            }
        }
        // Electron's void setter ignores registry failures.
    }
    pub fn get(&self, options: &Settings) -> Value {
        let run = Key::open(HKEY_CURRENT_USER, RUN, true);
        let old = run
            .as_ref()
            .and_then(|key| key.read(&self.app_id))
            .and_then(|(ty, data)| {
                if data.len() > 2048 {
                    return None;
                } // RegKey::ReadValue's 1024 wchar buffer.
                match ty {
                    REG_SZ => Some(text(&data)),
                    REG_EXPAND_SZ => unsafe {
                        let input = wide(&text(&data));
                        let mut out = [0u16; 1024];
                        let n = ExpandEnvironmentStringsW(
                            input.as_ptr(),
                            out.as_mut_ptr(),
                            out.len() as u32,
                        );
                        (n > 0 && n <= 1024)
                            .then(|| String::from_utf16_lossy(&out[..n as usize - 1]))
                    },
                    _ => None,
                }
            });
        // The legacy field ignores options.name and StartupApproved, and compares exactly.
        let open = old.is_some_and(|value| value == self.command(options));
        let lookup = parse_command(if options.path.is_empty() {
            &self.exe
        } else {
            &options.path
        })
        .0;
        let mut launch = vec![];
        if !lookup.is_empty() {
            for (root, scope) in [(HKEY_CURRENT_USER, "user"), (HKEY_LOCAL_MACHINE, "machine")] {
                let Some(run) = Key::open(root, RUN, false) else {
                    continue;
                };
                let approved = Key::open(root, APPROVED, false);
                for (name, data) in run.entries() {
                    let (path, args) = parse_command(&text(&data));
                    let (a, b) = (wide(&lookup), wide(&path));
                    if unsafe { CompareStringOrdinal(a.as_ptr(), -1, b.as_ptr(), -1, 1) }
                        != CSTR_EQUAL
                    {
                        continue;
                    }
                    let enabled = approved.as_ref().and_then(|key| key.read(&name)).map_or(
                        true,
                        |(ty, data)| {
                            // Electron compares NUL-terminated byte strings, not the full 12
                            // bytes. Its read buffer is 12 wchar_t (24 bytes).
                            ty != REG_BINARY
                                || data.len() > 24
                                || data.first() == Some(&0)
                                || data.starts_with(&[2, 0])
                        },
                    );
                    launch.push(json!({"name":name,"path":path,"args":args,"scope":scope,"enabled":enabled}));
                }
            }
        }
        json!({"openAtLogin":open,"openAsHidden":false,"restoreState":false,"wasOpenedAtLogin":false,"wasOpenedAsHidden":false,"executableWillLaunchAtLogin":launch.iter().any(|v| v["enabled"]==true),"launchItems":launch})
    }
}
// Chromium base/command_line.cc@130.0.6723.191: CommandLineToArgvW, trim args,
// remove switches (including /switch), and consume the first -- terminator.
fn parse_command(command: &str) -> (String, Vec<String>) {
    let command = command.trim();
    if command.is_empty() {
        return (String::new(), vec![]);
    }
    unsafe {
        let mut count = 0;
        let argv = CommandLineToArgvW(wide(command).as_ptr(), &mut count);
        if argv.is_null() {
            return (String::new(), vec![]);
        }
        let mut strings = Vec::new();
        for ptr in std::slice::from_raw_parts(argv, count as usize) {
            let mut n = 0;
            while *ptr.add(n) != 0 {
                n += 1;
            }
            strings.push(
                String::from_utf16_lossy(std::slice::from_raw_parts(*ptr, n))
                    .trim()
                    .to_owned(),
            );
        }
        LocalFree(argv.cast());
        let program = strings.first().cloned().unwrap_or_default();
        let mut args = vec![];
        let mut switches = true;
        for arg in strings.into_iter().skip(1) {
            if switches && arg == "--" {
                switches = false;
                args.push(arg);
                continue;
            }
            let prefix = ["--", "-", "/"].into_iter().find(|p| arg.starts_with(p));
            if switches
                && let Some(prefix) = prefix
                && arg.len() > prefix.len()
            {
                let key = arg.split('=').next().unwrap();
                if &key[prefix.len()..] == "single-argument" {
                    args.clear();
                    if let Some(i) = command.find(key) {
                        if let Some(rest) = command.get(i + key.len() + 1..) {
                            if !rest.is_empty() {
                                args.push(rest.into());
                            }
                        }
                    }
                    break;
                }
                continue;
            }
            args.push(arg);
        }
        // GetArgs erases the first --, even if it was itself an argument after a terminator.
        if let Some(i) = args.iter().position(|a| a == "--") {
            args.remove(i);
        }
        (program, args)
    }
}
