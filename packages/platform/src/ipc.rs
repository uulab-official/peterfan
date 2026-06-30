//! Tiny Unix-socket IPC between the menu-bar app and the `peterfand` daemon.
//!
//! The daemon runs as root (via the LaunchDaemon) and owns SMC writes; the
//! menu-bar app runs as the user with no privileges. So the app sends one-line
//! text commands over a Unix socket and the daemon performs the privileged
//! action. Protocol (newline-terminated):
//!
//! - `profile <name>` — switch the active curve profile
//! - `auto`           — hand fans back to the OS
//! - `ping`           — liveness check
//!
//! Responses are a single line: `ok ...` or `error: ...`.
//!
//! The daemon binds the first writable path; clients try them in order.
//! `/tmp` is world-writable, so this is a local-trust convenience, not a
//! security boundary — documented as such.

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

/// Candidate socket paths, in preference order (root daemon → user fallback).
pub const PATHS: [&str; 2] = ["/var/run/peterfand.sock", "/tmp/peterfand.sock"];

/// Bind a listener on the first usable path; returns it and the path bound.
pub fn bind_listener() -> io::Result<(UnixListener, PathBuf)> {
    let mut last = io::Error::other("no socket path available");
    for p in PATHS {
        let _ = std::fs::remove_file(p); // clear a stale socket
        match UnixListener::bind(p) {
            Ok(listener) => {
                // World-accessible so the (non-root) menu-bar app can connect.
                let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o666));
                return Ok((listener, PathBuf::from(p)));
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Connect to a running daemon, trying each candidate path.
pub fn connect() -> Option<UnixStream> {
    PATHS.iter().find_map(|p| UnixStream::connect(p).ok())
}

/// Send a newline-terminated command to the running daemon and return the reply.
/// Returns `None` when no daemon is reachable or the send/receive fails.
pub fn send_command(cmd: &str) -> Option<String> {
    use std::io::{BufRead, BufReader, Write};
    let mut stream = connect()?;
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(500)));
    writeln!(stream, "{cmd}").ok()?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).ok()?;
    Some(line.trim().to_string())
}
