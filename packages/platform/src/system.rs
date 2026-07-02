//! Real, cross-platform system metrics via the `sysinfo` and `battery` crates.
//!
//! `sysinfo` already abstracts over macOS, Windows, and Linux internally, so
//! this one backend serves every desktop OS for CPU/memory/disk/network/
//! process metrics — no per-OS code needed here. `battery` adds real battery
//! state on the same platforms.
//!
//! ## Optimization
//!
//! The monitor keeps a single long-lived `System` and refreshes only the
//! metric families it exposes (CPU, memory, processes) rather than calling
//! `refresh_all()`, which would also re-scan components and other data we don't
//! use. Network and disk handles are refreshed in place. Usage percentages and
//! network rates are deltas, so [`refresh`](SystemMonitor::refresh) tracks the
//! elapsed wall-clock interval to convert byte deltas into per-second rates.

use std::collections::HashMap;
use std::time::Instant;

use sysinfo::{
    DiskKind, Disks, MemoryRefreshKind, Networks, ProcessRefreshKind, ProcessesToUpdate,
    RefreshKind, System,
};

use peterfan_core::metrics::{
    BatteryInfo, CpuMetrics, DiskInfo, LoadAvg, MemoryBreakdown, MemoryMetrics, NetInterface,
    ProcSort, ProcessInfo, SystemInfo,
};
use peterfan_core::monitor::{MonitorCapabilities, SystemMonitor};

pub struct SysinfoMonitor {
    sys: System,
    disks: Disks,
    networks: Networks,
    /// Per-interface (rx_rate, tx_rate) in bytes/sec from the last interval.
    net_rates: HashMap<String, (f64, f64)>,
    /// Per-disk (read_rate, write_rate) in bytes/sec from the last interval.
    disk_rates: HashMap<String, (f64, f64)>,
    last_refresh: Option<Instant>,
    battery_mgr: Option<battery::Manager>,
    has_battery: bool,
    /// When true, `refresh()` skips processes, disks, and network I/O — used by
    /// commands that only need CPU% or memory (saves ~150 ms on macOS).
    quick: bool,
}

impl SysinfoMonitor {
    pub fn new() -> Self {
        Self::new_inner(false)
    }

    /// Light-weight variant: skips process enumeration and disk/network I/O on
    /// each refresh. Suitable for `memory`, `battery`, `system`, `doctor` — any
    /// command that does not need per-process data or I/O rates.
    pub fn new_quick() -> Self {
        Self::new_inner(true)
    }

    fn new_inner(quick: bool) -> Self {
        let rk = if quick {
            // Quick mode: skip process enumeration entirely (saves ~150 ms on macOS).
            RefreshKind::nothing()
                .with_cpu(sysinfo::CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything())
        } else {
            RefreshKind::nothing()
                .with_cpu(sysinfo::CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything())
                .with_processes(ProcessRefreshKind::everything())
        };
        let sys = System::new_with_specifics(rk);
        let disks = if quick {
            Disks::new()
        } else {
            Disks::new_with_refreshed_list()
        };
        let networks = if quick {
            Networks::new()
        } else {
            Networks::new_with_refreshed_list()
        };

        let battery_mgr = battery::Manager::new().ok();
        let has_battery = battery_mgr
            .as_ref()
            .and_then(|m| m.batteries().ok())
            .map(|mut b| b.next().is_some())
            .unwrap_or(false);

        Self {
            sys,
            disks,
            networks,
            net_rates: HashMap::new(),
            disk_rates: HashMap::new(),
            last_refresh: None,
            battery_mgr,
            has_battery,
            quick,
        }
    }
}

impl Default for SysinfoMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemMonitor for SysinfoMonitor {
    fn name(&self) -> &str {
        "sysinfo"
    }

    fn capabilities(&self) -> MonitorCapabilities {
        MonitorCapabilities {
            cpu: true,
            memory: true,
            disks: true,
            networks: true,
            processes: true,
            battery: self.has_battery,
        }
    }

    fn refresh(&mut self) {
        let now = Instant::now();
        let elapsed = self
            .last_refresh
            .map(|t| now.duration_since(t).as_secs_f64())
            .unwrap_or(0.0);

        self.sys.refresh_cpu_all();
        self.sys
            .refresh_memory_specifics(MemoryRefreshKind::everything());

        if !self.quick {
            self.sys.refresh_processes(ProcessesToUpdate::All, true);
            self.disks.refresh(true);
            self.networks.refresh(true);

            if elapsed > 0.0 {
                let mut net = HashMap::new();
                for (name, data) in self.networks.iter() {
                    net.insert(
                        name.clone(),
                        (
                            data.received() as f64 / elapsed,
                            data.transmitted() as f64 / elapsed,
                        ),
                    );
                }
                self.net_rates = net;

                let mut disk = HashMap::new();
                for d in self.disks.iter() {
                    let u = d.usage();
                    disk.insert(
                        d.name().to_string_lossy().into_owned(),
                        (
                            u.read_bytes as f64 / elapsed,
                            u.written_bytes as f64 / elapsed,
                        ),
                    );
                }
                self.disk_rates = disk;
            }
        }
        self.last_refresh = Some(now);
    }

