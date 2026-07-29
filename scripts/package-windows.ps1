[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Version,
    [string]$TargetDir = "target\x86_64-pc-windows-msvc\release",
    [string]$OutputDir = "dist"
)

$ErrorActionPreference = "Stop"
$Version = $Version.TrimStart("v")
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "Version must be semantic X.Y.Z, got '$Version'."
}

$Name = "peterfan-v$Version-x86_64-pc-windows-msvc"
$Stage = Join-Path $OutputDir $Name
$Archive = Join-Path $OutputDir "$Name.zip"

if (Test-Path $Stage) {
    Remove-Item $Stage -Recurse -Force
}
if (Test-Path $Archive) {
    Remove-Item $Archive -Force
}
New-Item -ItemType Directory -Force -Path $Stage | Out-Null

$Binaries = @{
    "peterfan.exe" = "peterfan.exe"
    "peterfan-tui.exe" = "peterfan-tui.exe"
    "peterfan-menubar.exe" = "PeterFan.exe"
}
foreach ($SourceName in $Binaries.Keys) {
    $Source = Join-Path $TargetDir $SourceName
    if (-not (Test-Path $Source -PathType Leaf)) {
        throw "Missing release binary: $Source"
    }
    Copy-Item $Source (Join-Path $Stage $Binaries[$SourceName])
}

Copy-Item "packaging\windows\install.ps1" $Stage
Copy-Item "packaging\windows\uninstall.ps1" $Stage
Copy-Item "packaging\windows\README-WINDOWS.txt" $Stage
Copy-Item "LICENSE" $Stage
Copy-Item "CHANGELOG.md" $Stage

Compress-Archive -Path $Stage -DestinationPath $Archive -CompressionLevel Optimal
$Hash = (Get-FileHash $Archive -Algorithm SHA256).Hash.ToLowerInvariant()

Write-Host "Built $Archive"
Write-Host "$Hash  $([System.IO.Path]::GetFileName($Archive))"
if ($env:GITHUB_OUTPUT) {
    "archive=$Archive" | Out-File -FilePath $env:GITHUB_OUTPUT -Append
    "asset_name=$([System.IO.Path]::GetFileName($Archive))" | Out-File -FilePath $env:GITHUB_OUTPUT -Append
    "sha256=$Hash" | Out-File -FilePath $env:GITHUB_OUTPUT -Append
}
