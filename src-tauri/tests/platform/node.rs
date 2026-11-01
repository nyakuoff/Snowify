// The existing fixture uses Unix sockets, permissions, or shell scripts.
#[cfg(unix)]
#[path = "../parity_node.rs"]
mod unix;

#[cfg(windows)]
#[path = "../windows/node.rs"]
mod windows;
