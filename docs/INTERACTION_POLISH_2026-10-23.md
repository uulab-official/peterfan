# Settings interaction verification — 2026-10-23

This pass fixes observable settings failures in the existing dashboard, while
preserving the shared runner cadence and existing configuration format.

## Reproductions and corrections

| Failure | Correction |
| --- | --- |
| An earlier `config imported` status completed a new request without a response, then replaced its TOML draft with the previous config. | Each window creates unique request IDs. Native results identify both the request and operation; unrelated or older results cannot complete the request. |
| Editing rules during a save cleared its pending state; the old save response could discard the newer draft. | Independent edit revisions preserve changes made after submission. Success only clears the submitted draft when no newer edits exist. |
| A nonempty daemon error reply counted as successful reload. | Only the daemon's explicit `ok reloaded (` response confirms reload. Disk persistence, deferred reload and rejected reload have separate results and copy. |
| Live refresh replaced a focused notification threshold of 93°C with the saved 85°C. | Focused temperature input retains the user's text; snapshots resume synchronization after blur. |
| Windows random selections retained the previous frame deadline, unlike fixed selections. | Automatic rotation and both random-selection entry points reset the deadline consistently. |

Legacy `rules:` and `importconfig:` IPC commands remain accepted. The dashboard
uses `settingsrequest:` with an ID, operation and payload. Native snapshots retain
at most 32 small result records. They contain no imported TOML payload.

An unconfirmed operation retains the draft and allows manual retry after 15
seconds. It is never automatically retransmitted: a missing acknowledgement
does not prove the disk write failed. Late results cannot acknowledge a retry
with a different ID.

## Verification

- Ten executable JavaScript regression cases failed before the correction and
  passed afterward. They run the actual dashboard functions, including stale
  results, other windows/operations, repeated saves, in-flight edits, failures,
  timeouts and focused temperature editing.
- Four native tests cover request parsing, result identity, bounded result
  history and positive/error/absent daemon replies.
- Full contributor gate: 437 Rust tests passed, 7 ignored; 42 Node tests passed;
  six dashboard behavior checks and native/frontend runner comparisons passed.
- Rust 1.99 native and Windows-target Clippy with warnings denied, Rust 1.88
  workspace check, formatting and Rust documentation passed locally.
- The real Rust HTML renderer was regenerated and exercised in the browser at
  440×520 in dark and light appearance. The old renderer reproduced both input
  overwrite and false import success. The corrected renderer preserved focused
  input, failed-import drafts and newer text entered during a successful import;
  it showed deferred daemon reload separately. No JavaScript errors were recorded.

The browser uses deterministic metrics and request-result fixtures. It does not
write the user's configuration or contact the privileged daemon. These results do
not establish physical fan behavior, energy consumption, two native WebViews
performing simultaneous writes, or public OTA delivery.

Local evidence: `target/interaction-polish-contributor.log`,
`target/interaction-polish-windows-clippy.log`, `target/interaction-polish-msrv.log`,
`target/polish-settings-red.log`, and `target/interaction-polish/` screenshots.
