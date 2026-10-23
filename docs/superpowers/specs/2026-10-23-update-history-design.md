# Update check history across restarts

The user requested continued production refinement after integrating the verified
runner work into main. This pass addresses the documented loss of check history
on restart; the implementation choices below are the agent's bounded choices.

## Approaches

- Reuse WebView storage: small, but not authoritative across windows or channels.
- Add fields to user TOML: durable, but mixes transient diagnostics with preferences.
- Use a native JSON sidecar: selected because it separates diagnostics from settings
  and the updater's independently verified installation receipt.

## Contract

Store schema version, update channel, last attempt, last successful discovery and
last discovery error in `update-check-history.json`, alongside `update-result.json`.
Retain only one channel's latest record. Restore only matching-channel, bounded,
valid history. Corrupt, oversized, unsupported or future-dated input falls back to
fresh discovery. Do not restore release metadata, phase, installation readiness,
retry state, notification claims or an old deadline.

Every normal launch still checks after 20 seconds. A completed check retains the
six-hour success interval and existing retry policy. Persist both attempts and
results, including an interrupted attempt, without keeping a state mutex during
disk I/O. Replace through an owned same-directory temporary file, sync and rename;
Unix permissions are 0600. Failed storage must not block discovery or installation.
Mock mode performs no history reads or writes. User settings and installer trust
checks remain untouched. Notification deduplication remains process-local.

## Evidence

Temporary-path regressions cover restart restoration, interrupted attempts, stale
release exclusion, channel/schema/time validation, corrupt/oversized records,
replacement failure cleanup and mock isolation. Run contributor, Windows/MSRV and
smoke checks. Verify the signed installed app records a real check, restores its
history on restart before network discovery, then checks again after 20 seconds.
CI, local installation and newer-version public OTA are separate evidence tiers.
