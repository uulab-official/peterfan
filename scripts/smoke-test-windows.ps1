[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$BinDir
)

$ErrorActionPreference = "Stop"
$Cli = Join-Path $BinDir "peterfan.exe"
$Tray = Join-Path $BinDir "peterfan-menubar.exe"
$Tui = Join-Path $BinDir "peterfan-tui.exe"
$Log = Join-Path $env:APPDATA "peterfan\menubar.log"

foreach ($Binary in @($Cli, $Tray, $Tui)) {
    if (-not (Test-Path $Binary -PathType Leaf)) {
        throw "Missing Windows binary: $Binary"
    }
}

function Wait-ForTrayReady([string]$Path, [int]$TimeoutSeconds = 20) {
    $Deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        if (Test-Path $Path -PathType Leaf) {
            $Content = Get-Content $Path -Raw
            if (
                $Content -match "tray created" -and
                $Content -match "popover webview created" -and
                $Content -match "popover webview ready"
            ) {
                return
            }
        }
        Start-Sleep -Milliseconds 250
    } while ([DateTime]::UtcNow -lt $Deadline)
    $Tail = if (Test-Path $Path) { (Get-Content $Path -Tail 30) -join "`n" } else { "<missing>" }
    throw "Windows tray or WebView2 did not become ready.`n$Tail"
}

& $Cli --version
if ($LASTEXITCODE -ne 0) {
    throw "peterfan.exe --version failed."
}
$Status = (& $Cli --json status) | ConvertFrom-Json
if (
    $null -eq $Status.cpu.usage_percent -or
    $null -eq $Status.memory.used_percent -or
    $Status.metrics_backend -ne "sysinfo" -or
    $Status.simulated_sensors
) {
    throw "Windows status JSON is incomplete."
}
$Doctor = (& $Cli --json doctor) | ConvertFrom-Json
if (
    $Doctor.os -ne "windows" -or
    $Doctor.arch -ne "x86_64" -or
    $Doctor.metrics_backend -ne "sysinfo" -or
    $Doctor.thermal_backend -ne "windows" -or
    -not $Doctor.metrics.cpu -or
    -not $Doctor.metrics.memory -or
    $Doctor.thermal.read_fans -or
    $Doctor.thermal.control_fans
) {
    throw "Windows doctor did not report the expected real metrics and honest thermal capabilities."
}
$ThermalRows = @($Status.temps)
foreach ($Sensor in $ThermalRows) {
    if (
        $Sensor.source -ne "acpi" -or
        $Sensor.kind -ne "mainboard" -or
        $Sensor.value -lt 1 -or
        $Sensor.value -gt 125
    ) {
        throw "Windows reported an invalid or mislabeled ACPI thermal sensor."
    }
}

$Update = (& $Cli --json update) | ConvertFrom-Json
if (
    -not $Update.ok -or
    $Update.asset_name -notmatch "x86_64-pc-windows-msvc\.zip$" -or
    -not $Update.asset_url -or
    -not $Update.checksum_url
) {
    $UpdateDetail = $Update | ConvertTo-Json -Depth 5 -Compress
    throw "Windows update metadata did not select the native verified ZIP: $UpdateDetail"
}

Remove-Item $Log -Force -ErrorAction SilentlyContinue
$First = Start-Process $Tray -PassThru
Wait-ForTrayReady $Log
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

Write-Host "Windows smoke test passed: real metrics/capabilities, tray, WebView2, single instance, restart."
