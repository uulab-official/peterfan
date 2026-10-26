//! On-demand macOS battery details. Only an explicit request can start a
//! profiler process; ordinary monitor ticks can only poll its owned child.

use peterfan_core::metrics::{BatteryHealthSource, BatteryInfo};
use serde_json::{json, Value};
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

const MAX_PROFILER_OUTPUT: usize = 128 * 1024;
const PROBE_INTERVAL: Duration = Duration::from_secs(300);
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const HEALTH_CACHE_TTL: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct BatteryCapacities {
    design: Option<u32>,
    full: Option<u32>,
    cycles: Option<u32>,
}

impl BatteryCapacities {
    fn from_registry(value: &Value) -> Self {
        fn number(value: Option<&Value>, allow_zero: bool) -> Option<u32> {
            let value = u32::try_from(value?.as_u64()?).ok()?;
            (allow_zero || value > 0).then_some(value)
        }
        let data = value.get("BatteryData");
        Self {
            design: number(data.and_then(|v| v.get("DesignCapacity")), false)
                .or_else(|| number(value.get("DesignCapacity"), false)),
            // These names designate physical mAh. MaxCapacity is intentionally
            // never a fallback: on Apple Silicon it can be a normalized100.
            full: number(data.and_then(|v| v.get("FullChargeCapacity")), false)
                .or_else(|| number(value.get("AppleRawMaxCapacity"), false)),
            cycles: number(value.get("CycleCount"), true),
        }
    }
}

fn valid_health(value: f32) -> Option<f32> {
    (value.is_finite() && value > 0.0 && value <= 100.0).then_some(value)
}

pub(crate) fn apply_details(
    info: &mut BatteryInfo,
    capacities: BatteryCapacities,
    reported: Option<f32>,
) {
    info.design_capacity_mah = capacities.design;
    info.full_charge_capacity_mah = capacities.full;
    info.cycle_count = capacities.cycles.or(info.cycle_count);
    let estimated = capacities
        .design
        .zip(capacities.full)
        .and_then(|(design, full)| valid_health(full as f32 / design as f32 * 100.0));
    let health = reported
        .and_then(valid_health)
        .map(|value| (value, BatteryHealthSource::Reported))
        .or_else(|| estimated.map(|value| (value, BatteryHealthSource::Estimated)));
    info.health_percent = health.map(|(value, _)| value);
    info.health_source = health.map(|(_, source)| source);
}

fn parse_profiler_health(output: &[u8]) -> Option<f32> {
    if output.len() > MAX_PROFILER_OUTPUT {
        return None;
    }
    let value: Value = serde_json::from_slice(output).ok()?;
    value
        .get("SPPowerDataType")?
        .as_array()?
        .iter()
        .find_map(|item| {
            let text = item
                .get("sppower_battery_health_info")?
                .get("sppower_battery_health_maximum_capacity")?
                .as_str()?;
            valid_health(text.strip_suffix('%')?.trim().parse().ok()?)
        })
}

