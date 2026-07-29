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
    "README-WINDOWS.txt",
    "LICENSE",
    "CHANGELOG.md"
)
foreach ($Name in $Required) {
    if (-not (Test-Path (Join-Path $Root $Name) -PathType Leaf)) {
        throw "Archive is missing $Name"
    }
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

Write-Host "Windows release archive verified: $Archive"
Write-Host "Version: $ExpectedVersion"
Write-Host "CPU and memory JSON fields are present."
