# Safe Process Control Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship v1.32.0 with complete-snapshot process search and a fail-closed, identity-checked macOS `SIGTERM` flow in PeterFan's System view.

**Architecture:** Extend the shared process metric with optional identity and ownership evidence, then let the sysinfo backend filter before top-N selection. A focused platform module re-reads and validates PID, start time, owner, and protected-name policy before signalling; the menu-bar event loop exposes that result to an inline two-step WebView interaction.

**Tech Stack:** Rust 2021, sysinfo 0.36, libc on macOS, serde/serde_json, Tao/Wry WebView IPC, embedded HTML/CSS/JavaScript, shell release gates.

## Global Constraints

- Target release is exactly `v1.32.0`.
- macOS process termination uses `SIGTERM` only; no `SIGKILL`, force quit, process tree, escalation, or administrator prompt.
- PID plus process start time is required and revalidated immediately before signalling.
- PID 0/1, PeterFan itself, `peterfand`, unknown owners, and other-user processes fail closed.
- The WebView never calls an OS process API and never supplies a trusted process name.
- Search is case-insensitive by name, exact by decimal PID, capped at 64 Unicode scalar values, and applied before top-N selection.
- An empty filter keeps the existing inexpensive top-process path; filtered results are capped at 20 rows.
- Non-macOS builds keep process metrics but return `Unsupported` for process termination.
- Existing fan-control behavior, CLI JSON shape compatibility, Windows compilation, runner quality, and release checks must remain intact.
- The canonical local app is replaced recoverably only after all source and release gates pass.

---

### Task 1: Shared process identity and filter-before-top-N metrics

**Files:**
- Modify: `packages/core/src/metrics.rs:111`
- Modify: `packages/core/src/monitor.rs:67`
- Modify: `packages/platform/src/system.rs:279`
- Modify: `packages/platform/src/mock_monitor.rs:138`
- Test: `packages/platform/src/system.rs:398`

**Interfaces:**
- Consumes: existing `ProcessInfo`, `ProcSort`, and `SystemMonitor::processes(limit, sort)`.
- Produces: `ProcessInfo::{start_time_unix_secs, owned_by_current_user}`,
  `peterfan_core::metrics::normalize_process_query(query)`, and
  `SystemMonitor::processes_matching(limit, sort, query)`.

- [ ] **Step 1: Write failing filter and metadata tests**

Add platform tests that construct explicit `ProcessInfo` values and require filtering before ranking:

```rust
fn proc(pid: u32, name: &str, cpu: f32, memory: u64) -> ProcessInfo {
    ProcessInfo {
        pid,
        name: name.into(),
        cpu_percent: cpu,
        memory,
        start_time_unix_secs: Some(1_700_000_000 + pid as u64),
        owned_by_current_user: Some(true),
    }
}

#[test]
fn process_filter_matches_name_case_insensitively_before_top_n() {
    let input = vec![
        proc(10, "WindowServer", 80.0, 10),
        proc(11, "Safari Networking", 30.0, 20),
        proc(12, "Safari", 20.0, 30),
    ];
    let result = top_processes_matching(input, 1, ProcSort::Cpu, "sAfArI");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].pid, 11);
}

#[test]
fn process_filter_matches_only_exact_decimal_pid() {
    let input = vec![proc(412, "Editor", 3.0, 10), proc(41, "Worker", 2.0, 20)];
    assert_eq!(top_processes_matching(input.clone(), 5, ProcSort::Cpu, "412")[0].pid, 412);
    assert!(top_processes_matching(input, 5, ProcSort::Cpu, "12").is_empty());
}

#[test]
fn process_filter_is_trimmed_and_capped_to_64_scalars() {
    assert_eq!(normalize_process_query("  Safari  "), "safari");
    assert_eq!(normalize_process_query(&"가".repeat(80)).chars().count(), 64);
}
```

- [ ] **Step 2: Run tests and capture RED**

Run:

```bash
cargo test -p peterfan-platform process_filter_ -- --nocapture
```

Expected: compilation fails because the new fields and filtering helpers do not exist.

