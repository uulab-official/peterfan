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
    BatteryInfo, CpuMetrics, DiskInfo, LoadAvg, MemoryMetrics, NetInterface, ProcSort, ProcessInfo,
    SystemInfo,
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
}

impl SysinfoMonitor {
    pub fn new() -> Self {
        let sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(sysinfo::CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything())
                .with_processes(ProcessRefreshKind::everything()),
        );
        let disks = Disks::new_with_refreshed_list();
        let networks = Networks::new_with_refreshed_list();

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
        self.sys.refresh_processes(ProcessesToUpdate::All, true);

        self.disks.refresh(true);
        self.networks.refresh(true);

        if elapsed > 0.0 {
            let mut net = HashMap::new();
            for (name, data) in self.networks.iter() {
                // received()/transmitted() are bytes since the previous refresh.
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
                let (read_bps, write_bps) = self.disk_rates.get(&name).copied().unwrap_or((0.0, 0.0));
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
        let mut procs: Vec<ProcessInfo> = self
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

        match sort {
            ProcSort::Cpu => procs.sort_by(|a, b| {
                b.cpu_percent
                    .partial_cmp(&a.cpu_percent)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            ProcSort::Memory => procs.sort_by(|a, b| b.memory.cmp(&a.memory)),
        }
        procs.truncate(limit);
        procs
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

fn pct(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        0.0
    } else {
        (part as f64 / whole as f64 * 100.0) as f32
    }
}
