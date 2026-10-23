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
$statePath = Join-Path $installDir '.peterfan-update-state.json'
$journal = $null
$intentPersisted = $false
$recoveryOnly = $null -ne (Get-Variable RecoverOnly -ErrorAction SilentlyContinue) -and $RecoverOnly
$settlingExisting = $false
$protectCommittedProcess = $false
$committed = $false
$allowed = @('PeterFan.exe', 'peterfan-cli.exe', 'peterfan-tui.exe', 'uninstall.ps1',
    'README-WINDOWS.txt', 'LICENSE', 'CHANGELOG.md', 'THIRD_PARTY_NOTICES.md')

function Assert-Regular([string]$path) {
    $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "Update file must be a regular file: $path"
    }
}

function Assert-Directory([string]$path) {
    $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
    if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "Update directory must be a regular directory: $path"
    }
}

function File-Hash([string]$path) {
    if (Test-Path -LiteralPath $path) {
        Assert-Regular $path
        return (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    return $null
}

function Copy-Durable([string]$source, [string]$destination) {
    Assert-Regular $source
    $inputFile = [IO.File]::Open($source, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    $outputFile = $null
    try {
        $outputFile = [IO.FileStream]::new($destination, [IO.FileMode]::CreateNew,
            [IO.FileAccess]::Write, [IO.FileShare]::None, 4096, [IO.FileOptions]::WriteThrough)
        $inputFile.CopyTo($outputFile)
        $outputFile.Flush($true)
    } finally {
        if ($null -ne $outputFile) { $outputFile.Dispose() }
        $inputFile.Dispose()
    }
}

function Write-DurableJson([string]$path, [string]$json) {
    [IO.Directory]::CreateDirectory((Split-Path -Parent $path)) | Out-Null
    $temporary = "$path.tmp-$transaction"
    $stream = $null
    try {
        $stream = [IO.FileStream]::new($temporary, [IO.FileMode]::CreateNew,
            [IO.FileAccess]::Write, [IO.FileShare]::None, 4096, [IO.FileOptions]::WriteThrough)
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes($json)
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
        $stream.Dispose()
        $stream = $null
        if ([IO.File]::Exists($path)) {
            Assert-Regular $path
            # PowerShell converts ordinary $null string arguments to "".
            [IO.File]::Replace($temporary, $path, [Management.Automation.Language.NullString]::Value)
        } else { [IO.File]::Move($temporary, $path) }
        # Ask the OS to flush the published file too. This is not a guarantee
        # against hardware/filesystem failures or uncooperative disk caches.
        $stream = [IO.File]::Open($path, [IO.FileMode]::Open,
            [IO.FileAccess]::ReadWrite, [IO.FileShare]::Read)
        $stream.Flush($true)
    } finally {
        if ($null -ne $stream) { $stream.Dispose() }
        if ([IO.File]::Exists($temporary)) { [IO.File]::Delete($temporary) }
    }
}

function Write-Result([string]$status, [string]$message, [string]$version = $spec.version) {
    $json = @{ status = $status; version = $version; message = $message;
        updated_at_unix = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds() } | ConvertTo-Json -Compress
    Write-DurableJson $result $json
}

function Write-State($state) {
    Write-DurableJson $statePath ($state | ConvertTo-Json -Depth 8 -Compress)
}

# Empty production hook. Native fixtures replace only this body with an owned
# barrier, then terminate the exact helper process without running finally.
function Transaction-Cutpoint([string]$name) { }

function Read-State {
    Assert-Regular $statePath
    if ((Get-Item -LiteralPath $statePath -Force).Length -gt 65536) { throw 'Update journal is too large.' }
    $state = Get-Content -LiteralPath $statePath -Raw -Force | ConvertFrom-Json
    foreach ($key in @('schema', 'transaction', 'install_dir', 'phase', 'version', 'files')) {
        if ($null -eq $state.PSObject.Properties[$key]) { throw "Update journal is missing $key." }
    }
    if ($state.schema -ne 1 -or $state.schema -is [bool] -or
            $state.transaction -isnot [string] -or $state.transaction -cnotmatch '^[0-9a-f]{32}$' -or
            $state.install_dir -isnot [string] -or
            -not [String]::Equals($state.install_dir, $installDir, [StringComparison]::OrdinalIgnoreCase) -or
            $state.phase -isnot [string] -or
            $state.phase -cnotin @('applying', 'rolling_back', 'committed', 'rolled_back') -or
            $state.version -isnot [string] -or $state.version -cnotmatch '^[0-9A-Za-z.+-]{1,128}$') {
        throw 'Update journal identity, path, or phase is invalid.'
    }
    $entries = @($state.files)
    if ($entries.Count -lt 1 -or $entries.Count -gt $allowed.Count) { throw 'Update journal file count is invalid.' }
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($entry in $entries) {
        if ($null -eq $entry.PSObject.Properties['name'] -or
                $null -eq $entry.PSObject.Properties['original_sha256'] -or
                $null -eq $entry.PSObject.Properties['new_sha256'] -or
                $entry.name -isnot [string] -or $allowed -cnotcontains $entry.name -or
                -not $seen.Add($entry.name) -or $entry.new_sha256 -isnot [string] -or
                $entry.new_sha256 -cnotmatch '^[0-9a-f]{64}$' -or
                ($null -ne $entry.original_sha256 -and
                    ($entry.original_sha256 -isnot [string] -or $entry.original_sha256 -cnotmatch '^[0-9a-f]{64}$'))) {
            throw 'Update journal file names or hashes are invalid.'
        }
    }
    foreach ($required in @('PeterFan.exe', 'peterfan-cli.exe', 'peterfan-tui.exe', 'uninstall.ps1')) {
        if (-not $seen.Contains($required)) { throw "Update journal is missing required file: $required" }
    }
    return $state
}

function State-Paths($state) {
    return @{ stage = (Join-Path $installDir ".peterfan-stage-$($state.transaction)");
        backup = (Join-Path $installDir ".peterfan-backup-$($state.transaction)") }
}

function Validate-StateFiles($state) {
    $paths = State-Paths $state
    foreach ($directory in @($paths.stage, $paths.backup)) {
        if (Test-Path -LiteralPath $directory) { Assert-Directory $directory }
    }
    foreach ($entry in @($state.files)) {
        $current = File-Hash (Join-Path $installDir $entry.name)
        if ($state.phase -ceq 'committed') {
            if ($current -cne $entry.new_sha256) { throw "Committed file changed: $($entry.name)" }
        } elseif ($state.phase -ceq 'rolled_back') {
            if ($current -cne $entry.original_sha256) { throw "Restored file changed: $($entry.name)" }
        } elseif ($null -ne $current -and $current -cne $entry.original_sha256 -and $current -cne $entry.new_sha256) {
            throw "Installed file has unknown contents: $($entry.name)"
        }
        $saved = File-Hash (Join-Path $paths.backup $entry.name)
        if ($null -ne $saved -and $saved -cne $entry.original_sha256) {
            throw "Recovery snapshot checksum mismatch: $($entry.name)"
        }
        if ($state.phase -cin @('applying', 'rolling_back') -and $null -ne $entry.original_sha256 -and
                $saved -cne $entry.original_sha256) { throw "Recovery snapshot is missing: $($entry.name)" }
    }
    # Only owned flat payloads and derived restore scratch files may be removed.
    # A scratch file can be partial after a kill during Copy-Durable; snapshots
    # themselves must always match their durable recorded original hashes.
    foreach ($kind in @('stage', 'backup')) {
        $directory = $paths[$kind]
        if (-not (Test-Path -LiteralPath $directory)) { continue }
        foreach ($item in @(Get-ChildItem -LiteralPath $directory -Force)) {
            Assert-Regular $item.FullName
            $entry = @($state.files | Where-Object { $_.name -ceq $item.Name })
            if ($entry.Count -eq 1) {
                $expected = if ($kind -ceq 'stage') { $entry[0].new_sha256 } else { $entry[0].original_sha256 }
                if ($null -eq $expected -or (File-Hash $item.FullName) -cne $expected) {
                    throw "Transaction file checksum mismatch: $($item.FullName)"
                }
            } elseif ($kind -ceq 'backup') {
                $scratch = @($state.files | Where-Object {
                    $null -ne $_.original_sha256 -and ('.restore-' + $_.name) -ceq $item.Name
                })
                if ($scratch.Count -ne 1) { throw "Unknown recovery file: $($item.FullName)" }
            } else { throw "Unknown transaction file: $($item.FullName)" }
        }
    }
}

function Cleanup-State($state, [bool]$keepJournal = $false) {
    $paths = State-Paths $state
    try {
        Validate-StateFiles $state
        foreach ($kind in @('stage', 'backup')) {
            $directory = $paths[$kind]
            if (Test-Path -LiteralPath $directory) {
                Remove-Item -LiteralPath $directory -Recurse -Force
                Transaction-Cutpoint ('cleanup:' + $kind)
            }
        }
        if (-not $keepJournal) { [IO.File]::Delete($statePath) }
    } catch { Write-Warning "Completed update cleanup will be retried: $_" }
}

function Recover-State($state) {
    Validate-StateFiles $state
    if ($state.phase -ceq 'committed') {
        if ($settlingExisting -and -not $recoveryOnly) {
            # The app/CLI that queued this helper is still exiting. Relaunch
            # the already committed version, not this request's newer payload.
            Wait-For-Exit
            Launch-Healthy $state.version
        }
        Write-Result 'installed' "The previous update transaction for PeterFan v$($state.version) is complete. Retry any newer update separately." $state.version
        Cleanup-State $state
        return 'committed'
    }
    if ($state.phase -cne 'rolled_back') {
        Wait-For-Exit
        $state.phase = 'rolling_back'
        Write-State $state
        $paths = State-Paths $state
        foreach ($entry in @($state.files)) {
            $target = Join-Path $installDir $entry.name
            $current = File-Hash $target
            if ($current -cne $entry.original_sha256) {
                if ($null -ne $current -and $current -cne $entry.new_sha256) {
                    throw "Installed file changed during recovery: $($entry.name)"
                }
                if ($null -eq $entry.original_sha256) {
                    if ($null -ne $current) { [IO.File]::Delete($target) }
                } else {
                    $restore = Join-Path $paths.backup ('.restore-' + $entry.name)
                    if (Test-Path -LiteralPath $restore) { Assert-Regular $restore; [IO.File]::Delete($restore) }
                    Copy-Durable (Join-Path $paths.backup $entry.name) $restore
                    if ((File-Hash $restore) -cne $entry.original_sha256) { throw 'Restored scratch checksum mismatch.' }
                    if ($null -ne $current) {
                        [IO.File]::Replace($restore, $target, [Management.Automation.Language.NullString]::Value)
                    } else { [IO.File]::Move($restore, $target) }
                }
            }
            Transaction-Cutpoint ('rollback:' + $entry.name)
        }
        $state.phase = 'rolled_back'
        Validate-StateFiles $state
        Write-State $state
    }
    $receiptWritten = $false
    try {
        Write-Result 'rolled_back' 'The update failed and the previous files were restored.' $state.version
        $receiptWritten = $true
    } catch { Write-Warning "Could not write update rollback result: $_" }
    Cleanup-State $state (-not $receiptWritten)
    return 'rolled_back'
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

function Launch-Healthy([string]$expectedVersion = $spec.version) {
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
            $receipt.pid -eq $script:launched.Id -and $receipt.version -ceq $expectedVersion
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
    Assert-Directory $installDir
    # Retain the stable lock inode so two helpers cannot lock different files.
    $lockPath = Join-Path $installDir '.peterfan-update.lock'
    if (Test-Path -LiteralPath $lockPath) { Assert-Regular $lockPath }
    $lock = [IO.File]::Open($lockPath, [IO.FileMode]::OpenOrCreate,
        [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    if (Test-Path -LiteralPath $statePath) {
        # Settle an interrupted transaction before accepting another update.
        # This path needs no downloaded payload and trusts only validated data.
        $settlingExisting = $true
        $outcome = Recover-State (Read-State)
        if ($outcome -ceq 'rolled_back' -and -not $recoveryOnly) {
            try { Wait-For-Exit; Launch-Previous } catch { Write-Warning $_ }
        }
        if ($outcome -ceq 'committed' -or $recoveryOnly) { exit 0 }
        exit 1
    }
    if ($recoveryOnly) { exit 0 }
    foreach ($required in @('PeterFan.exe', 'peterfan-cli.exe', 'peterfan-tui.exe', 'uninstall.ps1')) {
        if ($null -eq $spec.files.PSObject.Properties[$required]) { throw "Missing update file: $required" }
    }
    [IO.Directory]::CreateDirectory($stage) | Out-Null
    foreach ($entry in $spec.files.PSObject.Properties) {
        $name = $entry.Name
        if ($allowed -cnotcontains $name -or $entry.Value -cnotmatch '^[0-9a-f]{64}$') {
            throw 'Invalid update file manifest.'
        }
        $target = Join-Path $stage $name
        Copy-Durable (Join-Path $payloadDir $name) $target
        if ((File-Hash $target) -cne $entry.Value) { throw "Staged update checksum mismatch: $name" }
    }
    Wait-For-Exit
    [IO.Directory]::CreateDirectory($backup) | Out-Null
    $files = [Collections.Generic.List[object]]::new()
    foreach ($entry in $spec.files.PSObject.Properties) {
        $target = Join-Path $installDir $entry.Name
        $original = File-Hash $target
        if ($null -ne $original) {
            $saved = Join-Path $backup $entry.Name
            Copy-Durable $target $saved
            if ((File-Hash $saved) -cne $original) { throw "Recovery snapshot changed: $($entry.Name)" }
        }
        $files.Add([PSCustomObject]@{ name = $entry.Name; original_sha256 = $original; new_sha256 = $entry.Value })
    }
    $journal = [PSCustomObject]@{ schema = 1; transaction = $transaction; install_dir = $installDir;
        phase = 'applying'; version = $spec.version; files = @($files.ToArray()) }
    Write-State $journal
    $intentPersisted = $true
    Transaction-Cutpoint 'intent'
    Validate-StateFiles $journal
    foreach ($entry in @($journal.files)) {
        $target = Join-Path $installDir $entry.name
        if ((File-Hash $target) -cne $entry.original_sha256) {
            throw "Installed file changed before replacement: $($entry.name)"
        }
        $candidate = Join-Path $stage $entry.name
        if ($null -ne $entry.original_sha256) {
            [IO.File]::Replace($candidate, $target, [Management.Automation.Language.NullString]::Value)
        } else { [IO.File]::Move($candidate, $target) }
        Transaction-Cutpoint ('replace:' + $entry.name)
    }
    Launch-Healthy
    Write-Result 'installed' "PeterFan v$($spec.version) started successfully."
    $journal.phase = 'committed'
    Write-State $journal
    $committed = $true
    Transaction-Cutpoint 'committed'
    Cleanup-State $journal
} catch {
    $failure = $_.Exception.Message
    if ($null -ne $lock -and (Test-Path -LiteralPath $statePath)) {
        try {
            if ($settlingExisting) { throw 'The previous update transaction could not be settled safely.' }
            $stored = Read-State
            if ($stored.phase -ceq 'committed') { $protectCommittedProcess = $true }
            else { Stop-Replacement }
            $outcome = Recover-State $stored
            if ($outcome -ceq 'committed') { $committed = $true }
            else { try { Launch-Previous } catch { Write-Warning "Previous app could not be relaunched: $_" } }
        } catch {
            if (-not $committed -and -not $protectCommittedProcess -and -not $settlingExisting) {
                try { Stop-Replacement } catch { Write-Warning $_ }
            }
            try { Write-Result 'failed' "Update recovery needs manual attention. Journal and backups were retained. $failure. $_" }
            catch { Write-Warning "Could not write update failure result: $_" }
        }
    } elseif ($null -ne $lock) {
        try { Write-Result 'failed' "The existing installation was left unchanged. $failure" }
        catch { Write-Warning "Could not write update failure result: $_" }
        try { Launch-Previous } catch { Write-Warning "Previous app could not be relaunched: $_" }
    }
    Write-Warning $failure
} finally {
    # Before durable intent, no target file has changed. After intent, only
    # validated recovery/committed cleanup may remove snapshots or the journal.
    if (-not $intentPersisted -and -not (Test-Path -LiteralPath $statePath)) {
        Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $backup -Recurse -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath $health -Force -ErrorAction SilentlyContinue
    if ($null -ne $lock) { $lock.Dispose() }
}
if (-not $committed) { exit 1 }
Remove-Item -LiteralPath $spec.tmp_dir -Recurse -Force -ErrorAction SilentlyContinue
exit 0