- [ ] **Step 3: Add optional process identity fields**

Extend `ProcessInfo` without breaking older deserializers:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub start_time_unix_secs: Option<u64>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub owned_by_current_user: Option<bool>,
```

Update every production, mock, and test literal. The sysinfo backend sets
`start_time_unix_secs` from `Process::start_time()`. On macOS it compares
`Process::effective_user_id().or_else(|| Process::user_id())` with a
`sysinfo::Uid` built from `libc::geteuid()`; other platforms set ownership to
`None`.

- [ ] **Step 4: Implement native filter helpers and trait surface**

Add:

```rust
pub fn normalize_process_query(query: &str) -> String {
    query.trim().chars().take(64).collect::<String>().to_lowercase()
}

fn process_matches_query(process: &ProcessInfo, query: &str) -> bool {
    if query.is_empty() { return true; }
    if query.bytes().all(|byte| byte.is_ascii_digit()) {
        return query.parse::<u32>().ok() == Some(process.pid);
    }
    process.name.to_lowercase().contains(query)
}

fn top_processes_matching(
    mut processes: Vec<ProcessInfo>,
    limit: usize,
    sort: ProcSort,
    query: &str,
) -> Vec<ProcessInfo> {
    let query = normalize_process_query(query);
    processes.retain(|process| process_matches_query(process, &query));
    top_processes(processes, limit, sort)
}
```

Place `normalize_process_query` in `peterfan_core::metrics` so both the platform
backend and menu-bar command handler apply exactly the same cap. Add
`SystemMonitor::processes_matching` with a compatible default that filters
`self.processes(usize::MAX, sort)` and truncates. Override it in
`SysinfoMonitor` so filtering occurs on its complete snapshot before top-N.

- [ ] **Step 5: Run focused and core compatibility tests**

Run:

```bash
cargo test -p peterfan-platform process_filter_ -- --nocapture
cargo test -p peterfan-core
cargo test -p peterfan-platform top_processes_ -- --nocapture
```

Expected: all pass; empty-query ranking tests remain unchanged.

- [ ] **Step 6: Commit Task 1**

```bash
git add packages/core/src/metrics.rs packages/core/src/monitor.rs packages/platform/src/system.rs packages/platform/src/mock_monitor.rs
git commit -m "feat: add searchable process identity metrics"
```

### Task 2: Guarded macOS process-control boundary

**Files:**
- Create: `packages/platform/src/process_control.rs`
- Modify: `packages/platform/src/lib.rs:18`
- Test: `packages/platform/src/process_control.rs`

**Interfaces:**
- Consumes: `sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, Uid}` and macOS `libc`.
- Produces: `ProcessQuitRequest`, `ProcessQuitError`,
  `is_protected_process(pid, name)`, and `quit_process(request)` exactly as
  specified in the design.

- [ ] **Step 1: Write failing pure-policy tests**

Create the module with tests first, referring to production helpers that do not yet exist:

```rust
#[test]
fn protected_policy_rejects_reserved_and_peterfan_processes() {
    assert_eq!(validate_quit_target(0, 99, "Safari"), Err(ProcessQuitError::InvalidPid));
    assert_eq!(validate_quit_target(1, 99, "launchd"), Err(ProcessQuitError::InvalidPid));
    assert_eq!(validate_quit_target(99, 99, "Safari"), Err(ProcessQuitError::Protected));
    assert_eq!(validate_quit_target(100, 99, "peterfand"), Err(ProcessQuitError::Protected));
}

#[test]
fn protected_policy_rejects_unknown_or_other_owner() {
    assert_eq!(validate_owner(None, 501), Err(ProcessQuitError::OwnershipUnknown));
    assert_eq!(validate_owner(Some(502), 501), Err(ProcessQuitError::NotOwned));
    assert_eq!(validate_owner(Some(501), 501), Ok(()));
}

