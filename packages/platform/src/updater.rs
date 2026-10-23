//! Check GitHub Releases for a newer version and install verified desktop
//! updates in place on macOS and Windows. Shared by `peterfan update` and the
//! menu-bar app's automatic update check.
//!
//! Shells out to `curl`/`tar` rather than pulling in an HTTP client crate —
//! consistent with how the rest of the codebase talks to `osascript`/
//! `launchctl`, and keeps the menu-bar binary's dependency footprint small.

pub const REPO: &str = "uulab-official/peterfan";
pub const EXPECTED_BUNDLE_ID: &str = "kr.co.uulab.peterfan";
pub const EXPECTED_TEAM_ID: &str = "N99FMBQ662";

use peterfan_core::config::UpdateChannel;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UpdateInstallResult {
    /// `pending`, `installed`, `rolled_back`, or `failed`.
    pub status: String,
    pub version: String,
    pub message: String,
    pub updated_at_unix: u64,
}

impl UpdateInstallResult {
    fn new(status: &str, version: &str, message: impl Into<String>) -> Self {
        Self {
            status: status.to_string(),
            version: version.to_string(),
            message: message.into(),
            updated_at_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

/// The detached updater outlives the app process, so its final result must
/// live outside the temporary extraction directory that is removed on
/// success. Keeping one small JSON record also bounds storage across updates.
pub fn update_install_result_path() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|dir| dir.join("PeterFan").join("update-result.json"))
}

pub fn read_update_install_result() -> Option<UpdateInstallResult> {
    let path = update_install_result_path()?;
    read_update_install_result_from(&path)
}

fn read_update_install_result_from(path: &std::path::Path) -> Option<UpdateInstallResult> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_update_install_result_to(
    path: &std::path::Path,
    result: &UpdateInstallResult,
) -> Result<(), String> {
    let parent = path.parent().ok_or("update result path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = path.with_extension(format!("json.tmp-{}", std::process::id()));
    let json = serde_json::to_vec(result).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn prepare_update_helper(
    script_path: &std::path::Path,
    script: &[u8],
    result_path: &std::path::Path,
    pending: &UpdateInstallResult,
) -> Result<(), String> {
    // A script preparation failure must not leave a receipt claiming that a
    // detached installer is running. The caller records spawn failures below.
    std::fs::write(script_path, script).map_err(|e| e.to_string())?;
    write_update_install_result_to(result_path, pending)
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct IntegrityCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AppIntegrityReport {
    pub path: String,
    pub ok: bool,
    pub bundle_id: Option<String>,
    pub version: Option<String>,
    pub team_id: Option<String>,
    pub checks: Vec<IntegrityCheck>,
}

impl AppIntegrityReport {
    fn new(app: &std::path::Path) -> Self {
        Self {
            path: app.display().to_string(),
            ok: false,
            bundle_id: None,
            version: None,
            team_id: None,
            checks: Vec::new(),
        }
    }

    fn push(&mut self, name: impl Into<String>, ok: bool, detail: impl Into<String>) {
        self.checks.push(IntegrityCheck {
            name: name.into(),
            ok,
            detail: detail.into(),
        });
    }

    fn finish(mut self) -> Self {
        self.ok = !self.checks.is_empty() && self.checks.iter().all(|check| check.ok);
        self
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ReleaseIntegrityReport {
    pub tag: String,
    pub version: String,
    pub release_url: String,
    pub asset_name: Option<String>,
    pub asset_sha256: Option<String>,
    pub ok: bool,
    pub checks: Vec<IntegrityCheck>,
    pub app: Option<AppIntegrityReport>,
}

impl ReleaseIntegrityReport {
    fn new(release: &ReleaseInfo) -> Self {
        Self {
            tag: release.tag.clone(),
            version: release.version.clone(),
            release_url: release.html_url.clone(),
            asset_name: release.asset_name.clone(),
            asset_sha256: None,
            ok: false,
            checks: Vec::new(),
            app: None,
        }
    }

    fn push(&mut self, name: impl Into<String>, ok: bool, detail: impl Into<String>) {
        self.checks.push(IntegrityCheck {
            name: name.into(),
            ok,
            detail: detail.into(),
        });
    }

    fn finish(mut self) -> Self {
        self.ok = !self.checks.is_empty()
            && self.checks.iter().all(|check| check.ok)
            && self.app.as_ref().map(|app| app.ok).unwrap_or(false);
        self
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ArtifactIntegrityReport {
    pub path: String,
    pub asset_name: String,
    pub asset_sha256: Option<String>,
    pub ok: bool,
    pub checks: Vec<IntegrityCheck>,
    pub app: Option<AppIntegrityReport>,
}

impl ArtifactIntegrityReport {
    fn new(asset: &std::path::Path) -> Self {
        Self {
            path: asset.display().to_string(),
            asset_name: asset
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("artifact")
                .to_string(),
            asset_sha256: None,
            ok: false,
            checks: Vec::new(),
            app: None,
        }
    }

    fn push(&mut self, name: impl Into<String>, ok: bool, detail: impl Into<String>) {
        self.checks.push(IntegrityCheck {
            name: name.into(),
            ok,
            detail: detail.into(),
        });
    }

    fn finish(mut self) -> Self {
        self.ok = !self.checks.is_empty()
            && self.checks.iter().all(|check| check.ok)
            && self.app.as_ref().map(|app| app.ok).unwrap_or(false);
        self
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ReleaseDirectoryIntegrityReport {
    pub path: String,
    pub ok: bool,
    pub expected_version: Option<String>,
    pub checksums: Option<String>,
    pub checks: Vec<IntegrityCheck>,
    pub artifacts: Vec<ArtifactIntegrityReport>,
}

impl ReleaseDirectoryIntegrityReport {
    fn new(dir: &std::path::Path) -> Self {
        Self {
            path: dir.display().to_string(),
            ok: false,
            expected_version: None,
            checksums: None,
            checks: Vec::new(),
            artifacts: Vec::new(),
        }
    }

    fn push(&mut self, name: impl Into<String>, ok: bool, detail: impl Into<String>) {
        self.checks.push(IntegrityCheck {
            name: name.into(),
            ok,
            detail: detail.into(),
        });
    }

    fn finish(mut self) -> Self {
        self.ok = !self.checks.is_empty()
            && self.checks.iter().all(|check| check.ok)
            && !self.artifacts.is_empty()
            && self.artifacts.iter().all(|artifact| artifact.ok);
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReleaseInfo {
    /// Without the leading `v`, e.g. `"1.13.0"`.
    pub version: String,
    pub tag: String,
    pub html_url: String,
    /// Plain GitHub release notes. UI callers may present a shortened form,
    /// but keeping the source in the native response avoids a second WebView
    /// request that can be blocked by CORS or app transport policy.
    pub notes: String,
    /// Preferred direct download URL for the current platform's app asset.
    ///
    /// macOS prefers the notarized DMG and falls back to the universal archive.
    /// Windows selects the native x64 ZIP.
    pub asset_url: Option<String>,
    pub asset_name: Option<String>,
    /// Normalized lowercase SHA-256 digest for the selected update asset.
    pub asset_digest: Option<String>,
    pub archive_url: Option<String>,
    pub dmg_url: Option<String>,
    pub windows_url: Option<String>,
    /// Direct download URL for `checksums.txt`, used to verify the selected
    /// update asset before extraction.
    pub checksum_url: Option<String>,
    pub checksum_name: Option<String>,
    pub checksum_digest: Option<String>,
}

/// Return true only when the release exposes every artifact needed by the
/// verified in-app updater. Keeping this check beside the installer prevents
/// callers from presenting an install action that is guaranteed to fail.
pub fn is_installable_release(release: &ReleaseInfo) -> bool {
    let common_ready = release.asset_url.is_some()
        && release.asset_name.is_some()
        && release.asset_digest.is_some()
        && release.checksum_url.is_some()
        && release.checksum_digest.is_some();

    #[cfg(target_os = "windows")]
    {
        common_ready
            && release
                .asset_name
                .as_deref()
                .is_some_and(is_windows_archive)
    }
    #[cfg(not(target_os = "windows"))]
    {
        common_ready
    }
}

/// Query the GitHub API for the latest release. `Err` covers network
/// failure, missing `curl`, and unexpected response shapes alike — callers
/// treat "couldn't check" and "nothing to report" the same way.
pub fn fetch_latest_release() -> Result<ReleaseInfo, String> {
    fetch_release_api(&format!(
        "https://api.github.com/repos/{REPO}/releases/latest"
    ))
}

/// Fetch the release selected by the user's update-channel preference.
///
/// GitHub's `/releases/latest` endpoint intentionally excludes prereleases,
/// so the preview channel uses the ordered releases endpoint and ignores
/// drafts while retaining the newest published release.
pub fn fetch_release_for_channel(channel: UpdateChannel) -> Result<ReleaseInfo, String> {
    match channel {
        UpdateChannel::Stable => fetch_latest_release(),
        UpdateChannel::PreRelease => {
            let body = fetch_release_body(&format!(
                "https://api.github.com/repos/{REPO}/releases?per_page=20"
            ))?;
            parse_release_list_response(&body)
        }
    }
}

pub fn fetch_release_by_tag(tag: &str) -> Result<ReleaseInfo, String> {
    let tag = if tag.starts_with('v') {
        tag.to_string()
    } else {
        format!("v{tag}")
    };
    fetch_release_api(&format!(
        "https://api.github.com/repos/{REPO}/releases/tags/{tag}"
    ))
}

fn fetch_release_api(url: &str) -> Result<ReleaseInfo, String> {
    let body = fetch_release_body(url)?;
    parse_release_response(&body)
}

fn fetch_release_body(url: &str) -> Result<Vec<u8>, String> {
    let mut command = std::process::Command::new("curl");
    command.args([
        "-sS",
        "--fail-with-body",
        "--max-time",
        "8",
        "-H",
        "Accept: application/vnd.github+json",
        "-H",
        "User-Agent: peterfan-updater",
    ]);
    if let Some(token) = github_api_token() {
        command.args(["-H", &format!("Authorization: Bearer {token}")]);
    }
    let out = command
        .arg(url)
        .output()
        .map_err(|e| format!("curl not available: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "curl exited with {:?}: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(out.stdout)
}

fn github_api_token() -> Option<String> {
    ["GITHUB_TOKEN", "GH_TOKEN"].iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty())
    })
}

fn parse_release_response(body: &[u8]) -> Result<ReleaseInfo, String> {
    let val: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| format!("unexpected GitHub response: {e}"))?;
    parse_release_value(&val)
}

fn parse_release_list_response(body: &[u8]) -> Result<ReleaseInfo, String> {
    let val: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| format!("unexpected GitHub response: {e}"))?;
    let releases = val.as_array().ok_or("response has no release list")?;
    let release = releases
        .iter()
        .find(|release| !release["draft"].as_bool().unwrap_or(false))
        .ok_or_else(|| "release list has no published releases".to_string())?;
    parse_release_value(release)
}

fn parse_release_value(val: &serde_json::Value) -> Result<ReleaseInfo, String> {
    let tag = val["tag_name"]
        .as_str()
        .ok_or("response has no tag_name")?
        .to_string();
    let version = tag.strip_prefix('v').unwrap_or(&tag).to_string();
    if parse_update_version(&tag).is_none() {
        return Err("release tag has an invalid version".into());
    }
    let html_url = val["html_url"].as_str().unwrap_or_default().to_string();
    let notes = val["body"].as_str().unwrap_or_default().to_string();
    let empty_assets = Vec::new();
    let assets = val["assets"].as_array().unwrap_or(&empty_assets);
    let dmg = find_asset(assets, is_macos_dmg);
    let archive = find_asset(assets, is_preferred_macos_archive)
        .or_else(|| find_asset(assets, is_macos_archive));
    let windows = find_asset(assets, is_windows_archive);
    let checksums = find_asset(assets, is_checksum_asset);
    let preferred = if cfg!(target_os = "windows") {
        windows.as_ref()
    } else {
        dmg.as_ref().or(archive.as_ref())
    };
    Ok(ReleaseInfo {
        version,
        tag,
        html_url,
        notes,
        asset_url: preferred.map(|a| a.url.clone()),
        asset_name: preferred.map(|a| a.name.clone()),
        asset_digest: preferred.and_then(|a| a.digest.clone()),
        archive_url: archive.map(|a| a.url),
        dmg_url: dmg.map(|a| a.url),
        windows_url: windows.map(|a| a.url),
        checksum_url: checksums.as_ref().map(|a| a.url.clone()),
        checksum_name: checksums.as_ref().map(|a| a.name.clone()),
        checksum_digest: checksums.and_then(|a| a.digest),
    })
}

#[derive(Debug, Clone)]
struct Asset {
    name: String,
    url: String,
    digest: Option<String>,
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
                digest: parse_github_sha256_digest(asset["digest"].as_str()),
            })
        } else {
            None
        }
    })
}

fn parse_github_sha256_digest(digest: Option<&str>) -> Option<String> {
    normalize_sha256_digest(digest?)
}

fn normalize_sha256_digest(digest: &str) -> Option<String> {
    let hash = digest.strip_prefix("sha256:").unwrap_or(digest);
    (hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| hash.to_ascii_lowercase())
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

fn is_windows_archive(name: &str) -> bool {
    name.starts_with("peterfan-")
        && name.contains("x86_64-pc-windows-msvc")
        && name.ends_with(".zip")
}

fn is_checksum_asset(name: &str) -> bool {
    name == "checksums.txt"
}

/// Compare SemVer precedence, ignoring build metadata. A single lowercase `v`
/// prefix is accepted and missing minor/patch components are padded with zero.
/// Invalid versions on either side fail closed (no update is offered).
pub fn is_newer(current: &str, latest: &str) -> bool {
    match (parse_update_version(current), parse_update_version(latest)) {
        (Some(current), Some(latest)) => latest.cmp_precedence(&current).is_gt(),
        _ => false,
    }
}

fn parse_update_version(version: &str) -> Option<semver::Version> {
    let version = version.strip_prefix('v').unwrap_or(version);
    let core_end = version.find(['-', '+']).unwrap_or(version.len());
    let (core, suffix) = version.split_at(core_end);
    let mut normalized = core.to_string();
    match core.split('.').count() {
        1 => normalized.push_str(".0.0"),
        2 => normalized.push_str(".0"),
        3 => {}
        _ => return None,
    }
    normalized.push_str(suffix);
    semver::Version::parse(&normalized).ok()
}

#[cfg(target_os = "macos")]
pub fn default_installed_app_bundle() -> std::path::PathBuf {
    std::path::PathBuf::from("/Applications/PeterFan.app")
}

#[cfg(target_os = "macos")]
pub fn default_integrity_app_bundle() -> std::path::PathBuf {
    current_app_bundle()
        .ok()
        .filter(|path| path.exists())
        .unwrap_or_else(default_installed_app_bundle)
}

#[cfg(target_os = "macos")]
pub fn verify_app_integrity(app: &std::path::Path) -> AppIntegrityReport {
    let mut report = AppIntegrityReport::new(app);

    if !app.is_dir() {
        report.push("app bundle exists", false, "PeterFan.app was not found");
        return report.finish();
    }
    report.push("app bundle exists", true, "found PeterFan.app");

    report.bundle_id = plist_value(app, "CFBundleIdentifier");
    report.version = plist_value(app, "CFBundleShortVersionString");
    let bundle_ok = report.bundle_id.as_deref() == Some(EXPECTED_BUNDLE_ID);
    report.push(
        "bundle identifier",
        bundle_ok,
        report
            .bundle_id
            .as_deref()
            .map(|id| format!("{id} (expected {EXPECTED_BUNDLE_ID})"))
            .unwrap_or_else(|| format!("missing (expected {EXPECTED_BUNDLE_ID})")),
    );

    let helper = app.join("Contents/MacOS/peterfand");
    report.push(
        "bundled fan-control helper",
        helper.is_file() && is_executable(&helper),
        helper.display().to_string(),
    );

    let signature_ok = silent_status(
        std::process::Command::new("codesign")
            .args(["--verify", "--deep", "--strict", "--verbose=2"])
            .arg(app),
    );
    report.push(
        "code signature",
        signature_ok,
        if signature_ok {
            "codesign --verify --deep --strict passed"
        } else {
            "codesign verification failed"
        },
    );

    match codesign_details(app) {
        Ok(text) => {
            report.team_id = codesign_field(&text, "TeamIdentifier");
            let team_ok = report.team_id.as_deref() == Some(EXPECTED_TEAM_ID);
            report.push(
                "Developer ID team",
                team_ok && has_developer_id_authority(&text),
                report
                    .team_id
                    .as_deref()
                    .map(|team| format!("{team} (expected {EXPECTED_TEAM_ID})"))
                    .unwrap_or_else(|| format!("missing (expected {EXPECTED_TEAM_ID})")),
            );
            let signed_identifier = codesign_field(&text, "Identifier");
            report.push(
                "signed identifier",
                signed_identifier.as_deref() == Some(EXPECTED_BUNDLE_ID),
                signed_identifier
                    .map(|id| format!("{id} (expected {EXPECTED_BUNDLE_ID})"))
                    .unwrap_or_else(|| format!("missing (expected {EXPECTED_BUNDLE_ID})")),
            );
        }
        Err(err) => {
            report.push("Developer ID team", false, err.clone());
            report.push("signed identifier", false, err);
        }
    }

    let notarized = silent_status(
        std::process::Command::new("xcrun")
            .args(["stapler", "validate"])
            .arg(app),
    );
    report.push(
        "notarization ticket",
        notarized,
        if notarized {
            "stapled ticket is valid"
        } else {
            "stapler validation failed"
        },
    );

    match std::process::Command::new("spctl")
        .args(["-a", "-vv", "-t", "exec"])
        .arg(app)
        .output()
    {
        Ok(output) if output.status.success() => {
            report.push("Gatekeeper", true, "accepted by spctl");
        }
        Ok(output) if spctl_service_unavailable(&output) => {
            report.push(
                "Gatekeeper",
                true,
                "spctl unavailable system-wide; signature and notarization checks passed",
            );
        }
        Ok(output) => {
            let detail = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            report.push("Gatekeeper", false, detail.trim());
        }
        Err(err) => report.push("Gatekeeper", false, format!("spctl failed: {err}")),
    }

    report.finish()
}

#[cfg(not(target_os = "macos"))]
pub fn verify_app_integrity(app: &std::path::Path) -> AppIntegrityReport {
    let mut report = AppIntegrityReport::new(app);
    report.push(
        "macOS app integrity",
        false,
        "PeterFan.app integrity checks are only available on macOS",
    );
    report.finish()
}

#[cfg(target_os = "macos")]
pub fn verify_release_integrity(release: &ReleaseInfo) -> ReleaseIntegrityReport {
    let mut report = ReleaseIntegrityReport::new(release);
    let Some(asset_url) = release.asset_url.as_deref() else {
        report.push("release asset", false, "release has no macOS asset");
        return report.finish();
    };
    let Some(asset_name) = release.asset_name.as_deref() else {
        report.push("release asset", false, "release asset has no name");
        return report.finish();
    };
    let Some(checksum_url) = release.checksum_url.as_deref() else {
        report.push("checksums.txt", false, "release has no checksums.txt");
        return report.finish();
    };

    report.push("release asset", true, asset_name);
    let tmp_dir =
        std::env::temp_dir().join(format!("peterfan-release-integrity-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    if let Err(err) = std::fs::create_dir_all(&tmp_dir) {
        report.push(
            "workspace",
            false,
            format!("could not create temp dir: {err}"),
        );
        return report.finish();
    }

    let asset_path = tmp_dir.join(asset_name);
    match download_file(asset_url, &asset_path) {
        Ok(()) => report.push("download asset", true, asset_url),
        Err(err) => {
            report.push("download asset", false, err);
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return report.finish();
        }
    }

    let actual_sha = sha256_file(&asset_path).ok();
    report.asset_sha256 = actual_sha.clone();
    if let Some(expected) = release.asset_digest.as_deref() {
        let ok = actual_sha
            .as_deref()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(expected));
        report.push(
            "GitHub asset digest",
            ok,
            actual_sha
                .as_deref()
                .map(|actual| format!("expected {expected}, got {actual}"))
                .unwrap_or_else(|| format!("expected {expected}, but SHA-256 failed")),
        );
    } else {
        report.push(
            "GitHub asset digest",
            true,
            "not provided by GitHub; checksums.txt will be used",
        );
    }

    let checksums_path = tmp_dir.join("checksums.txt");
    match download_file(checksum_url, &checksums_path) {
        Ok(()) => report.push("download checksums.txt", true, checksum_url),
        Err(err) => {
            report.push("download checksums.txt", false, err);
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return report.finish();
        }
    }
    if let Some(expected) = release.checksum_digest.as_deref() {
        match verify_expected_sha256("GitHub checksums.txt digest", expected, &checksums_path) {
            Ok(()) => report.push("GitHub checksums.txt digest", true, expected),
            Err(err) => report.push("GitHub checksums.txt digest", false, err),
        }
    } else {
        report.push(
            "GitHub checksums.txt digest",
            true,
            "not provided by GitHub; file content will still be parsed",
        );
    }

    let checksums = std::fs::read_to_string(&checksums_path).unwrap_or_default();
    match verify_download_checksum(&checksums, asset_name, &asset_path) {
        Ok(()) => report.push("checksums.txt asset hash", true, asset_name),
        Err(err) => report.push("checksums.txt asset hash", false, err),
    }

    let app_path = if asset_name.ends_with(".dmg") {
        match validate_update_dmg(&asset_path) {
            Ok(()) => report.push(
                "DMG trust policy",
                true,
                "signature, notarization, Gatekeeper",
            ),
            Err(err) => report.push("DMG trust policy", false, err),
        }
        match extract_app_from_dmg(&asset_path, &tmp_dir) {
            Ok(path) => Some(path),
            Err(err) => {
                report.push("extract PeterFan.app", false, err);
                None
            }
        }
    } else {
        report.push("DMG trust policy", true, "not a DMG asset; skipped");
        match extract_app_from_archive(&asset_path, &tmp_dir) {
            Ok(path) => Some(path),
            Err(err) => {
                report.push("extract PeterFan.app", false, err);
                None
            }
        }
    };

    if let Some(app_path) = app_path {
        report.push("extract PeterFan.app", true, app_path.display().to_string());
        let app_report = verify_app_integrity(&app_path);
        report.app = Some(app_report);
    }

    let _ = std::fs::remove_dir_all(&tmp_dir);
    report.finish()
}

#[cfg(not(target_os = "macos"))]
pub fn verify_release_integrity(release: &ReleaseInfo) -> ReleaseIntegrityReport {
    let mut report = ReleaseIntegrityReport::new(release);
    report.push(
        "macOS release integrity",
        false,
        "release artifact integrity checks are only available on macOS",
    );
    report.finish()
}

#[cfg(target_os = "macos")]
pub fn verify_local_artifact_integrity(
    asset: &std::path::Path,
    checksums: Option<&std::path::Path>,
    expected_sha256: Option<&str>,
) -> ArtifactIntegrityReport {
    let mut report = ArtifactIntegrityReport::new(asset);
    let asset_name = report.asset_name.clone();

    if !asset.is_file() {
        report.push("artifact exists", false, "release artifact was not found");
        return report.finish();
    }
    report.push("artifact exists", true, asset.display().to_string());

    let actual_sha = sha256_file(asset).ok();
    report.asset_sha256 = actual_sha.clone();
    if let Some(expected) = expected_sha256 {
        let ok = actual_sha
            .as_deref()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(expected));
        report.push(
            "expected SHA-256",
            ok,
            actual_sha
                .as_deref()
                .map(|actual| format!("expected {expected}, got {actual}"))
                .unwrap_or_else(|| format!("expected {expected}, but SHA-256 failed")),
        );
    } else if let Some(actual) = actual_sha.as_deref() {
        report.push("SHA-256", true, actual);
    } else {
        report.push("SHA-256", false, "could not compute SHA-256");
    }

    if let Some(checksums_path) = checksums {
        if !checksums_path.is_file() {
            report.push("checksums.txt", false, "checksums.txt was not found");
        } else {
            match std::fs::read_to_string(checksums_path) {
                Ok(text) => match verify_download_checksum(&text, &asset_name, asset) {
                    Ok(()) => report.push("checksums.txt asset hash", true, &asset_name),
                    Err(err) => report.push("checksums.txt asset hash", false, err),
                },
                Err(err) => report.push("checksums.txt", false, err.to_string()),
            }
        }
    }

    let tmp_dir = std::env::temp_dir().join(format!(
        "peterfan-artifact-integrity-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    if let Err(err) = std::fs::create_dir_all(&tmp_dir) {
        report.push(
            "workspace",
            false,
            format!("could not create temp dir: {err}"),
        );
        return report.finish();
    }

    let app_path = if asset_name.ends_with(".dmg") {
        match validate_update_dmg(asset) {
            Ok(()) => report.push(
                "DMG trust policy",
                true,
                "signature, notarization, Gatekeeper",
            ),
            Err(err) => report.push("DMG trust policy", false, err),
        }
        match extract_app_from_dmg(asset, &tmp_dir) {
            Ok(path) => Some(path),
            Err(err) => {
                report.push("extract PeterFan.app", false, err);
                None
            }
        }
    } else if asset_name.ends_with(".tar.gz") {
        report.push("DMG trust policy", true, "not a DMG asset; skipped");
        match extract_app_from_archive(asset, &tmp_dir) {
            Ok(path) => Some(path),
            Err(err) => {
                report.push("extract PeterFan.app", false, err);
                None
            }
        }
    } else {
        report.push(
            "artifact format",
            false,
            "expected PeterFan .dmg or macOS .tar.gz release artifact",
        );
        None
    };

    if let Some(app_path) = app_path {
        report.push("extract PeterFan.app", true, app_path.display().to_string());
        report.app = Some(verify_app_integrity(&app_path));
    }

    let _ = std::fs::remove_dir_all(&tmp_dir);
    report.finish()
}

#[cfg(not(target_os = "macos"))]
pub fn verify_local_artifact_integrity(
    asset: &std::path::Path,
    _checksums: Option<&std::path::Path>,
    _expected_sha256: Option<&str>,
) -> ArtifactIntegrityReport {
    let mut report = ArtifactIntegrityReport::new(asset);
    report.push(
        "macOS artifact integrity",
        false,
        "release artifact integrity checks are only available on macOS",
    );
    report.finish()
}

#[cfg(target_os = "macos")]
pub fn verify_release_directory_integrity(
    dir: &std::path::Path,
) -> ReleaseDirectoryIntegrityReport {
    let mut report = ReleaseDirectoryIntegrityReport::new(dir);

    if !dir.is_dir() {
        report.push(
            "release directory",
            false,
            "release directory was not found",
        );
        return report.finish();
    }
    report.push("release directory", true, dir.display().to_string());

    let checksums_path = dir.join("checksums.txt");
    let checksums = if checksums_path.is_file() {
        report.checksums = Some(checksums_path.display().to_string());
        report.push("checksums.txt", true, checksums_path.display().to_string());
        Some(checksums_path)
    } else {
        report.push("checksums.txt", false, "checksums.txt was not found");
        None
    };

    let mut artifacts = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(is_release_artifact_name)
            })
            .collect::<Vec<_>>(),
        Err(err) => {
            report.push("release artifacts", false, err.to_string());
            return report.finish();
        }
    };
    artifacts.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

    let artifact_names = artifacts
        .iter()
        .filter_map(|path| path.file_name().and_then(|name| name.to_str()))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let inferred_version = infer_release_directory_version(dir, &artifact_names);
    report.expected_version = inferred_version.clone();
    match inferred_version.as_deref() {
        Some(version) => report.push("expected version", true, format!("v{version}")),
        None => report.push(
            "expected version",
            false,
            "could not infer one release version from directory/artifact names",
        ),
    }

    if artifacts.is_empty() {
        report.push(
            "release artifacts",
            false,
            "no PeterFan release artifacts found",
        );
        return report.finish();
    }
    report.push("release artifacts", true, artifact_names.join(", "));

    let dmg_count = artifact_names
        .iter()
        .filter(|name| name.starts_with("PeterFan-") && name.ends_with(".dmg"))
        .count();
    report.push(
        "DMG artifact",
        dmg_count == 1,
        format!("found {dmg_count} PeterFan DMG artifact(s)"),
    );
    let archive_count = artifact_names
        .iter()
        .filter(|name| {
            name.starts_with("peterfan-")
                && name.contains("universal-apple-darwin")
                && name.ends_with(".tar.gz")
        })
        .count();
    report.push(
        "universal macOS archive",
        archive_count == 1,
        format!("found {archive_count} universal apple-darwin archive(s)"),
    );

    if let Some(checksums_path) = checksums.as_deref() {
        match std::fs::read_to_string(checksums_path) {
            Ok(text) => {
                let mut listed = checksum_release_artifact_names(&text);
                let mut expected = artifact_names.clone();
                listed.sort();
                expected.sort();
                report.push(
                    "checksums manifest coverage",
                    listed == expected,
                    format!(
                        "listed [{}], found [{}]",
                        listed.join(", "),
                        expected.join(", ")
                    ),
                );
            }
            Err(err) => report.push("checksums manifest coverage", false, err.to_string()),
        }
    }

    for artifact in artifacts {
        report.artifacts.push(verify_local_artifact_integrity(
            &artifact,
            checksums.as_deref(),
            None,
        ));
    }

    if let Some(expected) = inferred_version.as_deref() {
        let mismatches = report
            .artifacts
            .iter()
            .filter_map(|artifact| {
                let name_version = release_artifact_version(&artifact.asset_name);
                (name_version.as_deref() != Some(expected))
                    .then(|| format!("{} -> {:?}", artifact.asset_name, name_version))
            })
            .collect::<Vec<_>>();
        report.push(
            "artifact filename versions",
            mismatches.is_empty(),
            if mismatches.is_empty() {
                format!("all artifacts use v{expected}")
            } else {
                mismatches.join(", ")
            },
        );

        let app_mismatches = report
            .artifacts
            .iter()
            .filter_map(|artifact| {
                let app_version = artifact.app.as_ref().and_then(|app| app.version.as_deref());
                (app_version != Some(expected))
                    .then(|| format!("{} app -> {:?}", artifact.asset_name, app_version))
            })
            .collect::<Vec<_>>();
        report.push(
            "app bundle versions",
            app_mismatches.is_empty(),
            if app_mismatches.is_empty() {
                format!("all embedded apps report v{expected}")
            } else {
                app_mismatches.join(", ")
            },
        );
    }

    report.finish()
}

#[cfg(not(target_os = "macos"))]
pub fn verify_release_directory_integrity(
    dir: &std::path::Path,
) -> ReleaseDirectoryIntegrityReport {
    let mut report = ReleaseDirectoryIntegrityReport::new(dir);
    report.push(
        "macOS release directory integrity",
        false,
        "release directory integrity checks are only available on macOS",
    );
    report.finish()
}

#[cfg(any(target_os = "macos", test))]
fn is_release_artifact_name(name: &str) -> bool {
    (name.starts_with("PeterFan-") && name.ends_with(".dmg"))
        || (name.starts_with("peterfan-")
            && name.contains("apple-darwin")
            && name.ends_with(".tar.gz"))
}

#[cfg(any(target_os = "macos", test))]
fn infer_release_directory_version(
    dir: &std::path::Path,
    artifact_names: &[String],
) -> Option<String> {
    let dir_version = dir
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix('v'))
        .filter(|version| is_semverish(version))
        .map(ToString::to_string);
    if dir_version.is_some() {
        return dir_version;
    }

    let mut versions = artifact_names
        .iter()
        .filter_map(|name| release_artifact_version(name))
        .collect::<Vec<_>>();
    versions.sort();
    versions.dedup();
    (versions.len() == 1).then(|| versions.remove(0))
}

#[cfg(any(target_os = "macos", test))]
fn release_artifact_version(name: &str) -> Option<String> {
    if let Some(rest) = name
        .strip_prefix("PeterFan-v")
        .and_then(|rest| rest.strip_suffix(".dmg"))
    {
        return is_semverish(rest).then(|| rest.to_string());
    }
    let rest = name.strip_prefix("peterfan-v")?;
    let version = rest.split('-').next()?;
    is_semverish(version).then(|| version.to_string())
}

#[cfg(any(target_os = "macos", test))]
fn checksum_release_artifact_names(checksums: &str) -> Vec<String> {
    checksums
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            let listed = parts.next()?.trim_start_matches('*');
            let name = std::path::Path::new(listed)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(listed);
            is_release_artifact_name(name).then(|| name.to_string())
        })
        .collect()
}

