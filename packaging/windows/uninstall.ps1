[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "Programs\PeterFan")
)

$ErrorActionPreference = "Stop"

Get-Process -Name "PeterFan" -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue

$RunKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
Remove-ItemProperty -Path $RunKey -Name "PeterFan" -ErrorAction SilentlyContinue

$ShortcutPath = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\PeterFan.lnk"
Remove-Item $ShortcutPath -Force -ErrorAction SilentlyContinue

if (Test-Path $InstallDir -PathType Container) {
    $Cleanup = @"
Start-Sleep -Milliseconds 500
Remove-Item -LiteralPath '$($InstallDir.Replace("'", "''"))' -Recurse -Force -ErrorAction SilentlyContinue
"@
    Start-Process powershell.exe -WindowStyle Hidden -ArgumentList @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-Command", $Cleanup
    )
}

Write-Host "PeterFan uninstall queued."
