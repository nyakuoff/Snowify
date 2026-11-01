// The existing fixture uses Unix sockets, permissions, or shell scripts.
#[cfg(unix)]
#[path = "../parity_discord.rs"]
mod unix;

#[cfg(windows)]
#[path = "../windows/discord.rs"]
mod windows;
