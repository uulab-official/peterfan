import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const supervisor = fileURLToPath(new URL('./macos-production-soak.py', import.meta.url));
const observer = fileURLToPath(new URL('./macos-soak.py', import.meta.url));
function python(body) {
  const result = spawnSync(process.platform === 'win32' ? 'python' : 'python3', ['-B', '-c', `
import importlib.util, json, shutil, sys, tempfile
from pathlib import Path
from unittest.mock import Mock, patch
spec=importlib.util.spec_from_file_location('production',sys.argv[1])
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
${body}`, supervisor, observer], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
}

test('production observer records insufficient storage before starting child observers or reading hardware', () => python(`
with tempfile.TemporaryDirectory() as d:
    m.ROOT=Path(d)
    shutil.copyfile(sys.argv[2],m.ROOT/'observer-source.py')
    (m.ROOT/'launch.json').write_text(json.dumps({'requested_seconds':21600,'sample_interval_seconds':10,'scope':'test only','job_label':'test-only'}))
    before={sig:m.signal.getsignal(sig) for sig in [m.signal.SIGTERM,m.signal.SIGINT]}
    with patch.object(m.shutil,'disk_usage',return_value=Mock(free=0)), patch.object(m.subprocess,'Popen',side_effect=AssertionError('must not start any process')), patch.object(m,'ipc',side_effect=AssertionError('must not access hardware')):
        status=m.main()
    receipt=json.loads((m.ROOT/'combined-summary.json').read_text())
    assert status==1 and not receipt['passed'] and receipt['samples']==0,receipt
    assert any('less than 1 GiB' in e for e in receipt['errors']),receipt
    assert all(m.signal.getsignal(sig)==handler for sig,handler in before.items())
    assert not list(m.ROOT.glob('.terminal-reserve-*'))
`));

test('production observer rejects unsafe automatic control and stale or cumulative helper failures', () => python(`
valid={'daemon_version':'1.32.7','mode':'auto','fan_targets':{},'fan_overrides':{},'applied_control_revision':3,'control_revision':3,'temps_stale':False,'control_health':{'failsafe_active':False,'last_error':None,**{key:0 for key in m.COUNTERS}}}
import copy
cases=[]
for key,value in [('mode','manual'),('fan_targets',{'0':4000}),('fan_overrides',{'0':4000}),('applied_control_revision',2),('temps_stale',True),('daemon_version','1.32.6')]:
    case=copy.deepcopy(valid);case[key]=value;cases.append(case)
for key,value in [('failsafe_active',True),('last_error','sensor unavailable'),*[(key,1) for key in m.COUNTERS]]:
    case=copy.deepcopy(valid);case['control_health'][key]=value;cases.append(case)
for case in [valid,*cases]:
    calls=[]
    def response(command):
        calls.append(command);return 'ok '+json.dumps(case)
    with patch.object(m,'ipc',side_effect=response):
        try: result=m.healthy_cache('1.32.7')
        except RuntimeError: assert case!=valid
        else: assert case==valid and result==valid
    assert calls==['temps'],calls
`));

test('production app acceptance rejects relaxed child thresholds and independently checks every metric', () => python(`
import copy
valid={'passed':True,'requested_seconds':21600,'measured_seconds':21600.2,'config_preserved':True,'thresholds':dict(m.LIMITS),'metrics':{key:0 for key in m.LIMITS}}
valid['metrics']['peak_rss_kb']=1024
m.validate_app_summary(valid,21600)
for field in m.LIMITS:
    for kind in ['relaxed','over_limit','nan','missing']:
        bad=copy.deepcopy(valid)
        if kind=='relaxed': bad['thresholds'][field]=m.LIMITS[field]+100
        if kind=='over_limit': bad['metrics'][field]=m.LIMITS[field]+1
        if kind=='nan': bad['metrics'][field]=float('nan')
        if kind=='missing': del bad['metrics'][field]
        try: m.validate_app_summary(bad,21600)
        except (RuntimeError,KeyError): pass
        else: raise AssertionError((field,kind,bad))
`));

test('production app environment pins all five acceptance limits despite inherited overrides', () => python(`
plan={'app_pid':123,'app_executable':'/Applications/PeterFan.app/Contents/MacOS/PeterFan','requested_seconds':21600,'sample_interval_seconds':10}
with patch.dict(m.os.environ,{'PETERFAN_SOAK_MAX_AVG_CPU':'100','PETERFAN_SOAK_MAX_PEAK_CPU':'100','PETERFAN_SOAK_MAX_RSS_KB':'999999999','PETERFAN_SOAK_MAX_RSS_GROWTH_KB':'999999999','PETERFAN_SOAK_MAX_THREAD_GROWTH':'999999999'}):
    env=m.app_observer_environment(plan)
assert env['PETERFAN_SOAK_MAX_AVG_CPU']=='8'
assert env['PETERFAN_SOAK_MAX_PEAK_CPU']=='35'
assert env['PETERFAN_SOAK_MAX_RSS_KB']=='262144'
assert env['PETERFAN_SOAK_MAX_RSS_GROWTH_KB']=='65536'
assert env['PETERFAN_SOAK_MAX_THREAD_GROWTH']=='16'
`));

const versionRun = `
from types import SimpleNamespace
import copy

def run_versions(app_version, pinned_daemon, actual_daemon, log_version, health_versions=None):
    with tempfile.TemporaryDirectory() as d:
        m.ROOT=Path(d)
        shutil.copyfile(sys.argv[2],m.ROOT/'observer-source.py')
        app=m.ROOT/'PeterFan';app.write_bytes(b'app')
        config=m.ROOT/'config.json';config.write_bytes(b'{}')
        log=m.ROOT/'menubar.log'
        log.write_text(f'101 v{log_version} app update completed automatic=true phase=current channel=stable last_attempt=Some(101) last_success=Some(101) next_check=Some(21701)\\n')
        (m.ROOT/'update-check-history.json').write_text(json.dumps({'check_error':None,'last_success_unix':101}))
        plan={'version':app_version,'app_pid':123,'daemon_pid':456,'app_started':'app-start','daemon_started':'helper-start','app_executable':str(app),'config_path':str(config),'requested_seconds':30,'sample_interval_seconds':10,'scope':'fixture only','job_label':'fixture','observer_sha256':'pinned','supervisor_sha256':'pinned','app_sha256':'pinned','daemon_sha256':'pinned','config_sha256':'pinned','require_scheduled_update':True,'started_unix':100,'menubar_log_path':str(log),'menubar_log_offset':0}
        if pinned_daemon is not None: plan['daemon_version']=pinned_daemon
        (m.ROOT/'launch.json').write_text(json.dumps(plan))
        receipt=m.ROOT/'observer/menubar-fixture/summary.json';receipt.parent.mkdir(parents=True)
        receipt.write_text(json.dumps({'passed':True,'requested_seconds':30,'measured_seconds':30,'config_preserved':True,'thresholds':dict(m.LIMITS),'metrics':{key:0 for key in m.LIMITS}}))
        original_load=m.importlib.util.module_from_spec
        def load(spec):
            module=original_load(spec)
            original_exec=spec.loader.exec_module
            def execute(target):
                original_exec(target)
                class Sampler:
                    def __init__(self,pid): self.pid=pid;self.clock=-10
                    def identity(self):
                        return target.Identity(str(app),'app-start') if self.pid==123 else target.Identity('/usr/local/bin/peterfand','helper-start')
                    def snapshot(self,pinned):
                        assert self.identity()==pinned
                        self.clock+=10
                        return target.Snapshot(self.clock,0,1024,1)
                target.PsSampler=Sampler
                target.ps=lambda *_:'fixture-start'
            spec.loader.exec_module=execute
            return module
        health=iter(health_versions) if health_versions else None
        def response(command):
            if command=='version': return 'ok peterfand '+actual_daemon
            if command=='status': return 'ok auto (macos)'
            assert command=='temps'
            return 'ok '+json.dumps({'daemon_version':next(health) if health else actual_daemon,'mode':'auto','fan_targets':{},'fan_overrides':{},'applied_control_revision':3,'control_revision':3,'temps_stale':False,'control_health':{'failsafe_active':False,'last_error':None,**{key:0 for key in m.COUNTERS}}})
        process=Mock(pid=789);process.poll.return_value=None;process.wait.return_value=0
        with patch.object(m.importlib.util,'module_from_spec',side_effect=load), patch.object(m,'sha',return_value='pinned'), patch.object(m,'ipc',side_effect=response), patch.object(m.subprocess,'Popen',return_value=process), patch.object(m.shutil,'disk_usage',return_value=Mock(free=2*1024**3)), patch.object(m.time,'sleep'):
            status=m.main()
        result=json.loads((m.ROOT/'combined-summary.json').read_text())
        return status,result
`;

test('production observer accepts independently pinned app and helper versions', () => python(versionRun + `
status,result=run_versions('1.32.8','1.32.7','1.32.7','1.32.8')
assert status==0 and result['passed'] and result['samples']==3,result
assert result['automatic_update_verified']=={'last_success_unix':101,'next_check_unix':21701},result
`));

test('production observer preserves legacy same-version baselines', () => python(versionRun + `
status,result=run_versions('1.32.7',None,'1.32.7','1.32.7')
assert status==0 and result['passed'] and result['samples']==3,result
`));

test('production observer rejects wrong helper health, periodic helper version and app update version', () => python(versionRun + `
for actual,log,health,expected in [
    ('1.32.8','1.32.8',None,'helper health/automatic'),
    ('1.32.8','1.32.8',['1.32.7'],'helper version/status'),
    ('1.32.7','1.32.8',['1.32.7','1.32.8'],'helper health/automatic'),
    ('1.32.7','1.32.7',None,'native automatic update log/history/deadline mismatch'),
]:
    status,result=run_versions('1.32.8','1.32.7',actual,log,health)
    assert status==1 and not result['passed'] and any(expected in error for error in result['errors']),result
`));

test('installed launcher freezes explicit helper version and defaults legacy baselines to app version', () => python(`
spec=importlib.util.spec_from_file_location('launcher',Path(sys.argv[1]).with_name('start-installed-soak.py'))
launcher=importlib.util.module_from_spec(spec);spec.loader.exec_module(launcher)
from datetime import timezone
for daemon_version,expected in [('1.32.7','1.32.7'),(None,'1.32.8')]:
    with tempfile.TemporaryDirectory() as d:
        home=Path(d)
        (home/'Library/Application Support').mkdir(parents=True)
        config=home/'config.json';config.write_text('{}')
        config.with_name('update-check-history.json').write_text('{}')
        log=home/'Library/Logs/PeterFan/menubar.log';log.parent.mkdir(parents=True);log.write_text('prior log')
        baseline={'version':'1.32.8','app_pid':123,'daemon_pid':456,'app_started':'pinned-start','daemon_started':'pinned-start','app_executable':'/Applications/PeterFan.app/Contents/MacOS/PeterFan','config_path':str(config),'app_sha256':'pinned','daemon_sha256':'pinned','config_sha256':'pinned'}
        if daemon_version is not None: baseline['daemon_version']=daemon_version
        source=home/'baseline.json';source.write_text(json.dumps(baseline))
        with patch.object(launcher.sys,'argv',['launcher','--baseline',str(source)]), patch.object(launcher.sys,'platform','darwin'), patch.object(launcher.os,'getuid',return_value=home.stat().st_uid,create=True), patch.object(launcher,'ZoneInfo',return_value=timezone.utc), patch.object(launcher.Path,'home',return_value=home), patch.object(launcher,'sha',return_value='pinned'), patch.object(launcher,'identity',return_value='pinned-start'), patch.object(launcher.subprocess,'check_output',return_value='source-commit'), patch.object(launcher.subprocess,'run'), patch.object(launcher.shutil,'disk_usage',return_value=Mock(free=2*1024**3)):
            launcher.main()
        launches=list((home/'Library/Application Support/PeterFan QA').glob('production-six-hour-*/launch.json'))
        assert len(launches)==1,launches
        plan=json.loads(launches[0].read_text())
        assert plan['version']=='1.32.8' and plan['daemon_version']==expected,plan
        assert plan['daemon_sha256']=='pinned' and plan['daemon_pid']==456 and plan['daemon_started']=='pinned-start',plan
        assert plan['require_scheduled_update'] and plan['requested_seconds']==21600 and plan['sample_interval_seconds']==10,plan
        assert json.loads(source.read_text())==baseline
`));
