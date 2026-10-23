//! Config file location and IO (the OS-specific half of configuration).
//!
//! The [`Config`] type lives in the core; here we
//! resolve its path (`~/.config/peterfan/config.toml` and the platform
//! equivalents, via `dirs`) and read/write it.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use peterfan_core::config::Config;

/// The config file path, e.g. `~/.config/peterfan/config.toml`.
pub fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("peterfan").join("config.toml"))
}

/// Load the config, falling back to defaults if it's missing or unparseable.
pub fn load() -> Config {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| Config::from_toml(&s).ok())
        .unwrap_or_default()
}

/// Edit the latest config under a cross-process lock and atomically save it.
///
/// Only a missing file starts from defaults. Unreadable or malformed config
/// files and errors from `edit` leave the existing file untouched. Cooperating
/// writers are serialized; external editors must also honor the sidecar lock
/// to participate. Lock acquisition times out after approximately 500 ms.
pub fn update<F>(edit: F) -> io::Result<PathBuf>
where
    F: FnOnce(&mut Config) -> io::Result<()>,
{
    let p = config_path()?;
    update_at(&p, edit)?;
    Ok(p)
}

fn update_at(p: &Path, edit: impl FnOnce(&mut Config) -> io::Result<()>) -> io::Result<()> {
    with_locked_path(p, |target| {
        let mut cfg = read_strict(target)?;
        edit(&mut cfg)?;
        atomic_write(target, &cfg.to_toml())
    })
}

/// Create the config file with defaults if it doesn't exist; return its path.
pub fn init_default() -> io::Result<PathBuf> {
    let p = config_path()?;
    init_default_at(&p)?;
    Ok(p)
}

fn init_default_at(p: &Path) -> io::Result<()> {
    with_locked_path(p, |target| match regular_metadata(target)? {
        Some(_) => Ok(()),
        None => atomic_write(target, &Config::default().to_toml()),
    })
}

/// Atomically replace the entire config, creating it if needed.
///
/// This is the explicit replacement/import operation. For partial changes use
/// [`update`] so concurrent edits to unrelated settings are retained.
pub fn save(cfg: &Config) -> io::Result<PathBuf> {
    let p = config_path()?;
    save_at(&p, cfg)?;
    Ok(p)
}

fn save_at(p: &Path, cfg: &Config) -> io::Result<()> {
    with_locked_path(p, |target| atomic_write(target, &cfg.to_toml()))
}

fn config_path() -> io::Result<PathBuf> {
    path().ok_or_else(|| io::Error::other("no config directory"))
}

fn with_locked_path(p: &Path, action: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<()> {
    let target = resolve_target(p)?;
    let _lock = ConfigLock::acquire(&target)?;
    action(&target)
}

// Resolve symlinks before selecting the lock as well as the destination. An
// existing symlink is never replaced, and dangling links fail clearly. Resolve
// the parent of an absent file too, so aliases use the same stable lock file.
fn resolve_target(p: &Path) -> io::Result<PathBuf> {
    match fs::symlink_metadata(p) {
        Ok(_) => {
            let target = p.canonicalize()?;
            regular_metadata(&target)?;
            Ok(target)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let parent = p
                .parent()
                .ok_or_else(|| io::Error::other("config has no parent"))?;
            fs::create_dir_all(parent)?;
            Ok(parent.canonicalize()?.join(
                p.file_name()
                    .ok_or_else(|| io::Error::other("config has no file name"))?,
            ))
        }
        Err(e) => Err(e),
    }
}

fn regular_metadata(target: &Path) -> io::Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.is_file() => Ok(Some(metadata)),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("config target is not a regular file: {}", target.display()),
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn read_strict(target: &Path) -> io::Result<Config> {
    regular_metadata(target)?;
    match fs::read_to_string(target) {
        Ok(text) => Config::from_toml(&text).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("cannot edit malformed config {}: {e}", target.display()),
            )
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e),
    }
}

fn sidecar(target: &Path) -> PathBuf {
    let mut name = target
        .file_name()
        .expect("resolved config file name")
        .to_os_string();
    name.push(".lock");
    target.with_file_name(name)
}

struct ConfigLock(File);

