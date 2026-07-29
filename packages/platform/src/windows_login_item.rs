//! Per-user Windows startup registration.
//!
//! PeterFan uses the current user's `Run` registry key, so toggling startup
//! never needs administrator approval.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "PeterFan";

pub fn is_installed() -> bool {
    Command::new("reg.exe")
        .args(["query", RUN_KEY, "/v", VALUE_NAME])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

pub fn find_app_binary(override_path: Option<&str>) -> Result<PathBuf, String> {
    if let Some(path) = override_path {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!("binary not found at '{}'", path.display()));
    }

    let current = std::env::current_exe().map_err(|error| error.to_string())?;
    let file_name = current.file_name().and_then(|name| name.to_str());
    if matches!(file_name, Some("PeterFan.exe" | "peterfan-menubar.exe")) {
        return Ok(current);
    }

    let parent = current
        .parent()
        .ok_or_else(|| "current executable has no parent directory".to_string())?;
    for name in ["PeterFan.exe", "peterfan-menubar.exe"] {
        let candidate = parent.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err("could not find PeterFan.exe next to the current executable".to_string())
}

pub fn install(override_path: Option<&str>) -> Result<PathBuf, String> {
    let binary = find_app_binary(override_path)?;
    let command = quote_registry_command(&binary);
    let status = Command::new("reg.exe")
        .args([
            "add", RUN_KEY, "/v", VALUE_NAME, "/t", "REG_SZ", "/d", &command, "/f",
        ])
        .stdout(Stdio::null())
        .status()
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!("reg.exe could not add {VALUE_NAME} to the Run key"));
    }
    Ok(binary)
}

pub fn remove() -> Result<bool, String> {
    if !is_installed() {
        return Ok(false);
    }
    let status = Command::new("reg.exe")
        .args(["delete", RUN_KEY, "/v", VALUE_NAME, "/f"])
        .stdout(Stdio::null())
        .status()
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!(
            "reg.exe could not remove {VALUE_NAME} from the Run key"
        ));
    }
    Ok(true)
}

fn quote_registry_command(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_command_quotes_paths_with_spaces() {
        assert_eq!(
            quote_registry_command(Path::new(r"C:\Program Files\PeterFan\PeterFan.exe")),
            r#""C:\Program Files\PeterFan\PeterFan.exe""#
        );
    }
}