#[test]
fn identity_check_rejects_pid_reuse() {
    assert_eq!(validate_identity(1_000, 1_001), Err(ProcessQuitError::IdentityChanged));
}
```

- [ ] **Step 2: Run policy tests and capture RED**

Run:

```bash
cargo test -p peterfan-platform process_control -- --nocapture
```

Expected: compilation fails because validation helpers and result types are absent.

- [ ] **Step 3: Implement public types, display copy, and pure policy**

Implement:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessQuitRequest {
    pub pid: u32,
    pub expected_start_time_unix_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
```

Implement the private helper
`fn validate_quit_target(pid: u32, current_pid: u32, name: &str) -> Result<(), ProcessQuitError>`.
Implement `Display` with bounded stable English messages. Keep name policy in
the private `is_protected_process_name(name)` helper and expose
`pub fn is_protected_process(pid: u32, name: &str) -> bool`, which also rejects
PID 0/1 and `std::process::id()`.

- [ ] **Step 4: Implement fresh macOS validation and `SIGTERM`**

On macOS, refresh only the requested PID with `ProcessRefreshKind::everything()`.
Require exact start-time equality, effective/current UID equality, and policy
success. Call `libc::kill(pid, libc::SIGTERM)` once. Map a nonzero return to
`SignalFailed(std::io::Error::last_os_error().raw_os_error().unwrap_or(-1))`.

On non-macOS targets, return `Err(ProcessQuitError::Unsupported)` without
spawning `taskkill`, a shell, or another helper.

- [ ] **Step 5: Add a dedicated-child live signal test**

On macOS only, spawn `/bin/sleep 30`, discover that exact child's start time,
call `quit_process`, and poll `try_wait()` for at most two seconds:

```rust
#[cfg(target_os = "macos")]
#[test]
fn guarded_quit_terminates_only_the_spawned_same_user_child() {
    let mut child = std::process::Command::new("/bin/sleep").arg("30").spawn().unwrap();
    let start = refreshed_process_start_time(child.id()).unwrap();
    let result = quit_process(ProcessQuitRequest {
        pid: child.id(),
        expected_start_time_unix_secs: start,
    });
    assert!(result.is_ok());
    assert!(wait_for_child_exit(&mut child, std::time::Duration::from_secs(2)));
}
```

The test cleanup must terminate only its own child if an assertion path leaves it running.

- [ ] **Step 6: Run module and platform tests**

```bash
cargo test -p peterfan-platform process_control -- --nocapture
cargo test -p peterfan-platform
```

Expected: policy and dedicated-child live tests pass; no unrelated process is signalled.

- [ ] **Step 7: Commit Task 2**

```bash
git add packages/platform/src/process_control.rs packages/platform/src/lib.rs
git commit -m "feat: guard macOS process termination"
```

### Task 3: Menu-bar process query and asynchronous action state

**Files:**
- Modify: `packages/menubar/src/main.rs:142`
- Modify: `packages/menubar/src/main.rs:730`
- Modify: `packages/menubar/src/main.rs:2420`
- Modify: `packages/menubar/src/main.rs:3310`
- Modify: `packages/menubar/src/main.rs:3501`
- Modify: `packages/menubar/src/main.rs:3919`
- Modify: `packages/menubar/src/main.rs:4622`
- Test: `packages/menubar/src/main.rs:9725`

**Interfaces:**
- Consumes: Task 1 `processes_matching` and Task 2 `ProcessQuitRequest`/`quit_process`.
- Produces: coalesced `processfilter:` and guarded `quitprocess:` pending commands plus `process_action` dashboard JSON.

- [ ] **Step 1: Write failing command and payload tests**

Add tests requiring:

```rust
#[test]
fn process_filter_commands_coalesce_to_the_latest_query() {
    let mut queue = Vec::new();
    queue_pending_command(&mut queue, "processfilter:saf".into());
    queue_pending_command(&mut queue, "processfilter:safari".into());
    assert_eq!(queue, vec!["processfilter:safari"]);
}

#[test]
fn guarded_process_request_requires_pid_and_start_token() {
    assert_eq!(parse_process_quit_command("quitprocess:42:1700000000"), Some(ProcessQuitRequest { pid: 42, expected_start_time_unix_secs: 1_700_000_000 }));
    assert_eq!(parse_process_quit_command("quitprocess:42"), None);
    assert_eq!(parse_process_quit_command("quitprocess:0:1"), None);
}

#[test]
fn old_direct_kill_ipc_path_is_absent() {
    let source = include_str!("main.rs");
    assert!(!source.contains("strip_prefix(\"killproc:\")"));
    assert!(!source.contains("fn kill_process("));
}
```

