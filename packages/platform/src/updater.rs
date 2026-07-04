//! Check GitHub Releases for a newer version, and (macOS) download + install
//! it in place. Shared by `peterfan update` and the menu-bar app's automatic
//! update check.
//!
//! Shells out to `curl`/`tar` rather than pulling in an HTTP client crate —
//! consistent with how the rest of the codebase talks to `osascript`/
//! `launchctl`, and keeps the menu-bar binary's dependency footprint small.

pub const REPO: &str = "uulab-official/peterfan";
pub const EXPECTED_BUNDLE_ID: &str = "kr.co.uulab.peterfan";
pub const EXPECTED_TEAM_ID: &str = "N99FMBQ662";

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
    /// GitHub release asset digest for the selected update asset, currently
    /// shaped like `sha256:<hex>` when present.
    pub asset_digest: Option<String>,
    pub archive_url: Option<String>,
    pub dmg_url: Option<String>,
    /// Direct download URL for `checksums.txt`, used to verify the selected
    /// update asset before extraction.
    pub checksum_url: Option<String>,
    pub checksum_name: Option<String>,
    pub checksum_digest: Option<String>,
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
    let out = std::process::Command::new("curl")
        .args([
            "-s",
            "--max-time",
            "8",
            "-H",
            "User-Agent: peterfan-updater",
            url,
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
    let checksums = find_asset(assets, is_checksum_asset);
    let preferred = dmg.as_ref().or(archive.as_ref());
    Ok(ReleaseInfo {
        version,
        tag,
        html_url,
        asset_url: preferred.map(|a| a.url.clone()),
        asset_name: preferred.map(|a| a.name.clone()),
        asset_digest: preferred.and_then(|a| a.digest.clone()),
        archive_url: archive.map(|a| a.url),
        dmg_url: dmg.map(|a| a.url),
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
    let digest = digest?;
    let hash = digest.strip_prefix("sha256:")?;
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
        Ok(output) if spctl_failed_from_resource_exhaustion(&output) => {
            report.push(
                "Gatekeeper",
                true,
                "spctl skipped because the system reported too many open files",
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
        release.asset_digest.as_deref(),
        checksum_url,
    )
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
    download_and_install_unchecked(asset_url, asset_name)
}

#[cfg(target_os = "macos")]
fn download_and_install_verified(
    asset_url: &str,
    asset_name: &str,
    asset_digest: Option<&str>,
    checksum_url: &str,
) -> Result<(), String> {
    let app_path = current_app_bundle()?;
    let tmp_dir = std::env::temp_dir().join(format!("peterfan-update-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;

    let download = tmp_dir.join(asset_name);
    download_file(asset_url, &download)?;
    if let Some(expected) = asset_digest {
        verify_expected_sha256("GitHub asset digest", expected, &download)?;
    }

    let checksums_path = tmp_dir.join("checksums.txt");
    download_file(checksum_url, &checksums_path)?;
    let checksums = std::fs::read_to_string(&checksums_path).map_err(|e| e.to_string())?;
    verify_download_checksum(&checksums, asset_name, &download)?;

    install_downloaded_update(&app_path, &tmp_dir, &download, asset_name)
}

#[cfg(target_os = "macos")]
fn download_and_install_unchecked(asset_url: &str, asset_name: &str) -> Result<(), String> {
    let app_path = current_app_bundle()?;
    let tmp_dir = std::env::temp_dir().join(format!("peterfan-update-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;
    let download = tmp_dir.join(asset_name);
    download_file(asset_url, &download)?;
    install_downloaded_update(&app_path, &tmp_dir, &download, asset_name)
}

#[cfg(target_os = "macos")]
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
    let script = format!(
        "#!/bin/bash\n\
         set -e\n\
         exec >{log} 2>&1\n\
         sleep 1\n\
         rm -rf {backup}\n\
         mv {app} {backup}\n\
         if ditto {new_app} {app}; then\n\
         \txcrun stapler validate {app} >/dev/null 2>&1 || true\n\
         \topen -g {app}\n\
         \trm -rf {tmp}\n\
         else\n\
         \trm -rf {app}\n\
         \tmv {backup} {app}\n\
         \topen -g {app}\n\
         \texit 1\n\
         fi\n",
        app = shell_quote(app_path),
        new_app = shell_quote(&new_app),
        backup = shell_quote(&backup_path),
        tmp = shell_quote(tmp_dir),
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
fn verify_download_checksum(
    checksums: &str,
    asset_name: &str,
    path: &std::path::Path,
) -> Result<(), String> {
    let expected = checksum_for_asset(checksums, asset_name)
        .ok_or_else(|| format!("checksums.txt does not list {asset_name}"))?;
    verify_expected_sha256("checksums.txt", &expected, path)
}

#[cfg(target_os = "macos")]
fn verify_expected_sha256(
    source: &str,
    expected: &str,
    path: &std::path::Path,
) -> Result<(), String> {
    let actual = sha256_file(path)?;
    if actual.eq_ignore_ascii_case(expected) {
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
    if !output.status.success() && !spctl_failed_from_resource_exhaustion(&output) {
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
    if !output.status.success() && !spctl_failed_from_resource_exhaustion(&output) {
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
fn spctl_failed_from_resource_exhaustion(output: &std::process::Output) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    stderr.contains("Too many open files") || stdout.contains("Too many open files")
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
        assert!(info.asset_url.unwrap().contains("aarch64-apple-darwin"));
        assert!(info.asset_name.unwrap().contains("aarch64-apple-darwin"));
        assert_eq!(
            info.asset_digest.as_deref(),
            Some("abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd")
        );
        assert_eq!(info.checksum_name.as_deref(), Some("checksums.txt"));
        assert!(info.checksum_url.unwrap().ends_with("/checksums.txt"));
        assert_eq!(
            info.checksum_digest.as_deref(),
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
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
}
