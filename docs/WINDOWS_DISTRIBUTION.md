# Windows Distribution

PeterFan ships one x64 Windows archive:

```text
peterfan-vX.Y.Z-x86_64-pc-windows-msvc.zip
```

The archive contains the tray app as `PeterFan.exe`, the `peterfan-cli.exe` CLI,
the `peterfan-tui.exe` terminal dashboard, per-user install/uninstall scripts,
the Microsoft-signed WebView2 Evergreen bootstrapper, license and changelog
files, and Windows-specific support notes.

## Build and test

The [Windows workflow](../.github/workflows/windows.yml) runs on
`windows-2022` for pushes, pull requests, and manual release dispatches. It
performs:

1. Workspace tests against `x86_64-pc-windows-msvc`.
2. Release compilation for the full workspace.
3. Real CPU and memory JSON smoke checks.
4. Native tray and WebView2 creation, duplicate-launch rejection, exit, and restart checks.
5. ZIP packaging and Microsoft Authenticode validation of the WebView2 bootstrapper.
6. Per-user install, Start menu, start-on-login, installed-app launch, and uninstall checks.
7. Workflow artifact upload or draft GitHub Release attachment and shared checksum refresh.

The macOS release remains locally signed and notarized. The local release
script creates a draft, dispatches this Windows workflow, waits for the verified
ZIP, and only then publishes. The CI release workflow similarly requires both
platform jobs before it creates the GitHub Release, so Windows never needs
access to Apple signing material.

## Install model

`install.ps1` installs per user to:

```text
%LOCALAPPDATA%\Programs\PeterFan
```

It creates a Start menu shortcut and can optionally enable Start on login. The
app's Settings screen controls the same current-user Run registry value. No
administrator approval is required.

PeterFan's popover uses Microsoft Edge WebView2. Windows 11 includes the
Evergreen Runtime and most Windows 10 systems already have it. The installer
checks Microsoft's documented registry locations and runs the bundled,
Microsoft-signed bootstrapper only when the runtime is absent.

## Hardware scope

The Windows build uses real cross-platform system metrics for CPU, memory,
disks, networks, processes, and battery. On systems whose firmware exposes
`MSAcpi_ThermalZoneTemperature`, PeterFan also shows the ACPI reading as
**System Thermal Zone**. It is deliberately not labeled as CPU temperature:
many vendors expose a chassis or board zone instead of CPU cores.

CPU-core temperature, fan RPM, and fan control remain unavailable until
PeterFan has a tested vendor-specific EC/WMI backend for the machine.
Production Windows builds never fall back to simulated sensors.

Update checks select the native Windows x64 ZIP, verify both the GitHub asset
digest and `checksums.txt`, validate the embedded CLI version, then run the
same per-user installer used for a clean install. Existing startup preference
is preserved and no administrator prompt is required.

## Manual release recovery

To rebuild or repair an existing release asset, run the Windows workflow
manually with the existing tag. The workflow verifies that the tag and Cargo
version match before uploading or changing checksums.
