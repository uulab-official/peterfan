# Optional CPU menu-bar widget

## Intent and chosen approach

The user selected **runner plus a separate CPU widget**, with bar, history and
number choices. Keep the current runner/temperature item and add an independently
enabled macOS CPU status item. The widget defaults off for existing installations.

Considered approaches:

1. **Separate CPU status item — selected.** Supports coexistence, independent
   placement with macOS Command-drag, and a stable width per display choice.
2. Replace the runner image with CPU graphs. Smaller overall footprint, but
   prevents the coexistence the user selected.
3. Append CPU text to the runner title. Simple, but makes the existing runner
   item wider and does not provide independent graph/display options.

Reference: [iStat Menus layout and display options](https://register.bjango.com/help/istatmenus7/welcome/).
Use its configurable approach as reference; implement original native rendering.

## Product behavior

- Settings offer **CPU widget: off/on**, **Bar / History / Number**, and an
  optional **Show percentage** toggle for the graphical modes.
- Bar shows aggregate CPU activity on a fixed 0–100% scale. History shows a
  bounded recent sequence on that same scale. Number shows `CPU 42%`.
- Fixed native widths and monospaced digits avoid moving neighboring items.
- Graphs follow the menu bar's light/dark and selected appearance; labels,
  keyboard operation and native hover information remain readable.
- Clicking either status item opens the existing dashboard at the clicked
  item's current native rectangle. CPU-widget right-click actions expose its
  display settings and dashboard entry without changing fan mode.
- Preferences persist through the existing configuration transaction layer.
  Failed saves keep the actual native state and settings controls consistent.
- The existing overview CPU-core toggle's missing IPC routing is corrected as
  part of the same settings integration and covered by a regression.

## Implementation boundaries

Reuse existing CPU sampling; do not create another system monitor, worker or
high-frequency animation timer. Keep history bounded, reject non-finite samples,
and update/redraw only when new data or display preferences require it.
Keep macOS-specific status-item behavior gated so other platforms keep working.

The installed v1.32.7 six-hour observation, daemon, settings and immutable release
artifacts stay unchanged. Feature work uses the existing isolated worktree on a
new `codex/` branch. New source/preview checks do not constitute installed-client
or six-hour acceptance of the new feature. Releasing a new native build requires
its own signed artifact, OTA/runtime checks and final-build observation.

## Acceptance

Verify default/backward-compatible config round trips, bounded graph data and
0/50/100%/invalid-sample rendering; all IPC routes; settings save failure and
reload; fixed widths; independent runner coexistence; click anchoring; dark/light
and narrow settings previews. Run appropriate Rust/Node checks and CI before
the authorized main integration. Keep current six-hour monitoring active, and
report its measured v1.32.7 result separately from this new feature's evidence.
