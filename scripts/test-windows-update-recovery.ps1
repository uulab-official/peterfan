[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Helper,
    [Parameter(Mandatory = $true)][string]$Root,
    [string]$PowerShell = 'powershell.exe',
    [string]$NativeApp = '',
    [switch]$ReplayOnly
)

# File-only crash fixtures. These launch and kill only the retained helper
# Process objects created here; they never launch a real app or touch user data.
$ErrorActionPreference = 'Stop'
if (Test-Path -LiteralPath $Root) { throw "Recovery fixture already exists: $Root" }
[IO.Directory]::CreateDirectory($Root) | Out-Null
$Root = [IO.Path]::GetFullPath($Root)
$body = [IO.File]::ReadAllText($Helper)
$names = @('CHANGELOG.md', 'LICENSE', 'PeterFan.exe', 'README-WINDOWS.txt',
    'THIRD_PARTY_NOTICES.md', 'peterfan-cli.exe', 'peterfan-tui.exe', 'uninstall.ps1')
$children = [Collections.Generic.List[object]]::new()

function Hash-File([string]$path) {
    if (Test-Path -LiteralPath $path -PathType Leaf) {
        return (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    return $null
}

function New-Case([string]$name) {
    $directory = Join-Path $Root $name
    $install = Join-Path $directory "My Apps\O'Brien\PeterFan"
    $payload = Join-Path $directory 'download\payload'
    $support = Join-Path $directory 'support'
    foreach ($path in @($install, $payload, $support)) { [IO.Directory]::CreateDirectory($path) | Out-Null }
    $manifest = [ordered]@{}
    $original = @{}
    foreach ($file in $names) {
        [IO.File]::WriteAllText((Join-Path $payload $file), "new bytes $file")
        $manifest[$file] = Hash-File (Join-Path $payload $file)
        if ($file -ne 'THIRD_PARTY_NOTICES.md') {
            [IO.File]::WriteAllText((Join-Path $install $file), "original bytes $file")
        }
        $original[$file] = Hash-File (Join-Path $install $file)
    }
    [IO.File]::WriteAllText((Join-Path $install 'user-data.txt'), 'unrelated bytes')
    $result = Join-Path $support 'update-result.json'
    [IO.File]::WriteAllText($result, '{"status":"pending"}')
    $spec = @{ install_dir = $install; payload_dir = $payload; tmp_dir = (Split-Path -Parent $payload);
        result_path = $result; version = '9.9.9'; nonce = [Guid]::NewGuid().ToString('N'); files = $manifest }
    return @{ directory = $directory; install = $install; payload = $payload; result = $result;
        original = $original; replacement = $manifest; spec = $spec;
        state = (Join-Path $install '.peterfan-update-state.json') }
}

function Replace-Function([string]$source, [string]$name, [string]$replacement) {
    $tokens = $null
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$errors)
    if ($errors.Count) { throw "Fixture source parse failed: $errors" }
    $node = $ast.Find({ param($item)
        $item -is [Management.Automation.Language.FunctionDefinitionAst] -and $item.Name -eq $name
    }, $true)
    if ($null -eq $node) { throw "Fixture function was not found: $name" }
    return $source.Substring(0, $node.Extent.StartOffset) + $replacement + $source.Substring($node.Extent.EndOffset)
}

function Start-Helper($case, [bool]$recover, [string]$stopAt = '', [bool]$failHealth = $false,
        [string]$traceStartup = '', [bool]$realStartup = $false) {
    $source = Replace-Function $body 'Launch-Previous' 'function Launch-Previous { }'
    $launch = if ($failHealth) { 'function Launch-Healthy { throw "fixture readiness failure" }' }
        else { 'function Launch-Healthy { }' }
    if ($traceStartup) {
        $tracePath = $traceStartup.Replace("'", "''")
        $wait = 'function Wait-For-Exit { [IO.File]::AppendAllText(''' + $tracePath + ''', "wait`n") }'
        $launch = 'function Launch-Healthy([string]$expectedVersion = $spec.version) { [IO.File]::AppendAllText(''' +
            $tracePath + ''', ("launch:" + $expectedVersion + "`n")) }'
        if ($realStartup) {
            $tokens = $null
            $errors = $null
            $ast = [Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$errors)
            $node = $ast.Find({ param($item)
                $item -is [Management.Automation.Language.FunctionDefinitionAst] -and $item.Name -eq 'Wait-For-Exit'
            }, $true)
            $wait = $node.Extent.Text
            $wait = $wait.Insert($wait.IndexOf('{') + 1,
                (' [IO.File]::AppendAllText(''' + $tracePath + ''', "wait`n"); '))
        }
        $source = Replace-Function $source 'Wait-For-Exit' $wait
    }
    if (-not $realStartup) { $source = Replace-Function $source 'Launch-Healthy' $launch }
    $marker = Join-Path $case.directory ('barrier-' + [Guid]::NewGuid().ToString('N'))
    if ($stopAt) {
        $hook = 'function Transaction-Cutpoint([string]$name) { if ($name -ceq ''' +
            $stopAt.Replace("'", "''") + ''') { [IO.File]::WriteAllText(''' +
            $marker.Replace("'", "''") + ''', $name); while ($true) { Start-Sleep -Milliseconds 20 } } }'
        $source = Replace-Function $source 'Transaction-Cutpoint' $hook
    }
    $json = ($case.spec | ConvertTo-Json -Depth 6 -Compress).Replace("'", "''")
    $script = Join-Path $case.directory ('helper-' + [Guid]::NewGuid().ToString('N') + '.ps1')
    $prefix = '$spec = ''' + $json + "' | ConvertFrom-Json`n" + '$RecoverOnly = $' + $recover.ToString().ToLowerInvariant() + "`n"
    [IO.File]::WriteAllText($script, ($prefix + $source), [Text.UTF8Encoding]::new($true))
    if ($script.Contains('"')) { throw 'Fixture path contains an unsupported quote.' }
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $PowerShell
    $start.Arguments = '-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + $script + '"'
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    if (-not $process.Start()) { throw 'Fixture helper did not start.' }
    $child = @{ process = $process; started = $process.StartTime; marker = $marker;
        stdout = $process.StandardOutput.ReadToEndAsync(); stderr = $process.StandardError.ReadToEndAsync() }
    $children.Add($child)
    return $child
}

function Kill-Owned($child) {
    $process = $child.process
    $process.Refresh()
    if (-not $process.HasExited) {
        if ($process.Id -eq $PID -or $process.StartTime -ne $child.started) { throw 'Helper identity changed.' }
        $process.Kill()
        if (-not $process.WaitForExit(10000)) { throw 'Owned helper did not exit after hard kill.' }
    }
}

function Wait-Barrier($child) {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while (-not (Test-Path -LiteralPath $child.marker)) {
        $child.process.Refresh()
        if ($child.process.HasExited -or $clock.ElapsedMilliseconds -ge 15000) {
            Kill-Owned $child
            throw "Helper did not reach barrier. $($child.stdout.Result) $($child.stderr.Result)"
        }
        Start-Sleep -Milliseconds 25
    }
    Kill-Owned $child
}

function Complete-Helper($child, [bool]$success = $true) {
    if (-not $child.process.WaitForExit(15000)) { Kill-Owned $child; throw 'Recovery helper timed out.' }
    $output = $child.stdout.Result + $child.stderr.Result
    if (($child.process.ExitCode -eq 0) -ne $success) {
        throw "Unexpected recovery helper exit $($child.process.ExitCode). $output"
    }
}

function Assert-Files($case, $expected) {
    foreach ($file in $names) {
        if ((Hash-File (Join-Path $case.install $file)) -cne $expected[$file]) {
            throw "Recovery changed unexpected bytes or did not restore $file."
        }
    }
    if ([IO.File]::ReadAllText((Join-Path $case.install 'user-data.txt')) -cne 'unrelated bytes') {
        throw 'Recovery modified unrelated data.'
    }
}

function Assert-Clean($case) {
    if (Test-Path -LiteralPath $case.state) { throw 'Successful replay left its journal.' }
    if (@(Get-ChildItem -LiteralPath $case.install -Force | Where-Object Name -match '^\.peterfan-(stage|backup)-').Count) {
        throw 'Successful replay left snapshots or stage files.'
    }
}

function Replay-Twice($case, $expected, [string]$status) {
    # Recovery must not depend on a downloaded archive or internet connectivity.
    $case.spec.payload_dir = Join-Path $case.directory 'missing-offline-payload'
    Complete-Helper (Start-Helper $case $true)
    Assert-Files $case $expected
    Assert-Clean $case
    Complete-Helper (Start-Helper $case $true)
    Assert-Files $case $expected
    $receipt = Get-Content -LiteralPath $case.result -Raw | ConvertFrom-Json
    if ($receipt.status -cne $status) { throw "Unexpected recovered status: $($receipt.status)" }
}

function Synthetic-Interrupted($case) {
    $id = [Guid]::NewGuid().ToString('N')
    $stage = Join-Path $case.install ".peterfan-stage-$id"
    $backup = Join-Path $case.install ".peterfan-backup-$id"
    [IO.Directory]::CreateDirectory($stage) | Out-Null
    [IO.Directory]::CreateDirectory($backup) | Out-Null
    $files = @()
    foreach ($file in $names) {
        Copy-Item -LiteralPath (Join-Path $case.payload $file) -Destination (Join-Path $stage $file)
        if ($null -ne $case.original[$file]) {
            Copy-Item -LiteralPath (Join-Path $case.install $file) -Destination (Join-Path $backup $file)
        }
        if ($file -cin @('PeterFan.exe', 'THIRD_PARTY_NOTICES.md')) {
            Copy-Item -LiteralPath (Join-Path $case.payload $file) -Destination (Join-Path $case.install $file) -Force
        }
        $files += @{ name = $file; original_sha256 = $case.original[$file]; new_sha256 = $case.replacement[$file] }
    }
    $state = @{ schema = 1; transaction = $id; install_dir = $case.install;
        phase = 'applying'; version = '9.9.9'; files = $files }
    [IO.File]::WriteAllText($case.state, ($state | ConvertTo-Json -Depth 6 -Compress))
    return $backup
}

function Make-Committed($case) {
    foreach ($file in $names) {
        Copy-Item -LiteralPath (Join-Path $case.payload $file) -Destination (Join-Path $case.install $file) -Force
    }
    $state = Get-Content -LiteralPath $case.state -Raw -Force | ConvertFrom-Json
    $state.phase = 'committed'
    [IO.File]::WriteAllText($case.state, ($state | ConvertTo-Json -Depth 6 -Compress))
    return $state
}

try {
    $case = New-Case 'offline-snapshot-replay'
    $backup = Synthetic-Interrupted $case
    [IO.File]::WriteAllText((Join-Path $backup '.restore-PeterFan.exe'), 'partial scratch from an interrupted copy')
    Replay-Twice $case $case.original 'rolled_back'
    Write-Host 'PASS durable recovery: offline replay twice, immutable snapshot, new file removal, partial restore scratch'
    $case = New-Case 'committed-replay-startup-order'
    $null = Synthetic-Interrupted $case
    $null = Make-Committed $case
    $case.spec.version = '10.0.0'
    $trace = Join-Path $case.directory 'startup-trace'
    Complete-Helper (Start-Helper $case $false '' $false $trace)
    if ([IO.File]::ReadAllText($trace) -cne "wait`nlaunch:9.9.9`n") {
        throw 'Committed production replay did not wait then relaunch the previously committed version.'
    }
    Assert-Files $case $case.replacement
    Assert-Clean $case
    $receipt = Get-Content -LiteralPath $case.result -Raw | ConvertFrom-Json
    if ($receipt.version -cne '9.9.9' -or $receipt.message -notmatch 'Retry any newer update separately') {
        throw 'Committed replay misreported the newer update request as installed.'
    }
    Write-Host 'PASS durable recovery: committed production replay waits, relaunches old transaction version, reports separate retry'
    if (-not $ReplayOnly) {
        foreach ($point in @('intent', 'replace:PeterFan.exe', 'replace:THIRD_PARTY_NOTICES.md',
                'replace:uninstall.ps1', 'rollback:PeterFan.exe', 'committed', 'cleanup:backup')) {
            $case = New-Case ('hard-kill-' + $point.Replace(':', '-'))
            Wait-Barrier (Start-Helper $case $false $point ($point.StartsWith('rollback:')))
            if (-not (Test-Path -LiteralPath $case.state)) { throw 'Kill barrier did not retain a durable journal.' }
            $keptCommit = $point -cin @('committed', 'cleanup:backup')
            $expected = if ($keptCommit) { $case.replacement } else { $case.original }
            $status = if ($keptCommit) { 'installed' } else { 'rolled_back' }
            Replay-Twice $case $expected $status
            Write-Host "PASS durable recovery: hard kill at $point, replay twice"
        }
        $case = New-Case 'hard-kill-replay-again'
        Wait-Barrier (Start-Helper $case $false 'replace:THIRD_PARTY_NOTICES.md')
        Wait-Barrier (Start-Helper $case $true 'rollback:PeterFan.exe')
        Replay-Twice $case $case.original 'rolled_back'
        Write-Host 'PASS durable recovery: second hard kill during recovery, replay twice'
    }
    foreach ($fault in @('journal-path', 'journal-name', 'journal-id', 'journal-truncated',
            'snapshot-hash', 'snapshot-extra', 'installed-hash', 'committed-installed-hash', 'committed-snapshot-hash')) {
        $case = New-Case ('reject-' + $fault)
        $backup = Synthetic-Interrupted $case
        $state = Get-Content -LiteralPath $case.state -Raw | ConvertFrom-Json
        if ($fault.StartsWith('committed-')) { $state = Make-Committed $case }
        if ($fault -ceq 'journal-path') { $state.install_dir = (Join-Path $case.directory 'outside') }
        elseif ($fault -ceq 'journal-name') { $state.files[0].name = '../user-data.txt' }
        elseif ($fault -ceq 'journal-id') { $state.transaction = '../outside' }
        elseif ($fault -cin @('snapshot-hash', 'committed-snapshot-hash')) { [IO.File]::WriteAllText((Join-Path $backup 'LICENSE'), 'corrupt snapshot') }
        elseif ($fault -ceq 'snapshot-extra') { [IO.File]::WriteAllText((Join-Path $backup 'user-data.txt'), 'unknown snapshot file') }
        elseif ($fault -cin @('installed-hash', 'committed-installed-hash')) { [IO.File]::WriteAllText((Join-Path $case.install 'PeterFan.exe'), 'unknown manual edit') }
        if ($fault -ceq 'journal-truncated') { [IO.File]::WriteAllText($case.state, '{') }
        else { [IO.File]::WriteAllText($case.state, ($state | ConvertTo-Json -Depth 6 -Compress)) }
        $before = @{}
        foreach ($file in $names) { $before[$file] = Hash-File (Join-Path $case.install $file) }
        Complete-Helper (Start-Helper $case $true) $false
        Assert-Files $case $before
        if (-not (Test-Path -LiteralPath $case.state) -or -not (Test-Path -LiteralPath $backup) -or
                -not (Test-Path -LiteralPath $case.payload)) {
            throw "$fault discarded unsafe journal or snapshot."
        }
        Write-Host "PASS durable recovery: reject $fault, preserve files and snapshots"
    }
    $case = New-Case 'reject-linked-snapshot'
    $backup = Synthetic-Interrupted $case
    $retained = $backup + '.retained'
    [IO.Directory]::Move($backup, $retained)
    $outside = Join-Path $case.directory 'outside-snapshot'
    [IO.Directory]::CreateDirectory($outside) | Out-Null
    [IO.File]::WriteAllText((Join-Path $outside 'sentinel'), 'outside bytes')
    $linkKind = if ($env:OS -eq 'Windows_NT') { 'Junction' } else { 'SymbolicLink' }
    New-Item -ItemType $linkKind -Path $backup -Target $outside | Out-Null
    $before = @{}
    foreach ($file in $names) { $before[$file] = Hash-File (Join-Path $case.install $file) }
    Complete-Helper (Start-Helper $case $true) $false
    Assert-Files $case $before
    if (-not (Test-Path -LiteralPath $case.state) -or -not (Test-Path -LiteralPath $retained) -or
            [IO.File]::ReadAllText((Join-Path $outside 'sentinel')) -cne 'outside bytes') {
        throw 'Linked snapshot recovery modified outside data or discarded recovery evidence.'
    }
    Write-Host 'PASS durable recovery: reject linked snapshot directory, preserve outside data and original snapshot'
    if ($NativeApp) {
        $case = New-Case 'native-committed-relaunch'
        $null = Synthetic-Interrupted $case
        Copy-Item -LiteralPath $NativeApp -Destination (Join-Path $case.payload 'PeterFan.exe') -Force
        $case.replacement['PeterFan.exe'] = Hash-File (Join-Path $case.payload 'PeterFan.exe')
        $state = Make-Committed $case
        @($state.files | Where-Object name -ceq 'PeterFan.exe')[0].new_sha256 = $case.replacement['PeterFan.exe']
        [IO.File]::WriteAllText($case.state, ($state | ConvertTo-Json -Depth 6 -Compress))
        $case.spec.version = '10.0.0'
        [IO.File]::WriteAllText((Join-Path $case.install 'fixture-mode'), 'healthy')
        $env:PETERFAN_TEST_HEALTH_DIR = Split-Path -Parent $case.result
        $appPath = Join-Path $case.install 'PeterFan.exe'
        $caller = Start-Process -FilePath $appPath -PassThru
        try {
            $trace = Join-Path $case.directory 'native-wait-entered'
            $helperProcess = Start-Helper $case $false '' $false $trace $true
            $deadline = [Diagnostics.Stopwatch]::StartNew()
            while (-not (Test-Path -LiteralPath $trace)) {
                $helperProcess.process.Refresh()
                if ($helperProcess.process.HasExited -or $deadline.ElapsedMilliseconds -ge 15000) {
                    throw 'Native committed replay did not enter its bounded caller-exit wait.'
                }
                Start-Sleep -Milliseconds 25
            }
            Start-Sleep -Milliseconds 500
            $helperProcess.process.Refresh()
            if ($helperProcess.process.HasExited -or -not (Test-Path -LiteralPath $case.state)) {
                throw 'Committed replay did not wait for the existing owned caller to exit.'
            }
            $beforeExit = @(Get-Process -Name PeterFan -ErrorAction SilentlyContinue | Where-Object {
                [String]::Equals($_.Path, $appPath, [StringComparison]::OrdinalIgnoreCase)
            })
            if ($beforeExit.Count -ne 1 -or $beforeExit[0].Id -ne $caller.Id) {
                throw 'Committed replay launched the replacement before the owned caller exited.'
            }
            $caller.Kill()
            if (-not $caller.WaitForExit(5000)) { throw 'Synthetic owned caller did not exit.' }
            Complete-Helper $helperProcess
            $running = @(Get-Process -Name PeterFan -ErrorAction SilentlyContinue | Where-Object {
                [String]::Equals($_.Path, $appPath, [StringComparison]::OrdinalIgnoreCase)
            })
            if ($running.Count -ne 1 -or $running[0].Id -eq $caller.Id) {
                throw 'Committed replay did not leave one new healthy synthetic app running.'
            }
            $receipt = Get-Content -LiteralPath $case.result -Raw | ConvertFrom-Json
            if ($receipt.status -cne 'installed' -or $receipt.version -cne '9.9.9') {
                throw 'Native committed replay did not confirm the committed app version.'
            }
            Assert-Files $case $case.replacement
            Assert-Clean $case
            Write-Host 'PASS durable recovery: native committed replay waits for owned caller, launches healthy exact app and reports prior version'
        } finally {
            if (-not $caller.HasExited) { $caller.Kill(); $caller.WaitForExit(5000) | Out-Null }
            foreach ($app in @(Get-Process -Name PeterFan -ErrorAction SilentlyContinue)) {
                if ([String]::Equals($app.Path, $appPath, [StringComparison]::OrdinalIgnoreCase)) {
                    $app.Kill()
                    if (-not $app.WaitForExit(5000)) { throw 'Synthetic relaunched app did not exit.' }
                }
            }
        }
    }
} finally {
    foreach ($child in $children) { Kill-Owned $child }
}
