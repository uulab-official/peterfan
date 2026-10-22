use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessQuitRequest {
    pub pid: u32,
    pub expected_start_time_unix_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessQuitError {
    InvalidPid,
    NotFound,
    IdentityChanged,
    OwnershipUnknown,
    NotOwned,
    Protected,
    Unsupported,
    SignalFailed(i32),
}

impl fmt::Display for ProcessQuitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPid => f.write_str("invalid process id"),
            Self::NotFound => f.write_str("process is no longer running"),
            Self::IdentityChanged => f.write_str("process identity changed"),
            Self::OwnershipUnknown => f.write_str("process owner could not be verified"),
            Self::NotOwned => f.write_str("process belongs to another user"),
            Self::Protected => f.write_str("process is protected"),
            Self::Unsupported => f.write_str("process termination is unavailable on this platform"),
            Self::SignalFailed(code) => write!(f, "operating system rejected termination ({code})"),
        }
    }
}

impl std::error::Error for ProcessQuitError {}

fn is_protected_process_name(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "peterfan" | "peterfan-menubar" | "peterfand"
    )
}

pub fn is_protected_process(pid: u32, name: &str) -> bool {
    pid <= 1 || pid == std::process::id() || is_protected_process_name(name)
}

fn validate_quit_target(pid: u32, current_pid: u32, name: &str) -> Result<(), ProcessQuitError> {
    if pid <= 1 {
        return Err(ProcessQuitError::InvalidPid);
    }
    if pid == current_pid || is_protected_process_name(name) {
        return Err(ProcessQuitError::Protected);
    }
    Ok(())
}

fn validate_owner(owner: Option<u32>, current: u32) -> Result<(), ProcessQuitError> {
    match owner {
        None => Err(ProcessQuitError::OwnershipUnknown),
        Some(owner) if owner != current => Err(ProcessQuitError::NotOwned),
        Some(_) => Ok(()),
    }
}

fn validate_identity(expected: u64, actual: u64) -> Result<(), ProcessQuitError> {
    if expected == actual {
        Ok(())
    } else {
        Err(ProcessQuitError::IdentityChanged)
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct RefreshedProcess {
    name: String,
    start_time_unix_secs: u64,
    effective_uid: Option<u32>,
}

#[cfg(target_os = "macos")]
fn refresh_process(pid: u32) -> Result<RefreshedProcess, ProcessQuitError> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

    let pid = Pid::from_u32(pid);
    let requested = [pid];
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&requested),
        true,
        ProcessRefreshKind::everything(),
    );
    let process = system.process(pid).ok_or(ProcessQuitError::NotFound)?;
    let start_time_unix_secs = process.start_time();
    if start_time_unix_secs == 0 {
        return Err(ProcessQuitError::IdentityChanged);
    }
    let effective_uid = process
        .effective_user_id()
        .or_else(|| process.user_id())
        .map(|uid| **uid);
    Ok(RefreshedProcess {
        name: process.name().to_string_lossy().into_owned(),
        start_time_unix_secs,
        effective_uid,
    })
}

#[cfg(all(target_os = "macos", test))]
fn refreshed_process_start_time(pid: u32) -> Option<u64> {
    refresh_process(pid)
        .ok()
        .map(|process| process.start_time_unix_secs)
}

#[cfg(target_os = "macos")]
pub fn quit_process(request: ProcessQuitRequest) -> Result<String, ProcessQuitError> {
    if request.pid <= 1 {
        return Err(ProcessQuitError::InvalidPid);
    }
    if request.pid == std::process::id() {
        return Err(ProcessQuitError::Protected);
    }

    let process = refresh_process(request.pid)?;
    validate_quit_target(request.pid, std::process::id(), &process.name)?;
    validate_identity(
        request.expected_start_time_unix_secs,
        process.start_time_unix_secs,
    )?;
    let current_uid = unsafe { libc::geteuid() } as u32;
    validate_owner(process.effective_uid, current_uid)?;

    let result = unsafe { libc::kill(request.pid as libc::pid_t, libc::SIGTERM) };
    if result != 0 {
        return Err(ProcessQuitError::SignalFailed(
            std::io::Error::last_os_error().raw_os_error().unwrap_or(-1),
        ));
    }
    Ok(process.name)
}

#[cfg(not(target_os = "macos"))]
pub fn quit_process(_request: ProcessQuitRequest) -> Result<String, ProcessQuitError> {
    Err(ProcessQuitError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    struct ChildGuard(std::process::Child);

    #[cfg(target_os = "macos")]
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn wait_for_child_exit(child: &mut std::process::Child) -> bool {
        for _ in 0..100 {
            if child.try_wait().ok().flatten().is_some() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn protected_policy_rejects_reserved_and_peterfan_processes() {
        assert_eq!(
            validate_quit_target(0, 99, "Safari"),
            Err(ProcessQuitError::InvalidPid)
        );
        assert_eq!(
            validate_quit_target(1, 99, "launchd"),
            Err(ProcessQuitError::InvalidPid)
        );
        assert_eq!(
            validate_quit_target(99, 99, "Safari"),
            Err(ProcessQuitError::Protected)
        );
        assert_eq!(
            validate_quit_target(100, 99, "peterfand"),
            Err(ProcessQuitError::Protected)
        );
    }

    #[test]
    fn protected_policy_rejects_unknown_or_other_owner() {
        assert_eq!(
            validate_owner(None, 501),
            Err(ProcessQuitError::OwnershipUnknown)
        );
        assert_eq!(
            validate_owner(Some(502), 501),
            Err(ProcessQuitError::NotOwned)
        );
        assert_eq!(validate_owner(Some(501), 501), Ok(()));
    }

    #[test]
    fn identity_check_rejects_pid_reuse() {
        assert_eq!(
            validate_identity(1_000, 1_001),
            Err(ProcessQuitError::IdentityChanged)
        );
        assert_eq!(validate_identity(1_000, 1_000), Ok(()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn guarded_quit_rejects_stale_identity_without_signalling_child() {
        let child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("spawn dedicated sleep child");
        let mut child = ChildGuard(child);
        let start = refreshed_process_start_time(child.0.id()).expect("child process start time");

        let result = quit_process(ProcessQuitRequest {
            pid: child.0.id(),
            expected_start_time_unix_secs: start + 1,
        });

        assert_eq!(result, Err(ProcessQuitError::IdentityChanged));
        assert!(child.0.try_wait().expect("read child status").is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn guarded_quit_terminates_only_the_spawned_same_user_child() {
        let child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("spawn dedicated sleep child");
        let mut child = ChildGuard(child);
        let start = refreshed_process_start_time(child.0.id()).expect("child process start time");

        let result = quit_process(ProcessQuitRequest {
            pid: child.0.id(),
            expected_start_time_unix_secs: start,
        });

        assert_eq!(result.as_deref(), Ok("sleep"));
        assert!(wait_for_child_exit(&mut child.0));
    }
}
