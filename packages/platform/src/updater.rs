//! Check GitHub Releases for a newer version, and (macOS) download + install
//! it in place. Shared by `peterfan update` and the menu-bar app's automatic
//! update check.
//!
//! Shells out to `curl`/`tar` rather than pulling in an HTTP client crate —
//! consistent with how the rest of the codebase talks to `osascript`/
//! `launchctl`, and keeps the menu-bar binary's dependency footprint small.

pub const REPO: &str = "uulab-official/peterfan";

#[derive(Debug, Clone, PartialEq)]
pub struct ReleaseInfo {
    /// Without the leading `v`, e.g. `"1.13.0"`.
    pub version: String,
    pub tag: String,
    pub html_url: String,
    /// Preferred direct download URL for the macOS app update asset.
    ///
    /// PeterFan prefers the notarized DMG because it is the same artifact end
    /// users install and it carries the strongest release validation. If that
    /// is absent, it falls back to the universal `apple-darwin.tar.gz`.
    pub asset_url: Option<String>,
    pub asset_name: Option<String>,
    pub archive_url: Option<String>,
    pub dmg_url: Option<String>,
}

/// Query the GitHub API for the latest release. `Err` covers network
/// failure, missing `curl`, and unexpected response shapes alike — callers
/// treat "couldn't check" and "nothing to report" the same way.
pub fn fetch_latest_release() -> Result<ReleaseInfo, String> {
    let out = std::process::Command::new("curl")
        .args([
            "-s",
            "--max-time",
            "8",
            "-H",
            "User-Agent: peterfan-updater",
            &format!("https://api.github.com/repos/{REPO}/releases/latest"),
        ])
        .output()
        .map_err(|e| format!("curl not available: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "curl exited with {:?}: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    parse_release_response(&out.stdout)
}

fn parse_release_response(body: &[u8]) -> Result<ReleaseInfo, String> {
    let val: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| format!("unexpected GitHub response: {e}"))?;
    let tag = val["tag_name"]
        .as_str()
        .ok_or("response has no tag_name")?
        .to_string();
    let version = tag.trim_start_matches('v').to_string();
    let html_url = val["html_url"].as_str().unwrap_or_default().to_string();
    let empty_assets = Vec::new();
    let assets = val["assets"].as_array().unwrap_or(&empty_assets);
    let dmg = find_asset(assets, is_macos_dmg);
    let archive = find_asset(assets, is_preferred_macos_archive)
        .or_else(|| find_asset(assets, is_macos_archive));
    let preferred = dmg.as_ref().or(archive.as_ref());
    Ok(ReleaseInfo {
        version,
        tag,
        html_url,
        asset_url: preferred.map(|a| a.url.clone()),
        asset_name: preferred.map(|a| a.name.clone()),
        archive_url: archive.map(|a| a.url),
        dmg_url: dmg.map(|a| a.url),
    })
}

#[derive(Debug, Clone)]
struct Asset {
    name: String,
    url: String,
}

fn find_asset<F>(assets: &[serde_json::Value], matches: F) -> Option<Asset>
where
    F: Fn(&str) -> bool,
{
    assets.iter().find_map(|asset| {
        let name = asset["name"].as_str().unwrap_or_default();
        let url = asset["browser_download_url"].as_str().unwrap_or_default();
        if matches(name) && !url.is_empty() {
            Some(Asset {
                name: name.to_string(),
                url: url.to_string(),
            })
        } else {
            None
        }
    })
}

fn is_preferred_macos_archive(name: &str) -> bool {
    name.contains("universal-apple-darwin") && name.ends_with(".tar.gz")
}

fn is_macos_archive(name: &str) -> bool {
    name.contains("apple-darwin") && name.ends_with(".tar.gz")
}

fn is_macos_dmg(name: &str) -> bool {
    name.starts_with("PeterFan-") && name.ends_with(".dmg")
}

/// Numeric semver-ish comparison (`"1.13.0"` vs `"1.9.6"` — a naive string
/// compare would get this backwards). Missing/non-numeric components count
/// as 0, so `"1.13"` and `"1.13.0"` compare equal.
pub fn is_newer(current: &str, latest: &str) -> bool {
    fn parts(s: &str) -> [u64; 3] {
        let mut out = [0u64; 3];
        for (i, p) in s.split('.').take(3).enumerate() {
            out[i] = p.parse().unwrap_or(0);
        }
        out
    }
    parts(latest) > parts(current)
}

/// Locate the `.app` bundle containing the currently running executable
/// (`.../PeterFan.app/Contents/MacOS/PeterFan` → `.../PeterFan.app`).
#[cfg(target_os = "macos")]
pub fn current_app_bundle() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let app = exe
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .ok_or("could not walk up to a .app bundle from the running executable")?;
    if app.extension().and_then(|e| e.to_str()) != Some("app") {
        return Err(format!(
            "not running from inside a .app bundle (looked at {})",
            app.display()
        ));
    }
    Ok(app.to_path_buf())
}

