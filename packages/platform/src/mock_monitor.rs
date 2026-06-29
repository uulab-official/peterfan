//! A simulated [`SystemMonitor`] for `--mock` and tests.
//!
//! Produces believable, lightly-varying metrics with no real OS access, so the
//! full dashboard demos on any machine and the front-ends have a deterministic
//! substrate to test against.

use peterfan_core::metrics::{
    BatteryInfo, CpuMetrics, DiskInfo, LoadAvg, MemoryMetrics, NetInterface, ProcSort, ProcessInfo,
    SystemInfo,
};
use peterfan_core::monitor::{MonitorCapabilities, SystemMonitor};

const GIB: u64 = 1024 * 1024 * 1024;

pub struct MockMonitor {
    tick: u64,
}

impl Default for MockMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl MockMonitor {
    pub fn new() -> Self {
        Self { tick: 0 }
    }

    /// Smooth 0..1 triangle wave driven by the refresh counter and `phase`.
    fn wave(&self, phase: f32) -> f32 {
        let x = ((self.tick as f32 * 0.1) + phase).fract();
        1.0 - (2.0 * x - 1.0).abs()
    }
}

impl SystemMonitor for MockMonitor {
    fn name(&self) -> &str {
        "mock"
    }

    fn capabilities(&self) -> MonitorCapabilities {
        MonitorCapabilities {
            cpu: true,
            memory: true,
            disks: true,
            networks: true,
            processes: true,
            battery: true,
        }
    }

    fn refresh(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    fn system_info(&self) -> SystemInfo {
        SystemInfo {
            host_name: Some("mockbook".into()),
            os_name: Some("Simulated OS".into()),
            os_version: Some("1.0".into()),
            kernel_version: Some("mock-1".into()),
            arch: "aarch64".into(),
            uptime_secs: 3 * 3600 + 42 * 60,
            logical_cores: 8,
            physical_cores: Some(8),
        }
    }

    fn cpu(&self) -> CpuMetrics {
        let base = 18.0 + self.wave(0.0) * 55.0;
        let per_core = (0..8)
            .map(|i| (base + self.wave(i as f32 * 0.13) * 20.0).clamp(0.0, 100.0))
            .collect();
        CpuMetrics {
            brand: "Mock CPU (8C/16T @ 4.5GHz)".into(),
            usage_percent: base.clamp(0.0, 100.0),
            per_core,
            frequency_mhz: 4500,
            load_avg: Some(LoadAvg {
                one: 1.8,
                five: 1.4,
                fifteen: 1.1,
            }),
        }
    }

    fn memory(&self) -> MemoryMetrics {
        let total = 32 * GIB;
        let used = (total as f32 * (0.45 + self.wave(0.3) * 0.2)) as u64;
        MemoryMetrics {
            total,
            used,
            available: total - used,
            used_percent: used as f32 / total as f32 * 100.0,
            swap_total: 8 * GIB,
            swap_used: (1.2 * GIB as f32) as u64,
        }
    }

    fn disks(&self) -> Vec<DiskInfo> {
        let total = 1000 * GIB;
        let used = 612 * GIB;
        vec![DiskInfo {
            name: "disk0".into(),
            mount: "/".into(),
            fs: "apfs".into(),
            total,
            available: total - used,
            used,
            used_percent: used as f32 / total as f32 * 100.0,
            removable: false,
            kind: "SSD".into(),
        }]
    }

    fn networks(&self) -> Vec<NetInterface> {
        let rx = (2.0e6 * self.wave(0.2) as f64).max(0.0);
        let tx = (5.0e5 * self.wave(0.6) as f64).max(0.0);
        vec![NetInterface {
            name: "en0".into(),
            rx_total: 4_800_000_000 + self.tick * 2_000_000,
            tx_total: 980_000_000 + self.tick * 500_000,
            rx_rate: rx,
            tx_rate: tx,
        }]
    }

    fn processes(&self, limit: usize, sort: ProcSort) -> Vec<ProcessInfo> {
        let mut procs = vec![
            ProcessInfo {
                pid: 412,
                name: "firefox".into(),
                cpu_percent: 32.4,
                memory: 1_400_000_000,
            },
            ProcessInfo {
                pid: 88,
                name: "WindowServer".into(),
                cpu_percent: 14.1,
                memory: 620_000_000,
            },
            ProcessInfo {
                pid: 901,
                name: "rust-analyzer".into(),
                cpu_percent: 9.7,
                memory: 980_000_000,
            },
            ProcessInfo {
                pid: 33,
                name: "kernel_task".into(),
                cpu_percent: 6.2,
                memory: 220_000_000,
            },
            ProcessInfo {
                pid: 555,
                name: "Code Helper".into(),
                cpu_percent: 3.1,
                memory: 540_000_000,
            },
        ];
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
        Some(BatteryInfo {
            charge_percent: 76.0,
            state: "discharging".into(),
            health_percent: Some(94.0),
            cycle_count: Some(212),
            time_to_full_secs: None,
            time_to_empty_secs: Some(3 * 3600 + 20 * 60),
            vendor: Some("MockCorp".into()),
            model: Some("MB-1000".into()),
            energy_rate_w: Some(-11.4),
        })
    }
}
