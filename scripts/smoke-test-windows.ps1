[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$BinDir
)

$ErrorActionPreference = "Stop"
$Cli = Join-Path $BinDir "peterfan.exe"
$Tray = Join-Path $BinDir "peterfan-menubar.exe"
$Tui = Join-Path $BinDir "peterfan-tui.exe"

foreach ($Binary in @($Cli, $Tray, $Tui)) {
    if (-not (Test-Path $Binary -PathType Leaf)) {
        throw "Missing Windows binary: $Binary"
    }
}

& $Cli --version
if ($LASTEXITCODE -ne 0) {
    throw "peterfan.exe --version failed."
}
$Status = (& $Cli --json status) | ConvertFrom-Json
if ($null -eq $Status.cpu_pct -or $null -eq $Status.mem_pct) {
    throw "Windows status JSON is incomplete."
}

$First = Start-Process $Tray -PassThru
Start-Sleep -Seconds 4
if ($First.HasExited) {
    throw "Windows tray app exited during startup."
}

$Second = Start-Process $Tray -PassThru -Wait
if ($Second.ExitCode -ne 0) {
    throw "Second tray launch did not exit cleanly."
}
$Running = @(Get-Process -Name "peterfan-menubar" -ErrorAction SilentlyContinue)
if ($Running.Count -ne 1) {
    throw "Expected one tray process after duplicate launch, found $($Running.Count)."
}

Stop-Process -Id $First.Id -Force
Start-Sleep -Seconds 1
$Restarted = Start-Process $Tray -PassThru
Start-Sleep -Seconds 3
if ($Restarted.HasExited) {
    throw "Tray app could not restart after the first process exited."
}
Stop-Process -Id $Restarted.Id -Force

Write-Host "Windows smoke test passed: metrics JSON, single instance, restart."
