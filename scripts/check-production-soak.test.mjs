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
