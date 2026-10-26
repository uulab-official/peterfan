# Independent menu-bar visibility release ledger

Status (2026-10-26): v1.32.10 is public and actual public-feed 9→10 OTA is
verified on one Mac. Installed visibility/settings/click acceptance remains
pending native dashboard access. Calibration and the new six-hour observation
have NOT STARTED; no older run or elapsed uptime contributes time.

The user approved independent CPU widget, runner character and metric-text
checkboxes. All seven non-empty combinations are supported. At least one item
stays visible so dashboard access is retained; hidden items keep their appearance,
selected metric and character preferences.

See the [approved design](superpowers/specs/2026-10-26-menu-item-visibility-design.md)
and [implementation plan](superpowers/plans/2026-10-26-menu-item-visibility.md).

## Source and isolated QA

- Existing Number/Runner/Both enum values remain unchanged. A default-on
  `primary_visible` flag permits CPU-only mode without introducing an unknown
  enum value to older resident helpers. Non-CPU/fan preferences are preserved.
- Native items and fallible image resources are prepared before the atomic
  settings save. A failed preparation/save retains the prior live set; successful
  changes preserve native status-item/button identity and runner timer ownership.
- Core/transaction tests cover all combinations, minimum-one protection, strict
  request payloads, migration, old-helper parsing, persistence failure/retry,
  external invalid configurations and unsupported-platform recovery.
- Development verification passes 570 Rust workspace tests and 104 Node tests,
  strict workspace Clippy, formatting and docs/OSS checks. Independent final
  review resolves all five findings; exact source PR/main CI also passes.
- Isolated AppKit QA passes eight CPU presentation snapshots and seven visibility
  transitions, including primary-item hiding/restoration, retained identities and
  last-item rejection. This harness does not operate the installed application.
- CUA tests the actual shared production settings template with fixture readings:
  all seven selections, singleton guards, hidden CPU style preservation, failed
  save/retry, keyboard Space and eight Korean/English 320/390px light/dark layouts.
  Each layout has document width equal to its viewport. Native click anchoring
  in the final installed client remains a separate acceptance gate.

![Settings using isolated fixture readings](images/peterfan-menu-items-settings-qa.png)

This screenshot is a production-template fixture, not the installed client.

## Delivery gates

- [x] User-approved design and implementation.
- [x] Local source tests and isolated native/browser QA.
- [x] Independent review, exact final PR/main CI and immutable v1.32.10 tag.
- [x] Universal Mac signing/notarization/stapling and six strict trust checks.
- [x] Native Windows package/install/tray/WebView2/startup/uninstall checks.
- [x] Four provider assets, actual downloads, three checksum entries and old
      public asset preservation independently matched.
- [x] Public feed and actual installed v1.32.9 → v1.32.10 OTA, fresh native
      readiness, exact installed bytes, compatible resident helper and config.
- [ ] Actual installed visibility/settings/click acceptance and agreed final state.
- [ ] Fresh calibration followed by one continuous 21,600-second observation,
      independent fixed-limit audit and final documentation/main integration.


## Public release and actual installation — 2026-10-26

