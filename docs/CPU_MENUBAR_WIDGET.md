# CPU menu-bar widget

Status: implemented in source; not included in the installed v1.32.7 release.

In macOS Settings, enable **CPU widget** to add a separate menu-bar item alongside
the runner. Choose **Bar**, **History** or **Number**. Graphs can also show a numeric
percentage; Number always shows one. Existing configurations default to off.
The original runner/temperature display remains independently configurable.

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

The concurrent six-hour observation covers the unchanged installed **v1.32.7**
app and helper. It does not validate this new native feature. Native release,
signed-artifact OTA adoption and a final-build long run remain separate gates.

On 2026-10-24, local macOS validation passed 549 workspace Rust tests and 89 Node
behavior tests, strict workspace Clippy, documentation/OSS checks and the explicit
runner sprite export gate. Independent review identified and resolved cross-item
toggle/focus ordering, native checkmark reconciliation, accelerated history
sampling during control confirmation and stale history after resume. Browser
fixture saves are separate from the isolated native lifecycle and persistence
unit evidence described above.

Reference: [iStat Menus CPU settings](https://register.bjango.com/help/istatmenus7/cpugpu/).
PeterFan uses original raster rendering and its existing CPU samples.