#[cfg(any(target_os = "macos", test))]
fn is_semverish(version: &str) -> bool {
    let parts = version.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// Locate the `.app` bundle containing the currently running executable
/// (`.../PeterFan.app/Contents/MacOS/PeterFan` → `.../PeterFan.app`).
#[cfg(target_os = "macos")]
pub fn current_app_bundle() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    app_bundle_containing_executable(&exe).ok_or_else(|| {
        format!(
            "not running from inside a .app bundle (executable: {})",
            exe.display()
        )
    })
}

#[cfg(target_os = "macos")]
fn app_bundle_containing_executable(exe: &std::path::Path) -> Option<std::path::PathBuf> {
    exe.ancestors()
        .find(|path| path.extension().and_then(|extension| extension.to_str()) == Some("app"))
        .map(std::path::Path::to_path_buf)
}

#[cfg(target_os = "macos")]
fn resolve_install_target<F>(
    current_exe: &std::path::Path,
    installed_app: &std::path::Path,
    verify_installed: F,
) -> Result<std::path::PathBuf, String>
where
    F: FnOnce(&std::path::Path) -> Result<(), String>,
{
    if let Some(app) = app_bundle_containing_executable(current_exe) {
        return Ok(app);
    }
    verify_installed(installed_app)?;
    Ok(installed_app.to_path_buf())
}

