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

- [ ] Prepare v1.32.8 metadata, current documentation and mixed-version QA tests.
      Verify Node, docs/OSS, reviewed source and CI; merge through a PR and tag
      the clean exact release commit without moving an existing tag.
- [ ] Build universal Mac assets, Developer ID sign, notarize and verify them;
      stage private release and attach the independently verified Windows asset.
      Compare provider digests and all manifest entries, then publish once.
- [ ] Use the signed v1.32.7 release CLI against the public feed. Verify queue,
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
