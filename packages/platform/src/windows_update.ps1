# Embedded in the already-running, trusted updater. Never execute the installer
# from the downloaded archive: OTA updates do not change shortcuts or registry.
$ErrorActionPreference = 'Stop'
$HealthTimeoutMs = 45000
$SurvivalMs = 2000
$ExitTimeoutMs = 10000
$pollMs = 100
$transaction = [Guid]::NewGuid().ToString('N')
$installDir = [IO.Path]::GetFullPath($spec.install_dir)
$payloadDir = [IO.Path]::GetFullPath($spec.payload_dir)
$result = [IO.Path]::GetFullPath($spec.result_path)
$existing = Join-Path $installDir 'PeterFan.exe'
$stage = Join-Path $installDir ".peterfan-stage-$transaction"
$backup = Join-Path $installDir ".peterfan-backup-$transaction"
$health = Join-Path (Split-Path -Parent $result) "update-health-$($spec.nonce).json"
$lock = $null
$launched = $null
$journal = [Collections.Generic.List[object]]::new()
$committed = $false
$preserveBackup = $false
$allowed = @('PeterFan.exe', 'peterfan-cli.exe', 'peterfan-tui.exe', 'uninstall.ps1',
    'README-WINDOWS.txt', 'LICENSE', 'CHANGELOG.md', 'THIRD_PARTY_NOTICES.md')

function Assert-Regular([string]$path) {
    $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "Update file must be a regular file: $path"
    }
}

function Write-Result([string]$status, [string]$message) {
    $directory = Split-Path -Parent $result
    [IO.Directory]::CreateDirectory($directory) | Out-Null
    $temporary = "$result.tmp-$transaction"
    $json = @{ status = $status; version = $spec.version; message = $message;
        updated_at_unix = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds() } | ConvertTo-Json -Compress
    try {
        [IO.File]::WriteAllText($temporary, $json, [Text.UTF8Encoding]::new($false))
        if ([IO.File]::Exists($result)) { [IO.File]::Replace($temporary, $result, $null) }
        else { [IO.File]::Move($temporary, $result) }
    } finally {
        if ([IO.File]::Exists($temporary)) { [IO.File]::Delete($temporary) }
    }
}

function Owned-Processes {
    foreach ($name in @('PeterFan', 'peterfan-cli', 'peterfan-tui')) {
        foreach ($process in @(Get-Process -Name $name -ErrorAction SilentlyContinue)) {
            # Other portable installations and processes merely sharing a name
            # are outside this transaction. Never stop a process by name.
            try { $path = $process.Path } catch { continue }
            if ($path -and [String]::Equals([IO.Path]::GetFullPath($path),
                    (Join-Path $installDir "$name.exe"), [StringComparison]::OrdinalIgnoreCase)) {
                $process
            }
        }
    }
}

function Wait-For-Exit {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while (@(Owned-Processes).Count -ne 0) {
        if ($clock.ElapsedMilliseconds -ge $ExitTimeoutMs) {
            throw 'The installed PeterFan app or CLI is still running. Close it and retry the update.'
        }
        Start-Sleep -Milliseconds $pollMs
    }
}

function Stop-Replacement {
    if ($null -ne $script:launched) {
        $script:launched.Refresh()
        if (-not $script:launched.HasExited) {
            # The retained Process object identifies only the child we started.
            $script:launched.Kill()
            if (-not $script:launched.WaitForExit(10000)) { throw 'The failed replacement did not exit.' }
        }
    }
}

function Launch-Previous {
    if (@(Owned-Processes).Count -eq 0 -and (Test-Path -LiteralPath $existing -PathType Leaf)) {
        Start-Process -FilePath $existing -WorkingDirectory $installDir | Out-Null
    }
}

