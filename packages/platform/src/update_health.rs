//! Per-launch update readiness, separate from installation authority.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Receipt {
    pub nonce: String,
    pub pid: u32,
    pub version: String,
}

pub fn receipt_path(result_path: &Path, nonce: &str) -> Option<PathBuf> {
    if nonce.len() != 32
        || !nonce
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return None;
    }
    Some(
        result_path
            .parent()?
            .join(format!("update-health-{nonce}.json")),
    )
}

pub struct Pending {
    path: PathBuf,
    nonce: String,
    ready_since: Option<Instant>,
}

impl Pending {
    pub fn from_args(
        args: &[String],
        result_path: Option<&Path>,
        mock: bool,
    ) -> Result<Option<Self>, String> {
        let mut values = args
            .iter()
            .enumerate()
            .filter(|(_, a)| *a == "--update-health-token");
        let Some((index, _)) = values.next() else {
            return Ok(None);
        };
        if values.next().is_some() {
            return Err("duplicate update health token".into());
        }
        let nonce = args.get(index + 1).ok_or("missing update health token")?;
        let path = receipt_path(result_path.ok_or("no update receipt directory")?, nonce)
            .ok_or("invalid update health token")?;
        if mock {
            return Ok(None);
        }
        Ok(Some(Self {
            path,
            nonce: nonce.clone(),
            ready_since: None,
        }))
    }

    /// Return true only after publishing this launch's readiness receipt.
    pub fn poll(
        &mut self,
        now: Instant,
        tray: bool,
        webview: bool,
        version: &str,
    ) -> std::io::Result<bool> {
        if !tray || !webview {
            self.ready_since = None;
            return Ok(false);
        }
        let since = *self.ready_since.get_or_insert(now);
        if now.saturating_duration_since(since) < Duration::from_secs(5) {
            return Ok(false);
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| std::io::Error::other("no health receipt directory"))?;
        std::fs::create_dir_all(parent)?;
        match std::fs::symlink_metadata(&self.path) {
            Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
                return Err(std::io::Error::other(
                    "health receipt is not a regular file",
                ))
            }
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        let temporary = self
            .path
            .with_extension(format!("tmp-{}", std::process::id()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        let written = (|| {
            let receipt = Receipt {
                nonce: self.nonce.clone(),
                pid: std::process::id(),
                version: version.into(),
            };
            let bytes = serde_json::to_vec(&receipt)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, &self.path)
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        written.map(|_| true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const NONCE: &str = "1234567890abcdef1234567890abcdef";
    fn args(nonce: &str) -> Vec<String> {
        vec![
            "PeterFan".into(),
            "--update-health-token".into(),
            nonce.into(),
        ]
    }

    #[test]
    fn process_existence_alone_cannot_acknowledge_an_update() {
        let root =
            std::env::temp_dir().join(format!("peterfan-health-gate-{}", std::process::id()));
        let result = root.join("update-result.json");
        let mut request = Pending::from_args(&args(NONCE), Some(&result), false)
            .unwrap()
            .unwrap();
        let now = Instant::now();
        assert!(!request.poll(now, true, false, "1.2.3").unwrap());
        assert!(!request
            .poll(now + Duration::from_secs(30), true, false, "1.2.3")
            .unwrap());
        assert!(!request.path.exists());
        assert!(!request
            .poll(now + Duration::from_secs(31), true, true, "1.2.3")
            .unwrap());
        assert!(!request
            .poll(now + Duration::from_secs(35), true, true, "1.2.3")
            .unwrap());
        assert!(request
            .poll(now + Duration::from_secs(36), true, true, "1.2.3")
            .unwrap());
        let receipt: Receipt =
            serde_json::from_slice(&std::fs::read(&request.path).unwrap()).unwrap();
        assert_eq!(receipt.nonce, NONCE);
        assert_eq!(receipt.pid, std::process::id());
        assert_eq!(receipt.version, "1.2.3");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_duplicate_and_mock_requests_never_publish() {
        let result = Path::new("/not-opened/update-result.json");
        for bad in ["", "../escape", "A234567890abcdef1234567890abcdef", "123"] {
            assert!(Pending::from_args(&args(bad), Some(result), false).is_err());
        }
        let mut duplicate = args(NONCE);
        duplicate.extend(args(NONCE).into_iter().skip(1));
        assert!(Pending::from_args(&duplicate, Some(result), false).is_err());
        assert!(Pending::from_args(&args(NONCE), Some(result), true)
            .unwrap()
            .is_none());
        assert!(Pending::from_args(&["PeterFan".into()], None, false)
            .unwrap()
            .is_none());
    }

    #[test]
    fn readiness_loss_restarts_the_survival_window_and_write_failure_is_visible() {
        let root =
            std::env::temp_dir().join(format!("peterfan-health-failure-{}", std::process::id()));
        std::fs::write(&root, b"blocked parent").unwrap();
        let result = root.join("update-result.json");
        let mut request = Pending::from_args(&args(NONCE), Some(&result), false)
            .unwrap()
            .unwrap();
        let now = Instant::now();
        assert!(!request.poll(now, true, true, "1.2.3").unwrap());
        assert!(!request
            .poll(now + Duration::from_secs(4), true, false, "1.2.3")
            .unwrap());
        assert!(!request
            .poll(now + Duration::from_secs(5), true, true, "1.2.3")
            .unwrap());
        assert!(!request
            .poll(now + Duration::from_secs(9), true, true, "1.2.3")
            .unwrap());
        assert!(request
            .poll(now + Duration::from_secs(10), true, true, "1.2.3")
            .is_err());
        std::fs::remove_file(root).unwrap();
    }
}
