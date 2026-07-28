//! One-time privileged install of the `peterfand` LaunchDaemon.
//!
//! Shared by the CLI's `peterfan install-daemon` and the menu-bar app's
//! "Enable Fan Control" menu item, so a GUI-only user never has to open a
//! terminal: clicking the menu item shows the exact same one-time macOS
//! admin-password dialog the CLI would trigger.

use std::path::PathBuf;

/// LaunchDaemon label + paths (kept in sync with `packaging/…plist`).
pub const DAEMON_LABEL: &str = "kr.co.uulab.peterfan.daemon";
pub const LEGACY_DAEMON_LABEL: &str = "com.uulab.peterfan.daemon";

pub const DAEMON_BIN: &str = "/usr/local/bin/peterfand";
pub const DAEMON_PLIST: &str = "/Library/LaunchDaemons/kr.co.uulab.peterfan.daemon.plist";
pub const APP_BUNDLE_DAEMON_BIN: &str = "/Applications/PeterFan.app/Contents/MacOS/peterfand";
pub const NEWSYSLOG_CONF: &str = "/etc/newsyslog.d/peterfand.conf";
const NEWSYSLOG_BODY: &str = "\
# PeterFan daemon log rotation (rotate at 1 MB, keep 5 compressed archives)\n\
/var/log/peterfand.log  root:wheel  644  5  1024  *  J\n\
/var/log/peterfand.err  root:wheel  644  3   512  *  J\n";

/// The LaunchDaemon plist, generated so the install needs no extra files.
fn daemon_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{DAEMON_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/local/bin/peterfand</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>/var/log/peterfand.log</string>
  <key>StandardErrorPath</key><string>/var/log/peterfand.err</string>
</dict>
</plist>
"#
    )
}

/// Find the `peterfand` binary shipped next to the current executable —
/// works both for the CLI's flat archive layout and for `PeterFan.app/
/// Contents/MacOS/` (see `scripts/bundle-macos.sh`, which copies `peterfand`
/// in alongside the menu-bar binary for exactly this lookup).
pub fn find_peterfand() -> Result<PathBuf, String> {
    let mut cands = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            cands.push(dir.join("peterfand"));
        }
    }
    cands.push(PathBuf::from("./peterfand"));
    cands.push(PathBuf::from("target/release/peterfand"));
    cands
        .into_iter()
        .find(|p| p.exists())
        .ok_or_else(|| "peterfand not found next to this binary".to_string())
}

/// Run a privileged shell script via one macOS admin-password GUI prompt.
fn run_privileged(script: &str, dry_run: bool) -> Result<String, String> {
    let path =
        std::env::temp_dir().join(format!("peterfan-daemon-install-{}.sh", std::process::id()));
    if path.to_string_lossy().contains('\'') {
        return Err("temp path contains a quote; aborting".into());
    }
    std::fs::write(&path, script).map_err(|e| e.to_string())?;
    let apple = format!(
        "do shell script \"/bin/bash '{}'\" with administrator privileges",
        path.display()
    );
    if dry_run {
        let out = format!(
            "--- script ({}) ---\n{script}\n--- osascript ---\n{apple}",
            path.display()
        );
        return Ok(out);
    }
    let output = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(&apple)
        .output()
        .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&path);
    if !output.status.success() {
        return Err(privileged_error_message(
            output.status.code(),
            &String::from_utf8_lossy(&output.stderr),
        ));
    }
    Ok(String::new())
}

fn privileged_error_message(status_code: Option<i32>, stderr: &str) -> String {
    let detail = stderr
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or_default();
    let lower = detail.to_ascii_lowercase();
    if status_code == Some(1)
        && (lower.contains("user canceled")
            || lower.contains("user cancelled")
            || lower.contains("(-128)"))
    {
        return "Administrator approval was cancelled. Fan control was not changed.".into();
    }
    if detail.is_empty() {
        "Administrator approval failed before the installer could run. Fan control was not changed."
            .into()
    } else {
        format!("Fan control installation failed: {detail}")
    }
}

/// Distinct from `Err`: the privileged script ran successfully, but the
/// daemon hasn't answered over IPC yet (slow launchd bootstrap, or a real
/// startup failure logged to `/var/log/peterfand.err`). Not a cancellation
/// or script error, so callers shouldn't treat it as one.
pub enum InstallOutcome {
    /// `dry_run` was set — nothing was actually run; this is the script that
    /// *would* run, plus the `osascript` invocation, for inspection.
    DryRun(String),
    /// Installed and the daemon answered over IPC.
    Installed,
    /// The privileged script completed, but the daemon isn't reachable yet.
    InstalledButUnreachable,
}