- [ ] **Step 2: Run tests and capture RED**

```bash
cargo test -p peterfan-menubar process_filter_commands_coalesce_to_the_latest_query -- --exact
cargo test -p peterfan-menubar guarded_process_request_requires_pid_and_start_token -- --exact
```

Expected: missing parser/state symbols and old direct kill path make the tests fail.

- [ ] **Step 3: Add native state and queue routing**

Add to `App`:

```rust
process_filter: String,
process_action_revision_seen: u64,
```

Add process action state and synchronization:

```rust
#[derive(Clone, Default)]
struct ProcessActionState {
    revision: u64,
    phase: &'static str,
    pid: Option<u32>,
    name: Option<String>,
    code: Option<&'static str>,
    message: Option<String>,
}

static PROCESS_ACTION: Mutex<ProcessActionState> = Mutex::new(ProcessActionState {
    revision: 0,
    phase: "idle",
    pid: None,
    name: None,
    code: None,
    message: None,
});
static PROCESS_ACTION_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static PROCESS_REFRESH_REQUESTED: AtomicBool = AtomicBool::new(false);
```

Give `processfilter:` the queue key `process-filter`. Route both WebView IPC
handlers to `enqueue_pending` for `processfilter:` and `quitprocess:`. Delete
the `killproc:` branches and all platform-local `kill_process` functions.

- [ ] **Step 4: Apply filter and run guarded quit off the event loop**

For `processfilter:`, normalize/cap the query through Task 1's public helper,
store it on `App`, and set `next_dashboard_slow_refresh = now`.

For `quitprocess:`, reject malformed input, enforce the one-in-flight atomic,
publish `pending`, and spawn a background task. The task calls Task 2, maps the
result to stable codes (`requested`, `not_found`, `identity_changed`,
`not_owned`, `protected`, `unsupported`, `signal_failed`), logs the verified
result, updates `PROCESS_ACTION`, sets `PROCESS_REFRESH_REQUESTED`, clears the
in-flight flag, and sends the event proxy.

At the start of each event-loop callback, consume `PROCESS_REFRESH_REQUESTED`,
set both the metric and slow-process deadlines to `now`, and refresh the payload.

- [ ] **Step 5: Filter the complete snapshot and serialize guarded fields**

Add `proc_filter: String` to `DashboardSlowCache`. Refresh slow data when sort
or filter changes. Use five rows for an empty filter and 20 rows otherwise:

```rust
let process_limit = if app.process_filter.is_empty() { 5 } else { 20 };
let procs = app.monitor.processes_matching(process_limit, proc_sort, &app.process_filter);
```

Serialize `pid`, `start_time`, `can_quit`, `name`, CPU, and memory. `can_quit`
is true only on macOS when ownership is `Some(true)`, start time is present,
PID is greater than 1, PID differs from the current process, and the protected
name policy allows the fresh snapshot.

Add `process_filter` and a cloned `process_action` object to dashboard JSON.

- [ ] **Step 6: Run focused menubar tests**

```bash
cargo test -p peterfan-menubar process_filter_ -- --nocapture
cargo test -p peterfan-menubar guarded_process_ -- --nocapture
cargo test -p peterfan-menubar old_direct_kill_ipc_path_is_absent -- --nocapture
```

Expected: all pass and no `killproc:`/direct-kill implementation remains.

- [ ] **Step 7: Commit Task 3**

```bash
git add packages/menubar/src/main.rs
git commit -m "feat: route guarded process actions"
```

### Task 4: Searchable System-view UI and inline confirmation

