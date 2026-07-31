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
    parse_release_response(&out.stdout)
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
    let tag = val["tag_name"]
        .as_str()
        .ok_or("response has no tag_name")?
        .to_string();
    let version = tag.trim_start_matches('v').to_string();
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

fn is_release_artifact_name(name: &str) -> bool {
    (name.starts_with("PeterFan-") && name.ends_with(".dmg"))
        || (name.starts_with("peterfan-")
            && name.contains("apple-darwin")
            && name.ends_with(".tar.gz"))
}

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
    for required in ["PeterFan.exe", "peterfan-cli.exe", "peterfan-tui.exe"] {
        if !payload_dir.join(required).is_file() {
            return Err(format!("downloaded Windows ZIP is missing {required}"));
        }
    }
    let version_output = std::process::Command::new(payload_dir.join("peterfan-cli.exe"))
        .arg("--version")
        .output()
        .map_err(|e| format!("could not inspect downloaded Windows CLI: {e}"))?;
    let version_text = String::from_utf8_lossy(&version_output.stdout);
    if !version_output.status.success()
        || !version_text
            .split_whitespace()
            .any(|part| part == release.version)
    {
        return Err(format!(
            "downloaded Windows payload version does not match v{}",
            release.version
        ));
    }

    let result_path =
        update_install_result_path().ok_or("could not determine update result path")?;
    let pending = UpdateInstallResult::new(
        "pending",
        &release.version,
        format!("PeterFan v{} is verified and queued.", release.version),
    );
    write_update_install_result_to(&result_path, &pending)?;

    let script_path = tmp_dir.join("apply-update.ps1");
    let script =
        build_windows_apply_update_script(&installer, &tmp_dir, &result_path, &release.version);
    std::fs::write(&script_path, script).map_err(|e| e.to_string())?;
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

