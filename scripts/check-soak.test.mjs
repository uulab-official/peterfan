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
    identity=m.Identity('/Applications/PeterFan.app/Contents/MacOS/peterfan-menubar','start')
    for scenario in ['pass','changed','missing','threshold']:
        case=Path(d)/scenario
        settings=m.Settings.from_env({'PETERFAN_SOAK_PID':'123','PETERFAN_SOAK_SECONDS':'30','PETERFAN_SOAK_LOG_DIR':str(case)})
        sampler=Mock(); sampler.identity.return_value=identity
        snapshots=[m.Snapshot(t,c,2048,2) for t,c in [(0,1),(10,1.1),(20,1.5),(30,1.7)]]
        sampler.snapshot.side_effect=RuntimeError('missing sample') if scenario=='missing' else snapshots
        original={'exists':True,'sha256':'abc'}
        after={'exists':True,'sha256':'changed'} if scenario=='changed' else original
        if scenario=='threshold': settings.limits['peak_cpu_percent']=1
        with patch.object(m.os,'getloadavg',return_value=(0.,0.,0.),create=True), patch.object(m,'PsSampler',return_value=sampler), patch.object(m.time,'sleep'), patch.object(m,'config_fingerprint',side_effect=[original,after]), patch.object(m.subprocess,'Popen',side_effect=AssertionError('external PID must never launch')):
            result=m.run(settings)
        # Filesystem timestamps may tie on Windows; select this case's unique output.
        receipts=list(case.glob('*/summary.json'))
        assert len(receipts)==1, receipts
        receipt=m.json.loads(receipts[0].read_text())
        # A previous case may even have a later timestamp; it must never be read.
        m.os.utime(receipts[0], (2145916800,2145916800))
        assert receipt['passed'] == (scenario=='pass'), receipt
        assert result == (0 if scenario=='pass' else 1)
        assert receipt['owned_process'] is False
        if scenario=='pass':
            assert receipt['samples']==3
            assert abs(receipt['metrics']['average_cpu_percent']-100*.7/30)<1e-8
            assert len((receipts[0].parent/'samples.csv').read_text().splitlines())==4
        if scenario=='missing': assert receipt['samples']==0
        if scenario=='changed': assert receipt['config_preserved'] is False
`));


test('soak accepts the installed PeterFan bundle and development binary while rejecting unrelated executables', () => python(`
for executable in ['/Applications/PeterFan.app/Contents/MacOS/PeterFan','/tmp/build/PeterFan.app/Contents/MacOS/PeterFan','/tmp/target/release/peterfan-menubar']:
    assert m.is_menu_bar_executable(executable), executable
for executable in ['/tmp/PeterFan','/Applications/Other.app/Contents/MacOS/PeterFan','/Applications/PeterFan.app/PeterFan','/bin/sleep','PeterFan.app/Contents/MacOS/PeterFan']:
    assert not m.is_menu_bar_executable(executable), executable
`));

test('soak rejects Darwin suspend gaps even when uptime advances by a normal sample interval', () => python(`
with tempfile.TemporaryDirectory() as d:
    settings=m.Settings.from_env({'PETERFAN_SOAK_PID':'123','PETERFAN_SOAK_SECONDS':'30','PETERFAN_SOAK_LOG_DIR':d})
    identity=m.Identity('/Applications/PeterFan.app/Contents/MacOS/PeterFan','start')
    original={'exists':True,'sha256':'unchanged'}
    with patch.object(m.sys,'platform','darwin'), patch.object(m.time,'CLOCK_MONOTONIC_RAW',4,create=True), patch.object(m.time,'clock_gettime',side_effect=[0.,130.],create=True) as continuous, patch.object(m.time,'monotonic',side_effect=[0.,10.,20.,30.]), patch.object(m.time,'sleep'), patch.object(m.PsSampler,'identity',return_value=identity), patch.object(m,'ps',return_value='00:01.00 2048'), patch.object(m,'thread_count',return_value=2), patch.object(m.os,'getloadavg',return_value=(0.,0.,0.),create=True), patch.object(m,'config_fingerprint',return_value=original), patch.object(m.subprocess,'Popen',side_effect=AssertionError('external process must never launch')):
        status=m.run(settings)
    receipt=m.json.loads(next(Path(d).glob('*/summary.json')).read_text())
    assert status==1 and not receipt['passed'],receipt
    assert any('sample gap exceeded' in e for e in receipt['errors']),receipt
    assert receipt['config_preserved'] and not receipt['owned_process']
    assert continuous.call_count==2
