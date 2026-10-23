//! Bounded update-check history; never release or scheduling authority.
use peterfan_core::config::UpdateChannel;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_BYTES: usize = 16 * 1024;
const MAX_ERROR_BYTES: usize = 1024;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateHistory {
    schema_version: u32,
    pub channel: UpdateChannel,
    pub last_attempt_unix: Option<u64>,
    pub last_success_unix: Option<u64>,
    pub check_error: Option<String>,
}

impl UpdateHistory {
    pub fn capture(
        channel: UpdateChannel,
        last_attempt_unix: Option<u64>,
        last_success_unix: Option<u64>,
        check_error: Option<String>,
    ) -> Self {
        let check_error = check_error.map(|mut error| {
            let mut end = error.len().min(MAX_ERROR_BYTES);
            while !error.is_char_boundary(end) {
                end -= 1;
            }
            error.truncate(end);
            error
        });
        Self {
            schema_version: 1,
            channel,
            last_attempt_unix,
            last_success_unix,
            check_error,
        }
    }
}

pub fn load(
    path: &Path,
    channel: UpdateChannel,
    now: u64,
    enabled: bool,
) -> io::Result<Option<UpdateHistory>> {
    if !enabled {
        return Ok(None);
    }
    // Inspect the path itself before open: a FIFO can block during open, and
    // symlinks must not redirect history reads to an unrelated file.
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    }
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !file.metadata()?.is_file() {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES {
        return Ok(None);
    }
    let Ok(history) = serde_json::from_slice::<UpdateHistory>(&bytes) else {
        return Ok(None);
    };
    let valid_time = |time: Option<u64>| time.is_none_or(|time| time > 0 && time <= now);
    if history.schema_version != 1
        || history.channel != channel
        || !valid_time(history.last_attempt_unix)
        || !valid_time(history.last_success_unix)
        || history
            .check_error
            .as_ref()
            .is_some_and(|error| error.len() > MAX_ERROR_BYTES)
    {
        return Ok(None);
    }
    Ok(Some(history))
}

