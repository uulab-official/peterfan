# Menu-bar item visibility

Status: design and implementation approved by the user on 2026-10-26; implemented and locally verified; final review and delivery pending.

## Intent and selected approach

Provide one **Menu-bar items / 메뉴 막대 표시 항목** group with independent
checkboxes for **CPU widget**, **Runner character**, and **Metric text**
(temperature in the supplied screenshot). A user may display any one, any two,
or all three. Keep the existing selected metric; hiding its text must not change
temperature source, fan control or other metric preferences.

Approaches considered:

- **Independent checkboxes — selected by the user.** Supports single-item,
  mixed and all-item combinations without adding a preset for every combination.
- Single/all presets only. Fewer controls, but cannot express useful pairs.

## Behavior

- Group the three visibility controls together in both settings hosts, with
  consistent native context-menu controls and confirmed save state.
- Preserve the existing appearance when upgrading. Existing CPU-enabled and
  primary Number/Runner/Both preferences determine the initial three checkboxes.
- Keep CPU appearance (Bar/Per-core bars/History/Number and percentage) as
  subordinate CPU settings. Keep runner character and selected metric settings.
  Hiding an item preserves its detailed preferences for later re-enabling.
- CPU-only, runner-only and metric-only states all retain dashboard/settings
  access through the visible menu item. Preserve same-item close and cross-item
  anchor behavior when more than one native item is visible.
- Require at least one visible item. Disable the last active checkbox with a
  short explanation; also reject invalid stale/native requests before saving.
- If external configuration requests no visible items, use a recoverable UI
  fallback without resetting the full configuration or fan preferences.
- Save the combined visibility change atomically, then apply it live only after
  persistence succeeds. Failed saves retain the prior visible items and offer
  retry. Keep dashboard position through settings refreshes.

## Compatibility and implementation choices

Reuse the existing CPU-enabled flag and primary Number/Runner/Both representation.
Add a default-on primary-item visibility flag for CPU-only mode. Do not add an
unknown value to a shared enum that an older resident helper must deserialize.
Older helpers must retain their full fan configuration. The app owns creation,
hiding and restoration of its own status items; preserve widths, click targets
and native timer lifecycle. Do not add a sampling worker or timer, change fan
control, or add item reordering in this change. Existing macOS Command-drag
reordering remains available. These controls remain macOS-only.

## Verification and delivery boundary

Check all seven non-empty combinations, last-item protection, old-config
migration, old-helper parsing, save/reload/failure/retry and external invalid
configuration. Verify native hide/show/click anchoring and timer ownership,
Korean/English settings at 320/390px, light/dark and keyboard operation.

Keep installed v1.32.9, its settings and immutable public artifacts unchanged
during development. The new native behavior needs a subsequent immutable version,
signed/notarized release, actual public OTA and a new final-installed-build
six-hour run. The supplied screenshot confirms the shown arrangement, not the
unimplemented visibility controls or a completed six-hour observation.