**Files:**
- Modify: `packages/menubar/src/main.rs:6979`
- Modify: `packages/menubar/src/main.rs:7536`
- Modify: `packages/menubar/src/main.rs:8071`
- Modify: `packages/menubar/src/main.rs:9374`
- Test: `packages/menubar/src/main.rs:10399`

**Interfaces:**
- Consumes: Task 3 dashboard `procs`, `process_filter`, and `process_action` fields.
- Produces: accessible search, read-only/safe row controls, and five-second inline confirmation posting `quitprocess:<pid>:<start_time>`.

- [ ] **Step 1: Write failing dashboard contract tests**

Add one focused test:

```rust
#[test]
fn process_manager_has_search_inline_confirmation_and_accessible_results() {
    let html = dashboard_html(ResolvedLanguage::En, false);
    assert!(html.contains("id=\"process-search\""));
    assert!(html.contains("id=\"process-empty\""));
    assert!(html.contains("id=\"process-action-status\""));
    assert!(html.contains("aria-live=\"polite\""));
    assert!(html.contains("quitprocess:"));
    assert!(html.contains("PROCESS_CONFIRM_WINDOW_MS=5000"));
    assert!(!html.contains("confirm(msg)"));
    assert!(!html.contains("killproc:"));
}
```

Add Korean assertions for `프로세스 검색`, `종료 확인`, `종료 요청 중`, and
the protected/not-owned result copy.

- [ ] **Step 2: Run dashboard test and capture RED**

```bash
cargo test -p peterfan-menubar process_manager_has_search_inline_confirmation_and_accessible_results -- --exact
```

Expected: missing controls/copy and the old modal confirmation fail the test.

- [ ] **Step 3: Add compact process-manager markup and styling**

Inside `sec-procs`, add a labelled search input, result-count/empty node,
process list, and `role="status" aria-live="polite"` action node. Add bounded
styles for the input, PID label, disabled/read-only rows, pending control,
confirmation control, empty result, and action tones without changing the
fixed 440×520 popover frame.

- [ ] **Step 4: Implement focus-preserving search and rendering**

Keep these JS globals:

```javascript
var PROCESS_FILTER_TIMER=0;
var PROCESS_CONFIRM=null;
var PROCESS_CONFIRM_WINDOW_MS=5000;
var PROCESS_ACTION_REVISION=0;
```

On input, preserve the DOM value immediately and debounce
`processfilter:` IPC by 180 ms. Native payload refresh must not overwrite the
field while it owns focus. Escape clears search and confirmation.

Render name, `PID <n>`, CPU, memory, and a button only when `can_quit` is true.
Rows without proof remain visible but read-only. Render a localized empty state
when native filtered results are empty.

- [ ] **Step 5: Replace modal quit with identity-bound inline confirmation**

First click stores `{pid, start, expires}` and rerenders that one control as
`Confirm`. Second click before expiry posts only:

```javascript
window.ipc.postMessage('quitprocess:'+pid+':'+startTime);
```

Changing row/view, Escape, timeout, or new native action revision clears the
confirmation. Pending action disables only the matching row. Map stable native
codes to Korean/English bounded messages in `process-action-status`.

- [ ] **Step 6: Parse embedded JavaScript and run UI tests**

```bash
cargo test -p peterfan-menubar process_manager_ -- --nocapture
cargo test -p peterfan-menubar dashboard_html_translates_known_labels -- --nocapture
```

Expected: all UI contract tests pass. The authoritative embedded-JavaScript
syntax check remains `scripts/smoke-test.sh` and runs after the v1.32.0 release
binaries are built in Task 5.

- [ ] **Step 7: Commit Task 4**

```bash
git add packages/menubar/src/main.rs
git commit -m "feat: add safe searchable process manager"
```

### Task 5: v1.32.0 release truth, regression gates, and local deployment

**Files:**
- Modify: `Cargo.toml:13`
- Modify: `Cargo.lock`
- Modify: `CHANGELOG.md:1`
- Modify: `README.ko.md`
- Modify: `docs/ROADMAP.md:171`
- Modify: `docs/PRODUCTION_CHECKLIST.md:18`
- Modify: `docs/GSTACK_PRODUCT_CHECKLIST.md`
- Test: workspace and release scripts

