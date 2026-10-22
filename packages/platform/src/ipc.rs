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
//! - `version`        — version of the process answering IPC
//! - `reload`         — reload the logged-in user's validated config
//! - `config-source`  — confirm support for user-aware config loading
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
    send_command_with_timeout(cmd, std::time::Duration::from_millis(500))
}

/// Send a command with a caller-selected reply timeout. Slow privileged
/// operations such as signature verification need longer than normal fan
/// control commands, while the default path should remain responsive.
pub fn send_command_with_timeout(cmd: &str, timeout: std::time::Duration) -> Option<String> {
    send_command_on_stream(connect()?, cmd, timeout)
}

fn send_command_on_stream(
    mut stream: UnixStream,
    cmd: &str,
    timeout: std::time::Duration,
) -> Option<String> {
    use std::io::{BufRead, BufReader, Write};
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    writeln!(stream, "{cmd}").ok()?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).ok()?;
    Some(line.trim().to_string())
}

/// Query the resident process. A socket connection or legacy ping reply does
/// not establish that the replacement daemon has actually started.
pub fn daemon_version() -> Option<String> {
    daemon_version_on_stream(connect()?, std::time::Duration::from_millis(500))
}

fn daemon_version_on_stream(stream: UnixStream, timeout: std::time::Duration) -> Option<String> {
    parse_daemon_version_reply(&send_command_on_stream(stream, "version", timeout)?)
}

fn parse_daemon_version_reply(reply: &str) -> Option<String> {
    let version = reply.strip_prefix("ok peterfand ")?;
    semver::Version::parse(version).ok()?;
    Some(version.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resident_version_reply_requires_an_explicit_valid_version() {
        assert_eq!(
            parse_daemon_version_reply("ok peterfand 1.32.3-beta.1+build.7"),
            Some("1.32.3-beta.1+build.7".into())
        );
        for reply in [
            "error: unknown command",
            "ok peterfand (macOS)",
            "ok rules:balanced (macOS)",
            "",
            "ok peterfand unknown",
            "ok peterfand 1.32.3 extra",
            "ok peterfand 1.32.3-",
            "ok peterfand 1.32.3\n",
        ] {
            assert_eq!(parse_daemon_version_reply(reply), None, "{reply:?}");
        }
    }

    #[test]
    fn resident_version_query_exchanges_a_read_only_command_over_ipc() {
        use std::io::{BufRead, BufReader, Write};
        let (client, mut server) = UnixStream::pair().unwrap();
        let daemon = std::thread::spawn(move || {
            let mut command = String::new();
            BufReader::new(server.try_clone().unwrap())
                .read_line(&mut command)
                .unwrap();
            assert_eq!(command, "version\n");
            writeln!(server, "ok peterfand 1.32.3").unwrap();
        });
        assert_eq!(
            daemon_version_on_stream(client, std::time::Duration::from_millis(500)),
            Some("1.32.3".into()),
        );
        daemon.join().unwrap();
    }

    #[test]
    fn a_connected_but_unresponsive_daemon_does_not_prove_runtime_health() {
        let (client, _server) = UnixStream::pair().unwrap();
        let start = std::time::Instant::now();
        assert!(daemon_version_on_stream(client, std::time::Duration::from_millis(30)).is_none());
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
    }
}
