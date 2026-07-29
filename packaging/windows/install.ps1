[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "Programs\PeterFan"),
    [switch]$StartAtLogin,
    [switch]$NoLaunch
)

$ErrorActionPreference = "Stop"
$SourceDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RequiredFiles = @("PeterFan.exe", "peterfan.exe", "peterfan-tui.exe", "uninstall.ps1")

foreach ($Name in $RequiredFiles) {
    if (-not (Test-Path (Join-Path $SourceDir $Name) -PathType Leaf)) {
        throw "Missing required package file: $Name"
    }
}

Get-Process -Name "PeterFan" -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
foreach ($Name in $RequiredFiles + @("README-WINDOWS.txt", "LICENSE", "CHANGELOG.md")) {
    $Source = Join-Path $SourceDir $Name
    if (Test-Path $Source -PathType Leaf) {
        Copy-Item $Source (Join-Path $InstallDir $Name) -Force
    }
}

$ProgramsDir = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
$ShortcutPath = Join-Path $ProgramsDir "PeterFan.lnk"
$Shell = New-Object -ComObject WScript.Shell
$Shortcut = $Shell.CreateShortcut($ShortcutPath)
$Shortcut.TargetPath = Join-Path $InstallDir "PeterFan.exe"
$Shortcut.WorkingDirectory = $InstallDir
$Shortcut.Description = "PeterFan system monitor"
$Shortcut.Save()

if ($StartAtLogin) {
    $RunKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
    New-Item -Path $RunKey -Force | Out-Null
    Set-ItemProperty -Path $RunKey -Name "PeterFan" -Value ('"{0}"' -f (Join-Path $InstallDir "PeterFan.exe"))
}

if (-not $NoLaunch) {
    Start-Process (Join-Path $InstallDir "PeterFan.exe")
}

Write-Host "PeterFan installed to $InstallDir"