    fn system_info(&self) -> SystemInfo {
        SystemInfo {
            host_name: System::host_name(),
            os_name: System::name(),
            os_version: System::os_version(),
            kernel_version: System::kernel_version(),
            arch: System::cpu_arch(),
            uptime_secs: System::uptime(),
            logical_cores: self.sys.cpus().len(),
            physical_cores: System::physical_core_count(),
        }
    }

    fn cpu(&self) -> CpuMetrics {
        let cpus = self.sys.cpus();
        let per_core: Vec<f32> = cpus.iter().map(|c| c.cpu_usage()).collect();
        let brand = cpus
            .first()
            .map(|c| c.brand().trim().to_string())
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| "CPU".to_string());
        let frequency_mhz = cpus.first().map(|c| c.frequency()).unwrap_or(0);

        // Load average is meaningless on Windows; sysinfo returns zeros there.
        let load_avg = if cfg!(windows) {
            None
        } else {
            let la = System::load_average();
            Some(LoadAvg {
                one: la.one,
                five: la.five,
                fifteen: la.fifteen,
            })
        };

        CpuMetrics {
            brand,
            usage_percent: self.sys.global_cpu_usage(),
            per_core,
            frequency_mhz,
            load_avg,
        }
    }

    fn memory(&self) -> MemoryMetrics {
        let total = self.sys.total_memory();
        let used = self.sys.used_memory();
        MemoryMetrics {
            total,
            used,
            available: self.sys.available_memory(),
            used_percent: pct(used, total),
            swap_total: self.sys.total_swap(),
            swap_used: self.sys.used_swap(),
            breakdown: memory_breakdown(),
        }
    }

    fn disks(&self) -> Vec<DiskInfo> {
        self.disks
            .iter()
            .map(|d| {
                let total = d.total_space();
                let available = d.available_space();
                let used = total.saturating_sub(available);
                let name = d.name().to_string_lossy().into_owned();
                let (read_bps, write_bps) =
                    self.disk_rates.get(&name).copied().unwrap_or((0.0, 0.0));
                DiskInfo {
                    mount: d.mount_point().to_string_lossy().into_owned(),
                    fs: d.file_system().to_string_lossy().into_owned(),
                    total,
                    available,
                    used,
                    used_percent: pct(used, total),
                    removable: d.is_removable(),
                    kind: match d.kind() {
                        DiskKind::SSD => "SSD".to_string(),
                        DiskKind::HDD => "HDD".to_string(),
                        _ => "—".to_string(),
                    },
                    read_bytes_per_sec: read_bps,
                    write_bytes_per_sec: write_bps,
                    name,
                }
            })
            .collect()
    }

    fn networks(&self) -> Vec<NetInterface> {
        self.networks
            .iter()
            .map(|(name, data)| {
                let (rx_rate, tx_rate) = self.net_rates.get(name).copied().unwrap_or((0.0, 0.0));
                // First non-loopback IPv4 address, if any.
                let ip = data
                    .ip_networks()
                    .iter()
                    .find(|n| n.addr.is_ipv4() && !n.addr.is_loopback())
                    .map(|n| n.addr.to_string());
                NetInterface {
                    name: name.clone(),
                    ip,
                    rx_total: data.total_received(),
                    tx_total: data.total_transmitted(),
                    rx_rate,
                    tx_rate,
                }
            })
            .collect()
    }

    fn processes(&self, limit: usize, sort: ProcSort) -> Vec<ProcessInfo> {
        let procs: Vec<ProcessInfo> = self
            .sys
            .processes()
            .values()
            .map(|p| ProcessInfo {
                pid: p.pid().as_u32(),
                name: p.name().to_string_lossy().into_owned(),
                cpu_percent: p.cpu_usage(),
                memory: p.memory(),
            })
            .collect();
        top_processes(procs, limit, sort)
    }

    fn battery(&self) -> Option<BatteryInfo> {
        let mgr = self.battery_mgr.as_ref()?;
        let batt = mgr.batteries().ok()?.next()?.ok()?;

        // The `battery` crate's state-of-health is unreliable on Apple Silicon
        // (a unit mismatch between reported max/design capacity can yield values
        // like 2%). Rather than present an obviously-wrong number as real, drop
        // implausible readings. A native SMC-based SoH is on the roadmap.
        let soh = batt.state_of_health().value * 100.0;
        let health_percent = (40.0..=120.0).contains(&soh).then_some(soh);

        Some(BatteryInfo {
            charge_percent: batt.state_of_charge().value * 100.0,
            state: format!("{:?}", batt.state()).to_lowercase(),
            health_percent,
            cycle_count: batt.cycle_count(),
            time_to_full_secs: batt.time_to_full().map(|t| t.value as u64),
            time_to_empty_secs: batt.time_to_empty().map(|t| t.value as u64),
            vendor: batt.vendor().map(|s| s.to_string()),
            model: batt.model().map(|s| s.to_string()),
            energy_rate_w: Some(batt.energy_rate().value),
        })
    }
}