impl ConfigLock {
    fn acquire(target: &Path) -> io::Result<Self> {
        let lock_path = sidecar(target);
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
            options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
        let file = options.open(&lock_path)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "config lock is not a regular file",
            ));
        }
        let started = Instant::now();
        loop {
            match try_lock(&file) {
                Ok(()) => return Ok(Self(file)),
                Err(e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::Interrupted =>
                {
                    let Some(remaining) = Duration::from_millis(500).checked_sub(started.elapsed())
                    else {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            format!(
                                "config is busy; timed out waiting for {}",
                                lock_path.display()
                            ),
                        ));
                    };
                    std::thread::sleep(remaining.min(Duration::from_millis(10)));
                }
                Err(e) => return Err(e),
            }
        }
    }
}

impl Drop for ConfigLock {
    fn drop(&mut self) {
        unlock(&self.0);
        // Never unlink this sidecar: other processes may be waiting on its
        // inode. The OS also releases the lock if this process exits/crashes.
    }
}

#[cfg(unix)]
fn try_lock(file: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    // SAFETY: the file owns a live descriptor for the duration of this call.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn unlock(file: &File) {
    use std::os::fd::AsRawFd;
    // SAFETY: the file owns a live descriptor. Closing is a fallback unlock.
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
}

#[cfg(windows)]
fn try_lock(file: &File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::ERROR_LOCK_VIOLATION;
    use windows_sys::Win32::Storage::FileSystem::{
        LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
    };
    use windows_sys::Win32::System::IO::OVERLAPPED;
    let mut overlapped = OVERLAPPED::default();
    // SAFETY: live synchronous handle and initialized offset-zero OVERLAPPED.
    // FAIL_IMMEDIATELY prevents a pending request retaining this stack pointer.
    if unsafe {
        LockFileEx(
            file.as_raw_handle(),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            1,
            0,
            &mut overlapped,
        )
    } != 0
    {
        Ok(())
    } else {
        let e = io::Error::last_os_error();
        if e.raw_os_error() == Some(ERROR_LOCK_VIOLATION as i32) {
            Err(io::Error::new(io::ErrorKind::WouldBlock, e))
        } else {
            Err(e)
        }
    }
}

#[cfg(windows)]
fn unlock(file: &File) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::UnlockFileEx;
    use windows_sys::Win32::System::IO::OVERLAPPED;
    let mut overlapped = OVERLAPPED::default();
    // SAFETY: same live handle, offset and range as the successful LockFileEx.
    unsafe { UnlockFileEx(file.as_raw_handle(), 0, 1, 0, &mut overlapped) };
}

#[cfg(not(any(unix, windows)))]
fn try_lock(_: &File) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "config locking is unsupported on this platform",
    ))
}

#[cfg(not(any(unix, windows)))]
fn unlock(_: &File) {}

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempConfig {
    path: PathBuf,
    file: Option<File>,
    cleanup: bool,
}

impl TempConfig {
    fn create(target: &Path) -> io::Result<Self> {
        for _ in 0..128 {
            let mut name = target
                .file_name()
                .expect("resolved config file name")
                .to_os_string();
            name.push(format!(
                ".{}.{}.tmp",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            let path = target.with_file_name(name);
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        file: Some(file),
                        cleanup: true,
                    })
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "cannot allocate a unique config temporary file",
        ))
    }
}

impl Drop for TempConfig {
    fn drop(&mut self) {
        self.file.take();
        // The path was allocated with create_new, so cleanup is only ours.
        if self.cleanup {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn atomic_write(target: &Path, text: &str) -> io::Result<()> {
    atomic_write_with(target, |file| file.write_all(text.as_bytes()))
}

fn atomic_write_with(
    target: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    let permissions = regular_metadata(target)?.map(|metadata| metadata.permissions());
    if let Some(permissions) = &permissions {
        if permissions.readonly() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "config file is read-only",
            ));
        }
        // Rename permission on the directory must not bypass file-level write
        // permissions/ACLs. Opening without truncate leaves its bytes intact.
        OpenOptions::new().write(true).open(target)?;
    }
    let mut temp = TempConfig::create(target)?;
    let file = temp.file.as_mut().expect("open temporary config");
    write(file)?;
    if let Some(permissions) = permissions {
        file.set_permissions(permissions)?;
    }
    file.sync_all()?;
    temp.file.take();
    // std::fs::rename replaces an existing destination on Unix and Windows.
    // All fallible preparation finishes before this atomic commit point.
    fs::rename(&temp.path, target)?;
    temp.cleanup = false;
    // Directory fsync is best-effort: some platforms/filesystems do not support
    // it, and a post-commit error must not suggest the old bytes were retained.
    #[cfg(unix)]
    if let Some(parent) = target.parent() {
        let _ = File::open(parent).and_then(|dir| dir.sync_all());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "peterfan-config-{}-{}",
                std::process::id(),
                NEXT_TEST.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }

