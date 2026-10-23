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

// A fresh adjacent inode avoids retaining a quarantined predecessor's xattrs.
// Keep preparation inside a subshell so cleanup traps do not affect the installer.
fn daemon_plist_replacement_script(destination: &str, body: &str, owner: &str) -> String {
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\"'\"'"));
    format!(
        r#"(
set -e
plist_tmp=$(/usr/bin/mktemp {template})
trap '/bin/rm -f "$plist_tmp"' EXIT
trap 'exit 1' HUP INT TERM
/bin/cat > "$plist_tmp" <<'PETERFAN_DAEMON_PLIST'
{body}PETERFAN_DAEMON_PLIST
/usr/bin/plutil -lint "$plist_tmp" >/dev/null
/usr/sbin/chown {owner} "$plist_tmp"
/bin/chmod 644 "$plist_tmp"
/bin/mv -f "$plist_tmp" {destination}
)
"#,
        template = quote(&format!("{destination}.XXXXXX")),
        destination = quote(destination),
        owner = quote(owner),
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

/// Milestones describe completed/entered installation stages, not bytes or elapsed time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallStage {
    Preflight,
    Installing,
    Verifying,
    Complete,
    Failed,
}

fn finish_reported_install(
    result: Result<InstallOutcome, String>,
    report: &mut impl FnMut(InstallStage),
) -> Result<InstallOutcome, String> {
    match &result {
        Ok(InstallOutcome::Installed) => report(InstallStage::Complete),
        Ok(InstallOutcome::DryRun(_)) => {}
        _ => report(InstallStage::Failed),
    }
    result
}

fn run_install_steps(
    install: impl FnOnce() -> Result<(), String>,
    verify: impl FnOnce() -> bool,
    report: &mut impl FnMut(InstallStage),
) -> Result<InstallOutcome, String> {
    report(InstallStage::Installing);
    install()?;
    report(InstallStage::Verifying);
    Ok(if verify() {
        InstallOutcome::Installed
    } else {
        InstallOutcome::InstalledButUnreachable
    })
}

/// Distinct from `Err`: the privileged script ran successfully, but the
/// daemon hasn't answered over IPC yet (slow launchd bootstrap, or a real
/// startup failure logged to `/var/log/peterfand.err`). Not a cancellation
/// or script error, so callers shouldn't treat it as one.
pub enum InstallOutcome {
    /// `dry_run` was set — nothing was actually run; this is the script that
    /// *would* run, plus the `osascript` invocation, for inspection.
    DryRun(String),
    /// Installed and the resident daemon confirmed a compatible version over IPC.
    Installed,
    /// The privileged script completed, but the required daemon version has
    /// not been confirmed over IPC yet.
    InstalledButUnreachable,
}

fn required_daemon_is_healthy(
    installed_version: Option<&str>,
    runtime_version: Option<&str>,
    required_version: &str,
) -> bool {
    match (installed_version, runtime_version) {
        (Some(installed), Some(runtime)) => {
            !crate::daemon_update_required(installed)
                && !crate::daemon_update_required(runtime)
                && !crate::updater::is_newer(installed, required_version)
                && !crate::updater::is_newer(runtime, installed)
        }
        _ => false,
    }
}

/// Verify the existing daemon without reinstalling it or requesting privileges.
pub fn verify_existing_with_progress(mut report: impl FnMut(InstallStage)) -> Result<(), String> {
    verify_existing_with_probe(
        || {
            (
                crate::installed_daemon_version(),
                crate::running_daemon_version(),
            )
        },
        &mut report,
    )
}

fn verify_existing_with_probe(
    probe: impl FnOnce() -> (Option<String>, Option<String>),
    report: &mut impl FnMut(InstallStage),
) -> Result<(), String> {
    report(InstallStage::Preflight);
    report(InstallStage::Verifying);
    let (installed, runtime) = probe();
    if required_daemon_is_healthy(
        installed.as_deref(),
        runtime.as_deref(),
        crate::MIN_REQUIRED_DAEMON_VERSION,
    ) {
        report(InstallStage::Complete);
        Ok(())
    } else {
        report(InstallStage::Failed);
        Err("The existing fan-control daemon could not be verified. No administrator changes were made; run Diagnostics and retry.".into())
    }
}

fn wait_for_required_daemon(timeout: std::time::Duration) -> bool {
    wait_for_required_daemon_with_probe(timeout, || {
        (
            crate::installed_daemon_version(),
            crate::running_daemon_version(),
        )
    })
}

