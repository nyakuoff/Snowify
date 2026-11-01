// The existing fixture uses Unix sockets, permissions, or shell scripts.
#[cfg(unix)]
#[path = "../parity_cloud_auth.rs"]
mod unix;