`));

test('soak releases real reserved space to retain failed sample and CSV ENOSPC receipts', () => python(`
import errno
for fault in ['sample','csv']:
    with tempfile.TemporaryDirectory() as d:
        settings=m.Settings.from_env({'PETERFAN_SOAK_PID':'123','PETERFAN_SOAK_SECONDS':'30','PETERFAN_SOAK_LOG_DIR':d})
        identity=m.Identity('/Applications/PeterFan.app/Contents/MacOS/PeterFan','start')
        sampler=Mock(); sampler.identity.return_value=identity
        original={'exists':True,'sha256':'unchanged'}
        disk={'full':False,'released':False,'reserve_seen':False,'samples':0}
        real_unlink=Path.unlink; real_write=Path.write_text; real_replace=m.os.replace; real_writer=m.csv.writer
        def exhausted():
            reserves=list(Path(d).glob('*/.terminal-reserve-*'))
            disk['reserve_seen']=len(reserves)==1 and 65536 <= reserves[0].stat().st_size <= 1048576
            if reserves and hasattr(reserves[0].stat(),'st_blocks'):
                assert reserves[0].stat().st_blocks*512 >= reserves[0].stat().st_size
            disk['full']=True
            raise OSError(errno.ENOSPC,'injected full volume')
        def snapshot(_identity):
            disk['samples']+=1
            if fault=='sample' and disk['samples']==2: exhausted()
            return m.Snapshot((disk['samples']-1)*10,1.,2048,2)
        sampler.snapshot.side_effect=snapshot
        class Writer:
            def __init__(self,*args,**kwargs): self.inner=real_writer(*args,**kwargs)
            def writerow(self,row):
                if fault=='csv' and row[0]!='elapsed_seconds': exhausted()
                return self.inner.writerow(row)
        def unlink(path,*args,**kwargs):
            result=real_unlink(path,*args,**kwargs)
            if Path(path).name.startswith('.terminal-reserve-'):
                disk['released']=True; disk['full']=False
            return result
        def write(path,*args,**kwargs):
            if disk['full']: raise OSError(errno.ENOSPC,'no space for terminal receipt')
            return real_write(path,*args,**kwargs)
        def replace(source,destination,*args,**kwargs):
            if disk['full']: raise OSError(errno.ENOSPC,'no space for terminal receipt')
            return real_replace(source,destination,*args,**kwargs)
        with patch.object(m,'PsSampler',return_value=sampler), patch.object(m.os,'getloadavg',return_value=(0.,0.,0.),create=True), patch.object(m.time,'sleep'), patch.object(m,'config_fingerprint',return_value=original), patch.object(m.csv,'writer',Writer), patch.object(Path,'unlink',unlink), patch.object(Path,'write_text',write), patch.object(m.os,'replace',side_effect=replace), patch.object(m.subprocess,'Popen',side_effect=AssertionError('external PID must never launch')):
            status=m.run(settings)
        receipt=m.json.loads(next(Path(d).glob('*/summary.json')).read_text())
        assert status==1 and not receipt['passed'],receipt
        assert disk['reserve_seen'] and disk['released'],disk
        assert receipt['config_preserved'] and not receipt['owned_process'],receipt
        assert any('full volume' in error for error in receipt['errors']),receipt
        assert not list(Path(d).glob('*/.terminal-reserve-*'))