fn wait_for_required_daemon_with_probe(
    timeout: std::time::Duration,
    mut probe: impl FnMut() -> (Option<String>, Option<String>),
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let (installed, runtime) = probe();
        if required_daemon_is_healthy(
            installed.as_deref(),
            runtime.as_deref(),
            env!("CARGO_PKG_VERSION"),
        ) {
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
    install_with_progress(dry_run, |_| {})
}

pub fn install_with_progress(
    dry_run: bool,
    report: impl FnMut(InstallStage),
) -> Result<InstallOutcome, String> {
    install_with_mode(dry_run, false, report)
}

/// Explicitly replace even a healthy daemon, after a fresh administrator approval.
pub fn repair(dry_run: bool) -> Result<InstallOutcome, String> {
    install_with_mode(dry_run, true, |_| {})
}

fn install_with_mode(
    dry_run: bool,
    force: bool,
    mut report: impl FnMut(InstallStage),
) -> Result<InstallOutcome, String> {
    report(InstallStage::Preflight);
    let result = install_reported(dry_run, force, &mut report);
    finish_reported_install(result, &mut report)
}

fn install_reported(
    dry_run: bool,
    force: bool,
    report: &mut impl FnMut(InstallStage),
) -> Result<InstallOutcome, String> {
    if !dry_run {
        let installed_version = crate::installed_daemon_version();
        if reuse_existing_daemon(
            force,
            installed_version.as_deref(),
            crate::running_daemon_version().as_deref(),
        ) {
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
    let plist_replacement =
        daemon_plist_replacement_script(&plist_dst, &daemon_plist(), "root:wheel");
    let legacy_plist_dst = format!("/Library/LaunchDaemons/{LEGACY_DAEMON_LABEL}.plist");
    let script = format!(
        "set -e\n\
         /bin/launchctl bootout 'system/{legacy_label}' 2>/dev/null || true\n\
         /bin/launchctl bootout 'system/{daemon_label}' 2>/dev/null || true\n\
         /bin/rm -f '{legacy_plist_dst}'\n\
         /bin/mkdir -p /usr/local/bin\n\
         /usr/bin/install -m 755 '{staged_bin}' {DAEMON_BIN}\n\
         /bin/rm -f '{staged_bin}'\n\
         {plist_replacement}\
         /bin/launchctl bootstrap system '{plist_dst}'\n\
         /bin/mkdir -p /etc/newsyslog.d\n\
         /usr/bin/printf '%s' '{newsyslog}' > {newsyslog_conf}\n\
         /bin/chmod 644 {newsyslog_conf}\n",
        staged_bin = staged_bin.display(),
        plist_replacement = plist_replacement,
        legacy_plist_dst = legacy_plist_dst,
        legacy_label = LEGACY_DAEMON_LABEL,
        daemon_label = DAEMON_LABEL,
        newsyslog = NEWSYSLOG_BODY,
        newsyslog_conf = NEWSYSLOG_CONF,
    );
    if dry_run {
        return run_privileged(&script, true).map(InstallOutcome::DryRun);
    }
    run_install_steps(
        || match run_privileged(&script, false) {
            Ok(_) => Ok(()),
            Err(error) => {
                let _ = std::fs::remove_file(&staged_bin);
                Err(error)
            }
        },
        || wait_for_required_daemon(std::time::Duration::from_secs(10)),
        report,
    )
}

fn reuse_existing_daemon(force: bool, installed: Option<&str>, resident: Option<&str>) -> bool {
    !force && required_daemon_is_healthy(installed, resident, crate::MIN_REQUIRED_DAEMON_VERSION)
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
    reinstall_via_running_daemon_with_progress(dry_run, |_| {})
}

#[cfg(target_os = "macos")]
pub fn reinstall_via_running_daemon_with_progress(
    dry_run: bool,
    mut report: impl FnMut(InstallStage),
) -> Result<InstallOutcome, String> {
    report(InstallStage::Preflight);
    let result = reinstall_reported(dry_run, &mut report);
    finish_reported_install(result, &mut report)
}

#[cfg(target_os = "macos")]
fn reinstall_reported(
    dry_run: bool,
    report: &mut impl FnMut(InstallStage),
) -> Result<InstallOutcome, String> {
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
    report(InstallStage::Installing);
    let reply = crate::ipc::send_command_with_timeout(&cmd, std::time::Duration::from_secs(5));
    if let Some(reply) = reply.as_deref() {
        if !reply.starts_with("ok ") {
            return Err(reply.to_string());
        }
    }

    report(InstallStage::Verifying);
    // The daemon verifies the bundled Developer ID signature before replying,
    // then deliberately replaces and restarts itself. Either step can close
    // IPC before the client receives the acknowledgement. Require the resident
    // process and on-disk binary to reach the bundled app's version. Before
    // the detached copy runs, both can still report the old version; after the
    // copy, a fresh connection can still reach the old process during shutdown.
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
    fn explicit_force_repairs_healthy_helpers_and_default_install_reuses_them() {
        let version = crate::MIN_REQUIRED_DAEMON_VERSION;
        assert!(reuse_existing_daemon(false, Some(version), Some(version)));
        assert!(!reuse_existing_daemon(true, Some(version), Some(version)));
        assert!(!reuse_existing_daemon(false, Some(version), None));
    }

    struct PlistFixture(PathBuf);

    impl PlistFixture {
        fn new() -> Self {
            let output = std::process::Command::new("/usr/bin/mktemp")
                .args(["-d", "-t", "peterfan-plist-replacement"])
                .output()
                .expect("create private test directory");
            assert!(output.status.success());
            Self(PathBuf::from(
                String::from_utf8(output.stdout).unwrap().trim(),
            ))
        }
        fn owner(&self) -> String {
            use std::os::unix::fs::MetadataExt;
            let metadata = std::fs::metadata(&self.0).unwrap();
            format!("{}:{}", metadata.uid(), metadata.gid())
        }
    }

    impl Drop for PlistFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn quarantine_plist(path: &std::path::Path) {
        let output = std::process::Command::new("/usr/bin/xattr")
            .args(["-w", "com.apple.quarantine", "0081;00000000;PeterFanTest;"])
            .arg(path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn plist_has_quarantine(path: &std::path::Path) -> bool {
        let output = std::process::Command::new("/usr/bin/xattr")
            .arg(path)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line == "com.apple.quarantine")
    }

    #[test]
    fn generated_plist_replaces_quarantined_inode_with_a_fresh_valid_file() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let fixture = PlistFixture::new();
        let path = fixture.0.join("owner's daemon.plist");
        std::fs::write(&path, "old content").unwrap();
        quarantine_plist(&path);
        let inode = std::fs::metadata(&path).unwrap().ino();
        let body = daemon_plist();
        // Reproduce the old redirect: replacing bytes leaves the inode's quarantine.
        std::fs::write(&path, &body).unwrap();
        assert!(plist_has_quarantine(&path));
        assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode);
        let script =
            daemon_plist_replacement_script(path.to_str().unwrap(), &body, &fixture.owner());
        let output = std::process::Command::new("/bin/bash")
            .args(["-ec", &script])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !plist_has_quarantine(&path),
            "generated plist must not inherit old inode quarantine"
        );
        assert_ne!(std::fs::metadata(&path).unwrap().ino(), inode);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );
        assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 1);
    }

    #[test]
    fn invalid_staged_plist_preserves_existing_bytes_and_cleans_temporary_file() {
        let fixture = PlistFixture::new();
        let path = fixture.0.join("daemon.plist");
        let old = daemon_plist();
        std::fs::write(&path, &old).unwrap();
        quarantine_plist(&path);
        let script = daemon_plist_replacement_script(
            path.to_str().unwrap(),
            "invalid plist\n",
            &fixture.owner(),
        );
        let output = std::process::Command::new("/bin/bash")
            .args(["-ec", &script])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), old);
        assert!(plist_has_quarantine(&path));
        assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 1);
    }

    #[test]
    fn existing_daemon_verification_requires_the_resident_version_before_completion() {
        let installed = env!("CARGO_PKG_VERSION");
        for runtime in [None, Some("unknown"), Some("1.0.0")] {
            let mut stages = Vec::new();
            assert!(verify_existing_with_probe(
                || (Some(installed.into()), runtime.map(str::to_string)),
                &mut |stage| stages.push(stage),
            )
            .is_err());
            assert_eq!(
                stages,
                [
                    InstallStage::Preflight,
                    InstallStage::Verifying,
                    InstallStage::Failed
                ]
            );
        }
        for compatible in [crate::MIN_REQUIRED_DAEMON_VERSION, installed] {
            let mut stages = Vec::new();
            verify_existing_with_probe(
                || (Some(compatible.into()), Some(compatible.into())),
                &mut |stage| stages.push(stage),
            )
            .unwrap();
            assert_eq!(
                stages,
                [
                    InstallStage::Preflight,
                    InstallStage::Verifying,
                    InstallStage::Complete
                ]
            );
        }
    }

    #[test]
    fn reported_install_stages_follow_real_work_and_stop_on_failure() {
        let mut stages = Vec::new();
        let result = run_install_steps(|| Ok(()), || true, &mut |stage| stages.push(stage));
        finish_reported_install(result, &mut |stage| stages.push(stage)).unwrap();
        assert_eq!(
            stages,
            [
                InstallStage::Installing,
                InstallStage::Verifying,
                InstallStage::Complete
            ]
        );
        stages.clear();
        let result = run_install_steps(
            || Err("cancelled".into()),
            || panic!("must not verify"),
            &mut |stage| stages.push(stage),
        );
        assert!(finish_reported_install(result, &mut |stage| stages.push(stage)).is_err());
        assert_eq!(stages, [InstallStage::Installing, InstallStage::Failed]);
        stages.clear();
        let result = run_install_steps(|| Ok(()), || false, &mut |stage| stages.push(stage));
        assert!(matches!(
            finish_reported_install(result, &mut |stage| stages.push(stage)),
            Ok(InstallOutcome::InstalledButUnreachable)
        ));
        assert_eq!(
            stages,
            [
                InstallStage::Installing,
                InstallStage::Verifying,
                InstallStage::Failed
            ]
        );
    }

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
    fn required_daemon_health_needs_both_installed_and_runtime_versions() {
        assert!(required_daemon_is_healthy(
            Some(crate::MIN_REQUIRED_DAEMON_VERSION),
            Some(crate::MIN_REQUIRED_DAEMON_VERSION),
            crate::MIN_REQUIRED_DAEMON_VERSION,
        ));
        assert!(!required_daemon_is_healthy(
            Some("1.27.22"),
            Some("1.27.22"),
            crate::MIN_REQUIRED_DAEMON_VERSION,
        ));
        assert!(!required_daemon_is_healthy(
            Some(crate::MIN_REQUIRED_DAEMON_VERSION),
            None,
            crate::MIN_REQUIRED_DAEMON_VERSION,
        ));
        assert!(!required_daemon_is_healthy(None, Some("1.32.3"), "1.32.3"));
    }

    #[test]
    fn required_daemon_health_rejects_a_stale_or_invalid_resident_after_disk_replacement() {
        assert!(!required_daemon_is_healthy(
            Some("1.32.5"),
            Some("1.32.4"),
            "1.32.5"
        ));
        assert!(!required_daemon_is_healthy(
            Some("1.32.5"),
            Some("1.32.5-beta.1"),
            "1.32.5",
        ));
        assert!(!required_daemon_is_healthy(
            Some("1.32.5"),
            Some("unknown"),
            "1.32.5"
        ));
        assert!(!required_daemon_is_healthy(
            Some("unknown"),
            Some("1.32.5"),
            "1.32.5"
        ));
        assert!(required_daemon_is_healthy(
            Some("1.32.5"),
            Some("1.32.5+build.7"),
            "1.32.5",
        ));
        assert!(required_daemon_is_healthy(
            Some("1.32.5"),
            Some("1.32.6"),
            "1.32.5"
        ));
    }

    #[test]
    fn install_waits_for_a_healthy_daemon_instead_of_using_a_fixed_delay() {
        let current = env!("CARGO_PKG_VERSION").to_string();
        let mut samples = [
            (Some("1.27.29".to_string()), Some("1.27.29".to_string())),
            (Some(current.clone()), Some("1.27.29".to_string())),
            (Some(current.clone()), Some(current)),
        ]
        .into_iter();
        assert!(wait_for_required_daemon_with_probe(
            std::time::Duration::from_secs(2),
            || samples
                .next()
                .expect("must finish after the new resident responds"),
        ));
        assert!(samples.next().is_none());
    }

    #[test]
    fn install_never_succeeds_when_only_the_old_resident_answers() {
        assert!(!wait_for_required_daemon_with_probe(
            std::time::Duration::from_millis(20),
            || (Some("1.27.29".into()), Some("1.27.29".into())),
        ));
    }
}
