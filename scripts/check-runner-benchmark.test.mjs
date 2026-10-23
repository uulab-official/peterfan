import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const script = fileURLToPath(new URL('./benchmark-runner-macos.py', import.meta.url));
const prelude = `import importlib.util, sys, subprocess\nspec=importlib.util.spec_from_file_location('benchmark',sys.argv[1])\nb=importlib.util.module_from_spec(spec)\nspec.loader.exec_module(b)\n`;
function python(code) {
  return spawnSync('python3', ['-c', prelude + code, script], { encoding: 'utf8', timeout: 10000 });
}

test('native CPU accounting handles seconds, hours and multi-day ps clocks', { skip: process.platform !== 'darwin' }, () => {
  const result = python(`assert b.cpu_seconds('00:12.34') == 12.34
assert b.cpu_seconds('01:02:03.5') == 3723.5
assert b.cpu_seconds('2-01:00:00') == 176400
`);
  assert.equal(result.status, 0, result.stderr);
});

test('benchmark cleanup terminates only its owned child and is repeatable', { skip: process.platform !== 'darwin' }, () => {
  const result = python(`owned=subprocess.Popen([sys.executable,'-c','import time; time.sleep(30)'])
other=subprocess.Popen([sys.executable,'-c','import time; time.sleep(30)'])
try:
 b.stop_owned(owned)
 assert owned.poll() is not None
 assert other.poll() is None
 b.stop_owned(owned)
finally:
 b.stop_owned(owned)
 b.stop_owned(other)
`);
  assert.equal(result.status, 0, result.stderr);
});

test('failed measurement cleans up the owned process without killing an unrelated child', { skip: process.platform !== 'darwin' }, () => {
  const result = python(`import tempfile, pathlib
from types import SimpleNamespace
other=subprocess.Popen([sys.executable,'-c','import time; time.sleep(30)'])
original=b.subprocess.Popen
children=[]
def fixture(*args, **kwargs):
 child=original([sys.executable,'-c','import time; time.sleep(30)'],**kwargs)
 children.append(child)
 return child
b.subprocess.Popen=fixture
b.sample=lambda pid: (_ for _ in ()).throw(RuntimeError('sample unavailable'))
try:
 with tempfile.TemporaryDirectory() as folder:
  try:
   b.measure(pathlib.Path(sys.executable),'fixture',0,SimpleNamespace(warmup=0,duration=5,character='cat',display='both'),pathlib.Path(folder))
   raise AssertionError('failure was swallowed')
  except RuntimeError as error:
   assert str(error)=='sample unavailable'
 assert len(children)==1 and children[0].poll() is not None
 assert other.poll() is None
finally:
 b.stop_owned(other)
 for child in children:b.stop_owned(child)
`);
  assert.equal(result.status, 0, result.stderr);
});