/// Remove only the temporary file this save successfully created, even on error.
struct OwnedTemp(Option<PathBuf>);
impl Drop for OwnedTemp {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

pub fn save(path: &Path, history: &UpdateHistory, enabled: bool) -> io::Result<()> {
    if !enabled {
        return Ok(());
    }
    let bytes = serde_json::to_vec(history)?;
    if bytes.len() > MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "update history exceeds limit",
        ));
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing history directory"))?;
    fs::create_dir_all(parent)?;
    let (mut temp, mut file) = loop {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temp = parent.join(format!(
            ".update-check-history-{}-{nonce}-{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&temp) {
            Ok(file) => break (OwnedTemp(Some(temp)), file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp.0.as_ref().expect("owned temporary file"), path)?;
    temp.0 = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "peterfan-history-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> PathBuf {
            self.0.join("update-check-history.json")
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn history() -> UpdateHistory {
        UpdateHistory::capture(
            UpdateChannel::Stable,
            Some(90),
            Some(80),
            Some("offline".into()),
        )
    }

    #[test]
    fn disk_roundtrip_and_overwrite_preserve_only_history() {
        let temp = Temp::new();
        save(&temp.path(), &history(), true).unwrap();
        let restored = load(&temp.path(), UpdateChannel::Stable, 100, true)
            .unwrap()
            .unwrap();
        assert_eq!(restored.last_attempt_unix, Some(90));
        assert_eq!(restored.last_success_unix, Some(80));
        assert_eq!(restored.check_error.as_deref(), Some("offline"));
        let attempt = UpdateHistory::capture(UpdateChannel::Stable, Some(99), Some(80), None);
        save(&temp.path(), &attempt, true).unwrap();
        assert_eq!(
            load(&temp.path(), UpdateChannel::Stable, 100, true)
                .unwrap()
                .unwrap()
                .last_attempt_unix,
            Some(99)
        );
        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(temp.path()).unwrap()).unwrap();
        assert_eq!(json.as_object().unwrap().len(), 5);
        assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(temp.path()).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn interrupted_attempt_on_disk_restores_history_but_still_checks_in_twenty_seconds() {
        let temp = Temp::new();
        let attempt = UpdateHistory::capture(UpdateChannel::Stable, Some(99), Some(80), None);
        save(&temp.path(), &attempt, true).unwrap();
        let restored = load(&temp.path(), UpdateChannel::Stable, 100, true).unwrap();
        let mut state = crate::APP_UPDATE_STATE.lock().unwrap().clone();
        state.phase = "available";
        state.latest = Some("9.0.0".into());
        state.install_ready = true;
        crate::initialize_update_discovery(&mut state, UpdateChannel::Stable, 100, restored);
        assert_eq!(state.schedule.last_attempt_unix, Some(99));
        assert_eq!(state.schedule.next_check_unix, Some(120));
        assert_eq!(state.phase, "idle");
        assert!(state.latest.is_none());
        assert!(!state.install_ready);
        state.schedule.begin(120);
        state.schedule.finish(121, true);
        assert_eq!(
            state.schedule.next_check_unix,
            Some(121 + crate::update_schedule::CHECK_INTERVAL)
        );
    }

    #[test]
    fn malformed_unsupported_channel_future_and_oversized_are_ignored() {
        let temp = Temp::new();
        assert!(load(&temp.path(), UpdateChannel::Stable, 100, true)
            .unwrap()
            .is_none());
        for invalid in ["{broken".into(), " ".repeat(MAX_BYTES + 1),
            r#"{"schema_version":2,"channel":"stable","last_attempt_unix":90,"last_success_unix":80,"check_error":null}"#.into(),
            r#"{"schema_version":1,"channel":"pre-release","last_attempt_unix":90,"last_success_unix":80,"check_error":null}"#.into(),
            r#"{"schema_version":1,"channel":"stable","last_attempt_unix":101,"last_success_unix":80,"check_error":null}"#.into(),
            r#"{"schema_version":1,"channel":"stable","last_attempt_unix":90,"last_success_unix":101,"check_error":null}"#.into(),
            r#"{"schema_version":1,"channel":"stable","last_attempt_unix":90,"last_success_unix":80,"check_error":null,"phase":"current"}"#.into(),
        ] {
            fs::write(temp.path(), invalid).unwrap();
            assert!(load(&temp.path(), UpdateChannel::Stable, 100, true).unwrap().is_none());
        }
    }

    #[test]
    fn failed_replace_cleans_owned_temp_preserves_other_files() {
        let temp = Temp::new();
        fs::create_dir(temp.path()).unwrap();
        fs::write(temp.path().join("sentinel"), "original").unwrap();
        fs::write(temp.0.join("unrelated.tmp"), "keep").unwrap();
        assert!(save(&temp.path(), &history(), true).is_err());
        assert_eq!(
            fs::read_to_string(temp.path().join("sentinel")).unwrap(),
            "original"
        );
        assert_eq!(
            fs::read_to_string(temp.0.join("unrelated.tmp")).unwrap(),
            "keep"
        );
        assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 2);
        assert!(save(&temp.0.join("unrelated.tmp/blocked"), &history(), true).is_err());
    }

    #[test]
    fn mock_neither_reads_invalid_path_nor_writes_existing_history() {
        let temp = Temp::new();
        fs::create_dir(temp.path()).unwrap();
        assert!(load(&temp.path(), UpdateChannel::Stable, 100, false)
            .unwrap()
            .is_none());
        assert!(save(&temp.path(), &history(), false).is_ok());
        assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 1);
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
        assert!(save(&temp.0.join("absent/history"), &history(), false).is_ok());
        assert!(!temp.0.join("absent").exists());
    }

    #[test]
    fn directory_and_symlink_history_paths_are_ignored() {
        let temp = Temp::new();
        fs::create_dir(temp.path()).unwrap();
        assert!(load(&temp.path(), UpdateChannel::Stable, 100, true)
            .unwrap()
            .is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            fs::remove_dir(temp.path()).unwrap();
            let sentinel = temp.0.join("sentinel.json");
            save(&sentinel, &history(), true).unwrap();
            let original = fs::read(&sentinel).unwrap();
            symlink(&sentinel, temp.path()).unwrap();
            assert!(load(&temp.path(), UpdateChannel::Stable, 100, true)
                .unwrap()
                .is_none());
            assert_eq!(fs::read(&sentinel).unwrap(), original);
        }
    }

    #[cfg(unix)]
    #[test]
    fn fifo_history_is_rejected_without_waiting_for_a_writer() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::OpenOptionsExt;
        use std::sync::mpsc;
        use std::time::Duration;
        let temp = Temp::new();
        let path = temp.path();
        let fifo_path = CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: fifo_path is a valid NUL-terminated path owned by this test.
        assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) }, 0);
        let (tx, rx) = mpsc::channel();
        let reader_path = path.clone();
        let reader = std::thread::spawn(move || {
            let result = load(&reader_path, UpdateChannel::Stable, 100, true);
            tx.send(result.map(|record| record.is_none())).unwrap();
        });
        let early = rx.recv_timeout(Duration::from_secs(2));
        if early.is_err() {
            // Unblock an incorrect blocking open before joining, so RED exits.
            let writer = OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&path);
            assert!(writer.is_ok(), "failed to unblock FIFO reader");
            drop(writer);
        }
        reader.join().unwrap();
        assert!(early.is_ok(), "history load waited for a FIFO writer");
        assert!(early.unwrap().unwrap());
    }

    #[test]
    fn error_capture_is_utf8_bounded_and_roundtrips() {
        let temp = Temp::new();
        let history = UpdateHistory::capture(
            UpdateChannel::Stable,
            Some(90),
            None,
            Some("실패".repeat(10000)),
        );
        assert!(history.check_error.as_ref().unwrap().len() <= MAX_ERROR_BYTES);
        save(&temp.path(), &history, true).unwrap();
        assert!(fs::metadata(temp.path()).unwrap().len() <= MAX_BYTES as u64);
        assert!(load(&temp.path(), UpdateChannel::Stable, 100, true)
            .unwrap()
            .is_some());
    }
}