**Interfaces:**
- Consumes: completed Tasks 1–4.
- Produces: truthful v1.32.0 source metadata, documented roadmap completion, verified release binary, and recoverably updated canonical local app.

- [ ] **Step 1: Add release-document assertions before changing versions**

Extend the existing documentation checks so they require:

```bash
grep -q '^version = "1.32.0"' Cargo.toml
grep -q '^## \[1.32.0\]' CHANGELOG.md
grep -q 'Process list filtering and "quit process" safeguards' docs/ROADMAP.md
grep -q 'safe process control' docs/PRODUCTION_CHECKLIST.md
```

Run `./scripts/check-docs.sh` and capture failure against v1.31.32/missing
release notes.

- [ ] **Step 2: Bump source version and release documentation**

Set workspace version to `1.32.0`, update lockfile package versions through
`cargo check --workspace`, add a changelog section describing search, identity
validation, ownership protection, inline confirmation, and local release truth.
Mark only the process-filter/safeguards roadmap item complete. Do not mark GPU,
TUI, signing, notarization, or public release items complete.

- [ ] **Step 3: Run all deterministic quality gates**

```bash
cargo fmt --all -- --check
./scripts/check-runner-quality.sh
./scripts/check-sleep-display-regressions.sh
./scripts/check-docs.sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release --workspace
./scripts/smoke-test.sh target/release
git diff --check
```

Expected: zero failures and zero Clippy warnings. Record the new passed/ignored
test count in `docs/PRODUCTION_CHECKLIST.md` and rerun docs/diff checks.

- [ ] **Step 4: Build and verify the release bundle**

```bash
cargo build --release --workspace
scripts/bundle-macos.sh target/release/peterfan-menubar dist
target/release/peterfan-menubar --version
plutil -p dist/PeterFan.app/Contents/Info.plist
codesign --verify --deep --strict dist/PeterFan.app
```

Expected: binary and both bundle version keys report `1.32.0`; local ad-hoc
signature verifies. Report that it is not Developer ID/notarized if credentials
remain unavailable.

- [ ] **Step 5: Replace the canonical local app recoverably**

Resolve the exact current PID and executable path first. Move
`/Users/uulab/Documents/workspace/uulab/peterfan/dist/PeterFan.app` to a unique
`PeterFan.app.backup-pre-v1.32.0-<timestamp>` sibling, copy the verified candidate
bundle into the canonical path, stop only the verified old PID, and launch the
canonical bundle. If startup verification fails, restore that exact backup.

- [ ] **Step 6: Verify runtime readiness and single instance**

Require exactly one process whose executable is the canonical PeterFan binary.
Verify the new log segment contains `v1.32.0 startup mock=false`, `tray created`,
`popover webview created`, and `popover webview ready`, with no new panic or
startup rejection. Confirm the bundle and running binary both report v1.32.0.

- [ ] **Step 7: Commit release metadata**

```bash
git add Cargo.toml Cargo.lock CHANGELOG.md README.ko.md docs/ROADMAP.md docs/PRODUCTION_CHECKLIST.md docs/GSTACK_PRODUCT_CHECKLIST.md scripts/check-docs.sh
git commit -m "release: prepare PeterFan v1.32.0"
```

Do not tag, push, sign with a public identity, notarize, or publish unless the
required credentials are present and a separate release instruction authorizes it.

## Completion Audit

- [ ] Every success criterion in `docs/superpowers/specs/2026-08-10-safe-process-control-design.md` has direct code, test, or runtime evidence.
- [ ] The source contains no direct `killproc:` handler, modal `confirm(msg)`, or unguarded `kill_process` function.
- [ ] Search operates on a complete native process snapshot and is bounded only after filtering.
- [ ] Every signal request proves PID, start time, ownership, and protected policy in native code.
- [ ] No source or documentation claim says Windows termination, Developer ID signing, notarization, or public v1.32.0 release is complete.
- [ ] The canonical local app is recoverable from a named backup and runs exactly one ready v1.32.0 process.
