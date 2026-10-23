import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const helper = fileURLToPath(new URL('./macos-soak.py', import.meta.url));
function python(body) {
  const result = spawnSync(process.platform === 'win32' ? 'python' : 'python3', ['-c', `import importlib.util, sys, tempfile\nfrom pathlib import Path\nfrom unittest.mock import Mock, patch\nsys.dont_write_bytecode=True\nspec=importlib.util.spec_from_file_location('soak',sys.argv[1])\nm=importlib.util.module_from_spec(spec)\nsys.modules['soak']=m\nspec.loader.exec_module(m)\n${body}`, helper], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
}

test('soak derives interval CPU from cumulative process time, including multi-core usage', () => python(`
assert m.parse_cpu_time('02:03.45') == 123.45
assert m.parse_cpu_time('120:03.45') == 7203.45
assert m.parse_cpu_time('01:02:03.50') == 3723.5
assert m.parse_cpu_time('2-01:02:03.50') == 176523.5
assert m.cpu_percent(100, 102, 1) == 200
assert m.cpu_percent(100, 100.1, 10) < 1.01
for args in [(2,1,1),(1,2,0)]:
    try: m.cpu_percent(*args)
    except ValueError: pass
    else: raise AssertionError('invalid interval accepted')
`));

test('soak rejects invalid and unbounded arguments before launching anything', () => python(`
for key,value in [('PETERFAN_SOAK_PID','-1'),('PETERFAN_SOAK_SECONDS','999999999999'),('PETERFAN_SOAK_SAMPLE_SECONDS','0'),('PETERFAN_SOAK_MAX_AVG_CPU','nan'),('PETERFAN_SOAK_MAX_PEAK_CPU','-1'),('PETERFAN_SOAK_MAX_RSS_KB','1.5')]:
    try: m.Settings.from_env({key:value})
    except ValueError: pass
    else: raise AssertionError((key,value))
`));

test('soak never terminates an external, exited, or replaced process', () => python(`
identity=m.Identity('/test/PeterFan', 'Fri Oct 23 10:00:00 2026')
proc=Mock(); proc.poll.return_value=None
lookup=Mock(return_value=m.Identity('/other/app','later'))
try: m.guarded_terminate(proc, identity, lookup)
except RuntimeError: pass
else: raise AssertionError('replaced identity must invalidate the run')
proc.terminate.assert_not_called()
proc.poll.return_value=0
m.guarded_terminate(proc,identity,Mock(return_value=identity))
proc.terminate.assert_not_called()
m.guarded_terminate(None,identity,Mock(return_value=identity))
proc.poll.return_value=None
m.guarded_terminate(proc,identity,Mock(return_value=identity))
proc.terminate.assert_called_once()
`));

test('soak requires valid samples and detects process identity changes', () => python(`
i=m.Identity('/test/app','start')
s=m.PsSampler(123)
with patch.object(s,'identity',side_effect=[i,m.Identity('/other','start')]), patch.object(m,'ps',return_value='00:01.00 2048'), patch.object(m,'thread_count',return_value=2):
    try: s.snapshot(i)
    except RuntimeError: pass
    else: raise AssertionError('replaced process accepted')
for row in ['', '00:01.00 0', 'bad 1024']:
    with patch.object(s,'identity',return_value=i), patch.object(m,'ps',return_value=row), patch.object(m,'thread_count',return_value=2):
        try: s.snapshot(i)
        except (RuntimeError,ValueError): pass
        else: raise AssertionError(row)
`));

test('soak receipts detect changed, created, and deleted configuration without writing it', () => python(`
with tempfile.TemporaryDirectory() as d:
    p=Path(d)/'config.toml'
    absent=m.config_fingerprint(p)
    p.write_bytes(b'original')
    original=m.config_fingerprint(p)
    assert original != absent
    assert p.read_bytes() == b'original'
    p.write_bytes(b'changed')
    assert m.config_fingerprint(p) != original
    p.unlink()
    assert m.config_fingerprint(p) == absent
`));

test('soak parses the actual macOS thread table and rejects header-only or foreign rows', () => python(`
result=Mock()
result.stdout='USER   PID   TT   %CPU STAT PRI STIME UTIME COMMAND\\nuulab 123 ?? 0 S 31T 0:00.00 0:00.00 app\\n      123    0 S 31T 0:00.00 0:00.00\\n'
with patch.object(m.subprocess,'run',return_value=result):
    assert m.thread_count(123) == 2
    for output in ['USER PID TT\\n', 'USER PID TT\\nuulab 124 ??\\n', 'USER PID TT\\nuulab 123 ??\\n124 0 S\\n']:
        result.stdout=output
        try: m.thread_count(123)
        except RuntimeError: pass
        else: raise AssertionError(output)
`));

test('soak writes a positive structured receipt and fails closed on configuration change or missing samples', () => python(`
with tempfile.TemporaryDirectory() as d:
    settings=m.Settings.from_env({'PETERFAN_SOAK_PID':'123','PETERFAN_SOAK_SECONDS':'30','PETERFAN_SOAK_LOG_DIR':d})
    identity=m.Identity('/Applications/PeterFan.app/Contents/MacOS/peterfan-menubar','start')
    for scenario in ['pass','changed','missing','threshold']:
        sampler=Mock(); sampler.identity.return_value=identity
        snapshots=[m.Snapshot(t,c,2048,2) for t,c in [(0,1),(10,1.1),(20,1.5),(30,1.7)]]
        sampler.snapshot.side_effect=RuntimeError('missing sample') if scenario=='missing' else snapshots
        original={'exists':True,'sha256':'abc'}
        after={'exists':True,'sha256':'changed'} if scenario=='changed' else original
        if scenario=='threshold': settings.limits['peak_cpu_percent']=1
        with patch.object(m.os,'getloadavg',return_value=(0.,0.,0.),create=True), patch.object(m,'PsSampler',return_value=sampler), patch.object(m.time,'sleep'), patch.object(m,'config_fingerprint',side_effect=[original,after]), patch.object(m.subprocess,'Popen',side_effect=AssertionError('external PID must never launch')):
            result=m.run(settings)
        receipts=sorted(Path(d).glob('*/summary.json'),key=lambda p:p.stat().st_mtime_ns)
        receipt=m.json.loads(receipts[-1].read_text())
        assert receipt['passed'] == (scenario=='pass'), receipt
        assert result == (0 if scenario=='pass' else 1)
        assert receipt['owned_process'] is False
        if scenario=='pass':
            assert receipt['samples']==3
            assert abs(receipt['metrics']['average_cpu_percent']-100*.7/30)<1e-8
            assert len((receipts[-1].parent/'samples.csv').read_text().splitlines())==4
        if scenario=='missing': assert receipt['samples']==0
        if scenario=='changed': assert receipt['config_preserved'] is False
`));


test('soak accepts the installed PeterFan bundle and development binary while rejecting unrelated executables', () => python(`
for executable in ['/Applications/PeterFan.app/Contents/MacOS/PeterFan','/tmp/build/PeterFan.app/Contents/MacOS/PeterFan','/tmp/target/release/peterfan-menubar']:
    assert m.is_menu_bar_executable(executable), executable
for executable in ['/tmp/PeterFan','/Applications/Other.app/Contents/MacOS/PeterFan','/Applications/PeterFan.app/PeterFan','/bin/sleep','PeterFan.app/Contents/MacOS/PeterFan']:
    assert not m.is_menu_bar_executable(executable), executable
`));
