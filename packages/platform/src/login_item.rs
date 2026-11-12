//! Menu-bar "launch at login" management — a per-user LaunchAgent, so unlike
//! [`crate::daemon_install`] this never needs an admin password. Shared by
//! the CLI's `peterfan login-item` and the menu-bar app's own "Launch at
//! Login" menu toggle.

use std::path::PathBuf;

const LABEL: &str = "kr.co.uulab.peterfan.menubar";
const BINARY_NAME: &str = "peterfan-menubar";
const APP_BINARY_NAME: &str = "PeterFan";

pub fn plist_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h: PathBuf| {
        h.join("Library")
            .join("LaunchAgents")
            .join(format!("{LABEL}.plist"))
    })
}

pub fn is_installed() -> bool {
    plist_path().is_some_and(|p| p.exists())
}

fn launchctl_domain() -> String {
    format!("gui/{}", unsafe { libc::getuid() })
}

fn launchctl_service() -> String {
    format!("{}/{}", launchctl_domain(), LABEL)
}

/// Find the `peterfan-menubar` binary next to the current executable (or an
/// explicit override path).
pub fn find_menubar_binary(override_path: Option<&str>) -> Result<PathBuf, String> {
    if let Some(p) = override_path {
        let path = PathBuf::from(p);
        return if path.exists() {
            Ok(path)
        } else {
            Err(format!("binary not found at '{p}'"))
        };
    }
    if let Ok(exe) = std::env::current_exe() {
        if exe
            .file_name()
            .is_some_and(|name| name == BINARY_NAME || name == APP_BINARY_NAME)
        {
            return Ok(exe);
        }
        let sibling = exe.parent().map(|d| d.join(BINARY_NAME));
        if let Some(s) = sibling.filter(|p| p.exists()) {
            return Ok(s);
        }
        let app_sibling = exe.parent().map(|d| d.join(APP_BINARY_NAME));
        if let Some(s) = app_sibling.filter(|p| p.exists()) {
            return Ok(s);
        }
    }
    if let Ok(out) = std::process::Command::new("which")
        .arg(BINARY_NAME)
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return Ok(PathBuf::from(s));
        }
    }
    Err(format!(
        "could not find '{BINARY_NAME}' — use --binary <path> to specify its location"
    ))
}

fn plist_contents(bin: &std::path::Path, metric: &str) -> String {
    let metric_arg = format!("\n    <string>--metric</string>\n    <string>{metric}</string>");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>       <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>{metric_arg}
  </array>
  <key>RunAtLoad</key>   <true/>
  <key>KeepAlive</key>   <false/>
  <key>StandardOutPath</key> <string>/tmp/peterfan-menubar.log</string>
  <key>StandardErrorPath</key> <string>/tmp/peterfan-menubar.log</string>
</dict>
</plist>
"#,
        bin.display()
    )
}

/// Install the login item and load it immediately — no admin
/// password needed, this is a per-user LaunchAgent. Returns the binary path
/// and plist path used.
pub fn install(override_binary: Option<&str>, metric: &str) -> Result<(PathBuf, PathBuf), String> {
    let metric = peterfan_core::config::MenubarMetric::parse(metric)
        .ok_or_else(|| format!("unknown menu-bar metric: {metric}"))?;
    let bin = find_menubar_binary(override_binary)?;
    let path = plist_path().ok_or("could not determine home directory")?;
    let agent = prepare_agent(&path, &bin)?;
    install_with_metric_save(
        || {
            crate::config::update(|config| {
                config.menubar.metric = metric;
                Ok(())
            })
            .map(|_| ())
            .map_err(|e| e.to_string())
        },
        || install_agent(&bin, &path, metric.as_str(), &agent),
    )?;
    Ok((bin, path))
}

/// Enable login without changing the user's selected metric.
pub fn install_preserving_metric(
    override_binary: Option<&str>,
) -> Result<(PathBuf, PathBuf), String> {
    let bin = find_menubar_binary(override_binary)?;
    let path = plist_path().ok_or("could not determine home directory")?;
    let agent = prepare_agent(&path, &bin)?;
    let metric = crate::config::load().menubar.metric;
    install_agent(&bin, &path, metric.as_str(), &agent)?;
    Ok((bin, path))
}