/// Only select integer capacity/cycle properties. No serial/model dictionary
/// or whole registry/profiler payload is retained or logged.
pub(crate) fn read_native_capacities() -> BatteryCapacities {
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;
    use core_foundation_sys::base::{kCFAllocatorDefault, CFGetTypeID, CFRelease, CFTypeRef};
    use core_foundation_sys::dictionary::{
        CFDictionaryGetTypeID, CFDictionaryGetValue, CFDictionaryRef,
    };
    use core_foundation_sys::number::{
        kCFNumberSInt64Type, CFNumberGetTypeID, CFNumberGetValue, CFNumberIsFloatType, CFNumberRef,
    };
    use core_foundation_sys::string::CFStringRef;
    use std::ffi::{c_char, c_void};

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOServiceMatching(name: *const c_char) -> *mut c_void;
        fn IOServiceGetMatchingService(port: u32, matching: *const c_void) -> u32;
        fn IORegistryEntryCreateCFProperty(
            entry: u32,
            key: CFStringRef,
            allocator: *const c_void,
            options: u32,
        ) -> CFTypeRef;
        fn IOObjectRelease(object: u32) -> i32;
    }
    unsafe fn integer(value: CFTypeRef) -> Option<Value> {
        if value.is_null()
            || CFGetTypeID(value) != CFNumberGetTypeID()
            || CFNumberIsFloatType(value as CFNumberRef) != 0
        {
            return None;
        }
        let mut number: i64 = 0;
        CFNumberGetValue(
            value as CFNumberRef,
            kCFNumberSInt64Type,
            &mut number as *mut _ as *mut c_void,
        )
        .then(|| json!(number))
    }
    unsafe fn property(entry: u32, name: &str) -> CFTypeRef {
        IORegistryEntryCreateCFProperty(
            entry,
            CFString::new(name).as_concrete_TypeRef(),
            kCFAllocatorDefault,
            0,
        )
    }
    unsafe {
        let matching = IOServiceMatching(c"AppleSmartBattery".as_ptr());
        if matching.is_null() {
            return BatteryCapacities::default();
        }
        // IOServiceGetMatchingService consumes the matching dictionary.
        let service = IOServiceGetMatchingService(0, matching);
        if service == 0 {
            return BatteryCapacities::default();
        }
        let mut selected = serde_json::Map::new();
        for name in ["DesignCapacity", "AppleRawMaxCapacity", "CycleCount"] {
            let raw = property(service, name);
            if let Some(value) = integer(raw) {
                selected.insert(name.into(), value);
            }
            if !raw.is_null() {
                CFRelease(raw);
            }
        }
        let data = property(service, "BatteryData");
        if !data.is_null() {
            if CFGetTypeID(data) == CFDictionaryGetTypeID() {
                let mut selected_data = serde_json::Map::new();
                for name in ["DesignCapacity", "FullChargeCapacity"] {
                    let key = CFString::new(name);
                    let raw = CFDictionaryGetValue(
                        data as CFDictionaryRef,
                        key.as_concrete_TypeRef() as *const c_void,
                    );
                    if let Some(value) = integer(raw) {
                        selected_data.insert(name.into(), value);
                    }
                }
                selected.insert("BatteryData".into(), Value::Object(selected_data));
            }
            CFRelease(data);
        }
        IOObjectRelease(service);
        BatteryCapacities::from_registry(&Value::Object(selected))
    }
}

enum ProbePoll {
    Pending,
    Complete { success: bool, output: Vec<u8> },
}
trait ProbeChild {
    fn poll(&mut self) -> ProbePoll;
    fn kill(&mut self);
}
struct PendingProbe {
    child: Box<dyn ProbeChild>,
    started_at: Instant,
    cancelled: bool,
}

#[derive(Default)]
pub(crate) struct ProfilerCache {
    // Every explicit attempt, including a failed spawn, consumes the throttle.
    pub(crate) last_attempt: Option<Instant>,
    pending: Option<PendingProbe>,
    health: Option<(Instant, f32)>,
}

impl ProfilerCache {
    pub(crate) fn request_native(&mut self, now: Instant) -> Option<f32> {
        self.request(now, || {
            OwnedProfiler::spawn().map(|child| Box::new(child) as Box<dyn ProbeChild>)
        })
    }

    fn request(
        &mut self,
        now: Instant,
        spawn: impl FnOnce() -> io::Result<Box<dyn ProbeChild>>,
    ) -> Option<f32> {
        self.poll(now);
        if self.pending.is_none()
            && self
                .last_attempt
                .is_none_or(|at| now.saturating_duration_since(at) >= PROBE_INTERVAL)
        {
            // Failed spawn/exit/parse attempts consume the same throttle.
            self.last_attempt = Some(now);
            if let Ok(child) = spawn() {
                self.pending = Some(PendingProbe {
                    child,
                    started_at: now,
                    cancelled: false,
                });
            }
        }
        self.cached(now)
    }

