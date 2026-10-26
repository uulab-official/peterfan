# CPU Widget Release and Installed Acceptance Plan

**Goal:** Publish v1.32.8, prove public OTA from the installed v1.32.7 app, enable
the approved independent CPU widget and start a fresh six-hour observation.

**Spec:** The approved CPU menu-bar widget design, plus the user's explicit
request to deploy, verify real OTA and observe the new installed build.

## Constraints

- Keep public v1.32.7 artifacts, prior observations and the primary dirty checkout.
- Keep signing secrets local; use the existing Developer ID and notary credentials.
- Keep the compatible v1.32.7 helper floor; do not require a privileged reinstall
  for an app-only feature. Pin actual app/helper versions independently in QA.
- Stage a complete draft release, verify signatures/notarization/assets/native
  Windows acceptance and exact-source CI before public publication.
- Use the existing public-feed updater for installation; no manual app copy.
- Enable only the authorized CPU preferences; compare all other configuration.
- New observation uses new fixed PIDs/hashes/settings, strict original limits and
  complete 21,600 measured seconds. Earlier observations contribute no time.

## Tasks

- [x] Prepare v1.32.8 metadata, current documentation and mixed-version QA tests.
      Verify Node, docs/OSS, reviewed source and CI; merge through a PR and tag
      the clean exact release commit without moving an existing tag.
- [x] Build universal Mac assets, Developer ID sign, notarize and verify them;
      stage private release and attach the independently verified Windows asset.
      Compare provider digests and all manifest entries, then publish once.
- [x] Use the signed v1.32.7 release CLI against the public feed. Verify queue,
      actual replacement/relaunch, native readiness, artifact bytes/trust and
      preserved configuration. Confirm the compatible helper and Auto health.
- [ ] Exercise actual installed CPU settings/styles, enable Bar with percentage,
      retain the runner and all unrelated settings, then freeze a new baseline.
- [ ] Calibrate the new baseline, start a full six-hour run, update the existing
      active pointer and resume the existing heartbeat with exact run identities.
      Record OTA/widget/native evidence and pending final-build acceptance.

## Acceptance boundaries

Publication is not installation. A short calibration is not six-hour acceptance.
The completed v1.32.7 run is historical. The new result applies only to the exact
new app, actual compatible helper and selected CPU-widget mode on this Mac.

## Execution record — 2026-10-25

Release source/CI, signed/notarized universal Mac assets, native Windows acceptance,
complete draft/public provider verification and actual public-feed OTA pass. The
compatible helper is unchanged and healthy; Bar with percentage is configured
without changing unrelated TOML values. Actual installed UI acceptance awaits one
user dashboard-open action because the hidden menu-bar app cannot be bound by
native automation. The new six-hour run has not started. See the release ledger
and sanitized evidence for exact hashes, process/readiness and remaining gates.


## Subsequent delivery record — 2026-10-26

The user subsequently approved per-core bars (v1.32.9) and independent CPU,
runner and metric visibility (v1.32.10). Both are immutable signed/notarized
public releases with actual public-feed OTA verified on this Mac. v1.32.10 source
is `d82e095`; the compatible resident helper remains v1.32.7. Current configuration
is Cores/no percentage plus the existing three items. See the
[visibility release ledger](../../MENU_ITEM_VISIBILITY_RELEASE_2026-10-26.md)
for exact release/CI/artifact/installed-byte receipts.

The original Bar/percentage target above is a historical plan checkpoint. Actual
installed UI acceptance, calibration and the new continuous six-hour observation
have not started. No older result or uptime is counted. A newly requested battery
detail feature remains at design approval and does not belong to published
v1.32.10; if implemented, use a subsequent immutable release and a new final-build
observation. Preserve every historical run and source tag.
