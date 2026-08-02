# iStat and RunCat Product Parity

PeterFan is not intended to copy either product screen for screen. This matrix
keeps the useful product contracts visible: iStat-class observability, a
RunCat-class glanceable CPU runner, and PeterFan's own safe fan control.

Reference points:

- [iStat Menus overview](https://bjango.com/mac/istatmenus/)
- [iStat Menus 7 help](https://bjango.com/help/istatmenus7/welcome/)
- [iStat Menus version history](https://bjango.com/mac/istatmenus/versionhistory/)
- [RunCat-style CPU animation behavior](https://github.com/win0err/gnome-runcat)

## Monitoring Matrix

| Capability | PeterFan | Product status |
| --- | --- | --- |
| Aggregate and per-core CPU usage | Live values and 2m/1h/1d history; Apple Silicon P/E group averages, peaks, and individual core detail | Ready |
| CPU frequency and load average | Live values; 1/5/15m load in System | Ready |
| Memory, swap, and macOS breakdown | Live values and history | Ready |
| Disk capacity and read/write activity | Live values and history | Ready |
| Network rates, interface, and local IP | Live values and history | Ready |
| CPU temperature headline | Calibrated CPU Core Average | Ready |
| Full temperature inventory | CPU/GPU/storage/board/battery groups with source | Ready |
| Fan RPM and per-fan control | Auto, profiles, manual RPM, readback, fail-safe | Ready on supported Macs |
| Battery charge, health, and cycles | Live where hardware reports it | Ready |
| System power | Live where the SMC backend reports it | Ready on supported Macs |
| Top CPU and memory processes | Sortable list with guarded quit action | Ready |
| Uptime and system identity | Live uptime; CLI exposes full identity | Ready |
| GPU utilization history | Temperature only today | Research |
| User notification rules | Native CPU-average threshold, fan failure, and signed update alerts; critical daemon safety remains independent | Ready |
| Support diagnostics | One-click privacy-safe JSON with CPU sensors, fan targets/readback, daemon revisions, and safety health | Ready |
| Weather, clocks, and calendar | Outside PeterFan's hardware-monitor focus | Out of scope |

## CPU Runner Contract

- [x] The runner appears only in the menu bar, never as decorative popover UI.
- [x] CPU usage controls frame interval continuously from a calm idle to sprint.
- [x] Load spikes accelerate quickly; falling load decays smoothly.
- [x] Eight cached contact, flight, and landing frames avoid drawing work in the animation loop.
- [x] Temperature, runner, and combined modes are available in native and app settings.
- [x] Cat, dog, rabbit, and fox runners are selectable without restarting the app.
- [x] The click target remains fixed-width while frames and temperatures change.
- [x] Settings report the current smoothed CPU load and pace.
- [ ] Custom runner packs with reviewed size and CPU-cost limits.
- [x] macOS Reduce Motion automatically switches the runner to a still frame.

## Next Product Priorities

1. Record the six-hour animation CPU and fan-control soak on release hardware.
2. Expand Apple Silicon core maps with privacy-safe M1 through M5 samples.
3. Add reviewed third-party runner packs with strict size and CPU-cost limits.
4. Research stable GPU utilization sources separately for macOS and Windows.