fn install_with_metric_save(
    save: impl FnOnce() -> Result<(), String>,
    publish: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    save().map_err(|e| format!("metric was not saved; login item unchanged: {e}"))?;
    publish()
        .map_err(|e| format!("metric was saved; login item installation may be incomplete: {e}"))
}

fn install_agent(
    bin: &std::path::Path,
    path: &std::path::Path,
    metric: &str,
    agent: &AgentPlan,
) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    match agent {
        AgentPlan::New => {
            write_new_agent(path, &plist_contents(bin, metric)).map_err(|e| e.to_string())?
        }
        AgentPlan::Reuse(original) => {
            if read_regular_agent(path).map_err(|e| e.to_string())? != *original {
                return Err("login item changed during installation; preserving the file".into());
            }
        }
    }
    let domain = launchctl_domain();
    let service = launchctl_service();
    let path_str = path.to_str().unwrap_or("");
    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &domain, path_str])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let bootstrap = std::process::Command::new("launchctl")
        .args(["bootstrap", &domain, path_str])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| e.to_string())?;
    if !bootstrap.success() {
        let legacy = std::process::Command::new("launchctl")
            .args(["load", "-w", path_str])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| e.to_string())?;
        if !legacy.success() {
            return Err("launchctl could not load the login item".into());
        }
    }
    let _ = std::process::Command::new("launchctl")
        .args(["enable", &service])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let _ = std::process::Command::new("launchctl")
        .args(["kickstart", "-k", &service])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    Ok(())
}

enum AgentPlan {
    New,
    Reuse(String),
}

fn is_owned_agent(contents: &str, bin: &std::path::Path) -> bool {
    let metrics = ["cpu", "memory", "temp", "fan", "network"];
    metrics
        .iter()
        .any(|metric| contents == plist_contents(bin, metric))
        || contents
            == plist_contents(bin, "cpu").replace(
                "\n    <string>--metric</string>\n    <string>cpu</string>",
                "",
            )
}

fn is_owned_agent_any_binary(contents: &str) -> bool {
    let Some(rest) = contents
        .split_once("  <array>\n    <string>")
        .map(|(_, tail)| tail)
    else {
        return false;
    };
    let Some((binary, _)) = rest.split_once("</string>") else {
        return false;
    };
    let path = std::path::Path::new(binary);
    path.is_absolute()
        && path
            .file_name()
            .is_some_and(|name| name == BINARY_NAME || name == APP_BINARY_NAME)
        && is_owned_agent(contents, path)
}

fn read_regular_agent(path: &std::path::Path) -> std::io::Result<String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > 65536 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "login item is not a bounded regular file",
        ));
    }
    let mut text = String::new();
    file.by_ref().take(65537).read_to_string(&mut text)?;
    if text.len() > 65536 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "login item exceeds size limit",
        ));
    }
    Ok(text)
}

fn prepare_agent(path: &std::path::Path, bin: &std::path::Path) -> Result<AgentPlan, String> {
    match read_regular_agent(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AgentPlan::New),
        Ok(contents) if is_owned_agent(&contents, bin) => Ok(AgentPlan::Reuse(contents)),
        Ok(_) => Err(format!(
            "login item at {} is customized or uses another binary; preserving it",
            path.display()
        )),
        Err(e) => Err(format!(
            "cannot safely inspect login item at {}: {e}",
            path.display()
        )),
    }
}

