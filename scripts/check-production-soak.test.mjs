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
