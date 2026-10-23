# Fan activation and public OTA acceptance — 2026-10-23

Scope: one physical Apple M5 Max Mac (`Mac17,7`). The native shared installer,
privileged helper, public release feed and detached updater are exercised.
Browser fixtures and source tests are not treated as native installation proof.

## Changes and reproduced failures

- Explicit activation repairs an installed but unreachable daemon through the
  shared administrator-approved installer. Unknown/old resident versions cannot
  claim readiness or use legacy passwordless replacement.
- A real launchd bootstrap failed because an existing plist retained its
  quarantine attribute. Fresh validated atomic replacement fixes the inode and
  attribute inheritance; invalid content preserves the predecessor.
- Installation succeeds only after the resident helper is verified. The CLI
  returns failure on an unreachable resident and provides explicit `--force`
  repair, which always requires administrator approval.
- Kernel-authenticated IPC, bounded request framing/deadlines, discovered fan-ID
  limits and private self-update staging protect the privileged helper. Developer
  ID identity and security/running version checks precede candidate execution.
- A real Windows release upload exposed an invalid GitHub commit-readback URL.
  Both publication workflows now use the valid tag-ref commit endpoint. The
  existing offline API contract regression failed before the fix; the corrected
  native Windows workflow completed upload and checksum refresh successfully.
- A real signed-artifact command exposed incorrect `codesign -R` inline argument
  syntax. The final code shares the correct `=`-prefixed argument. A native
  Apple-signed system binary regression accepts matching trust and rejects a wrong
  identifier. v1.32.5 remains a superseded private draft; its tag was not moved.

## Source verification

- Final contributor gate: 529 Rust tests passed; 7 remain opt-in/ignored. The
  canonical rendering test was explicitly invoked and passed separately.
- 69 Node tests, 12 executable dashboard checks, native Clippy and rustdoc passed.
- Deliberate permissive-peer, oversized-request and source-reopen mutations fail
  their behavioral regressions. Native inline-requirement test failed before
  the correction and passes afterward.
- CI results and final source commit: [source PR14 CI](https://github.com/uulab-official/peterfan/pull/14),
  [main source CI](https://github.com/uulab-official/peterfan/actions/runs/39963172259),
  [workflow-fix CI](https://github.com/uulab-official/peterfan/actions/runs/39965205578).
  Release source is `a6ea90519d9e8af05e4af477dc05b88c8b127d07` (immutable tag
  `v1.32.6`); publication-workflow fix is `caedef99b3af94c07bb64c0bd6fa460434822d93`.

## Signed release and delivery

[Public v1.32.6](https://github.com/uulab-official/peterfan/releases/tag/v1.32.6)
was published at 2026-10-23 14:02:12 UTC, with both platform archives, the DMG and
checksums uploaded. All provider SHA-256 digests match local artifact bytes and
the three manifest entries:

```text
a0d259f1777e9c15b0579fbc865de87097253a4528dca60d1a9efa40d3e5820e  PeterFan-v1.32.6.dmg
dd92c9d9310578220136c332838b41e6fe5f3d1144c6468c00fc7d418a2a70c3  peterfan-v1.32.6-universal-apple-darwin.tar.gz
8205bde7ecb3195616571056ce2251f344db9a1e883f5e6128c256bd077241fb  peterfan-v1.32.6-x86_64-pc-windows-msvc.zip
```

Both macOS architectures are included. The app and DMG were Developer ID signed,
Apple-notarized and stapled, and Gatekeeper accepted them. The complete macOS
release-directory integrity report passes for both embedded apps and hashes.
[Corrected Windows release run](https://github.com/uulab-official/peterfan/actions/runs/39965225606)
passes native build, archive verification, smoke tests, immutable-tag readback,
attachment and combined checksum refresh. The source tag was not retargeted.

The repository Homebrew Formula and Cask are pinned to these exact public archive
and DMG hashes and pass native strict audit. No external tap repository was
modified.

## Physical fan behavior

The final v1.32.6 helper was installed through the shared native installer with
explicit repair. Resident version and packaged-helper bytes match. The service
plist is root-owned mode 0644 and does not retain the predecessor's quarantine.

Requesting 100% via the daemon raised the two fans to 7,564 and 7,781 RPM
(reported maximum 7,826 RPM), reaching at least 95% in 9.15 seconds.
A `finally` block restored OS automatic mode; readback then measured 4,973
and 5,364 RPM. The test never requests low fan speed or extra heat.

The running helper also rejects the older signed v1.32.4 app helper without
changing its PID, binary or automatic fan mode. The actual final version probe
accepts signed v1.32.6 and rejects signed v1.32.5 as a downgrade.

## Automatic discovery and newer-version OTA

The installed v1.32.4 app discovered public v1.32.6 automatically in
20.07 seconds after a normal restart. Native logs record `automatic=true`,
`phase=available`, successful history persistence (mode 0600), and a next check
21,600 seconds after success. No fixture feed or manual discovery action is used
for this native startup result.

The older v1.32.4 CLI invokes the shared updater against the real public release.
It verifies the downloaded DMG and queues a detached installer. The existing app
is stopped using its exact executable and start identity. The replacement v1.32.6
starts, writes fresh native tray/WebView readiness acknowledgement, survives the
additional health interval, and yields an `installed` receipt. One new installed
process remains. Installed app executable, helper and Info.plist hashes match the
public candidate; strict signature, staple and Gatekeeper checks pass afterward.

The installed v1.32.6 helper then successfully replaces itself from the updated
signed app without another administrator prompt. Its new PID and root-owned 0755
binary are verified; bytes equal the bundled helper and OS automatic fan control
is preserved. Post-OTA `doctor` reports `daemon_ready`; an ordinary activation
reuses the healthy helper without a prompt. The new app also automatically checks
and reports `phase=current`, scheduling another check in 21,600 seconds.

This proves the existing private v1.32.4 client on this Mac updating from the
public server to v1.32.6. It does not measure adoption by other public v1.32.2
installations. The six-hour interval is verified as scheduled, not elapsed.

## Short soak and preservation

The exact OTA-installed process completed 600.04 seconds with
119 valid interval samples, pinned executable/start identity and no
configuration change. The observer does not terminate the external app.

| Metric | Observed | Gate |
| --- | --- | --- |
| Average app CPU | 1.633% of one core | <= 8% |
| Peak interval app CPU | 2.369% | <= 35% |
| Peak app RSS | 69.328 MiB | <= 256 MiB |
| RSS growth | 2.375 MiB | <= 64 MiB |
| Peak thread growth | +1 | <= 16 |

All bounds pass. This is an ordinary active-workload observation; host load varied,
so it is not an idle-energy or RunCat comparison. The installed app remains
running. Raw per-process, hardware, release and preservation receipts are retained
locally under `target/production-acceptance/` (untracked, not public test artifacts).

The primary dirty checkout and user configuration were preserved. Native UI
automation could not connect in this session, so activation uses the same native
installer through the CLI followed by the user's macOS authentication. This does
not establish a new automated click-through acceptance of every button.

## Remaining acceptance limits

This is one-machine activation and delivery acceptance. Six-hour soak, physical
sleep/wake, forced macOS interruption/power-loss recovery, additional Mac models,
multi-monitor energy measurement and a same-machine RunCat comparison remain
separate unfinished gates. The beta status is retained; these results do not
claim zero bugs or complete hardware coverage.