function Launch-Healthy {
    Remove-Item -LiteralPath $health -Force -ErrorAction SilentlyContinue
    $script:launched = Start-Process -FilePath $existing -WorkingDirectory $installDir `
        -ArgumentList @('--update-health-token', $spec.nonce) -PassThru
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $healthySince = $null
    while ($clock.ElapsedMilliseconds -lt $HealthTimeoutMs) {
        $script:launched.Refresh()
        if ($script:launched.HasExited) { throw 'The replacement exited before startup became healthy.' }
        $receipt = $null
        if (Test-Path -LiteralPath $health -PathType Leaf) {
            try { $receipt = Get-Content -LiteralPath $health -Raw | ConvertFrom-Json } catch { }
        }
        $matching = $null -ne $receipt -and $receipt.nonce -ceq $spec.nonce -and
            $receipt.pid -eq $script:launched.Id -and $receipt.version -ceq $spec.version
        if ($matching) {
            $path = $script:launched.Path
            if (-not [String]::Equals([IO.Path]::GetFullPath($path), $existing,
                    [StringComparison]::OrdinalIgnoreCase)) { throw 'Startup process path mismatch.' }
            if ($null -eq $healthySince) { $healthySince = $clock.ElapsedMilliseconds }
            if ($clock.ElapsedMilliseconds - $healthySince -ge $SurvivalMs) { return }
        } else { $healthySince = $null }
        Start-Sleep -Milliseconds $pollMs
    }
    throw 'The replacement did not confirm tray and WebView readiness before the startup deadline.'
}

try {
    $root = Get-Item -LiteralPath $installDir -Force
    if (-not $root.PSIsContainer -or ($root.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'The installation directory is not a regular directory.'
    }
    # The stable lock file is deliberately retained: unlinking it would let two
    # helpers lock separate file identities for the same installation.
    $lockPath = Join-Path $installDir '.peterfan-update.lock'
    if (Test-Path -LiteralPath $lockPath) { Assert-Regular $lockPath }
    $lock = [IO.File]::Open($lockPath, [IO.FileMode]::OpenOrCreate,
        [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    foreach ($required in @('PeterFan.exe', 'peterfan-cli.exe', 'peterfan-tui.exe', 'uninstall.ps1')) {
        if ($null -eq $spec.files.PSObject.Properties[$required]) { throw "Missing update file: $required" }
    }
    [IO.Directory]::CreateDirectory($stage) | Out-Null
    foreach ($entry in $spec.files.PSObject.Properties) {
        $name = $entry.Name
        if ($allowed -cnotcontains $name -or $entry.Value -cnotmatch '^[0-9a-f]{64}$') {
            throw 'Invalid update file manifest.'
        }
        $source = Join-Path $payloadDir $name
        $target = Join-Path $stage $name
        Assert-Regular $source
        Copy-Item -LiteralPath $source -Destination $target -ErrorAction Stop
        if ((Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant() -cne $entry.Value) {
            throw "Staged update checksum mismatch: $name"
        }
        $installed = Join-Path $installDir $name
        if (Test-Path -LiteralPath $installed) { Assert-Regular $installed }
    }
    Wait-For-Exit
    [IO.Directory]::CreateDirectory($backup) | Out-Null
    foreach ($entry in $spec.files.PSObject.Properties) {
        $name = $entry.Name
        $target = Join-Path $installDir $name
        $saved = Join-Path $backup $name
        $record = [PSCustomObject]@{ Name = $name; Saved = $false; Applied = $false }
        $journal.Add($record)
        if (Test-Path -LiteralPath $target) {
            [IO.File]::Move($target, $saved)
            $record.Saved = $true
        }
        [IO.File]::Move((Join-Path $stage $name), $target)
        $record.Applied = $true
    }
    Launch-Healthy
    # Keep all previous files until the healthy receipt is durably recorded.
    Write-Result 'installed' "PeterFan v$($spec.version) started successfully."
    $committed = $true
} catch {
    $failure = $_.Exception.Message
    if ($journal.Count -ne 0) {
        $restored = $false
        try {
            Stop-Replacement
            for ($index = $journal.Count - 1; $index -ge 0; $index--) {
                $record = $journal[$index]
                $target = Join-Path $installDir $record.Name
                if ($record.Applied) { [IO.File]::Delete($target) }
                if ($record.Saved) { [IO.File]::Move((Join-Path $backup $record.Name), $target) }
            }
            $restored = $true
        } catch {
            $preserveBackup = $true
            try { Write-Result 'failed' "Update rollback needs manual recovery. Backup: $backup. $failure. $_" }
            catch { Write-Warning "Could not write update failure result: $_" }
        }
        if ($restored) {
            # A locked/unwritable receipt must not prevent the restored app
            # from restarting or misclassify a completed file restoration.
            try { Write-Result 'rolled_back' "The update failed and the previous files were restored. $failure" }
            catch { Write-Warning "Could not write update rollback result: $_" }
            try { Launch-Previous } catch { Write-Warning "Previous app could not be relaunched: $_" }
        }
    } elseif ($null -ne $lock) {
        try { Write-Result 'failed' "The existing installation was left unchanged. $failure" }
        catch { Write-Warning "Could not write update failure result: $_" }
        try { Launch-Previous } catch { Write-Warning "Previous app could not be relaunched: $_" }
    }
    Write-Warning $failure
} finally {
    Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
    if (-not $preserveBackup) { Remove-Item -LiteralPath $backup -Recurse -Force -ErrorAction SilentlyContinue }
    Remove-Item -LiteralPath $health -Force -ErrorAction SilentlyContinue
    if ($null -ne $lock) { $lock.Dispose() }
}
if (-not $committed) { exit 1 }
Remove-Item -LiteralPath $spec.tmp_dir -Recurse -Force -ErrorAction SilentlyContinue
exit 0
