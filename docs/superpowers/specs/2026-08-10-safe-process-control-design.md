# Safe Process Control

Date: 2026-08-10
Status: Approved design
Target release: v1.32.0

## Objective

Complete the v1.32 process-management roadmap item with a searchable process
list and a native, fail-closed quit path. PeterFan should feel like a real
system-control product while making it difficult to terminate the wrong
process, a protected process, or a different process that reused the same PID.

## Success Criteria

- System view can search running processes by case-insensitive name or exact PID.
- CPU and memory sorting continue to work while a filter is active.
- Search results come from the complete refreshed process snapshot rather than
  only filtering the five rows already visible in the WebView.
- Every displayed process carries a PID and process-start identity token.
- The first quit click enters an inline five-second confirmation state; only a
  second click for the same identity sends a request.
- The native layer re-reads the process and rejects a missing process, PID reuse,
  another user's process, PeterFan itself, `peterfand`, PID 0/1, and protected
  macOS processes before signalling.
- macOS sends `SIGTERM` only. PeterFan does not expose force kill or escalation.
- Success, rejection, and OS errors appear in the process section and diagnostic
  log without freezing the event loop.
- A successful request invalidates the slow process cache so the row disappears
  promptly after the process exits.
- Existing fan control, dashboard refresh, Windows packaging, and CLI process
  output continue to compile and pass their tests.
- Version v1.32.0 builds, is bundled, replaces the canonical local app
  recoverably, and relaunches as exactly one ready menu-bar process.

## Scope

### Included

- Process identity metadata in the shared metrics model.
- Complete-snapshot native filtering with bounded returned rows.
- Search and empty-state UI in the System view.
- Inline two-step quit confirmation.
- A focused platform process-control module with macOS ownership, identity, and
  protected-process validation.
- Structured action state carried from native code to both PeterFan WebViews.
- English and Korean process-control copy.
- Unit, dashboard contract, workspace, lint, release-build, and local-launch
  verification.
- Workspace version bump from v1.31.32 to v1.32.0 and roadmap/checklist updates.

### Excluded

- `SIGKILL`, force quit, privilege escalation, or administrator prompts.
- Killing process trees or groups.
- Suspending, renicing, inspecting files, or changing process priority.
- Process control through the HTTP API, CLI, or root fan-control daemon.
- Windows process termination. Windows keeps read-only process metrics until a
  separate identity and ownership design is approved.
- GPU utilization and the TUI curve editor, which remain separate roadmap work.

## Chosen Approach

Use native filtering and a platform-owned guarded quit operation.

Two alternatives were considered. Filtering only the current top-five JSON rows
would be fast but misleading because most matching processes could never appear.
Sending every process to the WebView would make search immediate but expand each
dashboard payload and move safety-relevant identity handling into JavaScript.
The chosen design keeps the complete process snapshot native, returns a bounded
result set, and leaves the WebView responsible only for interaction and display.

## Architecture

### Shared process identity

`peterfan_core::metrics::ProcessInfo` gains:

- `start_time_unix_secs: Option<u64>` — the operating-system process start time;
- `owned_by_current_user: Option<bool>` — whether the refreshed process owner
  matches the PeterFan process owner.

The fields are optional and serde-defaulted so older JSON consumers and
platforms that cannot prove identity remain compatible. An absent value never
enables a quit button.

`SystemMonitor` gains a default `processes_matching(limit, sort, query)` method.
The sysinfo backend overrides it so filtering occurs before top-N selection.
The query is trimmed, capped at 64 Unicode scalar values, and matches either a
case-insensitive process-name substring or an exact decimal PID.

### Process-control boundary

Create `packages/platform/src/process_control.rs` with a small public surface:

```rust
pub struct ProcessQuitRequest {
    pub pid: u32,
    pub expected_start_time_unix_secs: u64,
}

pub enum ProcessQuitError {
    InvalidPid,
    NotFound,
    IdentityChanged,
    OwnershipUnknown,
    NotOwned,
    Protected,
    Unsupported,
    SignalFailed(i32),
}

pub fn quit_process(request: ProcessQuitRequest) -> Result<String, ProcessQuitError>;
```

On macOS the function refreshes the requested PID through sysinfo, verifies the
start time, compares the effective user ID with `geteuid`, applies the protected
policy, then calls `kill(pid, SIGTERM)`. The result records the verified process
name. Non-macOS builds return `Unsupported`; they do not preserve the current
unguarded `taskkill` behavior.

Protected policy rejects:

- PID 0 and PID 1;
- `std::process::id()`;
- `PeterFan`, `peterfan-menubar`, and `peterfand` after ASCII case folding;
- any process whose effective owner is unknown or differs from PeterFan's owner.

