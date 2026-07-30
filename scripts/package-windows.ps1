[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Version,
    [string]$TargetDir = "target\x86_64-pc-windows-msvc\release",
    [string]$OutputDir = "dist",
    [string]$WebView2Bootstrapper = ""
)

$ErrorActionPreference = "Stop"
$Version = $Version.TrimStart("v")
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "Version must be semantic X.Y.Z, got '$Version'."
}

$Name = "peterfan-v$Version-x86_64-pc-windows-msvc"
$Stage = Join-Path $OutputDir $Name
$Archive = Join-Path $OutputDir "$Name.zip"
$WebView2Download = "https://go.microsoft.com/fwlink/p/?LinkId=2124703"

if (Test-Path $Stage) {
    Remove-Item $Stage -Recurse -Force
}
if (Test-Path $Archive) {
    Remove-Item $Archive -Force
}
New-Item -ItemType Directory -Force -Path $Stage | Out-Null

$Binaries = @{
    "peterfan.exe" = "peterfan-cli.exe"
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

if (-not $WebView2Bootstrapper) {
    $WebView2Bootstrapper = Join-Path $env:TEMP "PeterFan-MicrosoftEdgeWebview2Setup.exe"
    Invoke-WebRequest -Uri $WebView2Download -OutFile $WebView2Bootstrapper -UseBasicParsing
}
if (-not (Test-Path $WebView2Bootstrapper -PathType Leaf)) {
    throw "Missing WebView2 bootstrapper: $WebView2Bootstrapper"
}
$Signature = Get-AuthenticodeSignature -FilePath $WebView2Bootstrapper
if (
    $Signature.Status -ne [System.Management.Automation.SignatureStatus]::Valid -or
    $null -eq $Signature.SignerCertificate -or
    $Signature.SignerCertificate.Subject -notmatch "CN=Microsoft Corporation"
) {
    throw "WebView2 bootstrapper does not have a valid Microsoft signature."
}
Copy-Item $WebView2Bootstrapper (Join-Path $Stage "MicrosoftEdgeWebview2Setup.exe")

Compress-Archive -Path $Stage -DestinationPath $Archive -CompressionLevel Optimal
$Hash = (Get-FileHash $Archive -Algorithm SHA256).Hash.ToLowerInvariant()

Write-Host "Built $Archive"
Write-Host "$Hash  $([System.IO.Path]::GetFileName($Archive))"
if ($env:GITHUB_OUTPUT) {
    "archive=$Archive" | Out-File -FilePath $env:GITHUB_OUTPUT -Append
    "asset_name=$([System.IO.Path]::GetFileName($Archive))" | Out-File -FilePath $env:GITHUB_OUTPUT -Append
    "sha256=$Hash" | Out-File -FilePath $env:GITHUB_OUTPUT -Append
}