    pub(crate) fn poll(&mut self, now: Instant) {
        let Some(pending) = self.pending.as_mut() else {
            return;
        };
        if !pending.cancelled && now.saturating_duration_since(pending.started_at) >= PROBE_TIMEOUT
        {
            pending.child.kill();
            pending.cancelled = true;
            // Retain ownership; a later existing tick performs try_wait.
            return;
        }
        if let ProbePoll::Complete { success, output } = pending.child.poll() {
            if success && !pending.cancelled {
                if let Some(health) = parse_profiler_health(&output) {
                    self.health = Some((now, health));
                }
            }
            self.pending = None; // Complete means the owned child is reaped.
        }
    }

    fn cached(&self, now: Instant) -> Option<f32> {
        self.health
            .filter(|(at, _)| now.saturating_duration_since(*at) < HEALTH_CACHE_TTL)
            .map(|(_, health)| health)
    }
}

struct OwnedProfiler {
    child: Child,
    stdout: Option<ChildStdout>,
    output: Vec<u8>,
    invalid: bool,
    reaped: bool,
}

impl OwnedProfiler {
    fn new(mut child: Child) -> Self {
        let stdout = child.stdout.take();
        let nonblocking = stdout.as_ref().is_some_and(|pipe| unsafe {
            let flags = libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL);
            flags >= 0
                && libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) == 0
        });
        let mut owned = Self {
            child,
            stdout,
            output: Vec::new(),
            invalid: false,
            reaped: false,
        };
        if !nonblocking {
            owned.kill();
        }
        owned
    }

    fn spawn() -> io::Result<Self> {
        Command::new("/usr/sbin/system_profiler")
            .args(["SPPowerDataType", "-json"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map(Self::new)
    }

    fn drain_stdout(&mut self) {
        if self.invalid {
            return;
        }
        let mut bytes = [0u8; 8192];
        // Bounded work even if the child continuously fills its pipe.
        for _ in 0..32 {
            let Some(pipe) = self.stdout.as_mut() else {
                break;
            };
            match pipe.read(&mut bytes) {
                Ok(0) => break,
                Ok(count) if self.output.len() + count <= MAX_PROFILER_OUTPUT => {
                    self.output.extend_from_slice(&bytes[..count])
                }
                Ok(_) => {
                    self.kill();
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    self.kill();
                    break;
                }
            }
        }
    }
}

impl ProbeChild for OwnedProfiler {
    fn poll(&mut self) -> ProbePoll {
        self.drain_stdout();
        match self.child.try_wait() {
            Ok(Some(status)) => {
                self.reaped = true;
                // Capture bytes written between the first drain and exit.
                self.drain_stdout();
                ProbePoll::Complete {
                    success: status.success() && !self.invalid,
                    output: if self.invalid {
                        Vec::new()
                    } else {
                        std::mem::take(&mut self.output)
                    },
                }
            }
            Ok(None) => ProbePoll::Pending,
            Err(error) if error.raw_os_error() == Some(libc::ECHILD) => {
                self.reaped = true;
                ProbePoll::Complete {
                    success: false,
                    output: Vec::new(),
                }
            }
            Err(_) => {
                self.kill();
                ProbePoll::Pending
            }
        }
    }

    fn kill(&mut self) {
        if !self.invalid {
            self.invalid = true;
            self.output.clear();
            self.stdout = None;
            let _ = self.child.kill();
        }
    }
}

impl Drop for OwnedProfiler {
    fn drop(&mut self) {
        if !self.reaped {
            // Only shutdown/drop can wait after SIGKILL. Polling and rendering
            // use try_wait exclusively, including timeout/oversize cleanup.
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use peterfan_core::metrics::BatteryInfo;
    use serde_json::{json, Value};
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    fn battery() -> BatteryInfo {
        serde_json::from_value(json!({
            "charge_percent": 76.0, "state": "discharging",
            "health_percent": 2.0, "cycle_count": null
        }))
        .unwrap()
    }

    fn profiler(health: Value) -> Vec<u8> {
        serde_json::to_vec(&json!({"SPPowerDataType":[{
            "sppower_battery_health_info":{
                "sppower_battery_health_maximum_capacity":health,
                "sppower_battery_cycle_count":0
            }
        }]}))
        .unwrap()
    }

    #[test]
    fn physical_capacity_estimate_is_labeled_and_reported_health_takes_precedence() {
        let capacities = BatteryCapacities::from_registry(&json!({
            "MaxCapacity":100, "CycleCount":0,
            "BatteryData":{"DesignCapacity":6249,"FullChargeCapacity":5655}
        }));
        let mut info = battery();
        apply_details(&mut info, capacities, None);
        let value = serde_json::to_value(&info).unwrap();
        assert_eq!(value["health_source"], "estimated");
        assert!((info.health_percent.unwrap() - 90.49448).abs() < 0.001);
        assert_eq!(value["design_capacity_mah"], 6249);
        assert_eq!(value["full_charge_capacity_mah"], 5655);
        assert_eq!(value["cycle_count"], 0);
        apply_details(&mut info, capacities, Some(96.0));
        let value = serde_json::to_value(&info).unwrap();
        assert_eq!(value["health_source"], "reported");
        assert_eq!(value["health_percent"], 96.0);
        assert_eq!(value["charge_percent"], 76.0);
    }

    #[test]
    fn normalized_max_capacity_is_never_used_as_physical_capacity() {
        let capacities = BatteryCapacities::from_registry(&json!({
            "DesignCapacity":6249,"MaxCapacity":100,
            "BatteryData":{"DesignCapacity":6249,"MaxCapacity":100}
        }));
        let mut info = battery();
        apply_details(&mut info, capacities, None);
        let value = serde_json::to_value(&info).unwrap();
        assert_eq!(value["design_capacity_mah"], 6249);
        assert!(value["full_charge_capacity_mah"].is_null());
        assert!(value["health_percent"].is_null());
        assert!(value["health_source"].is_null());
    }

    #[test]
    fn explicitly_physical_low_capacities_are_valid_without_a_magnitude_heuristic() {
        let capacities = BatteryCapacities::from_registry(&json!({
            "DesignCapacity":100,"AppleRawMaxCapacity":80,"MaxCapacity":100
        }));
        let mut info = battery();
        apply_details(&mut info, capacities, None);
        let value = serde_json::to_value(&info).unwrap();
        assert_eq!(value["design_capacity_mah"], 100);
        assert_eq!(value["full_charge_capacity_mah"], 80);
        assert_eq!(value["health_percent"], 80.0);
        assert_eq!(value["health_source"], "estimated");
    }

    #[test]
    fn malformed_capacity_fields_fail_closed_independently() {
        for invalid in [
            Value::Null,
            json!(0),
            json!(-1),
            json!("6249"),
            json!(true),
            json!(6249.5),
            json!(u64::MAX),
        ] {
            let capacities = BatteryCapacities::from_registry(&json!({
                "BatteryData":{"DesignCapacity":invalid,"FullChargeCapacity":5655}
            }));
            let mut info = battery();
            apply_details(&mut info, capacities, None);
            let value = serde_json::to_value(&info).unwrap();
            assert!(value["design_capacity_mah"].is_null(), "{invalid}");
            assert_eq!(value["full_charge_capacity_mah"], 5655);
            assert!(value["health_percent"].is_null());
        }
        for invalid in [f32::NAN, f32::INFINITY, 0.0, -1.0, 101.0] {
            let mut info = battery();
            apply_details(&mut info, BatteryCapacities::default(), Some(invalid));
            assert!(info.health_percent.is_none());
        }
    }

    #[test]
    fn profiler_parses_only_the_internal_battery_health_subtree() {
        assert_eq!(parse_profiler_health(&profiler(json!("96%"))), Some(96.0));
        for invalid in [
            Value::Null,
            json!(96),
            json!(true),
            json!("0%"),
            json!("101%"),
            json!("NaN%"),
            json!("inf%"),
            json!("96"),
            json!({}),
        ] {
            assert_eq!(
                parse_profiler_health(&profiler(invalid.clone())),
                None,
                "{invalid}"
            );
        }
        assert_eq!(parse_profiler_health(b"invalid JSON"), None);
        assert_eq!(parse_profiler_health(&serde_json::to_vec(&json!({
            "SPPowerDataType":[{"sppower_battery_charge_info":{
                "sppower_battery_health_maximum_capacity":"76%"}},
                {"sppower_battery_model_info":{"sppower_battery_health_maximum_capacity":"96%"}}]
        })).unwrap()),None);
        assert_eq!(
            parse_profiler_health(&vec![b' '; MAX_PROFILER_OUTPUT + 1]),
            None
        );
    }

    struct FakeChild {
        events: VecDeque<ProbePoll>,
        kills: Rc<Cell<usize>>,
    }

    impl ProbeChild for FakeChild {
        fn poll(&mut self) -> ProbePoll {
            self.events.pop_front().unwrap_or(ProbePoll::Pending)
        }
        fn kill(&mut self) {
            self.kills.set(self.kills.get() + 1);
        }
    }

    fn fake(events: Vec<ProbePoll>, kills: Rc<Cell<usize>>) -> Box<dyn ProbeChild> {
        Box::new(FakeChild {
            events: events.into(),
            kills,
        })
    }

    #[test]
    fn pending_profiler_is_pollable_and_never_restarted_before_300_seconds() {
        let start = Instant::now();
        let starts = Cell::new(0);
        let kills = Rc::new(Cell::new(0));
        let mut cache = ProfilerCache::default();
        assert_eq!(
            cache.request(start, || {
                starts.set(starts.get() + 1);
                Ok(fake(
                    vec![
                        ProbePoll::Pending,
                        ProbePoll::Complete {
                            success: true,
                            output: profiler(json!("96%")),
                        },
                    ],
                    kills.clone(),
                ))
            }),
            None
        );
        cache.poll(start + Duration::from_secs(1));
        assert_eq!(cache.cached(start + Duration::from_secs(1)), None);
        cache.poll(start + Duration::from_secs(2));
        assert_eq!(cache.cached(start + Duration::from_secs(2)), Some(96.0));
        assert_eq!(
            cache.request(start + Duration::from_secs(299), || {
                starts.set(starts.get() + 1);
                Err(std::io::Error::other("unexpected spawn"))
            }),
            Some(96.0)
        );
        assert_eq!(starts.get(), 1);
        cache.request(start + Duration::from_secs(300), || {
            starts.set(starts.get() + 1);
            Err(std::io::Error::other("controlled start failure"))
        });
        assert_eq!(starts.get(), 2);
        assert_eq!(kills.get(), 0);
    }

    #[test]
    fn failed_invalid_and_oversized_results_are_throttled_and_do_not_cache_health() {
        for (success, output) in [
            (false, profiler(json!("96%"))),
            (true, b"invalid JSON".to_vec()),
            (true, vec![b' '; MAX_PROFILER_OUTPUT + 1]),
        ] {
            let start = Instant::now();
            let starts = Cell::new(0);
            let mut cache = ProfilerCache::default();
            cache.request(start, || {
                starts.set(starts.get() + 1);
                Ok(fake(
                    vec![ProbePoll::Complete { success, output }],
                    Rc::new(Cell::new(0)),
                ))
            });
            cache.poll(start + Duration::from_secs(1));
            assert_eq!(cache.cached(start + Duration::from_secs(1)), None);
            cache.request(start + Duration::from_secs(299), || {
                starts.set(starts.get() + 1);
                Err(std::io::Error::other("unexpected spawn"))
            });
            assert_eq!(starts.get(), 1);
        }
    }

    #[test]
    fn successful_cache_expires_at_600_seconds_even_after_a_refresh_failure() {
        let start = Instant::now();
        let mut cache = ProfilerCache::default();
        cache.request(start, || {
            Ok(fake(
                vec![ProbePoll::Complete {
                    success: true,
                    output: profiler(json!("96%")),
                }],
                Rc::new(Cell::new(0)),
            ))
        });
        cache.poll(start + Duration::from_secs(1));
        cache.request(start + Duration::from_secs(301), || {
            Err(std::io::Error::other("controlled failure"))
        });
        assert_eq!(cache.cached(start + Duration::from_secs(600)), Some(96.0));
        assert_eq!(cache.cached(start + Duration::from_secs(601)), None);
    }

    #[test]
    fn timeout_kills_once_and_keeps_ownership_until_a_later_poll_reaps() {
        let start = Instant::now();
        let kills = Rc::new(Cell::new(0));
        let events = Rc::new(RefCell::new(VecDeque::new()));
        struct PendingChild {
            events: Rc<RefCell<VecDeque<ProbePoll>>>,
            kills: Rc<Cell<usize>>,
        }
        impl ProbeChild for PendingChild {
            fn poll(&mut self) -> ProbePoll {
                self.events
                    .borrow_mut()
                    .pop_front()
                    .unwrap_or(ProbePoll::Pending)
            }
            fn kill(&mut self) {
                self.kills.set(self.kills.get() + 1);
            }
        }
        let mut cache = ProfilerCache::default();
        cache.request(start, || {
            Ok(Box::new(PendingChild {
                events: events.clone(),
                kills: kills.clone(),
            }))
        });
        cache.poll(start + Duration::from_millis(9999));
        assert_eq!(kills.get(), 0);
        cache.poll(start + Duration::from_secs(10));
        cache.poll(start + Duration::from_secs(11));
        assert_eq!(kills.get(), 1);
        // A still-owned child prevents a second spawn even after the throttle.
        cache.request(start + Duration::from_secs(300), || {
            panic!("must retain the unreaped child")
        });
        events.borrow_mut().push_back(ProbePoll::Complete {
            success: true,
            output: profiler(json!("96%")),
        });
        cache.poll(start + Duration::from_secs(301));
        assert_eq!(cache.cached(start + Duration::from_secs(301)), None);
    }

    #[test]
    fn polling_without_an_explicit_request_cannot_create_a_process() {
        let mut cache = ProfilerCache::default();
        for seconds in [0, 10, 300, 600, 900] {
            cache.poll(Instant::now() + Duration::from_secs(seconds));
            assert_eq!(cache.cached(Instant::now()), None);
        }
    }

    #[test]
    fn owned_probe_drop_kills_and_reaps_only_its_child() {
        use std::process::{Command, Stdio};
        let mut unrelated = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let owned = Command::new("/bin/sleep")
            .arg("30")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = owned.id() as libc::pid_t;
        let probe = OwnedProfiler::new(owned);
        drop(probe);
        let mut status = 0;
        let owned_wait = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        let owned_errno = std::io::Error::last_os_error().raw_os_error();
        // Clean up even on RED so the regression itself cannot leak a child.
        if owned_wait == 0 {
            unsafe {
                libc::kill(pid, libc::SIGKILL);
                libc::waitpid(pid, &mut status, 0);
            }
        }
        let unrelated_alive = unrelated.try_wait().unwrap().is_none();
        unrelated.kill().unwrap();
        unrelated.wait().unwrap();
        assert_eq!(owned_wait, -1);
        assert_eq!(owned_errno, Some(libc::ECHILD));
        assert!(unrelated_alive);
    }

    fn poll_until_complete(probe: &mut OwnedProfiler) -> (bool, Vec<u8>) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            if let ProbePoll::Complete { success, output } = probe.poll() {
                return (success, output);
            }
            std::thread::yield_now();
        }
        panic!("owned child did not finish within the bounded polling window: buffered={}, invalid={}, reaped={}", probe.output.len(), probe.invalid, probe.reaped);
    }

    #[test]
    fn actual_owned_stdout_captures_ready_and_nonzero_exit_without_waiting_on_live_child() {
        use std::process::{Command, Stdio};
        let bytes = profiler(json!("96%"));
        let child = Command::new("/usr/bin/printf")
            .arg("%s")
            .arg(String::from_utf8(bytes.clone()).unwrap())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut probe = OwnedProfiler::new(child);
        let (success, output) = poll_until_complete(&mut probe);
        assert!(success);
        assert_eq!(output, bytes);
        let child = Command::new("/usr/bin/false")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut probe = OwnedProfiler::new(child);
        let (success, output) = poll_until_complete(&mut probe);
        assert!(!success);
        assert!(output.is_empty());
    }

    #[test]
    fn actual_oversized_pipe_is_drained_bounded_killed_and_reaped() {
        use std::process::{Command, Stdio};
        let child = Command::new("/usr/bin/printf")
            .arg("%s")
            .arg("x".repeat(MAX_PROFILER_OUTPUT + 1))
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id() as libc::pid_t;
        let mut probe = OwnedProfiler::new(child);
        let (success, output) = poll_until_complete(&mut probe);
        assert!(!success);
        assert!(
            output.is_empty(),
            "invalid/oversized output must be discarded"
        );
        let mut status = 0;
        assert_eq!(
            unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) },
            -1
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }

    #[test]
    fn actual_pending_pipe_returns_immediately_and_timeout_is_reaped_by_later_ticks() {
        use std::process::{Command, Stdio};
        let child = Command::new("/bin/sleep")
            .arg("30")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id() as libc::pid_t;
        let mut probe = OwnedProfiler::new(child);
        let start = Instant::now();
        assert!(matches!(probe.poll(), ProbePoll::Pending));
        assert!(
            start.elapsed() < Duration::from_millis(100),
            "empty live pipe blocked the caller"
        );
        let mut cache = ProfilerCache::default();
        cache.request(start, || Ok(Box::new(probe)));
        cache.poll(start + Duration::from_secs(10));
        // Tick polling, rather than timeout handling, must reap the killed child.
        for _ in 0..200 {
            cache.poll(start + Duration::from_secs(11));
            if unsafe { libc::kill(pid, 0) } == -1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut status = 0;
        let wait = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        let errno = std::io::Error::last_os_error().raw_os_error();
        if wait == 0 {
            unsafe {
                libc::kill(pid, libc::SIGKILL);
                libc::waitpid(pid, &mut status, 0);
            }
        }
        assert_eq!(wait, -1);
        assert_eq!(errno, Some(libc::ECHILD));
        assert_eq!(cache.cached(start + Duration::from_secs(11)), None);
    }

    #[test]
    #[ignore = "explicit read-only Mac probe; prints only physical capacities, cycles, and health"]
    fn read_only_native_battery_source_readback() {
        let capacities = read_native_capacities();
        let start = Instant::now();
        let mut cache = ProfilerCache::default();
        cache.request(start, || {
            OwnedProfiler::spawn().map(|child| Box::new(child) as Box<dyn ProbeChild>)
        });
        for _ in 0..120 {
            cache.poll(Instant::now());
            if let Some(health) = cache.cached(Instant::now()) {
                println!("selected native mAh/cycle fields: {capacities:?}; OS reported health: {health}%");
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("read-only profiler probe returned no validated health within12seconds");
    }
}