        fn config(&self) -> PathBuf {
            self.0.join("config.toml")
        }

        fn write(&self, bytes: &[u8]) {
            fs::create_dir_all(self.config().parent().unwrap()).unwrap();
            fs::write(self.config(), bytes).unwrap();
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn child(root: &Path, role: &str) -> Command {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "config::tests::transaction_child", "--nocapture"])
            .env("PETERFAN_CONFIG_TEST_PATH", root.join("config.toml"))
            .env("PETERFAN_CONFIG_TEST_ROLE", role);
        cmd
    }

    // Owned subprocesses are killed even when an assertion fails. Only a
    // task-specific explicit config path is passed; no real home is redirected.
    struct TestChild(Option<Child>);

    impl TestChild {
        fn spawn(root: &Path, role: &str) -> Self {
            Self(Some(
                child(root, role).stdout(Stdio::null()).spawn().unwrap(),
            ))
        }

        fn wait(mut self) {
            let status = self.0.as_mut().unwrap().wait().unwrap();
            self.0.take();
            assert!(status.success(), "config child failed: {status}");
        }
    }

    impl Drop for TestChild {
        fn drop(&mut self) {
            if let Some(child) = &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    fn wait_for(marker: &Path) {
        let started = Instant::now();
        while !marker.exists() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "timed out waiting for {}",
                marker.display()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn assert_no_temps(dir: &Path) {
        for entry in fs::read_dir(dir).unwrap() {
            assert!(!entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp"));
        }
    }

    #[test]
    fn corrupt_config_transaction_preserves_original_bytes() {
        let dir = TestDir::new();
        let bytes = b"interval_secs = [broken\n";
        dir.write(bytes);
        let output = child(&dir.0, "reject-corrupt").output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read(dir.config()).unwrap(), bytes);
    }

    #[test]
    fn failed_partial_write_preserves_bytes_and_cleans_only_owned_temp() {
        let dir = TestDir::new();
        let bytes = b"interval_secs = 7\n# retain even comments after failure\n";
        dir.write(bytes);
        let unrelated = dir.0.join("unrelated.tmp");
        fs::write(&unrelated, b"keep").unwrap();
        let error = with_locked_path(&dir.config(), |target| {
            atomic_write_with(target, |file| {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    assert_eq!(file.metadata()?.permissions().mode() & 0o777, 0o600);
                }
                file.write_all(b"incomplete")?;
                Err(io::Error::other("injected write failure"))
            })
        })
        .unwrap_err();
        assert!(error.to_string().contains("injected write failure"));
        assert_eq!(fs::read(dir.config()).unwrap(), bytes);
        assert_eq!(fs::read(&unrelated).unwrap(), b"keep");
        fs::remove_file(unrelated).unwrap();
        assert_no_temps(&dir.0);
    }

    #[test]
    fn edit_error_preserves_bytes_and_releases_lock() {
        let dir = TestDir::new();
        let bytes = b"interval_secs = 7\n# original\n";
        dir.write(bytes);
        let error = update_at(&dir.config(), |cfg| {
            cfg.interval_secs = 99;
            Err(io::Error::new(io::ErrorKind::InvalidInput, "edit rejected"))
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(fs::read(dir.config()).unwrap(), bytes);
        update_at(&dir.config(), |cfg| {
            cfg.interval_secs = 11;
            Ok(())
        })
        .unwrap();
        assert_eq!(read_strict(&dir.config()).unwrap().interval_secs, 11);
        assert_no_temps(&dir.0);
    }

    #[test]
    fn invalid_utf8_is_not_treated_as_missing_config() {
        let dir = TestDir::new();
        let bytes = b"interval_secs = 7\n\xff";
        dir.write(bytes);
        let mut edited = false;
        let error = update_at(&dir.config(), |_| {
            edited = true;
            Ok(())
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(!edited);
        assert_eq!(fs::read(dir.config()).unwrap(), bytes);
    }

    #[test]
    fn replacement_overwrites_existing_file_and_preserves_permissions() {
        let dir = TestDir::new();
        dir.write(b"malformed = [\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dir.config(), fs::Permissions::from_mode(0o640)).unwrap();
        }
        let cfg = Config {
            interval_secs: 19,
            ..Config::default()
        };
        save_at(&dir.config(), &cfg).unwrap();
        assert_eq!(read_strict(&dir.config()).unwrap().interval_secs, 19);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(dir.config()).unwrap().permissions().mode() & 0o777,
                0o640
            );
        }
        // Exercise a second overwrite, also on Windows runners.
        save_at(&dir.config(), &Config::default()).unwrap();
        assert_eq!(read_strict(&dir.config()).unwrap().interval_secs, 2);
        assert_no_temps(&dir.0);
    }

    #[test]
    fn missing_config_starts_from_defaults_and_is_private() {
        let dir = TestDir::new();
        let nested = dir.0.join("nested/config.toml");
        update_at(&nested, |cfg| {
            assert_eq!(cfg.interval_secs, 2);
            cfg.interval_secs = 13;
            Ok(())
        })
        .unwrap();
        assert_eq!(read_strict(&nested).unwrap().interval_secs, 13);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&nested).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(sidecar(&nested)).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_no_temps(nested.parent().unwrap());
    }

    #[test]
    fn readonly_config_is_preserved_by_update_save_and_init() {
        let dir = TestDir::new();
        let bytes = b"interval_secs = 7\n";
        dir.write(bytes);
        let permissions = fs::metadata(dir.config()).unwrap().permissions();
        let mut readonly = permissions.clone();
        readonly.set_readonly(true);
        fs::set_permissions(dir.config(), readonly).unwrap();
        let edit_error = update_at(&dir.config(), |cfg| {
            cfg.interval_secs = 8;
            Ok(())
        })
        .unwrap_err();
        let save_error = save_at(&dir.config(), &Config::default()).unwrap_err();
        init_default_at(&dir.config()).unwrap();
        assert_eq!(edit_error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(save_error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(fs::read(dir.config()).unwrap(), bytes);
        assert!(fs::metadata(dir.config()).unwrap().permissions().readonly());
        fs::set_permissions(dir.config(), permissions).unwrap();
        assert_no_temps(&dir.0);
    }

    #[test]
    fn nonregular_target_is_rejected_without_touching_its_contents() {
        let dir = TestDir::new();
        fs::create_dir(dir.config()).unwrap();
        fs::write(dir.config().join("keep"), b"keep").unwrap();
        for error in [
            update_at(&dir.config(), |_| Ok(())).unwrap_err(),
            save_at(&dir.config(), &Config::default()).unwrap_err(),
            init_default_at(&dir.config()).unwrap_err(),
        ] {
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        }
        assert_eq!(fs::read(dir.config().join("keep")).unwrap(), b"keep");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_target_is_updated_without_replacing_link_and_shares_lock() {
        use std::os::unix::fs::symlink;
        let dir = TestDir::new();
        let target_dir = TestDir::new();
        target_dir.write(b"interval_secs = 7\n");
        symlink(target_dir.config(), dir.config()).unwrap();
        update_at(&dir.config(), |cfg| {
            cfg.interval_secs = 23;
            Ok(())
        })
        .unwrap();
        assert!(fs::symlink_metadata(dir.config())
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(read_strict(&target_dir.config()).unwrap().interval_secs, 23);
        assert!(sidecar(&target_dir.config()).exists());
        assert!(!sidecar(&dir.config()).exists());
        let lock = ConfigLock::acquire(&target_dir.config()).unwrap();
        assert_eq!(
            update_at(&dir.config(), |_| Ok(())).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        drop(lock);
        save_at(&dir.config(), &Config::default()).unwrap();
        assert!(fs::symlink_metadata(dir.config())
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(read_strict(&target_dir.config()).unwrap().interval_secs, 2);
        assert_no_temps(&dir.0);
        assert_no_temps(&target_dir.0);
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlink_is_preserved_and_rejected() {
        use std::os::unix::fs::symlink;
        let dir = TestDir::new();
        let missing = dir.0.join("missing.toml");
        symlink(&missing, dir.config()).unwrap();
        assert_eq!(
            update_at(&dir.config(), |_| Ok(())).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert!(fs::symlink_metadata(dir.config())
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(!missing.exists());
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_config_fails_instead_of_loading_defaults() {
        use std::os::unix::fs::PermissionsExt;
        // A privileged root process can read mode 000; it cannot model this.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let dir = TestDir::new();
        let bytes = b"interval_secs = 7\n";
        dir.write(bytes);
        fs::set_permissions(dir.config(), fs::Permissions::from_mode(0o000)).unwrap();
        let mut edited = false;
        let error = update_at(&dir.config(), |_| {
            edited = true;
            Ok(())
        })
        .unwrap_err();
        fs::set_permissions(dir.config(), fs::Permissions::from_mode(0o600)).unwrap();
        assert!(!edited);
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(fs::read(dir.config()).unwrap(), bytes);
    }

    #[test]
    fn concurrent_disjoint_edits_from_real_processes_are_both_retained() {
        let dir = TestDir::new();
        dir.write(b"interval_secs = 7\ncritical_temp_c = 88.0\n");
        let first = TestChild::spawn(&dir.0, "edit-interval");
        wait_for(&dir.0.join("first-edit"));
        let second = TestChild::spawn(&dir.0, "edit-critical");
        wait_for(&dir.0.join("second-attempting"));
        // Keep the first transaction open across the second process's read.
        // An unlocked read/edit/write loses the second process's change here.
        std::thread::sleep(Duration::from_millis(40));
        fs::write(dir.0.join("release-first"), []).unwrap();
        first.wait();
        second.wait();
        let cfg = read_strict(&dir.config()).unwrap();
        assert_eq!(cfg.interval_secs, 17);
        assert_eq!(cfg.critical_temp_c, 82.5);
        assert_no_temps(&dir.0);
    }

    #[test]
    fn init_waits_for_pending_creation_and_does_not_clobber_it() {
        let dir = TestDir::new();
        let target = resolve_target(&dir.config()).unwrap();
        let lock = ConfigLock::acquire(&target).unwrap();
        let initializer = TestChild::spawn(&dir.0, "init");
        wait_for(&dir.0.join("init-attempting"));
        std::thread::sleep(Duration::from_millis(40));
        assert!(
            !dir.config().exists(),
            "init wrote while another transaction held the lock"
        );
        atomic_write(&target, "interval_secs = 29\n# keep this file\n").unwrap();
        drop(lock);
        initializer.wait();
        assert_eq!(
            fs::read(dir.config()).unwrap(),
            b"interval_secs = 29\n# keep this file\n"
        );
    }

    #[test]
    fn process_lock_contention_is_bounded_and_crash_releases_lock() {
        let dir = TestDir::new();
        dir.write(b"interval_secs = 7\n");
        let owner = TestChild::spawn(&dir.0, "hold-lock");
        wait_for(&dir.0.join("lock-ready"));
        let started = Instant::now();
        let mut edited = false;
        let error = update_at(&dir.config(), |_| {
            edited = true;
            Ok(())
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(!edited);
        assert!(started.elapsed() >= Duration::from_millis(450));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(fs::read(dir.config()).unwrap(), b"interval_secs = 7\n");
        drop(owner); // Kill the owner, skipping its lock guard's destructor.
        update_at(&dir.config(), |cfg| {
            cfg.interval_secs = 31;
            Ok(())
        })
        .unwrap();
        assert_eq!(read_strict(&dir.config()).unwrap().interval_secs, 31);
        assert!(
            sidecar(&dir.config()).exists(),
            "stable sidecar must not be unlinked"
        );
    }

    #[test]
    fn transaction_child() {
        let Ok(role) = std::env::var("PETERFAN_CONFIG_TEST_ROLE") else {
            return;
        };
        let p = PathBuf::from(std::env::var_os("PETERFAN_CONFIG_TEST_PATH").unwrap());
        let root = p.parent().unwrap();
        match role.as_str() {
            "reject-corrupt" => {
                let error = update_at(&p, |cfg| {
                    cfg.interval_secs = 1234;
                    Ok(())
                })
                .unwrap_err();
                assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            }
            "edit-interval" => update_at(&p, |cfg| {
                fs::write(root.join("first-edit"), [])?;
                wait_for(&root.join("release-first"));
                cfg.interval_secs = 17;
                Ok(())
            })
            .unwrap(),
            "edit-critical" => {
                fs::write(root.join("second-attempting"), []).unwrap();
                update_at(&p, |cfg| {
                    cfg.critical_temp_c = 82.5;
                    Ok(())
                })
                .unwrap();
            }
            "init" => {
                fs::write(root.join("init-attempting"), []).unwrap();
                init_default_at(&p).unwrap();
            }
            "hold-lock" => {
                let target = resolve_target(&p).unwrap();
                let _lock = ConfigLock::acquire(&target).unwrap();
                fs::write(root.join("lock-ready"), []).unwrap();
                std::thread::sleep(Duration::from_secs(30));
            }
            _ => panic!("unknown child role: {role}"),
        }
    }
}
