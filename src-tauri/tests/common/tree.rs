//! Filesystem fixtures for fs-touching parity tests: build the same tree twice at the same
//! path (once per implementation) and compare snapshots afterwards.
#![allow(dead_code)]

use std::{
    fs,
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::{ffi::OsStr, os::unix::{ffi::OsStrExt, fs::{PermissionsExt, symlink}}};
#[cfg(windows)]
fn symlink(target: String, path: &Path) -> std::io::Result<()> {
    if Path::new(&target).is_dir() { std::os::windows::fs::symlink_dir(target, path) }
    else { std::os::windows::fs::symlink_file(target, path) }
}
#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}
#[cfg(windows)]
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_readonly(mode & 0o200 == 0);
    fs::set_permissions(path, permissions)
}
#[cfg(unix)]
fn mode(metadata: &fs::Metadata) -> u32 { metadata.permissions().mode() & 0o777 }
#[cfg(windows)]
fn mode(metadata: &fs::Metadata) -> u32 {
    (if metadata.permissions().readonly() { 0o444 } else { 0o666 }) |
        if metadata.is_dir() { 0o111 } else { 0 }
}

/// One entry: path relative to the tree root (raw bytes allowed), kind, and an optional mode
/// applied after everything is created (deepest first, so a 0o000 dir can still be filled).
pub struct Entry {
    pub path: &'static [u8],
    pub kind: Kind,
    pub mode: Option<u32>,
}

pub enum Kind {
    Dir,
    File,
    Symlink(&'static str),
}

pub fn dir(path: &'static str) -> Entry {
    Entry {
        path: path.as_bytes(),
        kind: Kind::Dir,
        mode: None,
    }
}

pub fn file(path: &'static str) -> Entry {
    Entry {
        path: path.as_bytes(),
        kind: Kind::File,
        mode: None,
    }
}

pub fn link(path: &'static str, target: &'static str) -> Entry {
    Entry {
        path: path.as_bytes(),
        kind: Kind::Symlink(target),
        mode: None,
    }
}

pub fn raw_file(path: &'static [u8]) -> Entry {
    Entry {
        path,
        kind: Kind::File,
        mode: None,
    }
}

impl Entry {
    pub fn mode(mut self, mode: u32) -> Self {
        self.mode = Some(mode);
        self
    }
}

#[cfg(unix)]
fn at(root: &Path, rel: &[u8]) -> PathBuf {
    root.join(OsStr::from_bytes(rel))
}

#[cfg(windows)]
fn at(root: &Path, rel: &[u8]) -> PathBuf { root.join(String::from_utf8_lossy(rel).as_ref()) }

/// Makes everything under `root` writable again, then deletes it.
pub fn remove(root: &Path) {
    fn unlock(p: &Path) {
        if let Ok(m) = fs::symlink_metadata(p)
            && m.is_dir()
        {
            let _ = set_mode(p, 0o700);
            if let Ok(rd) = fs::read_dir(p) {
                for e in rd.flatten() {
                    unlock(&e.path());
                }
            }
        }
    }
    unlock(root);
    let _ = fs::remove_dir_all(root);
}

pub fn build(root: &Path, entries: &[Entry]) {
    remove(root);
    fs::create_dir_all(root).unwrap();
    for e in entries {
        let p = at(root, e.path);
        if !matches!(e.kind, Kind::Dir) {
            fs::create_dir_all(p.parent().unwrap()).unwrap();
        }
        match e.kind {
            Kind::Dir => fs::create_dir_all(&p).unwrap(),
            Kind::File => fs::write(&p, b"x").unwrap(),
            Kind::Symlink(t) => symlink(t.replace("{root}", &root.to_string_lossy()), &p).unwrap(),
        }
    }
    build_modes(root, entries);
}

/// Applies the entries' modes, deepest first (so a 0o000 dir can still be filled first).
pub fn build_modes(root: &Path, entries: &[Entry]) {
    let mut moded: Vec<&Entry> = entries.iter().filter(|e| e.mode.is_some()).collect();
    moded.sort_by_key(|e| std::cmp::Reverse(e.path.len()));
    for e in moded {
        set_mode(&at(root, e.path), e.mode.unwrap())
        .unwrap();
    }
}

/// Every entry under `root` with its kind and mode, without following symlinks.
pub fn snapshot(root: &Path) -> Vec<String> {
    fn walk(base: &Path, p: &Path, out: &mut Vec<String>) {
        let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
        let Ok(m) = fs::symlink_metadata(p) else {
            return;
        };
        let mode = mode(&m);
        if m.is_symlink() {
            out.push(format!("{rel} -> {}", fs::read_link(p).unwrap().display()));
        } else if m.is_dir() {
            out.push(format!("{rel}/ {mode:o}"));
            match fs::read_dir(p) {
                Ok(rd) => {
                    let mut kids: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
                    kids.sort();
                    for k in kids {
                        walk(base, &k, out);
                    }
                }
                Err(_) => out.push(format!("{rel}/ <unreadable>")),
            }
        } else {
            // Contents too: handlers write JSON and copy files, and the bytes are the contract.
            let body = fs::read(p)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_else(|_| "<unreadable>".into());
            out.push(format!("{rel} {mode:o} {body:?}"));
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out
}