/// Return the top `limit` processes by `sort`, sorted descending. The
/// popover only ever wants a handful of rows out of every running process
/// (often 300-600 on a real Mac), polled once a second — a full O(n log n)
/// sort of the whole list just to keep 5 rows was wasted work.
/// `select_nth_unstable_by` partitions the top `limit` in O(n), then only
/// that small slice gets fully sorted.
fn top_processes(mut procs: Vec<ProcessInfo>, limit: usize, sort: ProcSort) -> Vec<ProcessInfo> {
    let cmp = |a: &ProcessInfo, b: &ProcessInfo| match sort {
        ProcSort::Cpu => b
            .cpu_percent
            .partial_cmp(&a.cpu_percent)
            .unwrap_or(std::cmp::Ordering::Equal),
        ProcSort::Memory => b.memory.cmp(&a.memory),
    };
    if limit < procs.len() {
        procs.select_nth_unstable_by(limit, cmp);
        procs.truncate(limit);
    }
    procs.sort_by(cmp);
    procs
}

fn pct(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        0.0
    } else {
        (part as f64 / whole as f64 * 100.0) as f32
    }
}

/// macOS virtual-memory breakdown (wired/active/inactive/compressed), via the
/// mach `host_statistics64(HOST_VM_INFO64)` call — the same source Activity
/// Monitor uses. `None` on other platforms or if the syscall fails.
#[cfg(target_os = "macos")]
#[allow(deprecated)] // `mach_host_self` is the standard mach entry point here.
fn memory_breakdown() -> Option<MemoryBreakdown> {
    // SAFETY: a single mach call into a zeroed, correctly-sized struct; counts
    // are validated by the kernel returning KERN_SUCCESS (0).
    unsafe {
        let mut stats: libc::vm_statistics64 = std::mem::zeroed();
        let mut count = libc::HOST_VM_INFO64_COUNT;
        let rc = libc::host_statistics64(
            libc::mach_host_self(),
            libc::HOST_VM_INFO64,
            &mut stats as *mut _ as libc::host_info64_t,
            &mut count,
        );
        if rc != libc::KERN_SUCCESS {
            return None;
        }
        let page = libc::sysconf(libc::_SC_PAGESIZE).max(4096) as u64;
        Some(MemoryBreakdown {
            wired: stats.wire_count as u64 * page,
            active: stats.active_count as u64 * page,
            inactive: stats.inactive_count as u64 * page,
            compressed: stats.compressor_page_count as u64 * page,
        })
    }
}

#[cfg(not(target_os = "macos"))]
fn memory_breakdown() -> Option<MemoryBreakdown> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, cpu: f32, mem: u64) -> ProcessInfo {
        ProcessInfo {
            pid,
            name: format!("proc{pid}"),
            cpu_percent: cpu,
            memory: mem,
        }
    }

    #[test]
    fn top_processes_by_cpu_matches_full_sort_descending() {
        let procs = vec![
            proc(1, 5.0, 100),
            proc(2, 90.0, 50),
            proc(3, 42.0, 999),
            proc(4, 12.0, 10),
            proc(5, 60.0, 1),
        ];
        let top = top_processes(procs, 3, ProcSort::Cpu);
        let pids: Vec<u32> = top.iter().map(|p| p.pid).collect();
        assert_eq!(pids, vec![2, 5, 3]);
    }

    #[test]
    fn top_processes_by_memory_matches_full_sort_descending() {
        let procs = vec![
            proc(1, 5.0, 100),
            proc(2, 90.0, 50),
            proc(3, 42.0, 999),
            proc(4, 12.0, 10),
        ];
        let top = top_processes(procs, 2, ProcSort::Memory);
        let pids: Vec<u32> = top.iter().map(|p| p.pid).collect();
        assert_eq!(pids, vec![3, 1]);
    }

    #[test]
    fn top_processes_limit_larger_than_list_returns_everything_sorted() {
        let procs = vec![proc(1, 5.0, 100), proc(2, 90.0, 50)];
        let top = top_processes(procs, 10, ProcSort::Cpu);
        let pids: Vec<u32> = top.iter().map(|p| p.pid).collect();
        assert_eq!(pids, vec![2, 1]);
    }
}