/// Download `asset_url`, extract it, and write a detached helper script that
/// (after this process quits) replaces the running `.app` bundle and
/// relaunches it. Returns once the script is queued — the caller should quit
/// shortly after (see module docs on the menu-bar side for the confirm-first
/// flow this is meant to sit behind).
#[cfg(target_os = "macos")]
pub fn download_and_install(asset_url: &str) -> Result<(), String> {
    let app_path = current_app_bundle()?;
    let tmp_dir = std::env::temp_dir().join(format!("peterfan-update-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;

    let is_dmg = asset_url
        .split('?')
        .next()
        .unwrap_or(asset_url)
        .ends_with(".dmg");
    let download = tmp_dir.join(if is_dmg {
        "update.dmg"
    } else {
        "update.tar.gz"
    });
    let status = std::process::Command::new("curl")
        .args([
            "-fL",
            "--show-error",
            "--max-time",
            "120",
            "-H",
            "User-Agent: peterfan-updater",
            "-o",
        ])
        .arg(&download)
        .arg(asset_url)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("download failed".into());
    }

    let new_app = if is_dmg {
        extract_app_from_dmg(&download, &tmp_dir)?
    } else {
        extract_app_from_archive(&download, &tmp_dir)?
    };
    validate_update_app(&new_app)?;

    // A detached script rather than doing the replace in-process: this
    // process's own executable is inside the bundle being replaced, and the
    // switch has to happen after it quits.
    let script_path = tmp_dir.join("apply-update.sh");
    let backup_path = tmp_dir.join("PreviousPeterFan.app");
    let log_path = tmp_dir.join("apply-update.log");
    let script = format!(
        "#!/bin/bash\n\
         set -e\n\
         exec >{log} 2>&1\n\
         sleep 1\n\
         rm -rf {backup}\n\
         mv {app} {backup}\n\
         if ditto {new_app} {app}; then\n\
         \txcrun stapler validate {app} >/dev/null 2>&1 || true\n\
         \topen {app}\n\
         \trm -rf {tmp}\n\
         else\n\
         \trm -rf {app}\n\
         \tmv {backup} {app}\n\
         \topen {app}\n\
         \texit 1\n\
         fi\n",
        app = shell_quote(&app_path),
        new_app = shell_quote(&new_app),
        backup = shell_quote(&backup_path),
        tmp = shell_quote(&tmp_dir),
        log = shell_quote(&log_path),
    );
    std::fs::write(&script_path, script).map_err(|e| e.to_string())?;
    let _ = std::process::Command::new("chmod")
        .args(["+x"])
        .arg(&script_path)
        .status();

    std::process::Command::new("/bin/bash")
        .arg(&script_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("could not launch the updater script: {e}"))?;

    Ok(())
}

#[cfg(target_os = "macos")]
fn shell_quote(p: &std::path::Path) -> String {
    format!("'{}'", p.display().to_string().replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
fn extract_app_from_archive(
    archive: &std::path::Path,
    tmp_dir: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let status = std::process::Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(tmp_dir)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("extracting the update failed".into());
    }
    find_app_bundle(tmp_dir)
        .ok_or("downloaded archive did not contain a PeterFan.app bundle".into())
}

#[cfg(target_os = "macos")]
fn extract_app_from_dmg(
    dmg: &std::path::Path,
    tmp_dir: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let mount_dir = tmp_dir.join("mount");
    let extracted_app = tmp_dir.join("PeterFan.app");
    std::fs::create_dir_all(&mount_dir).map_err(|e| e.to_string())?;
    let status = std::process::Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-quiet", "-mountpoint"])
        .arg(&mount_dir)
        .arg(dmg)
        .status()
        .map_err(|e| format!("could not mount DMG: {e}"))?;
    if !status.success() {
        return Err("mounting the update DMG failed".into());
    }

    let result = (|| {
        let mounted_app = mount_dir.join("PeterFan.app");
        if !mounted_app.is_dir() {
            return Err("update DMG did not contain PeterFan.app".into());
        }
        let status = std::process::Command::new("ditto")
            .arg(&mounted_app)
            .arg(&extracted_app)
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("copying PeterFan.app out of the update DMG failed".into());
        }
        Ok(extracted_app)
    })();

    let _ = std::process::Command::new("hdiutil")
        .arg("detach")
        .arg("-quiet")
        .arg(&mount_dir)
        .status();
    result
}

#[cfg(target_os = "macos")]
fn validate_update_app(app: &std::path::Path) -> Result<(), String> {
    let status = std::process::Command::new("codesign")
        .args(["--verify", "--deep", "--strict", "--verbose=2"])
        .arg(app)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("downloaded PeterFan.app has an invalid code signature".into());
    }

    let status = std::process::Command::new("xcrun")
        .args(["stapler", "validate"])
        .arg(app)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("downloaded PeterFan.app is not notarized/stapled".into());
    }

    let status = std::process::Command::new("spctl")
        .args(["-a", "-vv", "-t", "exec"])
        .arg(app)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("Gatekeeper rejected the downloaded PeterFan.app".into());
    }

    Ok(())
}