`));

test('soak retries terminal ENOSPC as a failed durable receipt and never reports PASS', () => python(`
import errno, io
for persistent in [False,True]:
    with tempfile.TemporaryDirectory() as d:
        settings=m.Settings.from_env({'PETERFAN_SOAK_PID':'123','PETERFAN_SOAK_SECONDS':'30','PETERFAN_SOAK_LOG_DIR':d})
        sampler=Mock(); sampler.identity.return_value=m.Identity('/Applications/PeterFan.app/Contents/MacOS/PeterFan','start')
        sampler.snapshot.side_effect=[m.Snapshot(t,1.,2048,2) for t in [0,10,20,30]]
        original={'exists':False,'sha256':None}
        attempts=[0]; real_write=Path.write_text; real_replace=m.os.replace
        def failure():
            attempts[0]+=1
            if persistent or attempts[0]==1: raise OSError(errno.ENOSPC,'injected terminal ENOSPC')
        def write(path,*args,**kwargs):
            if path.name=='summary.json': failure()
            return real_write(path,*args,**kwargs)
        def replace(source,destination,*args,**kwargs):
            if Path(destination).name=='summary.json': failure()
            return real_replace(source,destination,*args,**kwargs)
        stderr=io.StringIO(); stdout=io.StringIO()
        with patch.object(m,'PsSampler',return_value=sampler), patch.object(m.os,'getloadavg',return_value=(0.,0.,0.),create=True), patch.object(m.time,'sleep'), patch.object(m,'config_fingerprint',return_value=original), patch.object(Path,'write_text',write), patch.object(m.os,'replace',side_effect=replace), patch.object(m.sys,'stderr',stderr), patch.object(m.sys,'stdout',stdout), patch.object(m.subprocess,'Popen',side_effect=AssertionError('external PID must never launch')):
            status=m.run(settings)
        assert status==1 and not stdout.getvalue().startswith('PASS'),stdout.getvalue()
        receipts=list(Path(d).glob('*/summary.json'))
        if not persistent:
            assert len(receipts)==1
            receipt=m.json.loads(receipts[0].read_text())
            assert not receipt['passed'] and receipt['config_preserved'],receipt
            assert any('terminal' in error for error in receipt['errors']),receipt
        else:
            assert len(stderr.getvalue()) <= 4096 and 'terminal' in stderr.getvalue(),stderr.getvalue()
            assert not receipts or not m.json.loads(receipts[0].read_text())['passed']
        assert 1 <= attempts[0] <= 2,attempts
        assert not list(Path(d).glob('*/.terminal-reserve-*'))
`));

test('soak reserves and fsyncs emergency bytes before sampling and fails closed if reservation fails', () => python(`
import errno
with tempfile.TemporaryDirectory() as d:
    settings=m.Settings.from_env({'PETERFAN_SOAK_PID':'123','PETERFAN_SOAK_SECONDS':'30','PETERFAN_SOAK_LOG_DIR':d})
    sampler=Mock(); sampler.identity.return_value=m.Identity('/Applications/PeterFan.app/Contents/MacOS/PeterFan','start')
    sampler.snapshot.side_effect=[m.Snapshot(t,1.,2048,2) for t in [0,10,20,30]]
    original={'exists':False,'sha256':None}; real_sync=m.os.fsync; synced=[0]
    def sync(descriptor):
        synced[0]+=1
        if synced[0]==1: raise OSError(errno.ENOSPC,'injected reserve fsync failure')
        return real_sync(descriptor)
    with patch.object(m,'PsSampler',return_value=sampler), patch.object(m.os,'getloadavg',return_value=(0.,0.,0.),create=True), patch.object(m.time,'sleep'), patch.object(m,'config_fingerprint',return_value=original), patch.object(m.os,'fsync',side_effect=sync), patch.object(m.subprocess,'Popen',side_effect=AssertionError('external PID must never launch')):
        status=m.run(settings)
    sampler.snapshot.assert_not_called()
    receipt=m.json.loads(next(Path(d).glob('*/summary.json')).read_text())
    assert status==1 and not receipt['passed'] and receipt['config_preserved'],receipt
    assert any('reserve fsync failure' in error for error in receipt['errors']),receipt
    assert not list(Path(d).glob('*/.terminal-reserve-*'))