#[cfg(target_os = "macos")]
fn install_target_app_bundle() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let installed = default_installed_app_bundle();
    resolve_install_target(&exe, &installed, |app| {
        let report = verify_app_integrity(app);
        if report.ok {
            return Ok(());
        }
        let failures = report
            .checks
            .iter()
            .filter(|check| !check.ok)
            .take(3)
            .map(|check| format!("{}: {}", check.name, check.detail))
            .collect::<Vec<_>>()
            .join("; ");
        Err(format!(
            "standalone updater refused untrusted installed app at {}{}",
            app.display(),
            if failures.is_empty() {
                String::new()
            } else {
                format!(" ({failures})")
            }
        ))
    })
}

/// Download `asset_url`, extract it, and write a detached helper script that
/// (after this process quits) replaces the running `.app` bundle and
/// relaunches it. Returns once the script is queued — the caller should quit
/// shortly after (see module docs on the menu-bar side for the confirm-first
/// flow this is meant to sit behind).
#[cfg(target_os = "macos")]
pub fn download_and_install_release(release: &ReleaseInfo) -> Result<(), String> {
    let asset_url = release
        .asset_url
        .as_deref()
        .ok_or("release has no macOS app update asset")?;
    let asset_name = release
        .asset_name
        .as_deref()
        .ok_or("release asset is missing a name")?;
    let checksum_url = release
        .checksum_url
        .as_deref()
        .ok_or("release has no checksums.txt; refusing OTA install")?;
    download_and_install_verified(
        asset_url,
        asset_name,
        &release.version,
        release.asset_digest.as_deref(),
        checksum_url,
        release.checksum_digest.as_deref(),
    )
}