fn required_daemon_is_healthy(installed_version: Option<&str>, reachable: bool) -> bool {
    reachable && installed_version.is_some_and(|version| !crate::daemon_update_required(version))
}

#[cfg(target_os = "macos")]
fn wait_for_required_daemon(timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let version = crate::installed_daemon_version();
        if required_daemon_is_healthy(version.as_deref(), crate::daemon_reachable()) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    false
}

/// Install the daemon: copies the binary to `/usr/local/bin`, registers the
/// LaunchDaemon, and sets up log rotation. Shows exactly one macOS admin
/// password dialog (via `osascript … with administrator privileges`).
/// `Err` means the user cancelled the prompt, the script failed, or
/// `peterfand` wasn't found next to this binary — genuine failures.
pub fn install(dry_run: bool) -> Result<InstallOutcome, String> {
    if !dry_run {
        let installed_version = crate::installed_daemon_version();
        if required_daemon_is_healthy(installed_version.as_deref(), crate::daemon_reachable()) {
            return Ok(InstallOutcome::Installed);
        }
    }
    let bin = find_peterfand()?;
    let staged_bin = std::env::temp_dir().join(format!("peterfand-install-{}", std::process::id()));
    if !dry_run {
        std::fs::copy(&bin, &staged_bin).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&staged_bin)
                .map_err(|e| e.to_string())?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&staged_bin, perms).map_err(|e| e.to_string())?;
        }
    }
    let plist_dst = DAEMON_PLIST.to_string();
    let legacy_plist_dst = format!("/Library/LaunchDaemons/{LEGACY_DAEMON_LABEL}.plist");
    let script = format!(
        "set -e\n\
         /bin/launchctl bootout 'system/{legacy_label}' 2>/dev/null || true\n\
         /bin/launchctl bootout 'system/{daemon_label}' 2>/dev/null || true\n\
         /bin/rm -f '{legacy_plist_dst}'\n\
         /bin/mkdir -p /usr/local/bin\n\
         /usr/bin/install -m 755 '{staged_bin}' {DAEMON_BIN}\n\
         /bin/rm -f '{staged_bin}'\n\
         cat > '{plist_dst}' <<'PLIST'\n{plist}PLIST\n\
         /usr/sbin/chown root:wheel '{plist_dst}'\n\
         /bin/chmod 644 '{plist_dst}'\n\
         /usr/bin/plutil -lint '{plist_dst}' >/dev/null\n\
         /bin/launchctl bootstrap system '{plist_dst}'\n\
         /bin/mkdir -p /etc/newsyslog.d\n\
         /usr/bin/printf '%s' '{newsyslog}' > {newsyslog_conf}\n\
         /bin/chmod 644 {newsyslog_conf}\n",
        staged_bin = staged_bin.display(),
        plist = daemon_plist(),
        legacy_plist_dst = legacy_plist_dst,
        legacy_label = LEGACY_DAEMON_LABEL,
        daemon_label = DAEMON_LABEL,
        newsyslog = NEWSYSLOG_BODY,
        newsyslog_conf = NEWSYSLOG_CONF,
    );
    let dry_run_output = match run_privileged(&script, dry_run) {
        Ok(out) => out,
        Err(e) => {
            let _ = std::fs::remove_file(&staged_bin);
            return Err(e);
        }
    };
    if dry_run {
        return Ok(InstallOutcome::DryRun(dry_run_output));
    }
    std::thread::sleep(std::time::Duration::from_millis(800));
    if crate::daemon_reachable() {
        Ok(InstallOutcome::Installed)
    } else {
        Ok(InstallOutcome::InstalledButUnreachable)
    }
}

/// Remove the daemon (LaunchDaemon, binary, log-rotation config). One admin
/// password dialog. `Err` means the user cancelled or the script failed.
pub fn uninstall(dry_run: bool) -> Result<InstallOutcome, String> {
    let plist_dst = format!("/Library/LaunchDaemons/{DAEMON_LABEL}.plist");
    let legacy_plist_dst = format!("/Library/LaunchDaemons/{LEGACY_DAEMON_LABEL}.plist");
    let script = format!(
        "launchctl bootout system '{plist_dst}' 2>/dev/null || true\n\
         launchctl bootout system '{legacy_plist_dst}' 2>/dev/null || true\n\
         rm -f '{plist_dst}' '{legacy_plist_dst}' {DAEMON_BIN}\n\
         rm -f {NEWSYSLOG_CONF}\n"
    );
    let dry_run_output = run_privileged(&script, dry_run)?;
    if dry_run {
        return Ok(InstallOutcome::DryRun(dry_run_output));
    }
    Ok(InstallOutcome::Installed)
}

