[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Helper,
    [Parameter(Mandatory = $true)][string]$Root,
    [switch]$ReceiptOnly
)

# Native CI fixture only. No real application paths, registry, shortcuts, user
# configuration, credentials, downloads, or machine-wide process stops.
$ErrorActionPreference = 'Stop'
if (Test-Path -LiteralPath $Root) { throw "Fixture path already exists: $Root" }
[IO.Directory]::CreateDirectory($Root) | Out-Null
$Root = [IO.Path]::GetFullPath($Root)
# Exercise the actual embedded writer before launching any fixture process.
# This also runs on portable PowerShell, catching .NET null-string binding
# differences that a syntax-only parser cannot detect.
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($Helper, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw "Helper parse errors: $parseErrors" }
$writer = $ast.Find({ param($node)
    $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Write-Result'
}, $true)
if ($null -eq $writer) { throw 'Embedded Write-Result function is missing.' }
. ([ScriptBlock]::Create($writer.Extent.Text))
$transaction = [Guid]::NewGuid().ToString('N')
$spec = @{ version = '9.9.9' }
$result = Join-Path $Root 'receipt-probe\update-result.json'
Write-Result 'pending' 'receipt probe pending'
Write-Result 'installed' 'receipt probe installed'
$receipt = Get-Content -LiteralPath $result -Raw | ConvertFrom-Json
if ($receipt.status -ne 'installed' -or $receipt.version -ne '9.9.9') {
    throw 'Atomic receipt replacement did not publish the new contents.'
}
$probeChecks = 'create, replace, and cleanup'
if ($env:OS -eq 'Windows_NT') {
    $before = [IO.File]::ReadAllText($result)
    $heldReceipt = [IO.File]::Open($result, [IO.FileMode]::Open,
        [IO.FileAccess]::Read, [IO.FileShare]::Read)
    $rejected = $false
    try {
        try { Write-Result 'failed' 'must preserve the locked receipt' }
        catch { $rejected = $true }
        if (-not $rejected -or [IO.File]::ReadAllText($result) -cne $before) {
            throw 'A rejected atomic replacement did not preserve the existing receipt.'
        }
    } finally { $heldReceipt.Dispose() }
    $probeChecks += ', plus locked-file preservation'
}
if (@(Get-ChildItem -LiteralPath (Split-Path -Parent $result) -Filter '*.tmp-*').Count -ne 0) {
    throw 'Receipt writer left a temporary file.'
}
Write-Host "PASS atomic update receipt $probeChecks"
if ($ReceiptOnly) { exit 0 }

$fixture = Join-Path $Root 'version-check.exe'
Add-Type -OutputAssembly $fixture -OutputType ConsoleApplication -TypeDefinition @'
using System;
using System.IO;
using System.Diagnostics;
using System.Threading;
public static class PeterFanUpdateFixture {
    public static void Main(string[] args) {
        if (args.Length == 1 && args[0] == "--version") {
            Console.WriteLine("peterfan 9.9.9");
            return;
        }
        if (args.Length == 2 && args[0] == "--update-health-token") {
            var mode = File.ReadAllText(Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "fixture-mode"));
            if (mode == "crash") Environment.Exit(17);
            if (mode != "never-ready") {
                var directory = Environment.GetEnvironmentVariable("PETERFAN_TEST_HEALTH_DIR");
                Directory.CreateDirectory(directory);
                var receipt = Path.Combine(directory, "update-health-" + args[1] + ".json");
                var nonce = mode == "wrong-nonce" ? new String('0', 32) : args[1];
                var pid = Process.GetCurrentProcess().Id + (mode == "wrong-pid" ? 1 : 0);
                var version = mode == "wrong-version" ? "1.0.0" : "9.9.9";
                File.WriteAllText(receipt + ".tmp", "{\"nonce\":\"" + nonce + "\",\"pid\":" + pid + ",\"version\":\"" + version + "\"}");
                File.Move(receipt + ".tmp", receipt);
                if (mode == "exit-after-ready") { Thread.Sleep(150); return; }
            }
        }
        Thread.Sleep(120000);
    }
}
'@
Add-Type -OutputAssembly (Join-Path $Root 'version-hang.exe') -OutputType ConsoleApplication -TypeDefinition @'
using System.Threading;
public static class PeterFanVersionHangFixture {
    public static void Main() { Thread.Sleep(30000); }
}
'@

