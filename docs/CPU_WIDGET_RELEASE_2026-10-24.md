# CPU-widget release and installed acceptance

**v1.32.8 was published on 2026-10-25 at 11:45:03 KST.** Public-feed OTA from
v1.32.7 completed on this Mac. Installed widget visual/click acceptance and the
new final-build six-hour result remain pending.

The previous [v1.32.7 six-hour acceptance](LONG_RUN_SAFETY_2026-10-24.md) remains
historical evidence. None of its elapsed time counts toward the new build.

## Source and release

- Immutable tag/source: `v1.32.8` / `e923ec9754513d6bc3eae05e3cb41d90d427c79d`.
  [PR20](https://github.com/uulab-official/peterfan/pull/20), its final head and
  exact main passed all six CI jobs. Follow-up documentation does not move the tag.
- [Public release](https://github.com/uulab-official/peterfan/releases/tag/v1.32.8):
  all four assets match provider digests and the complete three-entry checksum
  manifest. An unauthenticated latest-feed read and checksum download succeed.
  Existing public v1.32.7 asset IDs, digests and sizes are preserved.
- Universal arm64/x86_64 binaries have Developer ID signing. App and DMG Apple
  notarization/stapling, strict code signatures and Gatekeeper pass independently.
- [Windows release run](https://github.com/uulab-official/peterfan/actions/runs/40154956336)
  passed package/version checks, real metrics, Microsoft bootstrapper signature,
  installation, tray/WebView2 readiness, startup registration and uninstallation.
  Downloaded ZIP contents and three x64 PE headers also match the native contract.

| Asset | SHA-256 |
| --- | --- |
| macOS DMG | `c59189fde969a8e15e8ee46d68e9503a693330ea754c7d4c6338abaad40df3f6` |
| macOS universal archive | `5a7f892708e429063dd3dab71a02717421559ae9791fb499fd1e7a551bc6cade` |
| Windows x64 archive | `dea858c73844b43b156fcdf2abd4bfc91a92ace75983b1fbebcae05194ef3440` |

## Actual installed OTA and compatibility

The signed v1.32.7 release CLI used the public feed and queued the existing
updater. Only after queue verification was the pinned old app allowed to exit.
The installer replaced and relaunched the app as PID 65859 (11:45:55 KST), wrote
a fresh matching per-launch native readiness receipt and reported installation.
Installed app/helper-bundle/Info bytes match the release artifact; strict code
signature, stapled ticket and Gatekeeper pass. Configuration bytes are unchanged
through OTA. New app startup automatically checks the public release successfully
at 11:46:16 KST and stores the next six-hour deadline at 17:46:16 KST.

The compatible resident helper remains **v1.32.7**, PID 324 with its original
start time and binary hash. The app requires helper v1.32.7; another privileged
install is unnecessary for this app-only feature. Doctor reports healthy metrics,
thermal readings and fan-control readiness with no issues. Auto control has
matching desired/applied revision 4, fresh sensors, empty manual targets/overrides
and zero cumulative sensor, fan-write and readback failures.

## Installed CPU widget and remaining acceptance

Existing users default to the separate widget being off. This Mac's authorized
configuration now enables **Bar with percentage** alongside the unchanged runner.
Only the three CPU-widget settings were added under the existing configuration
lock with an atomic durable replacement. All unrelated TOML values are equal to
the original configuration. No CPU-widget error has been observed in the app log.

The native UI automation connection times out while the menu-bar app's dashboard
is hidden. The user has been asked to open that dashboard once. Actual installed
Bar/History/Number, percentage, settings acknowledgement and click/anchor checks
are **pending**; browser fixtures and the isolated native QA item do not replace
those checks. A new frozen baseline, short calibration and complete 21,600-second
observation follow installed UI acceptance. No new six-hour run is claimed yet.

## Acceptance ledger

- [x] Reviewed exact v1.32.8 source and six passing CI jobs.
- [x] Signed/notarized universal Mac assets, native Windows acceptance and complete
      provider/manifest/public-feed verification.
- [x] Actual public-feed OTA v1.32.7 → v1.32.8, fresh surviving native readiness,
      exact artifact bytes, trust checks and pre-OTA settings preservation.
- [x] Compatible helper remains healthy in automatic mode with zero failures.
- [x] Requested CPU-only Bar/percentage configuration applied; unrelated values preserved.
- [ ] Actual installed visual/style/settings/click acceptance.
- [ ] New frozen installed-build baseline and short strict-limit calibration.
- [ ] New complete 21,600-second installed-build observation and final audit.

See the [sanitized release/OTA evidence](evidence/2026-10-25-cpu-widget-v1.32.8.json).
Local raw receipts and logs remain under `target/cpu-widget-release-v1328/`.
Publication is not six-hour acceptance. Physical sleep/wake, forced macOS
installation interruption and other hardware remain separate gates. Beta status
is retained.