#[cfg(target_os = "windows")]
pub fn download_and_install_release(release: &ReleaseInfo) -> Result<(), String> {
    let asset_url = release
        .asset_url
        .as_deref()
        .ok_or("release has no Windows x64 update asset")?;
    let asset_name = release
        .asset_name
        .as_deref()
        .filter(|name| is_windows_archive(name))
        .ok_or("selected update asset is not a Windows x64 ZIP")?;
    let asset_digest = release
        .asset_digest
        .as_deref()
        .ok_or("Windows release asset has no GitHub SHA-256 digest")?;
    let checksum_url = release
        .checksum_url
        .as_deref()
        .ok_or("release has no checksums.txt; refusing OTA install")?;
    let checksum_digest = release
        .checksum_digest
        .as_deref()
        .ok_or("checksums.txt has no GitHub SHA-256 digest")?;

    // Resolve the running app before downloading or recording a pending update.
    // A custom/portable installation must never fall back to LOCALAPPDATA.
    let executable = std::env::current_exe()
        .map_err(|error| format!("could not locate the running Windows app: {error}"))?;
    let install_dir = windows_install_directory(&executable)?;

    let tmp_dir = std::env::temp_dir().join(format!(
        "peterfan-update-{}-{}",
        std::process::id(),
        UpdateInstallResult::new("pending", &release.version, "").updated_at_unix
    ));
    let package_dir = tmp_dir.join("package");
    std::fs::create_dir_all(&package_dir).map_err(|e| e.to_string())?;
    let archive = tmp_dir.join(asset_name);
    let checksums_path = tmp_dir.join("checksums.txt");

    download_file(asset_url, &archive)?;
    verify_expected_sha256("GitHub asset digest", asset_digest, &archive)?;
    download_file(checksum_url, &checksums_path)?;
    verify_expected_sha256(
        "GitHub checksums.txt digest",
        checksum_digest,
        &checksums_path,
    )?;
    let checksums = std::fs::read_to_string(&checksums_path).map_err(|e| e.to_string())?;
    verify_download_checksum(&checksums, asset_name, &archive)?;

    let command = format!(
        "Expand-Archive -LiteralPath {} -DestinationPath {} -Force",
        powershell_quote(&archive),
        powershell_quote(&package_dir)
    );
    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &command,
        ])
        .output()
        .map_err(|e| format!("could not extract Windows update: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "could not extract Windows update: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let installer = find_file_named(&package_dir, "install.ps1")
        .ok_or("downloaded Windows ZIP did not contain install.ps1")?;
    let payload_dir = installer
        .parent()
        .ok_or("Windows installer has no payload directory")?;
    for required in [
        "PeterFan.exe",
        "peterfan-cli.exe",
        "peterfan-tui.exe",
        "uninstall.ps1",
    ] {
        if !payload_dir.join(required).is_file() {
            return Err(format!("downloaded Windows ZIP is missing {required}"));
        }
    }
    verify_windows_payload_version(
        &payload_dir.join("peterfan-cli.exe"),
        &release.version,
        &tmp_dir.join("payload-version.txt"),
        std::time::Duration::from_secs(10),
    )?;
    // Freeze the verified payload's owned files. The detached helper copies
    // and rechecks each digest before touching the existing installation.
    let mut files = std::collections::BTreeMap::new();
    for name in WINDOWS_UPDATE_FILES {
        let path = payload_dir.join(name);
        if path.is_file() {
            files.insert(name.to_string(), sha256_file(&path)?);
        }
    }
    let nonce = format!(
        "{:032x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            ^ u128::from(std::process::id())
    );

    let result_path =
        update_install_result_path().ok_or("could not determine update result path")?;
    let pending = UpdateInstallResult::new(
        "pending",
        &release.version,
        format!("PeterFan v{} is verified and queued.", release.version),
    );
    let script_path = tmp_dir.join("apply-update.ps1");
    let script = build_windows_apply_update_script(
        payload_dir,
        &install_dir,
        &tmp_dir,
        &result_path,
        &release.version,
        &nonce,
        &files,
    );
    prepare_update_helper(
        &script_path,
        format!("\u{feff}{script}").as_bytes(),
        &result_path,
        &pending,
    )?;
    if let Err(error) = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script_path)
        .spawn()
    {
        let failed = UpdateInstallResult::new(
            "failed",
            &release.version,
            format!("Could not launch the Windows updater: {error}"),
        );
        let _ = write_update_install_result_to(&result_path, &failed);
        return Err(format!("could not launch Windows updater: {error}"));
    }
    Ok(())
}

#[cfg(any(target_os = "windows", test))]
fn windows_install_directory(executable: &std::path::Path) -> Result<std::path::PathBuf, String> {
    if !executable.is_absolute()
        || executable.to_str().is_none()
        || !executable
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.eq_ignore_ascii_case("PeterFan.exe")
                    || name.eq_ignore_ascii_case("peterfan-cli.exe")
            })
        || !executable.is_file()
    {
        return Err("could not determine a safe existing Windows app location".into());
    }
    let directory = executable
        .parent()
        .filter(|directory| directory.parent().is_some() && directory.is_dir())
        .ok_or("Windows app has no safe installation directory")?;
    if !directory.join("PeterFan.exe").is_file() {
        return Err("Windows installation is missing PeterFan.exe".into());
    }
    Ok(directory.to_path_buf())
}

#[cfg(any(target_os = "windows", test))]
const WINDOWS_UPDATE_FILES: [&str; 8] = [
    "PeterFan.exe",
    "peterfan-cli.exe",
    "peterfan-tui.exe",
    "uninstall.ps1",
    "README-WINDOWS.txt",
    "LICENSE",
    "CHANGELOG.md",
    "THIRD_PARTY_NOTICES.md",
];

#[cfg(target_os = "windows")]
fn verify_windows_payload_version(
    executable: &std::path::Path,
    expected_version: &str,
    output_path: &std::path::Path,
    timeout: std::time::Duration,
) -> Result<(), String> {
    // A redirected regular file avoids deadlocking on a full pipe or a child
    // that exits while another process retains its stdout handle.
    let output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)
        .map_err(|error| format!("could not prepare Windows version check: {error}"))?;
    let mut child = std::process::Command::new(executable)
        .arg("--version")
        .stdout(output)
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| format!("could not inspect downloaded Windows CLI: {error}"))?;
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("downloaded Windows CLI version check timed out".into());
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "could not wait for downloaded Windows CLI: {error}"
                ));
            }
        }
    };
    if !status.success()
        || std::fs::metadata(output_path)
            .map_err(|error| error.to_string())?
            .len()
            > 4096
    {
        return Err(format!(
            "downloaded Windows payload version does not match v{expected_version}"
        ));
    }
    let version = std::fs::read_to_string(output_path).map_err(|error| error.to_string())?;
    if !version
        .split_whitespace()
        .any(|part| part == expected_version)
    {
        return Err(format!(
            "downloaded Windows payload version does not match v{expected_version}"
        ));
    }
    Ok(())
}

#[cfg(any(target_os = "windows", test))]
fn build_windows_apply_update_script(
    payload_dir: &std::path::Path,
    install_dir: &std::path::Path,
    tmp_dir: &std::path::Path,
    result_path: &std::path::Path,
    target_version: &str,
    nonce: &str,
    files: &std::collections::BTreeMap<String, String>,
) -> String {
    let spec = serde_json::json!({
        "payload_dir": payload_dir,
        "install_dir": install_dir,
        "tmp_dir": tmp_dir,
        "result_path": result_path,
        "version": target_version,
        "nonce": nonce,
        "files": files,
    });
    format!(
        "$spec = {} | ConvertFrom-Json\n{}",
        powershell_quote_str(&spec.to_string()),
        include_str!("windows_update.ps1"),
    )
}

