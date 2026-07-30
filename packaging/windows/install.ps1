[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "Programs\PeterFan"),
    [switch]$StartAtLogin,
    [switch]$NoLaunch,
    [switch]$SkipWebView2Install
)

$ErrorActionPreference = "Stop"
$SourceDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RequiredFiles = @("PeterFan.exe", "peterfan-cli.exe", "peterfan-tui.exe", "uninstall.ps1")
$WebView2ClientId = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
$WebView2Download = "https://go.microsoft.com/fwlink/p/?LinkId=2124703"

foreach ($Name in $RequiredFiles) {
    if (-not (Test-Path (Join-Path $SourceDir $Name) -PathType Leaf)) {
        throw "Missing required package file: $Name"
    }
}

function Get-WebView2RuntimeVersion {
    $Locations = @(
        "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\$WebView2ClientId",
        "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$WebView2ClientId",
        "HKCU:\Software\Microsoft\EdgeUpdate\Clients\$WebView2ClientId"
    )
    foreach ($Location in $Locations) {
        try {
            $Version = (Get-ItemProperty -LiteralPath $Location -Name "pv" -ErrorAction Stop).pv
            if ($Version -and $Version -ne "0.0.0.0") {
                return [string]$Version
            }
        } catch {
            continue
        }
    }
    return $null
}

function Assert-MicrosoftSignedBootstrapper([string]$Path) {
    $Signature = Get-AuthenticodeSignature -FilePath $Path
    if (
        $Signature.Status -ne [System.Management.Automation.SignatureStatus]::Valid -or
        $null -eq $Signature.SignerCertificate -or
        $Signature.SignerCertificate.Subject -notmatch "CN=Microsoft Corporation"
    ) {
        throw "WebView2 bootstrapper does not have a valid Microsoft signature."
    }
}

function Install-WebView2Runtime {
    $InstalledVersion = Get-WebView2RuntimeVersion
    if ($InstalledVersion) {
        Write-Host "Microsoft Edge WebView2 Runtime $InstalledVersion is ready."
        return
    }
    if ($SkipWebView2Install) {
        throw "Microsoft Edge WebView2 Runtime is required but is not installed."
    }

    $PackagedBootstrapper = Join-Path $SourceDir "MicrosoftEdgeWebview2Setup.exe"
    $TemporaryBootstrapper = $null
    if (Test-Path $PackagedBootstrapper -PathType Leaf) {
        $Bootstrapper = $PackagedBootstrapper
    } else {
        $TemporaryBootstrapper = Join-Path $env:TEMP "PeterFan-WebView2Setup.exe"
        Write-Host "Downloading the Microsoft Edge WebView2 Runtime bootstrapper..."
        Invoke-WebRequest -Uri $WebView2Download -OutFile $TemporaryBootstrapper -UseBasicParsing
        $Bootstrapper = $TemporaryBootstrapper
    }

    try {
        Assert-MicrosoftSignedBootstrapper $Bootstrapper
        $Install = Start-Process -FilePath $Bootstrapper `
            -ArgumentList @("/silent", "/install") `
            -Wait `
            -PassThru
        $InstalledVersion = Get-WebView2RuntimeVersion
        if (-not $InstalledVersion) {
            throw "WebView2 Runtime installation failed with exit code $($Install.ExitCode)."
        }
        Write-Host "Installed Microsoft Edge WebView2 Runtime $InstalledVersion."
    } finally {
        if ($TemporaryBootstrapper -and (Test-Path $TemporaryBootstrapper)) {
            Remove-Item -LiteralPath $TemporaryBootstrapper -Force -ErrorAction SilentlyContinue
        }
    }
}

Install-WebView2Runtime

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