// Hard-linking a completed temporary file commits atomically without ever
// replacing an existing regular file or symlink, including a racing writer.
fn write_new_agent(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("login item has no parent directory"))?;
    let temp = parent.join(format!(
        ".peterfan-agent-{}-{}.tmp",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let result = (|| {
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        std::fs::hard_link(&temp, path)
    })();
    drop(file);
    let _ = std::fs::remove_file(temp);
    result
}

/// Remove the login item. `Ok(false)` if it wasn't installed.
pub fn remove() -> Result<bool, String> {
    let Some(path) = plist_path() else {
        return Err("could not determine home directory".into());
    };
    let original = match read_regular_agent(&path) {
        Ok(original) if is_owned_agent_any_binary(&original) => original,
        Ok(_) => return Err("login item is customized; preserving it".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("cannot safely inspect login item: {e}")),
    };
    let domain = launchctl_domain();
    let path_str = path.to_str().unwrap_or("");
    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &domain, path_str])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let _ = std::process::Command::new("launchctl")
        .args(["unload", "-w", path_str])
        .status();
    if read_regular_agent(&path).map_err(|e| e.to_string())? != original {
        return Err("login item changed during removal; preserving it".into());
    }
    std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod metric_contract_tests {
    use super::*;

    #[test]
    fn login_item_passes_default_and_explicit_metrics_to_the_app() {
        let bin = std::path::Path::new("/Applications/PeterFan.app/Contents/MacOS/PeterFan");
        for metric in ["cpu", "memory", "temp", "fan", "network"] {
            let plist = plist_contents(bin, metric);
            assert!(
                plist.contains("<string>--metric</string>"),
                "{metric} missing flag"
            );
            assert!(plist.contains(&format!("<string>{metric}</string>")));
        }
    }

    #[test]
    fn explicit_install_reports_partial_failure_and_never_publishes_after_save_failure() {
        use std::cell::Cell;
        let calls = Cell::new(0);
        let err = install_with_metric_save(
            || Err("config failed".into()),
            || {
                calls.set(calls.get() + 1);
                Ok(())
            },
        )
        .unwrap_err();
        assert!(err.contains("config failed"));
        assert_eq!(calls.get(), 0);
        let err =
            install_with_metric_save(|| Ok(()), || Err("bootstrap failed".into())).unwrap_err();
        assert!(err.contains("metric was saved"));
        assert!(err.contains("bootstrap failed"));
    }

    #[test]
    fn launch_agent_creation_never_replaces_an_existing_file() {
        let directory =
            std::env::temp_dir().join(format!("peterfan-login-agent-test-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("agent.plist");
        write_new_agent(&path, "original").unwrap();
        assert!(write_new_agent(&path, "replacement").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn plan_reuses_own_agent_and_rejects_custom_files_and_symlinks() {
        use std::os::unix::fs::symlink;
        let directory =
            std::env::temp_dir().join(format!("peterfan-login-plan-test-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("agent.plist");
        let bin = std::path::Path::new("/Applications/PeterFan.app/Contents/MacOS/PeterFan");
        assert!(matches!(prepare_agent(&path, bin), Ok(AgentPlan::New)));
        let expected = plist_contents(bin, "cpu");
        std::fs::write(&path, &expected).unwrap();
        assert!(matches!(prepare_agent(&path, bin), Ok(AgentPlan::Reuse(_))));
        std::fs::write(&path, "custom").unwrap();
        assert!(prepare_agent(&path, bin).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "custom");
        let link = directory.join("link.plist");
        symlink(&path, &link).unwrap();
        assert!(prepare_agent(&link, bin).is_err());
        std::fs::remove_file(link).unwrap();
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn existing_own_unmodified_agent_can_be_reused_but_custom_content_cannot() {
        let bin = std::path::Path::new("/Applications/PeterFan.app/Contents/MacOS/PeterFan");
        for metric in ["cpu", "memory", "temp", "fan", "network"] {
            assert!(is_owned_agent(&plist_contents(bin, metric), bin));
        }
        let legacy = plist_contents(bin, "cpu").replace(
            "\n    <string>--metric</string>\n    <string>cpu</string>",
            "",
        );
        assert!(is_owned_agent(&legacy, bin));
        assert!(is_owned_agent_any_binary(&legacy));
        assert!(!is_owned_agent(&legacy.replace("<false/>", "<true/>"), bin));
        assert!(!is_owned_agent(
            &legacy,
            std::path::Path::new("/different/PeterFan")
        ));
        assert!(!is_owned_agent_any_binary(
            &legacy.replace("<false/>", "<true/>")
        ));
    }
}