/// Find the first `*.app` directory anywhere under `root` (one or two levels
/// deep — archives extract to a version-named folder containing the bundle).
#[cfg(target_os = "macos")]
fn find_app_bundle(root: &std::path::Path) -> Option<std::path::PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("app") {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_app_bundle(&path) {
                return Some(found);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_compares_numerically_not_lexically() {
        assert!(is_newer("1.9.6", "1.13.0"));
        assert!(!is_newer("1.13.0", "1.9.6"));
        assert!(!is_newer("1.13.0", "1.13.0"));
        assert!(is_newer("1.13.0", "2.0.0"));
        assert!(!is_newer("2.0.0", "1.99.99"));
    }

    #[test]
    fn is_newer_treats_missing_components_as_zero() {
        assert!(!is_newer("1.13", "1.13.0"));
        assert!(is_newer("1.13", "1.13.1"));
    }

    #[test]
    fn parses_real_github_release_response() {
        let body = br#"{
            "tag_name": "v0.27.1",
            "html_url": "https://github.com/uulab-official/peterfan/releases/tag/v0.27.1",
            "assets": [
                {"name": "peterfan-v0.27.1-aarch64-apple-darwin.tar.gz",
                 "browser_download_url": "https://github.com/uulab-official/peterfan/releases/download/v0.27.1/peterfan-v0.27.1-aarch64-apple-darwin.tar.gz"},
                {"name": "peterfan-v0.27.1-x86_64-pc-windows-msvc.zip",
                 "browser_download_url": "https://example.com/windows.zip"}
            ]
        }"#;
        let info = parse_release_response(body).unwrap();
        assert_eq!(info.version, "0.27.1");
        assert_eq!(info.tag, "v0.27.1");
        assert!(info.asset_url.unwrap().contains("aarch64-apple-darwin"));
        assert!(info.asset_name.unwrap().contains("aarch64-apple-darwin"));
    }

    #[test]
    fn prefers_dmg_for_ota_when_both_are_present() {
        let body = br#"{
            "tag_name": "v2.0.0",
            "html_url": "https://example.com",
            "assets": [
                {"name": "PeterFan-v2.0.0.dmg",
                 "browser_download_url": "https://example.com/PeterFan.dmg"},
                {"name": "peterfan-v2.0.0-aarch64-apple-darwin.tar.gz",
                 "browser_download_url": "https://example.com/arm.tar.gz"},
                {"name": "peterfan-v2.0.0-universal-apple-darwin.tar.gz",
                 "browser_download_url": "https://example.com/universal.tar.gz"}
            ]
        }"#;
        let info = parse_release_response(body).unwrap();
        assert_eq!(info.asset_url.unwrap(), "https://example.com/PeterFan.dmg");
        assert_eq!(
            info.archive_url.unwrap(),
            "https://example.com/universal.tar.gz"
        );
        assert_eq!(info.dmg_url.unwrap(), "https://example.com/PeterFan.dmg");
    }

    #[test]
    fn falls_back_to_dmg_when_archive_is_missing() {
        let body = br#"{
            "tag_name": "v2.1.0",
            "html_url": "https://example.com",
            "assets": [
                {"name": "PeterFan-v2.1.0.dmg",
                 "browser_download_url": "https://example.com/PeterFan.dmg"}
            ]
        }"#;
        let info = parse_release_response(body).unwrap();
        assert_eq!(info.asset_url.unwrap(), "https://example.com/PeterFan.dmg");
        assert!(info.archive_url.is_none());
        assert_eq!(info.dmg_url.unwrap(), "https://example.com/PeterFan.dmg");
    }

    #[test]
    fn missing_tag_name_is_an_error() {
        assert!(parse_release_response(b"{}").is_err());
    }

    #[test]
    fn release_without_assets_still_reports_version() {
        let body = br#"{
            "tag_name": "v2.2.0",
            "html_url": "https://example.com",
            "assets": []
        }"#;
        let info = parse_release_response(body).unwrap();
        assert_eq!(info.version, "2.2.0");
        assert!(info.asset_url.is_none());
        assert!(info.asset_name.is_none());
    }
}