`));

test('soak records scoped SIGTERM and SIGINT failures and restores previous handlers', () => python(`
for signum in [m.signal.SIGTERM,m.signal.SIGINT]:
    with tempfile.TemporaryDirectory() as d:
        settings=m.Settings.from_env({'PETERFAN_SOAK_PID':'123','PETERFAN_SOAK_SECONDS':'30','PETERFAN_SOAK_LOG_DIR':d})
        previous={sig:m.signal.getsignal(sig) for sig in [m.signal.SIGTERM,m.signal.SIGINT]}
        sampler=Mock(); sampler.identity.return_value=m.Identity('/Applications/PeterFan.app/Contents/MacOS/PeterFan','start')
        sampler.snapshot.return_value=m.Snapshot(0.,1.,2048,2)
        original={'exists':True,'sha256':'unchanged'}
        def interrupt(_duration):
            handler=m.signal.getsignal(signum)
            assert callable(handler),'run did not install its scoped handler'
            handler(signum,None)
        with patch.object(m,'PsSampler',return_value=sampler), patch.object(m.os,'getloadavg',return_value=(0.,0.,0.),create=True), patch.object(m.time,'sleep',side_effect=interrupt), patch.object(m,'config_fingerprint',return_value=original), patch.object(m.subprocess,'Popen',side_effect=AssertionError('external PID must never launch')):
            status=m.run(settings)
        receipt=m.json.loads(next(Path(d).glob('*/summary.json')).read_text())
        assert status==1 and not receipt['passed'] and receipt['config_preserved'],receipt
        assert any(m.signal.Signals(signum).name in error for error in receipt['errors']),receipt
        assert all(m.signal.getsignal(sig)==handler for sig,handler in previous.items())
        assert not list(Path(d).glob('*/.terminal-reserve-*'))
`));

test('soak reserve closes its descriptor before unlink and never removes foreign files', () => python(`
with tempfile.TemporaryDirectory() as d:
    output=Path(d)
    foreign=output/'.terminal-reserve-foreign'; foreign.write_bytes(b'preserve')
    reservation=m.reserve_result_space(output)
    descriptor=reservation.descriptor
    assert reservation.path.stat().st_size==m.RESULT_RESERVE_BYTES
    real_unlink=Path.unlink
    def unlink(path,*args,**kwargs):
        if path==reservation.path:
            try: m.os.fstat(descriptor)
            except OSError: pass
            else: raise AssertionError('open reserve cannot be unlinked portably')
        return real_unlink(path,*args,**kwargs)
    with patch.object(Path,'unlink',unlink):
        reservation.release(); reservation.release()
    assert not reservation.path.exists()
    assert foreign.read_bytes()==b'preserve'
`));

test('soak partial terminal writes are cleaned and retried only as failed receipts', () => python(`
import errno
with tempfile.TemporaryDirectory() as d:
    output=Path(d); reservation=m.reserve_result_space(output)
    receipt={'passed':True,'errors':[],'config_preserved':True}
    real_write=m.os.write; injected=[False]
    def write(descriptor,data):
        if not injected[0]:
            injected[0]=True
            real_write(descriptor,data[:20])
            raise OSError(errno.ENOSPC,'injected partial terminal write')
        return real_write(descriptor,data)
    with patch.object(m.os,'write',side_effect=write):
        persisted=m.publish_terminal_result(output,receipt,reservation)
    assert persisted and not receipt['passed'],receipt
    saved=m.json.loads((output/'summary.json').read_text())
    assert not saved['passed'] and saved['config_preserved'],saved
    assert any('partial terminal write' in error for error in saved['errors']),saved
    assert not list(output.glob('.terminal-*'))
`));

test('soak terminal publication supports a named supervisor result and rejects path escape', () => python(`
with tempfile.TemporaryDirectory() as d:
    output=Path(d)/'run'; output.mkdir()
    unrelated=output/'summary.json'; unrelated.write_bytes(b'preserve default receipt')
    reservation=m.reserve_result_space(output)
    receipt={'passed':False,'errors':['injected failed supervisor result']}
    for filename in ['../outside.json','/absolute.json','..','nested/result.json',r'nested\\result.json']:
        try: m.publish_terminal_result(output,receipt,reservation,filename=filename)
        except ValueError: pass
        else: raise AssertionError(filename)
        assert reservation.path.exists(),'invalid arguments consumed reserve'
    assert m.publish_terminal_result(output,receipt,reservation,filename='combined-summary.json')
    assert m.json.loads((output/'combined-summary.json').read_text())==receipt
    assert unrelated.read_bytes()==b'preserve default receipt'
    assert not list(output.glob('.terminal-*'))
