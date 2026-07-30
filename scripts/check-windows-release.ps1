[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Archive,
    [Parameter(Mandatory = $true)]
    [string]$ExpectedVersion
)

$ErrorActionPreference = "Stop"
$ExpectedVersion = $ExpectedVersion.TrimStart("v")
if (-not (Test-Path $Archive -PathType Leaf)) {
    throw "Windows archive not found: $Archive"
}

$Work = Join-Path $env:RUNNER_TEMP "peterfan-windows-release-check"
if (Test-Path $Work) {
    Remove-Item $Work -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $Work | Out-Null
Expand-Archive -Path $Archive -DestinationPath $Work -Force

$Roots = @(Get-ChildItem $Work -Directory)
if ($Roots.Count -ne 1) {
    throw "Archive must contain exactly one top-level directory."
}
$Root = $Roots[0].FullName
$Required = @(
    "PeterFan.exe",
    "peterfan-cli.exe",
    "peterfan-tui.exe",
    "install.ps1",
    "uninstall.ps1",
    "MicrosoftEdgeWebview2Setup.exe",
    "README-WINDOWS.txt",
    "LICENSE",
    "CHANGELOG.md"
)
foreach ($Name in $Required) {
    if (-not (Test-Path (Join-Path $Root $Name) -PathType Leaf)) {
        throw "Archive is missing $Name"
    }
}

$Bootstrapper = Join-Path $Root "MicrosoftEdgeWebview2Setup.exe"
$BootstrapperSignature = Get-AuthenticodeSignature -FilePath $Bootstrapper
if (
    $BootstrapperSignature.Status -ne [System.Management.Automation.SignatureStatus]::Valid -or
    $null -eq $BootstrapperSignature.SignerCertificate -or
    $BootstrapperSignature.SignerCertificate.Subject -notmatch "CN=Microsoft Corporation"
) {
    throw "Packaged WebView2 bootstrapper is not validly signed by Microsoft."
}

$CliVersion = & (Join-Path $Root "peterfan-cli.exe") --version
if ($LASTEXITCODE -ne 0 -or $CliVersion -notmatch [regex]::Escape($ExpectedVersion)) {
    throw "CLI version mismatch: $CliVersion"
}

$StatusJson = & (Join-Path $Root "peterfan-cli.exe") --json status
if ($LASTEXITCODE -ne 0) {
    throw "peterfan --json status failed."
}
$Status = $StatusJson | ConvertFrom-Json
if (
    $null -eq $Status.cpu.usage_percent -or
    $null -eq $Status.memory.used_percent -or
    $Status.metrics_backend -ne "sysinfo"
) {
    throw "Status JSON is missing real system metric fields."
}

& (Join-Path $Root "PeterFan.exe") --version
if ($LASTEXITCODE -ne 0) {
    throw "PeterFan.exe --version failed."
}

$InstallRoot = Join-Path $env:RUNNER_TEMP "peterfan-windows-installed"
$Log = Join-Path $env:APPDATA "peterfan\menubar.log"
$RunKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
$Shortcut = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\PeterFan.lnk"
if (Test-Path $InstallRoot) {
    Remove-Item $InstallRoot -Recurse -Force
}
Remove-Item $Log -Force -ErrorAction SilentlyContinue

try {
    & (Join-Path $Root "install.ps1") `
        -InstallDir $InstallRoot `
        -StartAtLogin `
        -NoLaunch
    if ($LASTEXITCODE -ne 0) {
        throw "Windows per-user installer failed."
    }

    foreach ($Name in @("PeterFan.exe", "peterfan-cli.exe", "peterfan-tui.exe", "uninstall.ps1")) {
        if (-not (Test-Path (Join-Path $InstallRoot $Name) -PathType Leaf)) {
            throw "Installed PeterFan is missing $Name"
        }
    }
    $RunValue = (Get-ItemProperty -Path $RunKey -Name "PeterFan" -ErrorAction Stop).PeterFan
    if ($RunValue -notmatch [regex]::Escape((Join-Path $InstallRoot "PeterFan.exe"))) {
        throw "Start-on-login registry value does not point to the installed PeterFan.exe."
    }
    if (-not (Test-Path $Shortcut -PathType Leaf)) {
        throw "PeterFan Start menu shortcut was not created."
    }

    $InstalledStatus = (& (Join-Path $InstallRoot "peterfan-cli.exe") --json status) | ConvertFrom-Json
    if (
        $null -eq $InstalledStatus.cpu.usage_percent -or
        $null -eq $InstalledStatus.memory.used_percent -or
        $InstalledStatus.metrics_backend -ne "sysinfo"
    ) {
        throw "Installed PeterFan CLI did not return real Windows metrics."
    }

    $InstalledTray = Start-Process (Join-Path $InstallRoot "PeterFan.exe") -PassThru
    $Deadline = [DateTime]::UtcNow.AddSeconds(45)
    do {
        if (Test-Path $Log -PathType Leaf) {
            $LogContent = Get-Content $Log -Raw
            if (
                $LogContent -match "tray created" -and
                $LogContent -match "popover webview created" -and
                $LogContent -match "popover webview ready"
            ) {
                break
            }
        }
        if ($InstalledTray.HasExited) {
            throw "Installed PeterFan tray app exited before WebView2 became ready."
        }
        Start-Sleep -Milliseconds 250
    } while ([DateTime]::UtcNow -lt $Deadline)
    if (-not $LogContent -or $LogContent -notmatch "popover webview ready") {
        $Tail = if (Test-Path $Log) { (Get-Content $Log -Tail 40) -join "`n" } else { "<missing>" }
        throw "Installed PeterFan tray and WebView2 did not become ready.`n$Tail"
    }
} finally {
    if (Test-Path (Join-Path $InstallRoot "uninstall.ps1")) {
        & (Join-Path $InstallRoot "uninstall.ps1") -InstallDir $InstallRoot
    }
    $Deadline = [DateTime]::UtcNow.AddSeconds(15)
    while ((Test-Path $InstallRoot) -and [DateTime]::UtcNow -lt $Deadline) {
        Start-Sleep -Milliseconds 250
    }
}

if (Test-Path $InstallRoot) {
    throw "PeterFan uninstaller did not remove the per-user installation."
}
if ((Get-ItemProperty -Path $RunKey -Name "PeterFan" -ErrorAction SilentlyContinue).PeterFan) {
    throw "PeterFan uninstaller left the start-on-login registry value behind."
}
if (Test-Path $Shortcut) {
    throw "PeterFan uninstaller left the Start menu shortcut behind."
}

Write-Host "Windows release archive verified: $Archive"
Write-Host "Version: $ExpectedVersion"
Write-Host "Real metrics, Microsoft WebView2 signature, install, tray readiness, startup, and uninstall passed."
