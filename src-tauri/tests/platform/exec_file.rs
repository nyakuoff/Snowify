// The existing fixture uses Unix sockets, permissions, or shell scripts.
#[cfg(unix)]
#[path = "../parity_exec_file.rs"]
mod unix;

#[cfg(windows)]
#[path = "../windows/exec_file.rs"]
mod windows;