PID and start time are validated again immediately before `SIGTERM`. A stale row
therefore cannot terminate a new process that reused the PID.

### Menu-bar application state

The menu-bar application stores:

- `process_filter: String`;
- `process_action: ProcessActionState` containing revision, phase, PID, process
  name, and localized-neutral status code/message.

Both popover and detail WebViews enqueue `processfilter:<query>` and
`quitprocess:<pid>:<start_time>` commands through the existing pending-command
queue. Process-filter commands coalesce under one key. Quit requests do not run
in the WebView IPC closure; the event loop starts one background validation task
and receives its result through the existing event proxy.

The slow dashboard cache refreshes immediately after a filter change and after a
quit result. At most 20 matching processes are serialized into the dashboard
payload. An empty query preserves the inexpensive top-process path.

## User Experience

The System process card gains a compact search field above the rows. Its
placeholder is `프로세스 검색` in Korean and `Search processes` in English. The
field preserves focus and text across three-second native payload refreshes.

Rows show process name, PID, CPU, and memory. A quit control appears only when
the native snapshot proves ownership and identity metadata is complete.
Otherwise the row is read-only.

The first quit click changes only that row's control to `종료 확인` / `Confirm`
for five seconds and announces the state through an ARIA live region. A second
click for the same PID and start token submits the request. Clicking another row,
pressing Escape, changing views, or reaching the timeout cancels confirmation.

While native validation runs, the row shows `종료 요청 중…` / `Requesting…`.
The process section then displays one bounded result message:

- requested successfully;
- process already exited;
- process identity changed;
- protected or not owned;
- permission or operating-system failure.

No modal browser confirmation is used and no error is silently discarded.

## Data Flow

1. Slow native refresh reads the process snapshot and computes identity metadata.
2. The active native query filters the complete snapshot and selects the top 20.
3. Dashboard payload updates rows without replacing the focused search input.
4. First row click creates only local WebView confirmation state.
5. Second click posts PID plus process-start token.
6. The IPC closure enqueues the request and wakes the event loop.
7. A background native task re-reads and validates the process, then sends
   `SIGTERM` only when every invariant passes.
8. The event loop publishes structured action state, invalidates the slow cache,
   refreshes the process list, and logs the result.

## Error Handling and Safety Invariants

- Missing, malformed, zero, and PID-1 requests fail before any OS call.
- Missing start tokens fail closed.
- Unknown owner information disables UI control and fails native validation.
- A start-time mismatch is reported as identity change, never retried.
- Only one process action may be in flight. A duplicate request reports the
  existing pending state instead of starting another signal operation.
- The process name supplied by JavaScript is never trusted for validation or
  logging; native code uses the freshly read name.
- The raw filter is capped before storage and escaped by JSON serialization.
- OS error codes are mapped to bounded user copy; arbitrary system text is not
  injected into HTML.
- Diagnostic logs record PID, verified native name, decision, and error code but
  no command line, path, username, or environment data.

## Testing

### Core and platform tests

- name substring and exact PID filtering;
- filter-before-top-N behavior;
- empty-query compatibility with existing ranking;
- current-user ownership derivation;
- PID 0/1 and PeterFan-family protected policy;
- stale start-time rejection;
- unknown/different owner rejection;
- successful macOS `SIGTERM` against a spawned, same-user test child;
- no signal sent for every rejection path.

The live signal test spawns a dedicated sleeping child owned by the test process,
captures its start time, sends the guarded request, and waits for termination.
It never targets an unrelated process.

### Menu-bar tests

- IPC handlers enqueue process actions rather than calling `kill` directly;
- filter commands coalesce;
- dashboard payload contains start token and native `can_quit` state;
- search input, empty state, PID label, inline confirmation, timeout, Escape,
  pending state, and ARIA live result are present;
- the old direct `killproc:` and modal `confirm()` paths are absent;
- process action result invalidates the slow cache.

### Release verification

1. Focused process-control tests pass.
2. `cargo fmt --all -- --check` passes.
3. `cargo test --workspace` passes.
4. `cargo clippy --workspace --all-targets -- -D warnings` passes.
5. Runner, sleep/display, docs, and diff gates pass.
6. `cargo build --release --workspace` succeeds at v1.32.0.
7. A fresh ad-hoc signed local bundle verifies with `codesign`.
8. The existing canonical app is moved to a uniquely named backup.
9. The new canonical app launches as one process and logs startup, tray creation,
   WebView creation, and WebView readiness.

Public Developer ID signing, notarization, and GitHub publication remain blocked
unless the required release credentials are present; local success is not
reported as a public production release.
