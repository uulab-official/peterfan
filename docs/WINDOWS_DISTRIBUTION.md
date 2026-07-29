# Windows Distribution

PeterFan ships one x64 Windows archive:

```text
peterfan-vX.Y.Z-x86_64-pc-windows-msvc.zip
```

The archive contains the tray app as `PeterFan.exe`, the `peterfan-cli.exe` CLI,
the `peterfan-tui.exe` terminal dashboard, per-user install/uninstall scripts,
license and changelog files, and Windows-specific support notes.

## Build and test

The [Windows workflow](../.github/workflows/windows.yml) runs on
`windows-latest` for pushes, pull requests, published releases, and manual
dispatches. It performs:

1. Workspace tests against `x86_64-pc-windows-msvc`.
2. Release compilation for the full workspace.
3. Real CPU and memory JSON smoke checks.
4. Tray startup, duplicate-launch rejection, exit, and restart checks.
5. ZIP packaging and extraction-based validation.
6. Workflow artifact upload.
7. GitHub Release attachment and shared checksum refresh for release runs.

The macOS release remains locally signed and notarized. Publishing a macOS
release triggers the Windows workflow through the GitHub `release.published`
event, so Windows does not need access to Apple signing material.

## Install model

`install.ps1` installs per user to:

```text
%LOCALAPPDATA%\Programs\PeterFan
```

It creates a Start menu shortcut and can optionally enable Start on login. The
app's Settings screen controls the same current-user Run registry value. No
administrator approval is required.

## Hardware scope

The Windows build uses real cross-platform system metrics for CPU, memory,
disks, networks, processes, and battery. The hardware provider reports
temperature, fan RPM, and fan control as unsupported until PeterFan has a
tested EC/WMI backend for the machine. Production Windows builds never fall
back to simulated sensors.

## Manual release recovery

If the `release.published` event did not attach the Windows ZIP, run the
Windows workflow manually with the existing tag. The workflow verifies that
the tag and Cargo version match before uploading or changing checksums.