`));

test('soak withdraws an unconfirmed PASS if directory sync and failed replacement cannot persist', () => python(`
import errno,io
with tempfile.TemporaryDirectory() as d:
    output=Path(d); reservation=m.reserve_result_space(output)
    receipt={'passed':True,'errors':[]}; summary=output/'summary.json'
    real_sync=m._sync_directory; real_replace=m.os.replace; attempts=[0]
    def sync(directory):
        if summary.exists() and m.json.loads(summary.read_text())['passed']:
            raise OSError(errno.EIO,'injected post-rename directory sync failure')
        return real_sync(directory)
    def replace(source,destination):
        attempts[0]+=1
        if attempts[0]>1: raise OSError(errno.ENOSPC,'injected failed replacement')
        return real_replace(source,destination)
    with patch.object(m,'_sync_directory',side_effect=sync), patch.object(m.os,'replace',side_effect=replace), patch.object(m.sys,'stderr',io.StringIO()):
        persisted=m.publish_terminal_result(output,receipt,reservation)
    assert not persisted and not receipt['passed'],receipt
    assert not summary.exists(),'unconfirmed positive receipt survived'
    assert attempts[0]==2
    assert not list(output.glob('.terminal-*'))
`));

test('soak SIGTERM on an owned observer child durably fails without touching external target or private config', { skip: process.platform === 'win32' }, () => python(`
import subprocess,time,signal
child_code='''
import importlib.util,sys
from pathlib import Path
from unittest.mock import Mock,patch
sys.dont_write_bytecode=True
spec=importlib.util.spec_from_file_location('soak_child',sys.argv[1]); m=importlib.util.module_from_spec(spec); sys.modules['soak_child']=m; spec.loader.exec_module(m)
root=Path(sys.argv[2]); ready=root/'ready'; private_home=root/'home'
settings=m.Settings.from_env({'PETERFAN_SOAK_PID':'123','PETERFAN_SOAK_SECONDS':'30','PETERFAN_SOAK_LOG_DIR':str(root/'logs')})
sampler=Mock(); sampler.identity.return_value=m.Identity('/Applications/PeterFan.app/Contents/MacOS/PeterFan','start')
def snapshot(_identity):
    ready.write_text('ready')
    return m.Snapshot(m.observation_clock(),1.,2048,2)
sampler.snapshot.side_effect=snapshot
with patch.object(m,'PsSampler',return_value=sampler), patch.object(Path,'home',return_value=private_home), patch.object(m.subprocess,'Popen',side_effect=AssertionError('external PID must never launch')):
    status=m.run(settings)
sys.exit(status)
'''
with tempfile.TemporaryDirectory() as d:
    root=Path(d); config=root/'home/Library/Application Support/peterfan/config.toml'; config.parent.mkdir(parents=True); config.write_bytes(b'private original config')
    child=subprocess.Popen([sys.executable,'-c',child_code,sys.argv[1],str(root)],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    try:
        deadline=time.monotonic()+10
        while not (root/'ready').exists():
            assert child.poll() is None,'observer exited before readiness'
            assert time.monotonic()<deadline,'observer readiness timed out'
            time.sleep(.01)
        child.send_signal(signal.SIGTERM)
        stdout,stderr=child.communicate(timeout=10)
        assert child.returncode==1,(stdout,stderr,child.returncode)
        receipt=m.json.loads(next((root/'logs').glob('*/summary.json')).read_text())
        assert not receipt['passed'] and receipt['config_preserved'] and not receipt['owned_process'],receipt
        assert any('SIGTERM' in error for error in receipt['errors']),receipt
        assert config.read_bytes()==b'private original config'
        assert not list((root/'logs').glob('*/.terminal-*'))
    finally:
        if child.poll() is None: child.kill(); child.wait(timeout=10)
`));
