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
    <string>--profile</string><string>balanced</string>
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
    let path = std::env::temp_dir().join("peterfan-daemon-install.sh");
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
    let status = std::process::Command::new("osascript")
        .arg("-e")
        .arg(&apple)
        .status()
        .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&path);
    if !status.success() {
        return Err("privileged step was cancelled or failed".into());
    }
    Ok(String::new())
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

/// Install the daemon: copies the binary to `/usr/local/bin`, registers the
/// LaunchDaemon, and sets up log rotation. Shows exactly one macOS admin
/// password dialog (via `osascript … with administrator privileges`).
/// `Err` means the user cancelled the prompt, the script failed, or
/// `peterfand` wasn't found next to this binary — genuine failures.
pub fn install(dry_run: bool) -> Result<InstallOutcome, String> {
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
    let plist_dst = format!("/Library/LaunchDaemons/{DAEMON_LABEL}.plist");
    let legacy_plist_dst = format!("/Library/LaunchDaemons/{LEGACY_DAEMON_LABEL}.plist");
    let script = format!(
        "set -e\n\
         launchctl bootout system '{legacy_plist_dst}' 2>/dev/null || true\n\
         rm -f '{legacy_plist_dst}'\n\
         install -m 755 '{staged_bin}' /usr/local/bin/peterfand\n\
         rm -f '{staged_bin}'\n\
         cat > '{plist_dst}' <<'PLIST'\n{plist}PLIST\n\
         chown root:wheel '{plist_dst}'\n\
         chmod 644 '{plist_dst}'\n\
         launchctl bootout system '{plist_dst}' 2>/dev/null || true\n\
         launchctl bootstrap system '{plist_dst}'\n\
         mkdir -p /etc/newsyslog.d\n\
         printf '%s' '{newsyslog}' > {newsyslog_conf}\n\
         chmod 644 {newsyslog_conf}\n",
        staged_bin = staged_bin.display(),
        plist = daemon_plist(),
        legacy_plist_dst = legacy_plist_dst,
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
         rm -f '{plist_dst}' '{legacy_plist_dst}' /usr/local/bin/peterfand\n\
         rm -f {NEWSYSLOG_CONF}\n"
    );
    let dry_run_output = run_privileged(&script, dry_run)?;
    if dry_run {
        return Ok(InstallOutcome::DryRun(dry_run_output));
    }
    Ok(InstallOutcome::Installed)
}