/// Ask an already-running root daemon to reinstall fan control from the
/// signed app bundle. This avoids another administrator-password prompt after
/// the initial LaunchDaemon install. Older daemons do not understand this
/// command, so callers should fall back to [`install`] when this returns `Err`.
#[cfg(target_os = "macos")]
pub fn reinstall_via_running_daemon(dry_run: bool) -> Result<InstallOutcome, String> {
    let bin = find_peterfand()?;
    let bin = std::fs::canonicalize(&bin).map_err(|e| e.to_string())?;
    if bin.as_path() != std::path::Path::new(APP_BUNDLE_DAEMON_BIN) {
        return Err(format!(
            "daemon self-reinstall only works from {APP_BUNDLE_DAEMON_BIN}"
        ));
    }
    let cmd = format!("reinstall-fan-control {}", bin.display());
    if dry_run {
        return Ok(InstallOutcome::DryRun(cmd));
    }
    let reply = crate::ipc::send_command_with_timeout(&cmd, std::time::Duration::from_secs(5));
    if let Some(reply) = reply.as_deref() {
        if !reply.starts_with("ok ") {
            return Err(reply.to_string());
        }
    }

    // The daemon verifies the bundled Developer ID signature before replying,
    // then deliberately replaces and restarts itself. Either step can close
    // IPC before the client receives the acknowledgement, so the installed
    // binary version plus a fresh socket is the authoritative success signal.
    if wait_for_required_daemon(std::time::Duration::from_secs(10)) {
        Ok(InstallOutcome::Installed)
    } else if reply.is_some() {
        Ok(InstallOutcome::InstalledButUnreachable)
    } else {
        Err("fan-control daemon did not acknowledge or complete its update".to_string())
    }
}

#[cfg(not(target_os = "macos"))]
pub fn reinstall_via_running_daemon(_dry_run: bool) -> Result<InstallOutcome, String> {
    Err("daemon self-reinstall is only available on macOS".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privileged_errors_distinguish_cancellation_from_script_failure() {
        assert_eq!(
            privileged_error_message(Some(1), "execution error: User canceled. (-128)\n"),
            "Administrator approval was cancelled. Fan control was not changed."
        );
        assert_eq!(
            privileged_error_message(
                Some(1),
                "execution error: launchctl bootstrap failed: 5 (1)\n"
            ),
            "Fan control installation failed: execution error: launchctl bootstrap failed: 5 (1)"
        );
        assert_eq!(
            privileged_error_message(Some(1), ""),
            "Administrator approval failed before the installer could run. Fan control was not changed."
        );
    }

    #[test]
    fn launch_daemon_restarts_after_crash_and_boot() {
        let plist = daemon_plist();
        let path = std::env::temp_dir().join(format!(
            "peterfan-daemon-plist-test-{}.plist",
            std::process::id()
        ));
        std::fs::write(&path, plist).expect("write test plist");

        let lint = std::process::Command::new("plutil")
            .args(["-lint", path.to_str().expect("utf-8 path")])
            .output()
            .expect("run plutil");
        assert!(
            lint.status.success(),
            "{}",
            String::from_utf8_lossy(&lint.stderr)
        );

        let extract = |key: &str| {
            let output = std::process::Command::new("plutil")
                .args([
                    "-extract",
                    key,
                    "raw",
                    "-o",
                    "-",
                    path.to_str().expect("utf-8 path"),
                ])
                .output()
                .expect("extract plist value");
            assert!(output.status.success());
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };
        assert_eq!(extract("RunAtLoad"), "true");
        assert_eq!(extract("KeepAlive"), "true");
        assert_eq!(extract("Label"), DAEMON_LABEL);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn required_daemon_health_needs_both_version_and_reachability() {
        assert!(required_daemon_is_healthy(
            Some(crate::MIN_REQUIRED_DAEMON_VERSION),
            true
        ));
        assert!(!required_daemon_is_healthy(Some("1.27.22"), true));
        assert!(!required_daemon_is_healthy(
            Some(crate::MIN_REQUIRED_DAEMON_VERSION),
            false
        ));
        assert!(!required_daemon_is_healthy(None, true));
    }
}