$helperBody = [IO.File]::ReadAllText($Helper)
$helperBody = $helperBody.Replace('$HealthTimeoutMs = 45000', '$HealthTimeoutMs = 5000')
$helperBody = $helperBody.Replace('$SurvivalMs = 2000', '$SurvivalMs = 250')
$helperBody = $helperBody.Replace('$ExitTimeoutMs = 10000', '$ExitTimeoutMs = 500')
$names = @('CHANGELOG.md', 'LICENSE', 'PeterFan.exe', 'README-WINDOWS.txt',
    'THIRD_PARTY_NOTICES.md', 'peterfan-cli.exe', 'peterfan-tui.exe', 'uninstall.ps1')

function Stop-FixtureProcesses([string]$directory) {
    foreach ($process in @(Get-Process -Name PeterFan -ErrorAction SilentlyContinue)) {
        try { $path = $process.Path } catch { continue }
        if ($path -and $path.StartsWith($directory + [IO.Path]::DirectorySeparatorChar,
                [StringComparison]::OrdinalIgnoreCase)) {
            $process.Kill()
            if (-not $process.WaitForExit(5000)) { throw 'Fixture process did not exit.' }
        }
    }
}

function Invoke-Case([string]$name, [string]$mode, [string]$expected, [string]$fault = '') {
    $case = Join-Path $Root $name
    $install = Join-Path $case "My Apps\O'Brien\PeterFan"
    $payload = Join-Path $case 'download\payload'
    $temporary = Join-Path $case 'download'
    $support = Join-Path $case 'support'
    foreach ($directory in @($install, $payload, $support)) {
        [IO.Directory]::CreateDirectory($directory) | Out-Null
    }
    $manifest = [ordered]@{}
    $before = @{}
    foreach ($file in $names) {
        $source = Join-Path $payload $file
        $destination = Join-Path $install $file
        if ($file -eq 'PeterFan.exe') {
            Copy-Item -LiteralPath $fixture -Destination $source
            Copy-Item -LiteralPath $fixture -Destination $destination
            # PE loaders accept trailing data, making previous and replacement
            # executables independently hashable without another compiler run.
            [IO.File]::AppendAllText($destination, 'previous-version')
        } else {
            [IO.File]::WriteAllText($source, "new $file")
            if ($file -ne 'THIRD_PARTY_NOTICES.md') {
                [IO.File]::WriteAllText($destination, "old $file")
            }
        }
        $manifest[$file] = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
        if (Test-Path -LiteralPath $destination) {
            $before[$file] = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash
        }
    }
    [IO.File]::WriteAllText((Join-Path $install 'fixture-mode'), $mode)
    [IO.File]::WriteAllText((Join-Path $install 'user-data.txt'), 'preserve unrelated bytes')
    $result = Join-Path $support 'update-result.json'
    [IO.File]::WriteAllText($result, '{"status":"pending"}')
    $spec = @{ install_dir = $install; payload_dir = $payload; tmp_dir = $temporary;
        result_path = $result; version = '9.9.9'; nonce = [Guid]::NewGuid().ToString('N'); files = $manifest }
    $json = ($spec | ConvertTo-Json -Depth 5 -Compress).Replace("'", "''")
    $script = Join-Path $case 'apply.ps1'
    [IO.File]::WriteAllText($script, ('$spec = ''' + $json + "' | ConvertFrom-Json`n" + $helperBody),
        [Text.UTF8Encoding]::new($true))
    $held = $null
    $env:PETERFAN_TEST_HEALTH_DIR = $support
    try {
        if ($fault -eq 'mid-file') {
            $held = [IO.File]::Open((Join-Path $install 'peterfan-cli.exe'),
                [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
        } elseif ($fault -eq 'tamper') {
            [IO.File]::WriteAllText((Join-Path $payload 'LICENSE'), 'tampered after verification')
        } elseif ($fault -eq 'busy-lock') {
            $held = [IO.File]::Open((Join-Path $install '.peterfan-update.lock'),
                [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        } elseif ($fault -eq 'busy-process') {
            $busy = Start-Process -FilePath (Join-Path $install 'PeterFan.exe') -PassThru
        } elseif ($fault -eq 'receipt-locked') {
            $held = [IO.File]::Open($result, [IO.FileMode]::Open,
                [IO.FileAccess]::Read, [IO.FileShare]::Read)
        }
        & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $script `
            *> (Join-Path $case 'helper.log')
        $exitCode = $LASTEXITCODE
        if (($expected -eq 'installed' -and $exitCode -ne 0) -or
                ($expected -ne 'installed' -and $exitCode -eq 0)) {
            throw "$name returned unexpected exit code $exitCode. $(Get-Content (Join-Path $case 'helper.log') -Raw)"
        }
        $receipt = Get-Content -LiteralPath $result -Raw | ConvertFrom-Json
        if ($receipt.status -ne $expected) { throw "$name status: $($receipt | ConvertTo-Json -Compress)" }
        if ($fault -eq 'busy-process') {
            $busy.Refresh()
            if ($busy.HasExited) { throw 'Busy installed app was stopped rather than leaving its files unchanged.' }
        }
        if ((Get-Content -LiteralPath (Join-Path $install 'user-data.txt') -Raw) -cne 'preserve unrelated bytes') {
            throw "$name modified unrelated user data."
        }
        foreach ($file in $names) {
            $destination = Join-Path $install $file
            if ($expected -eq 'installed') {
                if ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant() -cne $manifest[$file]) {
                    throw "$name did not commit $file"
                }
            } elseif ($before.ContainsKey($file)) {
                if ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -cne $before[$file]) {
                    throw "$name did not restore $file"
                }
            } elseif (Test-Path -LiteralPath $destination) { throw "$name left a newly added $file after rollback" }
        }
        if (@(Get-ChildItem -LiteralPath $install -Force | Where-Object Name -match '^\.peterfan-(stage|backup)-').Count -ne 0) {
            throw "$name left a stage or backup after successful commit or restoration."
        }
        if (@(Get-ChildItem -LiteralPath $support -Filter 'update-health-*.json').Count -ne 0) {
            throw "$name left a startup receipt."
        }
        Write-Host "PASS Windows update transaction: $name"
    } finally {
        if ($null -ne $held) { $held.Dispose() }
        Stop-FixtureProcesses $case
    }
}

$otherDirectory = Join-Path $Root 'unrelated-portable-install'
[IO.Directory]::CreateDirectory($otherDirectory) | Out-Null
$otherApp = Join-Path $otherDirectory 'PeterFan.exe'
Copy-Item -LiteralPath $fixture -Destination $otherApp
$unrelated = Start-Process -FilePath $otherApp -PassThru
try {
    Invoke-Case 'healthy-commit' 'healthy' 'installed'
    Invoke-Case 'startup-exits' 'crash' 'rolled_back'
    Invoke-Case 'startup-never-ready' 'never-ready' 'rolled_back'
    Invoke-Case 'receipt-wrong-nonce' 'wrong-nonce' 'rolled_back'
    Invoke-Case 'receipt-wrong-pid' 'wrong-pid' 'rolled_back'
    Invoke-Case 'receipt-wrong-version' 'wrong-version' 'rolled_back'
    Invoke-Case 'crash-after-ready' 'exit-after-ready' 'rolled_back'
    Invoke-Case 'mid-file-failure' 'healthy' 'rolled_back' 'mid-file'
    Invoke-Case 'payload-tampered' 'healthy' 'failed' 'tamper'
    Invoke-Case 'installed-app-busy' 'healthy' 'failed' 'busy-process'
    Invoke-Case 'receipt-write-fails' 'healthy' 'pending' 'receipt-locked'
    Invoke-Case 'concurrent-helper-refused' 'healthy' 'pending' 'busy-lock'
    $unrelated.Refresh()
    if ($unrelated.HasExited) { throw 'Update stopped a separate portable installation.' }
    Write-Host 'PASS unrelated portable PeterFan process remained alive'
} finally {
    Stop-FixtureProcesses $Root
    Remove-Item Env:PETERFAN_TEST_HEALTH_DIR -ErrorAction SilentlyContinue
}
