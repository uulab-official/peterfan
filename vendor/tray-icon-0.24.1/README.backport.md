# PeterFan macOS 27 click forwarding backport

This directory contains the exact packaged `tray-icon 0.24.1` crate, with only
the official macOS menu-lifecycle fix described below. PeterFan selects it with
the root `[patch.crates-io]` entry. Its package version and dependencies remain
unchanged. No global Cargo registry source is modified.

## Package provenance

- Crates.io package: <https://crates.io/crates/tray-icon/0.24.1>
- Archive: <https://static.crates.io/crates/tray-icon/tray-icon-0.24.1.crate>
- Archive SHA-256 / registry checksum:
  `65ba1e5f6b9ef9fd87e21b9c6f351554dbd717960089168fcfdef854686961dc`
- Packaged VCS commit: `304c7fb3779ae898c75d1d3d5be5f8f6997f0c4c` in
  <https://github.com/tauri-apps/tray-icon> (empty `path_in_vcs`).
- The archive checksum was checked against the existing PeterFan lockfile and
  the crates.io sparse registry entry. Every retained packaged file was also
  compared byte-for-byte with the cached unpacked package before patching.
- Upstream `Cargo.toml`, `Cargo.toml.orig`, `README.md`, source files, and all
  three license files are retained. The packaged development `Cargo.lock` and
  `.cargo_vcs_info.json` are omitted; the VCS information is recorded above.
  Cargo cache markers are not included. The workspace lockfile remains the
  dependency authority.

## Sole source change

Backported upstream commit
[`42eb44ea1507d51b68a8b2fbb0d96a9c85f5b4cd`](https://github.com/tauri-apps/tray-icon/commit/42eb44ea1507d51b68a8b2fbb0d96a9c85f5b4cd),
PR [#365](https://github.com/tauri-apps/tray-icon/pull/365), originally released in
[`tray-icon 0.25.1`](https://github.com/tauri-apps/tray-icon/releases/tag/tray-icon-v0.25.1).
The upstream changeset is available as
[a patch](https://github.com/tauri-apps/tray-icon/commit/42eb44ea1507d51b68a8b2fbb0d96a9c85f5b4cd.patch).

Only `src/platform_impl/macos/mod.rs` changes: 17 added and 20 removed lines,
matching the upstream patch's added/deleted source lines. The original 0.24.1
code surrounding those lines remains intact; no 0.24.2 optimizations or other
platform changes are included. The upstream release-note file is not needed
for the backport.

On macOS 27, permanently assigning an `NSMenu` to the status item can intercept
left clicks before the tray's event view receives them, even when left-click
menu presentation is disabled. The fix retains the menu separately and assigns
it to the status item only during menu presentation. The retained clone also
avoids holding a `RefCell` borrow across the nested menu event loop.

The backport preserves the workspace's Rust 1.88 requirement. Official 0.25.1
requires Rust 1.90 and changes the menu dependency/features, so upgrading it
would expand this bug fix. Keeping the official fix inside the owning library
also avoids a second application-level native menu routing implementation.

## Removal

Remove this directory and the root patch entry when PeterFan's approved MSRV
supports Rust 1.90 and the workspace can adopt official `tray-icon 0.25.1` or a
later compatible release. Update the dependency and lockfile, then verify native
left-click dashboard toggling, right-click menu actions/dismissal, programmatic
menu presentation, and retained runner rendering/geometry on supported macOS
versions before removing the backport.