#[cfg(target_os = "windows")]
fn find_file_named(root: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.is_file() && path.file_name().and_then(|value| value.to_str()) == Some(name) {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_file_named(&path, name) {
                return Some(found);
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn powershell_quote(path: &std::path::Path) -> String {
    powershell_quote_str(&path.display().to_string())
}

#[cfg(any(target_os = "windows", test))]
fn powershell_quote_str(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(target_os = "macos")]
pub fn download_and_install(asset_url: &str) -> Result<(), String> {
    let asset_name = asset_url
        .split('?')
        .next()
        .unwrap_or(asset_url)
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("update.tar.gz");
    let target_version = release_artifact_version(asset_name).unwrap_or_else(|| "unknown".into());
    download_and_install_unchecked(asset_url, asset_name, &target_version)
}

#[cfg(target_os = "macos")]
fn download_and_install_verified(
    asset_url: &str,
    asset_name: &str,
    target_version: &str,
    asset_digest: Option<&str>,
    checksum_url: &str,
    checksum_digest: Option<&str>,
) -> Result<(), String> {
    let app_path = install_target_app_bundle()?;
    let tmp_dir = std::env::temp_dir().join(format!("peterfan-update-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;

    let download = tmp_dir.join(asset_name);
    download_file(asset_url, &download)?;
    if let Some(expected) = asset_digest {
        verify_expected_sha256("GitHub asset digest", expected, &download)?;
    }

    let checksums_path = tmp_dir.join("checksums.txt");
    download_file(checksum_url, &checksums_path)?;
    if let Some(expected) = checksum_digest {
        verify_expected_sha256("GitHub checksums.txt digest", expected, &checksums_path)?;
    }
    let checksums = std::fs::read_to_string(&checksums_path).map_err(|e| e.to_string())?;
    verify_download_checksum(&checksums, asset_name, &download)?;

    install_downloaded_update(&app_path, &tmp_dir, &download, asset_name, target_version)
}

#[cfg(target_os = "macos")]
fn download_and_install_unchecked(
    asset_url: &str,
    asset_name: &str,
    target_version: &str,
) -> Result<(), String> {
    let app_path = install_target_app_bundle()?;
    let tmp_dir = std::env::temp_dir().join(format!("peterfan-update-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;
    let download = tmp_dir.join(asset_name);
    download_file(asset_url, &download)?;
    install_downloaded_update(&app_path, &tmp_dir, &download, asset_name, target_version)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn download_file(url: &str, destination: &std::path::Path) -> Result<(), String> {
    let status = std::process::Command::new("curl")
        .args([
            "-fL",
            "-sS",
            "--show-error",
            "--max-time",
            "120",
            "-H",
            "User-Agent: peterfan-updater",
            "-o",
        ])
        .arg(destination)
        .arg(url)
        .status()
        .map_err(|e| e.to_string())?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("download failed: {url}"))
}

#[cfg(target_os = "macos")]
fn install_downloaded_update(
    app_path: &std::path::Path,
    tmp_dir: &std::path::Path,
    download: &std::path::Path,
    asset_name: &str,
    target_version: &str,
) -> Result<(), String> {
    let is_dmg = asset_name.ends_with(".dmg");
    if is_dmg {
        validate_update_dmg(download)?;
    }

    let new_app = if is_dmg {
        extract_app_from_dmg(download, tmp_dir)?
    } else {
        extract_app_from_archive(download, tmp_dir)?
    };
    validate_update_app(&new_app, target_version)?;

    // A detached script rather than doing the replace in-process: this
    // process's own executable is inside the bundle being replaced, and the
    // switch has to happen after it quits.
    let script_path = tmp_dir.join("apply-update.sh");
    let backup_path = tmp_dir.join("PreviousPeterFan.app");
    let log_path = tmp_dir.join("apply-update.log");
    let result_path = update_install_result_path().ok_or("could not locate Application Support")?;
    let pending = UpdateInstallResult::new(
        "pending",
        target_version,
        format!("Installing PeterFan v{target_version}."),
    );
    let updater_label = macos_updater_label(std::process::id(), target_version);
    let script = build_apply_update_script(
        app_path,
        &new_app,
        &backup_path,
        tmp_dir,
        &log_path,
        &result_path,
        target_version,
        &updater_label,
    );
    prepare_update_helper(&script_path, script.as_bytes(), &result_path, &pending)?;
    let _ = std::process::Command::new("chmod")
        .args(["+x"])
        .arg(&script_path)
        .status();

    let launched = std::process::Command::new("/bin/launchctl")
        .args(["submit", "-l", &updater_label, "--", "/bin/bash"])
        .arg(&script_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    if !matches!(launched, Ok(status) if status.success()) {
        let detail = match launched {
            Ok(status) => format!("launchctl exited with {status}"),
            Err(error) => error.to_string(),
        };
        let failed = UpdateInstallResult::new(
            "failed",
            target_version,
            format!("Could not launch the updater: {detail}"),
        );
        let _ = write_update_install_result_to(&result_path, &failed);
        return Err(format!("could not launch the updater script: {detail}"));
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_updater_label(pid: u32, target_version: &str) -> String {
    let version = target_version
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    format!("kr.co.uulab.peterfan.updater.{pid}.{version}")
}

#[cfg(target_os = "macos")]
#[allow(clippy::too_many_arguments)]
fn build_apply_update_script(
    app_path: &std::path::Path,
    new_app: &std::path::Path,
    backup_path: &std::path::Path,
    tmp_dir: &std::path::Path,
    log_path: &std::path::Path,
    result_path: &std::path::Path,
    target_version: &str,
    updater_label: &str,
) -> String {
    let executable = app_path.join("Contents/MacOS/PeterFan");
    let result_tmp = result_path.with_extension("json.tmp-updater");
    let nonce = format!(
        "{:032x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            ^ u128::from(std::process::id())
    );
    let health_path = crate::update_health::receipt_path(result_path, &nonce)
        .expect("valid updater nonce and receipt path");
    let receipt = |status, message| {
        shell_quote_str(
            &serde_json::to_string(&UpdateInstallResult::new(status, target_version, message))
                .expect("update result serializes"),
        )
    };
    format!(
        r#"#!/bin/bash
set -u
umask 077
exec >{log} 2>&1
if [ "${{1:-}}" != "--lock-held" ]; then
    [ ! -L {lock} ] && {{ [ ! -e {lock} ] || [ -f {lock} ]; }} || exit 1
    exec /usr/bin/lockf -k -t 0 {lock} /bin/bash "$0" --lock-held
fi
executable={executable}
health={health}
nonce={nonce}
version={version}
cleanup_job() {{
    /bin/rm -f "$health"
    /bin/launchctl remove {updater_label} >/dev/null 2>&1 || true
}}
trap cleanup_job EXIT
write_result() {{
    /usr/bin/printf '%s\n' "$1" > {result_tmp} && /bin/mv -f {result_tmp} {result}
}}
find_app_pids() {{
    while read -r pid command; do
        [ "$command" = "$executable" ] && /usr/bin/printf '%s\n' "$pid"
    done < <(/bin/ps -wwaxo pid=,comm=)
}}
wait_for_previous_exit() {{
    for ((i=0; i<40; i++)); do
        [ -z "$(find_app_pids)" ] && return 0
        sleep 0.25
    done
    return 1
}}
launch_previous() {{
    /usr/bin/open -g -j {app} || /usr/bin/open -g -j -n {app} || true
}}
health_matches() {{
    [ -f "$health" ] && [ ! -L "$health" ] || return 1
    [ "$(/usr/bin/plutil -extract nonce raw -o - "$health" 2>/dev/null)" = "$nonce" ] || return 1
    [ "$(/usr/bin/plutil -extract version raw -o - "$health" 2>/dev/null)" = "$version" ] || return 1
    health_pid=$(/usr/bin/plutil -extract pid raw -o - "$health" 2>/dev/null) || return 1
    case "$health_pid" in ''|*[!0-9]*) return 1 ;; esac
    [ "$(/bin/ps -p "$health_pid" -o comm=)" = "$executable" ] || return 1
    identity=$(/bin/ps -p "$health_pid" -o lstart=,comm=) || return 1
    sleep 2
    [ "$(/bin/ps -p "$health_pid" -o lstart=,comm=)" = "$identity" ]
}}
launch_until_healthy() {{
    /bin/rm -f "$health"
    /usr/bin/open -g -j {app} --args --update-health-token "$nonce" || return 1
    for ((i=0; i<45; i++)); do
        sleep 1
        health_matches && return 0
    done
    return 1
}}
fail_before_replace() {{
    write_result {failed}
    launch_previous
    exit 1
}}
rollback() {{
    for pid in $(find_app_pids); do
        if [ "$(/bin/ps -p "$pid" -o comm=)" = "$executable" ]; then
            /bin/kill -TERM "$pid" || true
        fi
    done
    if ! wait_for_previous_exit; then
        write_result {restore_failed}
        exit 1
    fi
    if ! /bin/rm -rf {app} || [ -e {app} ] || [ -L {app} ]; then
        write_result {restore_failed}
        exit 1
    fi
    if /bin/mv {backup} {app}; then
        write_result {rolled_back}
        launch_previous
    else
        write_result {restore_failed}
    fi
    exit 1
}}
sleep 0.5
wait_for_previous_exit || fail_before_replace
/bin/rm -rf {backup}
/bin/mv {app} {backup} || fail_before_replace
/usr/bin/ditto {new_app} {app} || rollback
/usr/bin/codesign --verify --deep --strict {app} || rollback
/usr/bin/xcrun stapler validate {app} >/dev/null 2>&1 || rollback
launch_until_healthy || rollback
write_result {installed} || rollback
/bin/rm -rf {backup}
/bin/rm -rf {tmp}
"#,
        app = shell_quote(app_path),
        new_app = shell_quote(new_app),
        backup = shell_quote(backup_path),
        tmp = shell_quote(tmp_dir),
        log = shell_quote(log_path),
        result = shell_quote(result_path),
        result_tmp = shell_quote(&result_tmp),
        executable = shell_quote(&executable),
        lock = shell_quote(&app_path.with_extension("app.update.lock")),
        health = shell_quote(&health_path),
        nonce = shell_quote_str(&nonce),
        version = shell_quote_str(target_version),
        updater_label = shell_quote_str(updater_label),
        installed = receipt(
            "installed",
            "PeterFan was installed and acknowledged UI readiness."
        ),
        rolled_back = receipt(
            "rolled_back",
            "The update failed and the previous PeterFan version was restored."
        ),
        failed = receipt(
            "failed",
            "The existing app could not be moved and was left unchanged."
        ),
        restore_failed = receipt(
            "failed",
            "Automatic rollback could not complete. The previous app backup was retained."
        ),
    )
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn verify_download_checksum(
    checksums: &str,
    asset_name: &str,
    path: &std::path::Path,
) -> Result<(), String> {
    let expected = checksum_for_asset(checksums, asset_name)
        .ok_or_else(|| format!("checksums.txt does not list {asset_name}"))?;
    verify_expected_sha256("checksums.txt", &expected, path)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn verify_expected_sha256(
    source: &str,
    expected: &str,
    path: &std::path::Path,
) -> Result<(), String> {
    let expected = normalize_sha256_digest(expected)
        .ok_or_else(|| format!("invalid SHA-256 digest from {source}: {expected}"))?;
    let actual = sha256_file(path)?;
    if actual.eq_ignore_ascii_case(&expected) {
        Ok(())
    } else {
        Err(format!(
            "checksum mismatch from {source}: expected {expected}, got {actual}"
        ))
    }
}

fn checksum_for_asset(checksums: &str, asset_name: &str) -> Option<String> {
    checksums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let listed = parts.next()?.trim_start_matches('*');
        let listed_name = std::path::Path::new(listed)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(listed);
        (listed_name == asset_name
            && hash.len() == 64
            && hash.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| hash.to_ascii_lowercase())
    })
}

#[cfg(target_os = "macos")]
fn sha256_file(path: &std::path::Path) -> Result<String, String> {
    let out = std::process::Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .map_err(|e| format!("shasum not available: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "shasum failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .split_whitespace()
        .next()
        .filter(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(|hash| hash.to_ascii_lowercase())
        .ok_or_else(|| "shasum did not print a SHA-256 digest".to_string())
}

#[cfg(target_os = "windows")]
fn sha256_file(path: &std::path::Path) -> Result<String, String> {
    let out = std::process::Command::new("certutil.exe")
        .args(["-hashfile"])
        .arg(path)
        .arg("SHA256")
        .output()
        .map_err(|e| format!("certutil is not available: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "certutil failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .find(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map(|value| value.to_ascii_lowercase())
        .ok_or_else(|| "certutil did not print a SHA-256 digest".to_string())
}

#[cfg(target_os = "macos")]
fn shell_quote(p: &std::path::Path) -> String {
    shell_quote_str(&p.display().to_string())
}

#[cfg(target_os = "macos")]
fn shell_quote_str(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
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
fn validate_update_dmg(dmg: &std::path::Path) -> Result<(), String> {
    if !silent_status(
        std::process::Command::new("codesign")
            .args(["--verify", "--verbose=2"])
            .arg(dmg),
    ) {
        return Err("downloaded update DMG has an invalid code signature".into());
    }

    if !silent_status(
        std::process::Command::new("xcrun")
            .args(["stapler", "validate"])
            .arg(dmg),
    ) {
        return Err("downloaded update DMG is not notarized/stapled".into());
    }

    let output = std::process::Command::new("spctl")
        .args([
            "-a",
            "-vv",
            "-t",
            "open",
            "--context",
            "context:primary-signature",
        ])
        .arg(dmg)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() && !spctl_service_unavailable(&output) {
        return Err("Gatekeeper rejected the downloaded update DMG".into());
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn validate_update_app(app: &std::path::Path, expected_version: &str) -> Result<(), String> {
    validate_update_app_contents(app, expected_version)?;
    if !silent_status(
        std::process::Command::new("codesign")
            .args(["--verify", "--deep", "--strict", "--verbose=2"])
            .arg(app),
    ) {
        return Err("downloaded PeterFan.app has an invalid code signature".into());
    }
    validate_update_app_identity(app)?;

    if !silent_status(
        std::process::Command::new("xcrun")
            .args(["stapler", "validate"])
            .arg(app),
    ) {
        return Err("downloaded PeterFan.app is not notarized/stapled".into());
    }

    let output = std::process::Command::new("spctl")
        .args(["-a", "-vv", "-t", "exec"])
        .arg(app)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() && !spctl_service_unavailable(&output) {
        return Err("Gatekeeper rejected the downloaded PeterFan.app".into());
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn validate_update_app_contents(
    app: &std::path::Path,
    expected_version: &str,
) -> Result<(), String> {
    // Bind the extracted payload to the selected release before queuing any
    // replacement. Both archive and DMG installs pass through this gate.
    if expected_version.is_empty() {
        return Err("downloaded PeterFan.app has no expected release version".into());
    }
    let version = plist_value(app, "CFBundleShortVersionString")
        .ok_or("downloaded PeterFan.app is missing a readable bundle version")?;
    if version != expected_version {
        return Err(format!(
            "downloaded PeterFan.app has version {version}, expected {expected_version}"
        ));
    }

    let helper = app.join("Contents/MacOS/peterfand");
    if !helper.is_file() || !is_executable(&helper) {
        return Err(
            "downloaded PeterFan.app is missing an executable bundled peterfand helper".into(),
        );
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn validate_update_app_identity(app: &std::path::Path) -> Result<(), String> {
    let text = codesign_details(app)?;
    if !has_developer_id_authority(&text) {
        return Err(
            "downloaded PeterFan.app is not signed by the expected Developer ID team".into(),
        );
    }
    if codesign_field(&text, "Identifier").as_deref() != Some(EXPECTED_BUNDLE_ID) {
        return Err("downloaded PeterFan.app has the wrong bundle identifier".into());
    }
    if codesign_field(&text, "TeamIdentifier").as_deref() != Some(EXPECTED_TEAM_ID) {
        return Err("downloaded PeterFan.app has the wrong Team ID".into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn codesign_details(app: &std::path::Path) -> Result<String, String> {
    let output = std::process::Command::new("codesign")
        .args(["-dv", "--verbose=4"])
        .arg(app)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("could not inspect PeterFan.app signature".into());
    }
    Ok(format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

#[cfg(any(target_os = "macos", test))]
fn codesign_field(text: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    text.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(&prefix).map(ToString::to_string))
}

#[cfg(any(target_os = "macos", test))]
fn has_developer_id_authority(text: &str) -> bool {
    text.lines().any(|line| {
        line.contains("Authority=Developer ID Application:") && line.contains(EXPECTED_TEAM_ID)
    })
}

#[cfg(target_os = "macos")]
fn silent_status(command: &mut std::process::Command) -> bool {
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn plist_value(app: &std::path::Path, key: &str) -> Option<String> {
    let plist = app.join("Contents/Info.plist");
    let out = std::process::Command::new("plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(plist)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(target_os = "macos")]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|meta| meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn spctl_service_unavailable(output: &std::process::Output) -> bool {
    let target = spctl_output_text(output);
    if spctl_text_is_resource_exhaustion(&target) {
        return true;
    }
    if !spctl_text_is_internal_assessment_failure(&target) {
        return false;
    }

    let probe = [
        "/System/Applications/Calculator.app",
        "/System/Library/CoreServices/Finder.app",
    ]
    .into_iter()
    .find(|path| std::path::Path::new(path).is_dir())
    .and_then(|path| {
        std::process::Command::new("spctl")
            .args(["-a", "-vv", "-t", "exec", path])
            .output()
            .ok()
    });
    let Some(probe) = probe else { return false };
    if probe.status.success() {
        return false;
    }
    let probe_text = spctl_output_text(&probe);
    spctl_internal_failure_is_system_wide(&target, &probe_text)
}

#[cfg(target_os = "macos")]
fn spctl_output_text(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[cfg(any(target_os = "macos", test))]
fn spctl_text_is_resource_exhaustion(text: &str) -> bool {
    text.contains("Too many open files")
}

#[cfg(any(target_os = "macos", test))]
fn spctl_text_is_internal_assessment_failure(text: &str) -> bool {
    text.contains("bundle format unrecognized, invalid, or unsuitable")
}

#[cfg(any(target_os = "macos", test))]
fn spctl_internal_failure_is_system_wide(target: &str, known_good_probe: &str) -> bool {
    spctl_text_is_internal_assessment_failure(target)
        && (spctl_text_is_resource_exhaustion(known_good_probe)
            || spctl_text_is_internal_assessment_failure(known_good_probe))
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
        assert!(!is_newer("1", "1.0"));
        assert!(is_newer("1.13-beta.1", "1.13"));
    }

    #[test]
    fn is_newer_handles_prerelease_upgrade_paths() {
        assert!(is_newer("1.32.2", "1.32.3-beta.1"));
        assert!(is_newer("1.32.3-beta.1", "1.32.3"));
        assert!(!is_newer("1.32.3", "1.32.3-beta.1"));
        assert!(!is_newer("1.32.3-beta.1", "1.32.2"));
    }

    #[test]
    fn is_newer_obeys_semver_prerelease_identifier_precedence() {
        let ordered = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.10",
            "1.0.0-rc.1",
            "1.0.0",
        ];
        for pair in ordered.windows(2) {
            assert!(is_newer(pair[0], pair[1]), "{} -> {}", pair[0], pair[1]);
            assert!(!is_newer(pair[1], pair[0]));
        }
        assert!(is_newer(
            "1.0.0-999999999999999999999999999999",
            "1.0.0-alpha"
        ));
    }

    #[test]
    fn is_newer_accepts_v_prefix_and_ignores_build_metadata() {
        assert!(is_newer("v1.32.2", "v1.32.3-beta.1+build.7"));
        assert!(!is_newer("1.32.3+build.1", "1.32.3+build.2"));
        assert!(!is_newer("1.32.3-beta.1+build.2", "1.32.3-beta.1+build.1"));
    }

    #[test]
    fn is_newer_fails_closed_for_invalid_versions() {
        for invalid in [
            "",
            "v",
            "V1.2.3",
            "vv1.2.3",
            " 1.2.3",
            "1.2.3\n",
            "1..2",
            ".1",
            "1.2.",
            "1.2.3.4",
            "01.2.3",
            "1.02.3",
            "1.2.03",
            "1.2.3-01",
            "1.2.3-beta..1",
            "1.2.3-",
            "1.2.3+",
            "1.2.3+bad_metadata",
            "18446744073709551616.0.0",
            "garbage",
        ] {
            assert!(!is_newer(invalid, "2.0.0"), "invalid current: {invalid:?}");
            assert!(!is_newer("0.0.0", invalid), "invalid latest: {invalid:?}");
        }
    }

    #[test]
    fn installable_release_requires_the_complete_verified_asset_set() {
        let mut release = ReleaseInfo {
            version: "1.27.60".into(),
            tag: "v1.27.60".into(),
            html_url: "https://example.com/release".into(),
            notes: String::new(),
            asset_url: Some("https://example.com/PeterFan-v1.27.60.dmg".into()),
            asset_name: Some(
                if cfg!(target_os = "windows") {
                    "peterfan-v1.27.60-x86_64-pc-windows-msvc.zip"
                } else {
                    "PeterFan-v1.27.60.dmg"
                }
                .into(),
            ),
            asset_digest: Some("a".repeat(64)),
            archive_url: None,
            dmg_url: None,
            windows_url: None,
            checksum_url: Some("https://example.com/checksums.txt".into()),
            checksum_name: Some("checksums.txt".into()),
            checksum_digest: Some("b".repeat(64)),
        };

        assert!(is_installable_release(&release));
        release.checksum_digest = None;
        assert!(!is_installable_release(&release));
        release.checksum_digest = Some("b".repeat(64));
        release.asset_digest = None;
        assert!(!is_installable_release(&release));
    }

    #[test]
    fn parses_real_github_release_response() {
        let body = br#"{
            "tag_name": "v0.27.1",
            "html_url": "https://github.com/uulab-official/peterfan/releases/tag/v0.27.1",
            "body": "Signed update with native progress reporting.",
            "assets": [
                {"name": "peterfan-v0.27.1-aarch64-apple-darwin.tar.gz",
                 "browser_download_url": "https://github.com/uulab-official/peterfan/releases/download/v0.27.1/peterfan-v0.27.1-aarch64-apple-darwin.tar.gz",
                 "digest": "sha256:ABCDEFabcdefABCDEFabcdefABCDEFabcdefABCDEFabcdefABCDEFabcdefABCD"},
                {"name": "checksums.txt",
                 "browser_download_url": "https://github.com/uulab-official/peterfan/releases/download/v0.27.1/checksums.txt",
                 "digest": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"},
                {"name": "peterfan-v0.27.1-x86_64-pc-windows-msvc.zip",
                 "browser_download_url": "https://example.com/windows.zip"}
            ]
        }"#;
        let info = parse_release_response(body).unwrap();
        assert_eq!(info.version, "0.27.1");
        assert_eq!(info.tag, "v0.27.1");
        assert_eq!(info.notes, "Signed update with native progress reporting.");
        #[cfg(not(target_os = "windows"))]
        {
            assert!(info.asset_url.unwrap().contains("aarch64-apple-darwin"));
            assert!(info.asset_name.unwrap().contains("aarch64-apple-darwin"));
            assert_eq!(
                info.asset_digest.as_deref(),
                Some("abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd")
            );
        }
        #[cfg(target_os = "windows")]
        {
            assert_eq!(
                info.asset_name.as_deref(),
                Some("peterfan-v0.27.1-x86_64-pc-windows-msvc.zip")
            );
            assert_eq!(
                info.asset_url.as_deref(),
                Some("https://example.com/windows.zip")
            );
            assert!(info.asset_digest.is_none());
        }
        assert_eq!(
            info.windows_url.as_deref(),
            Some("https://example.com/windows.zip")
        );
        assert_eq!(info.checksum_name.as_deref(), Some("checksums.txt"));
        assert!(info.checksum_url.unwrap().ends_with("/checksums.txt"));
        assert_eq!(
            info.checksum_digest.as_deref(),
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
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
    #[cfg(not(target_os = "windows"))]
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
    fn recognizes_only_native_windows_x64_archives() {
        assert!(is_windows_archive(
            "peterfan-v1.27.59-x86_64-pc-windows-msvc.zip"
        ));
        assert!(!is_windows_archive("PeterFan-v1.27.59.dmg"));
        assert!(!is_windows_archive(
            "peterfan-v1.27.59-aarch64-apple-darwin.tar.gz"
        ));
    }

    #[test]
    fn missing_tag_name_is_an_error() {
        assert!(parse_release_response(b"{}").is_err());
    }

    #[test]
    fn malformed_release_versions_are_failed_checks_not_current_versions() {
        for tag in ["", "invalid", "vv1.2.3", "1.02.3", "1.2.3-beta.01"] {
            let body = serde_json::to_vec(&serde_json::json!({"tag_name":tag})).unwrap();
            assert!(parse_release_response(&body).is_err(), "{tag}");
        }
        for tag in ["v1.2.3", "1.2", "v1.2.3-beta.1+build.5"] {
            let body = serde_json::to_vec(&serde_json::json!({"tag_name":tag})).unwrap();
            assert!(parse_release_response(&body).is_ok(), "{tag}");
        }
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
        assert!(info.checksum_url.is_none());
        assert!(info.asset_digest.is_none());
        assert!(info.checksum_digest.is_none());
    }

    #[test]
    fn prerelease_channel_uses_newest_published_release_and_skips_drafts() {
        let body = br#"[
            {"tag_name":"v3.0.0-draft","draft":true,"html_url":"https://example.com/draft","assets":[]},
            {"tag_name":"v2.9.0-beta.1","prerelease":true,"html_url":"https://example.com/beta","assets":[]},
            {"tag_name":"v2.8.0","prerelease":false,"html_url":"https://example.com/stable","assets":[]}
        ]"#;
        let info = parse_release_list_response(body).unwrap();
        assert_eq!(info.version, "2.9.0-beta.1");
        assert_eq!(info.tag, "v2.9.0-beta.1");
    }

    #[test]
    fn parse_github_sha256_digest_accepts_only_sha256_hex() {
        assert_eq!(
            parse_github_sha256_digest(Some(
                "sha256:ABCDEFabcdefABCDEFabcdefABCDEFabcdefABCDEFabcdefABCDEFabcdefABCD"
            ))
            .as_deref(),
            Some("abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd")
        );
        assert_eq!(parse_github_sha256_digest(Some("sha512:abc")), None);
        assert_eq!(
            normalize_sha256_digest(
                "0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF"
            )
            .as_deref(),
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
        assert_eq!(parse_github_sha256_digest(Some("sha256:not-hex")), None);
        assert_eq!(parse_github_sha256_digest(None), None);
    }

    #[test]
    fn checksum_for_asset_accepts_shasum_and_coreutils_formats() {
        let checksums = "\
abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd  PeterFan-v1.2.3.dmg\n\
0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef *nested/peterfan-v1.2.3-universal-apple-darwin.tar.gz\n";

        assert_eq!(
            checksum_for_asset(checksums, "PeterFan-v1.2.3.dmg").as_deref(),
            Some("abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd")
        );
        assert_eq!(
            checksum_for_asset(checksums, "peterfan-v1.2.3-universal-apple-darwin.tar.gz")
                .as_deref(),
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
    }

    #[test]
    fn checksum_for_asset_rejects_missing_or_malformed_hashes() {
        let checksums = "\
not-a-sha PeterFan-v1.2.3.dmg\n\
abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabc  other.dmg\n";

        assert_eq!(checksum_for_asset(checksums, "PeterFan-v1.2.3.dmg"), None);
        assert_eq!(checksum_for_asset(checksums, "missing.dmg"), None);
    }

    #[test]
    fn release_artifact_name_filter_only_accepts_macos_release_assets() {
        assert!(is_release_artifact_name("PeterFan-v1.2.3.dmg"));
        assert!(is_release_artifact_name(
            "peterfan-v1.2.3-universal-apple-darwin.tar.gz"
        ));
        assert!(!is_release_artifact_name("checksums.txt"));
        assert!(!is_release_artifact_name("peterfan-v1.2.3-windows.zip"));
        assert!(!is_release_artifact_name("PeterFan-v1.2.3.zip"));
    }

    #[test]
    fn release_version_helpers_infer_directory_and_artifact_versions() {
        let names = vec![
            "PeterFan-v1.2.3.dmg".to_string(),
            "peterfan-v1.2.3-universal-apple-darwin.tar.gz".to_string(),
        ];
        assert_eq!(
            infer_release_directory_version(std::path::Path::new("/tmp/v1.2.3"), &names).as_deref(),
            Some("1.2.3")
        );
        assert_eq!(
            infer_release_directory_version(std::path::Path::new("/tmp/release"), &names)
                .as_deref(),
            Some("1.2.3")
        );
        assert_eq!(
            release_artifact_version("PeterFan-v1.2.3.dmg").as_deref(),
            Some("1.2.3")
        );
        assert_eq!(
            release_artifact_version("peterfan-v1.2.3-universal-apple-darwin.tar.gz").as_deref(),
            Some("1.2.3")
        );
        assert_eq!(release_artifact_version("PeterFan-latest.dmg"), None);
    }

    #[test]
    fn checksum_release_artifact_names_returns_only_macos_release_assets() {
        let checksums = "\
abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd  PeterFan-v1.2.3.dmg\n\
0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef *nested/peterfan-v1.2.3-universal-apple-darwin.tar.gz\n\
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  peterfan-v1.2.3-windows.zip\n";

        assert_eq!(
            checksum_release_artifact_names(checksums),
            vec![
                "PeterFan-v1.2.3.dmg".to_string(),
                "peterfan-v1.2.3-universal-apple-darwin.tar.gz".to_string(),
            ]
        );
    }

    #[test]
    fn codesign_detail_helpers_extract_identity_fields() {
        let details = "\
Executable=/Applications/PeterFan.app/Contents/MacOS/PeterFan
Identifier=kr.co.uulab.peterfan
Format=app bundle with Mach-O thin (arm64)
Authority=Developer ID Application: Choi Tae Ho (N99FMBQ662)
Authority=Developer ID Certification Authority
Authority=Apple Root CA
TeamIdentifier=N99FMBQ662
";

        assert_eq!(
            codesign_field(details, "Identifier").as_deref(),
            Some(EXPECTED_BUNDLE_ID)
        );
        assert_eq!(
            codesign_field(details, "TeamIdentifier").as_deref(),
            Some(EXPECTED_TEAM_ID)
        );
        assert!(has_developer_id_authority(details));
    }

    #[test]
    fn update_install_result_round_trips_as_one_bounded_record() {
        let root = std::env::temp_dir().join(format!(
            "peterfan-update-result-test-{}",
            std::process::id()
        ));
        let path = root.join("nested/update-result.json");
        let _ = std::fs::remove_dir_all(&root);

        let result = UpdateInstallResult::new("installed", "1.2.3", "installed successfully");
        write_update_install_result_to(&path, &result).unwrap();
        assert_eq!(read_update_install_result_from(&path), Some(result));
        assert!(!path.with_extension("json.tmp-updater").exists());

        let replacement = UpdateInstallResult::new("rolled_back", "1.2.4", "restored");
        write_update_install_result_to(&path, &replacement).unwrap();
        assert_eq!(read_update_install_result_from(&path), Some(replacement));
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn helper_preparation_failure_does_not_claim_pending_installation() {
        let root = std::env::temp_dir().join(format!(
            "peterfan-update-helper-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let result_path = root.join("result.json");
        let previous = UpdateInstallResult::new("installed", "1.2.3", "previous install");
        write_update_install_result_to(&result_path, &previous).unwrap();
        let pending = UpdateInstallResult::new("pending", "1.2.4", "queued");
        assert!(prepare_update_helper(&root, b"script", &result_path, &pending).is_err());
        assert_eq!(
            read_update_install_result_from(&result_path),
            Some(previous)
        );
        let script_path = root.join("apply-update");
        prepare_update_helper(&script_path, b"script", &result_path, &pending).unwrap();
        assert_eq!(std::fs::read(script_path).unwrap(), b"script");
        assert_eq!(read_update_install_result_from(&result_path), Some(pending));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn windows_install_directory_requires_an_existing_absolute_app() {
        let root = std::env::temp_dir().join(format!(
            "peterfan-windows-install-location-test-{}",
            std::process::id()
        ));
        let custom = root.join("My Apps").join("O'Brien").join("PeterFan");
        std::fs::create_dir_all(&custom).unwrap();
        let executable = custom.join("PeterFan.exe");
        assert!(windows_install_directory(&executable).is_err());
        std::fs::write(&executable, b"fixture").unwrap();
        assert_eq!(windows_install_directory(&executable).unwrap(), custom);
        let cli = custom.join("peterfan-cli.exe");
        std::fs::write(&cli, b"fixture").unwrap();
        assert_eq!(windows_install_directory(&cli).unwrap(), custom);
        std::fs::remove_file(&executable).unwrap();
        assert!(windows_install_directory(&cli).is_err());
        assert!(windows_install_directory(std::path::Path::new("PeterFan.exe")).is_err());
        let other = root.join("other.exe");
        std::fs::write(&other, b"fixture").unwrap();
        assert!(windows_install_directory(&other).is_err());
        assert!(windows_install_directory(&root).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn windows_apply_script_requires_transactional_files_and_healthy_launch() {
        let files = WINDOWS_UPDATE_FILES
            .into_iter()
            .map(|name| (name.to_string(), "a".repeat(64)))
            .collect();
        let script = build_windows_apply_update_script(
            std::path::Path::new(r"C:\Temp\PeterFan update\payload"),
            std::path::Path::new(r"D:\My Apps\O'Brien\PeterFan"),
            std::path::Path::new(r"C:\Temp\PeterFan update"),
            std::path::Path::new(r"C:\Users\Test\AppData\Local\PeterFan\update-result.json"),
            "1.2.3",
            "0123456789abcdef0123456789abcdef",
            &files,
        );

        assert!(!script.contains("& 'C:\\Temp\\PeterFan update\\install.ps1'"));
        assert!(script.contains("O''Brien"));
        assert!(script.contains("$existing = Join-Path $installDir 'PeterFan.exe'"));
        assert!(!script.contains("LOCALAPPDATA"));
        assert!(!script.contains("Stop-Process -Name"));
        assert!(script.contains("[IO.FileShare]::None"));
        assert!(script.contains("Staged update checksum mismatch"));
        assert!(script.contains("$receipt.pid -eq $script:launched.Id"));
        assert!(script.contains("$receipt.version -ceq $spec.version"));
        assert!(script.contains("[StringComparison]::OrdinalIgnoreCase"));
        assert!(script.contains("The update failed and the previous files were restored"));
        assert!(
            script.find("    Launch-Healthy\n").unwrap()
                < script.find("    Write-Result 'installed'").unwrap()
        );
        assert!(
            script.find("    Write-Result 'installed'").unwrap()
                < script.find("if (-not $preserveBackup)").unwrap()
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_updater_transaction_restores_failures_in_isolated_native_fixture() {
        let root = std::env::temp_dir().join(format!(
            "peterfan-update-transaction-test-{}",
            std::process::id()
        ));
        let script_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/test-windows-update-transaction.ps1");
        let helper =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/windows_update.ps1");
        let output = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&script_path)
            .arg("-Helper")
            .arg(&helper)
            .arg("-Root")
            .arg(&root)
            .output()
            .expect("native Windows PowerShell fixture must start");
        assert!(
            output.status.success(),
            "native update fixture failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let cli = root.join("version-check.exe");
        let version = root.join("version.txt");
        verify_windows_payload_version(&cli, "9.9.9", &version, std::time::Duration::from_secs(5))
            .unwrap();
        let _ = std::fs::remove_file(&version);
        let error = verify_windows_payload_version(
            &root.join("version-hang.exe"),
            "9.9.9",
            &version,
            std::time::Duration::from_millis(150),
        )
        .unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ota_health_requires_fresh_ui_ack_and_exact_process_identity() {
        let script = build_apply_update_script(
            std::path::Path::new("/Applications/PeterFan.app"),
            std::path::Path::new("/tmp/update/PeterFan.app"),
            std::path::Path::new("/tmp/update/PreviousPeterFan.app"),
            std::path::Path::new("/tmp/update"),
            std::path::Path::new("/tmp/update/apply-update.log"),
            std::path::Path::new("/tmp/support/update-result.json"),
            "1.2.3",
            "kr.co.uulab.peterfan.updater.test-health",
        );
        assert!(script.contains("--update-health-token"));
        assert!(script.contains("plutil -extract nonce raw"));
        assert!(script.contains("plutil -extract version raw"));
        assert!(script.contains("lstart=,comm="));
        assert!(!script.contains("pkill -fx"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ota_apply_script_keeps_backup_until_new_app_is_healthy() {
        let script = build_apply_update_script(
            std::path::Path::new("/Applications/PeterFan.app"),
            std::path::Path::new("/tmp/update/PeterFan.app"),
            std::path::Path::new("/tmp/update/PreviousPeterFan.app"),
            std::path::Path::new("/tmp/update"),
            std::path::Path::new("/tmp/update/apply-update.log"),
            std::path::Path::new("/tmp/support/update-result.json"),
            "1.2.3",
            "kr.co.uulab.peterfan.updater.test-success",
        );

        let health_check = script.rfind("launch_until_healthy || rollback").unwrap();
        let delete_backup = script
            .rfind("rm -rf '/tmp/update/PreviousPeterFan.app'")
            .unwrap();
        assert!(script.contains("codesign --verify --deep --strict"));
        assert!(script.contains("stapler validate"));
        assert!(script.contains("open -g -j"));
        assert!(script.contains("open -g -j -n"));
        assert!(script.contains("find_app_pids"));
        assert!(script.contains("wait_for_previous_exit"));
        assert!(script.contains("wait_for_previous_exit || fail_before_replace"));
        assert!(script.contains("launch_until_healthy || rollback"));
        assert!(script.contains("write_result"));
        assert!(script.contains("trap cleanup_job EXIT"));
        assert!(script.contains("launchctl remove 'kr.co.uulab.peterfan.updater.test-success'"));
        assert!(script.contains("update-result.json"));
        assert!(script.contains(r#""status":"installed""#));
        assert!(script.contains(r#""status":"rolled_back""#));
        assert!(delete_backup > health_check);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn install_target_prefers_the_bundle_containing_the_running_app() {
        let embedded = std::path::Path::new("/Applications/PeterFan.app/Contents/MacOS/PeterFan");
        let fallback = std::path::Path::new("/Applications/PeterFan.app");
        let target = resolve_install_target(embedded, fallback, |_| {
            panic!("embedded app must not need fallback verification")
        })
        .unwrap();
        assert_eq!(target, fallback);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn standalone_cli_uses_only_a_verified_installed_app() {
        let standalone = std::path::Path::new("/tmp/peterfan-release/peterfan");
        let installed = std::path::Path::new("/Applications/PeterFan.app");

        let target = resolve_install_target(standalone, installed, |candidate| {
            assert_eq!(candidate, installed);
            Ok(())
        })
        .unwrap();
        assert_eq!(target, installed);

        let error = resolve_install_target(standalone, installed, |_| {
            Err("signature verification failed".to_string())
        })
        .unwrap_err();
        assert_eq!(error, "signature verification failed");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_updater_label_is_unique_and_launchd_safe() {
        assert_eq!(
            macos_updater_label(42, "1.27.67-beta.1"),
            "kr.co.uulab.peterfan.updater.42.1-27-67-beta-1"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ota_apply_script_rolls_back_when_replacement_signature_is_invalid() {
        let root = std::env::temp_dir().join(format!(
            "peterfan updater rollback test {}",
            std::process::id()
        ));
        let app = root.join("Installed/PeterFan.app");
        let new_app = root.join("Update/PeterFan.app");
        let backup = root.join("Update/PreviousPeterFan.app");
        let log = root.join("Update/apply-update.log");
        let result = root.join("Support/update-result.json");
        let script_path = root.join("Update/apply-update.sh");
        let old_marker = app.join("old-version");
        let new_marker = new_app.join("new-version");

        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::create_dir_all(new_app.join("Contents/MacOS")).unwrap();
        std::fs::create_dir_all(result.parent().unwrap()).unwrap();
        std::fs::write(&old_marker, "old").unwrap();
        std::fs::write(&new_marker, "new").unwrap();
        std::fs::write(app.join("Contents/MacOS/PeterFan"), "not executable").unwrap();
        std::fs::write(new_app.join("Contents/MacOS/PeterFan"), "invalid signature").unwrap();

        let script = build_apply_update_script(
            &app,
            &new_app,
            &backup,
            &root.join("Update"),
            &log,
            &result,
            "1.2.3",
            "kr.co.uulab.peterfan.updater.test-rollback",
        );
        std::fs::write(&script_path, script).unwrap();
        let status = std::process::Command::new("/bin/bash")
            .arg(&script_path)
            .status()
            .unwrap();

        assert!(!status.success());
        assert_eq!(std::fs::read_to_string(&old_marker).unwrap(), "old");
        assert!(!app.join("new-version").exists());
        assert!(!backup.exists());
        assert_eq!(
            read_update_install_result_from(&result)
                .expect("rollback result")
                .status,
            "rolled_back"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(target_os = "macos")]
    struct UpdateAppFixture(std::path::PathBuf);

    #[cfg(target_os = "macos")]
    #[test]
    fn concurrent_macos_helper_refuses_before_touching_app_or_receipt() {
        let root =
            std::env::temp_dir().join(format!("peterfan-apply-contention-{}", std::process::id()));
        let app = root.join("Installed/PeterFan.app");
        let candidate = root.join("Update/PeterFan.app");
        let backup = root.join("Update/PreviousPeterFan.app");
        let result = root.join("Support/update-result.json");
        for path in [&app, &candidate, result.parent().unwrap()] {
            std::fs::create_dir_all(path).unwrap();
        }
        std::fs::write(app.join("original"), b"old app").unwrap();
        std::fs::write(&result, b"active transaction receipt").unwrap();
        let marker = root.join("lock-ready");
        let mut holder = std::process::Command::new("/usr/bin/lockf")
            .args(["-k", "-t", "0"])
            .arg(app.with_extension("app.update.lock"))
            .args(["/bin/sh", "-c", "touch \"$1\"; sleep 2", "fixture"])
            .arg(&marker)
            .spawn()
            .unwrap();
        let start = std::time::Instant::now();
        while !marker.exists() {
            assert!(start.elapsed() < std::time::Duration::from_secs(5));
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let script = build_apply_update_script(
            &app,
            &candidate,
            &backup,
            &root.join("Update"),
            &root.join("Update/apply.log"),
            &result,
            "1.2.3",
            "kr.co.uulab.peterfan.updater.test-contention",
        );
        let path = root.join("Update/apply.sh");
        std::fs::write(&path, script).unwrap();
        let status = std::process::Command::new("/bin/bash")
            .arg(&path)
            .status()
            .unwrap();
        assert!(holder.wait().unwrap().success());
        assert!(!status.success());
        assert_eq!(
            std::fs::read(&result).unwrap(),
            b"active transaction receipt"
        );
        assert_eq!(std::fs::read(app.join("original")).unwrap(), b"old app");
        assert!(!backup.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ota_failed_removal_preserves_separate_recovery_backup() {
        let root =
            std::env::temp_dir().join(format!("peterfan-failed-removal-{}", std::process::id()));
        let app = root.join("Installed/PeterFan.app");
        let candidate = root.join("Update/PeterFan.app");
        let backup = root.join("Update/PreviousPeterFan.app");
        let result = root.join("Support/update-result.json");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::create_dir_all(&candidate).unwrap();
        std::fs::create_dir_all(result.parent().unwrap()).unwrap();
        std::fs::write(app.join("old-marker"), b"recovery").unwrap();
        let script = build_apply_update_script(
            &app,
            &candidate,
            &backup,
            &root.join("Update"),
            &root.join("Update/apply.log"),
            &result,
            "1.2.3",
            "kr.co.uulab.peterfan.updater.test-failed-removal",
        )
        .replace(
            "/usr/bin/codesign --verify --deep --strict",
            "/usr/bin/true",
        )
        .replace("/usr/bin/xcrun stapler validate", "/usr/bin/true")
        .replace("/usr/bin/open", "/usr/bin/true")
        .replace("i<45", "i<2")
        .replace("sleep 1", "sleep 0.01")
        .replace(
            &format!("/bin/rm -rf {}", shell_quote(&app)),
            "/usr/bin/false",
        );
        let path = root.join("Update/apply.sh");
        std::fs::write(&path, script).unwrap();
        assert!(!std::process::Command::new("/bin/bash")
            .arg(&path)
            .status()
            .unwrap()
            .success());
        assert!(
            backup.join("old-marker").exists(),
            "failed removal must retain standalone backup"
        );
        assert!(!app.join("PreviousPeterFan.app").exists());
        assert_eq!(
            read_update_install_result_from(&result).unwrap().status,
            "failed"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ota_missing_ui_ack_restores_old_app_after_successful_trust_checks() {
        let root = std::env::temp_dir().join(format!("peterfan-no-ui-ack-{}", std::process::id()));
        let app = root.join("Installed/PeterFan.app");
        let candidate = root.join("Update/PeterFan.app");
        let backup = root.join("Update/PreviousPeterFan.app");
        let result = root.join("Support/update-result.json");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::create_dir_all(candidate.join("Contents/MacOS")).unwrap();
        std::fs::create_dir_all(result.parent().unwrap()).unwrap();
        std::fs::write(app.join("old-marker"), b"preserved").unwrap();
        std::fs::write(candidate.join("new-marker"), b"candidate").unwrap();
        // Fixture trust/launch commands succeed; no app acknowledges its UI.
        // Only this generated test copy shortens its deadline and substitutes
        // external tools. Production scripts always use real tools/deadlines.
        let script = build_apply_update_script(
            &app,
            &candidate,
            &backup,
            &root.join("Update"),
            &root.join("Update/apply-update.log"),
            &result,
            "1.2.3",
            "kr.co.uulab.peterfan.updater.test-no-ui-ack",
        )
        .replace(
            "/usr/bin/codesign --verify --deep --strict",
            "/usr/bin/true",
        )
        .replace("/usr/bin/xcrun stapler validate", "/usr/bin/true")
        .replace("/usr/bin/open", "/usr/bin/true")
        .replace("i<45", "i<2")
        .replace("sleep 1", "sleep 0.01");
        let script_path = root.join("Update/apply-update.sh");
        std::fs::write(&script_path, script).unwrap();
        assert!(!std::process::Command::new("/bin/bash")
            .arg(&script_path)
            .status()
            .unwrap()
            .success());
        assert_eq!(std::fs::read(app.join("old-marker")).unwrap(), b"preserved");
        assert!(!app.join("new-marker").exists());
        assert_eq!(
            read_update_install_result_from(&result).unwrap().status,
            "rolled_back"
        );
        assert!(!backup.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "macos")]
    impl UpdateAppFixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "peterfan-update-validation-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(root.join("Contents/MacOS")).unwrap();
            let fixture = Self(root);
            fixture.write_version(Some("1.31.32"));
            fixture.write_helper(0o755);
            fixture
        }

        fn write_version(&self, version: Option<&str>) {
            let entry = version
                .map(|v| format!("<key>CFBundleShortVersionString</key><string>{v}</string>"))
                .unwrap_or_default();
            std::fs::write(
                self.0.join("Contents/Info.plist"),
                format!(
                    "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>{entry}</dict></plist>"
                ),
            )
            .unwrap();
        }

        fn write_helper(&self, mode: u32) {
            use std::os::unix::fs::PermissionsExt;
            let helper = self.0.join("Contents/MacOS/peterfand");
            std::fs::write(&helper, "#!/bin/sh\nexit 0\n").unwrap();
            std::fs::set_permissions(helper, std::fs::Permissions::from_mode(mode)).unwrap();
        }
    }

    #[cfg(target_os = "macos")]
    impl Drop for UpdateAppFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn update_app_contents_accepts_exact_release_version_and_executable_helper() {
        let app = UpdateAppFixture::new();
        assert_eq!(validate_update_app_contents(&app.0, "1.31.32"), Ok(()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn update_app_contents_rejects_mismatched_or_missing_bundle_version() {
        let app = UpdateAppFixture::new();
        assert!(validate_update_app_contents(&app.0, "1.31.33")
            .unwrap_err()
            .contains("version"));
        app.write_version(None);
        assert!(validate_update_app_contents(&app.0, "1.31.32")
            .unwrap_err()
            .contains("version"));
        std::fs::write(app.0.join("Contents/Info.plist"), "invalid plist").unwrap();
        assert!(validate_update_app_contents(&app.0, "1.31.32").is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn update_app_contents_rejects_empty_expected_version() {
        let app = UpdateAppFixture::new();
        assert!(validate_update_app_contents(&app.0, "").is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn update_app_contents_rejects_missing_nonexecutable_or_directory_helper() {
        let app = UpdateAppFixture::new();
        let helper = app.0.join("Contents/MacOS/peterfand");
        app.write_helper(0o644);
        assert!(validate_update_app_contents(&app.0, "1.31.32")
            .unwrap_err()
            .contains("helper"));
        std::fs::remove_file(&helper).unwrap();
        assert!(validate_update_app_contents(&app.0, "1.31.32").is_err());
        std::fs::create_dir(&helper).unwrap();
        assert!(validate_update_app_contents(&app.0, "1.31.32").is_err());
    }

    #[test]
    fn app_integrity_report_ok_requires_every_check_to_pass() {
        let mut report =
            AppIntegrityReport::new(std::path::Path::new("/Applications/PeterFan.app"));
        report.push("one", true, "ok");
        report.push("two", false, "bad");
        assert!(!report.finish().ok);

        let mut report =
            AppIntegrityReport::new(std::path::Path::new("/Applications/PeterFan.app"));
        report.push("one", true, "ok");
        report.push("two", true, "ok");
        assert!(report.finish().ok);
    }

    #[test]
    fn spctl_service_error_markers_do_not_match_normal_rejections() {
        assert!(spctl_text_is_resource_exhaustion("Too many open files"));
        assert!(spctl_text_is_internal_assessment_failure(
            "bundle format unrecognized, invalid, or unsuitable"
        ));
        assert!(!spctl_text_is_resource_exhaustion(
            "rejected source=Unnotarized Developer ID"
        ));
        assert!(!spctl_text_is_internal_assessment_failure(
            "rejected source=Unnotarized Developer ID"
        ));
        let internal = "bundle format unrecognized, invalid, or unsuitable";
        let rejected = "rejected source=Unnotarized Developer ID";
        assert!(spctl_internal_failure_is_system_wide(internal, internal));
        assert!(spctl_internal_failure_is_system_wide(
            internal,
            "Too many open files"
        ));
        assert!(!spctl_internal_failure_is_system_wide(internal, rejected));
        assert!(!spctl_internal_failure_is_system_wide(rejected, internal));
    }

    #[test]
    fn artifact_integrity_report_ok_requires_artifact_checks_and_app_ok() {
        let artifact = std::path::Path::new("/tmp/PeterFan-v1.2.3.dmg");

        let mut report = ArtifactIntegrityReport::new(artifact);
        report.push("artifact", true, "ok");
        assert!(!report.finish().ok);

        let mut bad_app = AppIntegrityReport::new(std::path::Path::new("/tmp/PeterFan.app"));
        bad_app.push("app", false, "bad");
        let mut report = ArtifactIntegrityReport::new(artifact);
        report.push("artifact", true, "ok");
        report.app = Some(bad_app.finish());
        assert!(!report.finish().ok);

        let mut good_app = AppIntegrityReport::new(std::path::Path::new("/tmp/PeterFan.app"));
        good_app.push("app", true, "ok");
        let mut report = ArtifactIntegrityReport::new(artifact);
        report.push("artifact", true, "ok");
        report.app = Some(good_app.finish());
        assert!(report.finish().ok);
    }

    #[test]
    fn release_directory_report_ok_requires_checks_and_artifacts() {
        let dir = std::path::Path::new("/tmp/release");

        let mut report = ReleaseDirectoryIntegrityReport::new(dir);
        report.push("directory", true, "ok");
        assert!(!report.finish().ok);

        let mut artifact = ArtifactIntegrityReport::new(std::path::Path::new("/tmp/app.dmg"));
        artifact.push("artifact", true, "ok");
        let mut app = AppIntegrityReport::new(std::path::Path::new("/tmp/PeterFan.app"));
        app.push("app", true, "ok");
        artifact.app = Some(app.finish());

        let mut report = ReleaseDirectoryIntegrityReport::new(dir);
        report.push("directory", true, "ok");
        report.artifacts.push(artifact.finish());
        assert!(report.finish().ok);
    }
}
