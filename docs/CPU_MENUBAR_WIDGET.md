# CPU menu-bar widget

Status: initially released in v1.32.8; v1.32.9 adds Per-core bars and v1.32.10
adds independent item visibility (2026-10-26). See
[visibility release and installed acceptance](MENU_ITEM_VISIBILITY_RELEASE_2026-10-26.md)
for current publication, actual OTA and the pending final-build observation.

Released v1.32.10 groups **CPU widget**, **Runner character** and
**Metric text** under **Menu-bar items / 메뉴 막대 표시 항목** in macOS Settings.
Each checkbox is independent: all seven non-empty combinations are supported.
The last visible item stays enabled so dashboard/settings access remains available.
Existing installations retain their CPU and Number/Runner/Both selections.

Choose **Bar**, **Per-core bars**, **History** or **Number** in the subordinate
CPU appearance controls. Graphs can also show a numeric percentage; Number always
shows one. Hiding an item retains its CPU style, percentage, character and selected
metric preferences. The native right-click menus offer the same visibility choices.
Windows retains its existing display selector and primary tray item.

The graph uses a fixed 0–100% axis and retains at most 60 samples from the existing
CPU refresh. Invalid readings appear as gaps or `--%`. Native template images
adapt to menu-bar appearance, and fixed widths with monospaced digits prevent
load changes from moving neighboring items. No additional sampling thread or
animation timer is created. Long resume gaps discard old widget history.

Click either item to open the dashboard below that item. Clicking the other item
while the dashboard is open moves it to the new anchor; clicking the same item
closes it. Right-click the CPU item for display options or to hide it.
macOS supports rearranging menu-bar items with Command-drag.

Preference changes use the existing atomic configuration transaction and typed
request acknowledgement. Failed persistence preserves the live configuration;
unconfirmed requests offer retry. Both popover and detail settings use the same
CPU-widget path. Windows hides these Mac-only controls.

## Validation boundary

- Core tests cover legacy/partial/default settings and round trips.
- Portable tests cover raster bounds, fixed scale, missing data, history capacity
  and sampling cadence. GUI tests cover request validation and failed persistence.
- Settings behavior tests cover pending/stale/failure/retry states and reload.
- An isolated macOS QA item exercises creation, all layouts, hiding and drop
  without launching the installed app or touching its configuration/helper.
- Production settings HTML is exported for Korean/English browser QA, including
  320/390px widths, light/dark appearance and keyboard toggling. Exported pages
  use fixture data and simulated acknowledgements; they do not perform disk saves.

The completed six-hour observation covers the unchanged installed **v1.32.7**
app and helper. It does not validate this new native feature. Signed/notarized
v1.32.8 release and public-feed OTA are verified on one Mac. Actual installed
widget visual/click acceptance and the new final-build long run remain pending;
see the linked release record for the current boundary.

On 2026-10-24, local macOS validation passed 549 workspace Rust tests and 89 Node
behavior tests, strict workspace Clippy, documentation/OSS checks and the explicit
runner sprite export gate. Independent review identified and resolved cross-item
toggle/focus ordering, native checkmark reconciliation, accelerated history
sampling during control confirmation and stale history after resume. Browser
fixture saves are separate from the isolated native lifecycle and persistence
unit evidence described above.

Reference: [iStat Menus CPU settings](https://register.bjango.com/help/istatmenus7/cpugpu/).
PeterFan uses original raster rendering and its existing CPU samples.

## Independent visibility (released in v1.32.10)

The app stores one default-on `menubar.primary_visible` flag together with the
existing Number/Runner/Both enum and CPU-enabled flag. No shared enum gains a new
value. CPU-only mode keeps the previous primary display selection on disk, and
older helpers can still deserialize the full shared fan configuration.

Visibility requests contain exactly three booleans: `cpu`, `runner`, and `metric`.
The GUI prepares native resources before its atomic configuration update, then
applies the visible set after persistence succeeds. Failed preparation or saves
retain the prior visible set. Settings wait for the exact request ID and operation;
failed or timed-out saves allow manual retry. Appearance and visibility changes
are serialized in each settings host, and appearance requests preserve the current
native CPU visibility even when another window changed it.

External all-off configuration recovers a visible metric item without rewriting
the configuration or fan preferences. Native preparation failure during external
reload restores primary access. Hidden primary items stop runner animation and
its timer; showing them restores the retained native button, width and image.
No new sampler, worker or animation timer is added. Existing dashboard geometry
and same-item close/cross-item reanchor handling remain in use.

Unit/behavioral checks and isolated native fixtures are development evidence.
Signed/notarized v1.32.10 publication and actual public-feed 9→10 OTA are now
verified separately on one Mac: fresh native readiness, exact installed artifact
bytes, strict trust, compatible resident helper7 and configuration preservation.
Actual installed visibility/settings/click acceptance remains pending native
dashboard access. The fresh final-build six-hour observation has not started.
See the [visibility release ledger](MENU_ITEM_VISIBILITY_RELEASE_2026-10-26.md);
no earlier run or elapsed uptime is counted.

## Per-core bars (released in v1.32.9)

The new **Per-core bars / 코어별 바** mode shows one vertical bar for each
logical CPU in the existing monitor samples. Each bar has its own fixed 0–100%
scale: eight logical CPUs produce eight bars and ten produce ten. These are
simultaneous individual readings, rather than columns of aggregate history.
Intel Hyper-Threading CPUs are included as separate logical CPUs.

The JSON/UI style is `cores`. On disk, the compatible representation is
`style = "bar"` plus `per_core_bars = true` inside `[menubar.cpu_widget]`.
Existing helpers can therefore read the full shared fan configuration without
a new enum value or a helper upgrade. New apps resolve the flag to per-core mode.

Entering this mode hides the aggregate percentage. You can enable it again;
selecting the already active mode retains that choice. Existing modes keep
their preferences. The native graph expands for the reported topology, while
its width stays fixed as utilization changes. Settings previews scale to fit
narrow windows. Missing values have unavailable markers and never reuse an old
load. The mode uses the existing widget cadence and creates no sampling worker.

Signed/notarized v1.32.9 publication and actual public-feed 8→9 OTA are verified
on one Mac. Per-core mode with percentage off is configured; unrelated settings
and the compatible resident helper are preserved. Actual installed visual/click
acceptance awaits native dashboard access, and the new six-hour run has not
started. See [per-core release acceptance](CPU_PER_CORE_RELEASE_2026-10-25.md).
No earlier run covers this mode.

![Per-core settings preview with fixture readings](images/peterfan-cpu-cores-settings-qa.png)

The settings image uses test data; it is not installed-client or OTA evidence.
See [per-core release acceptance](CPU_PER_CORE_RELEASE_2026-10-25.md).