#[cfg(target_os = "windows")]
fn build_windows_apply_update_script(
    installer: &std::path::Path,
    tmp_dir: &std::path::Path,
    result_path: &std::path::Path,
    target_version: &str,
) -> String {
    let result_tmp = result_path.with_extension("json.tmp-updater");
    let installed = serde_json::to_string(&UpdateInstallResult::new(
        "installed",
        target_version,
        format!("PeterFan v{target_version} was installed successfully."),
    ))
    .expect("update result serializes");
    let failed = serde_json::to_string(&UpdateInstallResult::new(
        "failed",
        target_version,
        "The Windows update failed. The existing installation was left in place.",
    ))
    .expect("update result serializes");
    format!(
        "$ErrorActionPreference = 'Stop'\n\
         $result = {result}\n\
         $resultTmp = {result_tmp}\n\
         function Write-Result([string]$json) {{\n\
         \tNew-Item -ItemType Directory -Force -Path (Split-Path -Parent $result) | Out-Null\n\
         \t[System.IO.File]::WriteAllText($resultTmp, $json, [System.Text.UTF8Encoding]::new($false))\n\
         \tMove-Item -LiteralPath $resultTmp -Destination $result -Force\n\
         }}\n\
         try {{\n\
         \t& {installer}\n\
         \tWrite-Result {installed}\n\
         }} catch {{\n\
         \tWrite-Result {failed}\n\
         \t$existing = Join-Path $env:LOCALAPPDATA 'Programs\\PeterFan\\PeterFan.exe'\n\
         \tif (Test-Path $existing -PathType Leaf) {{ Start-Process $existing }}\n\
         \texit 1\n\
         }}\n\
         Start-Sleep -Seconds 2\n\
         Remove-Item -LiteralPath {tmp} -Recurse -Force -ErrorAction SilentlyContinue\n",
        result = powershell_quote(result_path),
        result_tmp = powershell_quote(&result_tmp),
        installer = powershell_quote(installer),
        installed = powershell_quote_str(&installed),
        failed = powershell_quote_str(&failed),
        tmp = powershell_quote(tmp_dir),
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

#[cfg(target_os = "windows")]
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
    validate_update_app(&new_app)?;

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
    write_update_install_result_to(&result_path, &pending)?;
    let script = build_apply_update_script(
        app_path,
        &new_app,
        &backup_path,
        tmp_dir,
        &log_path,
        &result_path,
        target_version,
    );
    std::fs::write(&script_path, script).map_err(|e| e.to_string())?;
    let _ = std::process::Command::new("chmod")
        .args(["+x"])
        .arg(&script_path)
        .status();

    let updater_label = macos_updater_label(std::process::id(), target_version);
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
fn build_apply_update_script(
    app_path: &std::path::Path,
    new_app: &std::path::Path,
    backup_path: &std::path::Path,
    tmp_dir: &std::path::Path,
    log_path: &std::path::Path,
    result_path: &std::path::Path,
    target_version: &str,
) -> String {
    let executable = app_path.join("Contents/MacOS/PeterFan");
    let result_tmp = result_path.with_extension("json.tmp-updater");
    let installed = serde_json::to_string(&UpdateInstallResult::new(
        "installed",
        target_version,
        format!("PeterFan v{target_version} was installed successfully."),
    ))
    .expect("update result serializes");
    let rolled_back = serde_json::to_string(&UpdateInstallResult::new(
        "rolled_back",
        target_version,
        "The update failed and the previous PeterFan version was restored.",
    ))
    .expect("update result serializes");
    let failed = serde_json::to_string(&UpdateInstallResult::new(
        "failed",
        target_version,
        "The installed app could not be moved. The existing app was left unchanged.",
    ))
    .expect("update result serializes");
    let restore_failed = serde_json::to_string(&UpdateInstallResult::new(
        "failed",
        target_version,
        "The update failed and the previous PeterFan version could not be restored automatically.",
    ))
    .expect("update result serializes");
    format!(
        "#!/bin/bash\n\
         set -u\n\
         exec >{log} 2>&1\n\
         write_result() {{\n\
         \t/usr/bin/printf '%s\\n' \"$1\" > {result_tmp} && /bin/mv -f {result_tmp} {result}\n\
         }}\n\
         launch_until_healthy() {{\n\
         \tfor _ in 1 2 3 4; do\n\
         \t\t/usr/bin/open -g -j {app} || /usr/bin/open -g -j -n {app} || true\n\
         \t\tfor __ in 1 2; do\n\
         \t\t\tsleep 1\n\
         \t\t\t/usr/bin/pgrep -fx {executable} >/dev/null 2>&1 && return 0\n\
         \t\tdone\n\
         \tdone\n\
         \treturn 1\n\
         }}\n\
         wait_for_previous_exit() {{\n\
         \t# The old process holds the single-instance lock. Do not replace\n\
         \t# the bundle until it has really exited, or the new launch can\n\
         \t# be rejected as a duplicate and the health check can see the old\n\
         \t# process instead of the replacement.\n\
         \tfor _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40; do\n\
         \t\tif ! /usr/bin/pgrep -fx {executable} >/dev/null 2>&1; then\n\
         \t\t\treturn 0\n\
         \t\tfi\n\
         \t\tsleep 0.25\n\
         \tdone\n\
         \treturn 1\n\
         }}\n\
         fail_before_replace() {{\n\
         \twrite_result {failed}\n\
         \tlaunch_until_healthy || true\n\
         \texit 1\n\
         }}\n\
         rollback() {{\n\
         \t/usr/bin/pkill -fx {executable} >/dev/null 2>&1 || true\n\
         \t/bin/rm -rf {app}\n\
         \tif /bin/mv {backup} {app}; then\n\
         \t\twrite_result {rolled_back}\n\
         \t\tlaunch_until_healthy || true\n\
         \telse\n\
         \t\twrite_result {restore_failed}\n\
         \tfi\n\
         \texit 1\n\
         }}\n\
         sleep 0.5\n\
         wait_for_previous_exit || fail_before_replace\n\
         /bin/rm -rf {backup}\n\
         /bin/mv {app} {backup} || fail_before_replace\n\
         /usr/bin/ditto {new_app} {app} || rollback\n\
         /usr/bin/codesign --verify --deep --strict {app} || rollback\n\
         /usr/bin/xcrun stapler validate {app} >/dev/null 2>&1 || rollback\n\
         launch_until_healthy || rollback\n\
         write_result {installed}\n\
         /bin/rm -rf {backup}\n\
         /bin/rm -rf {tmp}\n",
        app = shell_quote(app_path),
        new_app = shell_quote(new_app),
        backup = shell_quote(backup_path),
        tmp = shell_quote(tmp_dir),
        log = shell_quote(log_path),
        result = shell_quote(result_path),
        result_tmp = shell_quote(&result_tmp),
        installed = shell_quote_str(&installed),
        rolled_back = shell_quote_str(&rolled_back),
        failed = shell_quote_str(&failed),
        restore_failed = shell_quote_str(&restore_failed),
        executable = shell_quote(&executable),
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
fn validate_update_app(app: &std::path::Path) -> Result<(), String> {
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

fn codesign_field(text: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    text.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(&prefix).map(ToString::to_string))
}

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

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_apply_script_records_outcome_and_preserves_existing_install_on_failure() {
        let script = build_windows_apply_update_script(
            std::path::Path::new(r"C:\Temp\PeterFan update\install.ps1"),
            std::path::Path::new(r"C:\Temp\PeterFan update"),
            std::path::Path::new(r"C:\Users\Test\AppData\Local\PeterFan\update-result.json"),
            "1.2.3",
        );

        assert!(script.contains("& 'C:\\Temp\\PeterFan update\\install.ps1'"));
        assert!(script.contains("Write-Result"));
        assert!(script.contains(r#""status":"installed""#));
        assert!(script.contains(r#""status":"failed""#));
        assert!(script.contains("Join-Path $env:LOCALAPPDATA 'Programs\\PeterFan\\PeterFan.exe'"));
        assert!(script.contains("Start-Process $existing"));
        assert!(script.contains("Remove-Item -LiteralPath 'C:\\Temp\\PeterFan update'"));
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
        );

        let health_check = script.rfind("launch_until_healthy || rollback").unwrap();
        let delete_backup = script
            .rfind("rm -rf '/tmp/update/PreviousPeterFan.app'")
            .unwrap();
        assert!(script.contains("codesign --verify --deep --strict"));
        assert!(script.contains("stapler validate"));
        assert!(script.contains("open -g -j"));
        assert!(script.contains("open -g -j -n"));
        assert!(script.contains("pgrep -fx"));
        assert!(script.contains("wait_for_previous_exit"));
        assert!(script.contains("wait_for_previous_exit || fail_before_replace"));
        assert!(script.contains("launch_until_healthy || rollback"));
        assert!(script.contains("write_result"));
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
