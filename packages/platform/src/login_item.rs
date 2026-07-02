//! Menu-bar "launch at login" management — a per-user LaunchAgent, so unlike
//! [`crate::daemon_install`] this never needs an admin password. Shared by
//! the CLI's `peterfan login-item` and the menu-bar app's own "Launch at
//! Login" menu toggle.

use std::path::PathBuf;

const LABEL: &str = "dev.peterfan.menubar";
const BINARY_NAME: &str = "peterfan-menubar";

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
        let sibling = exe.parent().map(|d| d.join(BINARY_NAME));
        if let Some(s) = sibling.filter(|p| p.exists()) {
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
    let metric_arg = if metric == "cpu" {
        String::new()
    } else {
        format!("\n    <string>--metric</string>\n    <string>{metric}</string>")
    };
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

/// Install (or reinstall) the login item and load it immediately — no admin
/// password needed, this is a per-user LaunchAgent. Returns the binary path
/// and plist path used.
pub fn install(override_binary: Option<&str>, metric: &str) -> Result<(PathBuf, PathBuf), String> {
    let bin = find_menubar_binary(override_binary)?;
    let path = plist_path().ok_or("could not determine home directory")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, plist_contents(&bin, metric)).map_err(|e| e.to_string())?;
    let _ = std::process::Command::new("launchctl")
        .args(["load", "-w", path.to_str().unwrap_or("")])
        .status();
    Ok((bin, path))
}

/// Remove the login item. `Ok(false)` if it wasn't installed.
pub fn remove() -> Result<bool, String> {
    let Some(path) = plist_path() else {
        return Err("could not determine home directory".into());
    };
    if !path.exists() {
        return Ok(false);
    }
    let _ = std::process::Command::new("launchctl")
        .args(["unload", "-w", path.to_str().unwrap_or("")])
        .status();
    std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    Ok(true)
}
