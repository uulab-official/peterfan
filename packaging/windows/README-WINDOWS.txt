PeterFan for Windows
====================

Quick start
-----------

1. Extract the ZIP.
2. Run install.ps1 from PowerShell.
3. Launch PeterFan from the Start menu.

The installer is per-user and writes to:

  %LOCALAPPDATA%\Programs\PeterFan

It does not need administrator approval. The PeterFan Settings screen can
enable or disable "Start on login" using the current user's registry Run key.

Included programs
-----------------

  PeterFan.exe        Windows tray app
  peterfan-cli.exe    Command-line interface
  peterfan-tui.exe    Terminal dashboard
  install.ps1         Per-user installer
  uninstall.ps1       Per-user uninstaller

Current Windows support
-----------------------

CPU, memory, disks, networks, processes, and battery data use the real
cross-platform sysinfo/battery backends. Temperature, fan RPM, and fan control
are shown as unavailable unless a real Windows hardware backend is added.
PeterFan never substitutes simulated sensor values in a normal Windows build.

Verification
------------

Every GitHub Release includes checksums.txt. Verify the ZIP with:

  (Get-FileHash .\peterfan-vX.Y.Z-x86_64-pc-windows-msvc.zip -Algorithm SHA256).Hash

Compare the result with the matching line in checksums.txt.
