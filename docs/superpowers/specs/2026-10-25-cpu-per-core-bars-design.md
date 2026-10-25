# CPU menu-bar bars per core

Status: design approved by the user on 2026-10-25; implementation in progress.

## Intent

Add **Per-core bars / 코어별 바** to the existing independent CPU widget. Show
one vertical bar per logical CPU reported by the existing monitor: eight CPUs
produce eight bars, ten produce ten. Every bar uses a fixed 0–100% scale for its
own CPU. This is current utilization, not a sequence of aggregate history samples.

## Approaches

- **Add a display mode — selected by the user.** Preserve Bar, History and Number
  and their existing preferences while adding the requested view.
- Replace Bar with per-core bars. Fewer choices, but changes an existing display.

## Proposed behavior and implementation choices

- Selecting Per-core bars turns the optional aggregate percentage off. The user
  can enable that percentage again. Re-selecting the already active mode retains
  the user's percentage choice.
- Bars share a baseline and height. Height reflects actual per-core utilization;
  they do not all repeat the aggregate value or normalize to the busiest CPU.
  Logical CPU order stays stable. Intel Hyper-Threading can therefore produce
  more bars than physical cores; labels and help explain this distinction.
- Detect the bar count from `CpuMetrics.per_core`, without hard-coding eight or
  ten. Allocate enough native image width for separate bars and gaps. Width is
  stable as load changes and updates only for topology or preference changes.
- Reuse the existing samples and the widget's one-second cadence. Do not add a
  monitor, background thread or animation timer. Missing or invalid samples show
  an unavailable marker instead of an invented zero or an old reading.
- Use original monochrome template graphics that follow macOS appearance.
  Performance/efficiency colors and physical-core aggregation are outside this
  change. Keep the runner, fan settings, helper and existing click behavior.
- Expose the mode in both settings views and the native context menu, with a
  real per-core preview, Korean/English labels and existing save acknowledgements.
  Settings previews must fit narrow screens even when the native item is wider.

## Acceptance and delivery boundary

Verify distinct 0/50/100% per-core heights, 1/8/10/18/32/64 CPU layouts, missing
and non-finite samples, topology changes, stable width, and existing modes.
Verify save/reload/failure/retry, native menu parity, light/dark rendering,
320/390px settings and keyboard operation. Reuse the collected per-core data;
do not introduce new fan-control requirements.

Source and isolated/native preview tests do not prove installed-client behavior.
Keep installed v1.32.8 and its evidence unchanged during development. A later
release must use a fresh version and immutable signed/notarized artifacts,
actual public OTA and its own final-installation six-hour run. The currently
pending v1.32.8 UI acceptance and historical v1.32.7 run do not validate this mode.

Reference: [iStat Menus CPU documentation](https://register.bjango.com/help/istatmenus7/cpugpu/).