[PR24](https://github.com/uulab-official/peterfan/pull/24) final head
`e20f52aaf850ef18572d67ffe6f80817e26f756f` passes all six required jobs in
[CI40267248678](https://github.com/uulab-official/peterfan/actions/runs/40267248678)
and [Windows40267248633](https://github.com/uulab-official/peterfan/actions/runs/40267248633).
Squash main and immutable annotated `v1.32.10` point to
`d82e095e46c895a52a7f28973919e496ee953517`; their tree matches the reviewed PR.
Exact-main [CI40267938049](https://github.com/uulab-official/peterfan/actions/runs/40267938049)
and [Windows40267938036](https://github.com/uulab-official/peterfan/actions/runs/40267938036)
pass the same six required jobs; release attachment is intentionally skipped in CI.

[Public v1.32.10](https://github.com/uulab-official/peterfan/releases/tag/v1.32.10)
(release ID `423861599`) was published at **2026-10-26 13:42:57 KST**.
Four universal Mac binaries, Developer ID team `N99FMBQ662`, app/DMG notarization,
stapling and all six strict signature/Gatekeeper checks pass. Native tagged
[Windows40269744133](https://github.com/uulab-official/peterfan/actions/runs/40269744133)
uses the exact immutable source and passes real metrics, Microsoft WebView2
signature, installation, tray readiness, startup, uninstall and smoke checks.
All four provider digests/sizes match actual downloads; the three-entry manifest,
local Mac bytes and Windows archive layout/AMD64 executables match. Unauthenticated
public latest/checksum readback passes. Prior v1.32.9/v1.32.8/v1.32.7 asset IDs,
names, sizes and digests are preserved. Homebrew pins these published bytes.

| Public artifact | SHA-256 |
| --- | --- |
| Mac DMG | `b70c6d144aef11fbcf2e806fc37f4fad86df00361b43dc22870fbeffb0b07148` |
| Mac universal archive | `18f21cedd4239ccd56b069803938cfc3522d5a6dfd976cf5ee954e1a82cb2e6d` |
| Windows x64 ZIP | `f48981ee3a1d7878a42dac9f2efc86a7bd319dbc258ed8af94cb44322d243cbe` |
| Checksums manifest | `21320a873fd15146daef9e6399d7296a767f4f4ff3500e63453f827223409820` |

The genuine signed/released v1.32.9 CLI used the public feed and reported the
9→10 installation queue before only pinned old app PID1022 was terminated.
App10 PID76256 started at **2026-10-26 13:44:51 KST**. Its fresh nonce readiness
receipt matches its PID/version; the terminal result is installed, and the app,
bundled helper and Info.plist bytes exactly match the verified public artifact.
Installed signature/staple/Gatekeeper pass and the process survives the check.

The live compatible helper stays **v1.32.7**, PID604/start 11:13:02 KST, with the
same binary SHA. The newer bundled helper is intentionally different and does not
require a privileged reinstall. Configuration remains byte-for-byte unchanged:
`51fcf01e1c33e0c22b601967520c99f1109ec3018b5f9db8954ea19d7484a382`, mode0644,
Cores enabled, percentage off, with the existing runner and metric retained.
Doctor reports health=ok, no issues and metrics/thermal/fan readiness. Post-OTA
Auto desired/applied revision3 matches, sensors are fresh, manual targets/overrides
are empty and cumulative sensor/write/readback errors are zero. No forced fan
write was issued; the post-reboot Auto readback timestamp may remain unavailable.

App10's natural startup automatic check succeeds at **13:45:13 KST** with matching
native log and persisted history; the next check is **19:45:13 KST** (+21,600s).
This is outside a new observation window and is not six-hour evidence.

CUA still cannot enumerate the hidden installed dashboard. The earlier user
request to open it remains pending and has not been repeated. Native readiness,
configuration and isolated fixtures do not prove installed visual/click behavior.
Actual installed UI acceptance and a new full observation remain pending.
A subsequently requested battery detail feature is still at design approval;
it is absent from v1.32.10 and will require its own immutable release and final
installed-build observation if implemented.

See [sanitized release/OTA evidence](evidence/2026-10-26-menu-item-visibility-v1.32.10.json).

Preserve immutable v1.32.9/v1.32.8 artifacts and every historical raw run. The
earlier v1.32.7 six-hour result and v1.32.9 uptime add no time to this feature.
The host rebooted on 2026-10-26 at 11:12:11 KST before any new observation began;
installed v1.32.9, helper v1.32.7 and configuration hashes were preserved and the
new processes were healthy. This is not a failed six-hour observation.

Final acceptance remains one Mac and the selected final mode/visibility state.
Physical sleep, forced macOS install interruption and other hardware are unverified.
