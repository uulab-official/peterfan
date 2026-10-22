import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, chmodSync, rmSync, symlinkSync, realpathSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import nodeTest from 'node:test';

// This macOS launcher uses POSIX process paths. Its fake tools use symlinks;
// Windows contributors should not need Developer Mode/admin rights for them.
const test = (name, fn) => nodeTest(name, {
  skip: process.platform === 'win32' ? 'macOS-only launcher; POSIX fixtures run on macOS/Linux.' : false,
}, fn);

const launcher = readFileSync(new URL('../script/build_and_run.sh', import.meta.url), 'utf8');
function runCase({ mode = '--verify', own = [], foreign = [], buildFail = false, missing = false, platform = 'Darwin', openForeign = false, psFail = false, reused = false } = {}) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'peterfan launcher [test] ')));
  try {
    const bin = join(root, 'fake-bin');
    mkdirSync(bin);
    mkdirSync(join(root, 'script'));
    mkdirSync(join(root, 'scripts'));
    const app = join(root, 'dist/PeterFan.app/Contents/MacOS/PeterFan');
    const raw = join(root, 'target/release/peterfan-menubar');
    const state = join(root, 'state.json');
    const events = join(root, 'events.txt');
    writeFileSync(state, JSON.stringify([
      ...own.map((kind, i) => ({ pid: 100 + i, exe: kind === 'raw' ? raw : app })),
      ...foreign.map((exe, i) => ({ pid: 900 + i, exe })),
    ]));
    writeFileSync(events, '');
    if (!missing) { mkdirSync(join(root, 'dist/PeterFan.app/Contents/MacOS'), { recursive: true }); writeFileSync(app, 'fixture'); chmodSync(app, 0o755); }
    const tool = join(bin, 'tool.cjs');
    writeFileSync(tool, `#!${process.execPath}
const fs=require('node:fs'),path=require('node:path');
const name=path.basename(process.argv[1]),args=process.argv.slice(2),env=process.env;
let state=JSON.parse(fs.readFileSync(env.FAKE_STATE));
const log=s=>fs.appendFileSync(env.FAKE_EVENTS,s+'\\n');
if(name==='uname'){console.log(env.FAKE_PLATFORM);process.exit(0)}
if(name==='ps'){
 if(env.FAKE_PS_FAIL==='1')process.exit(2);
 if(args.includes('-p')){const p=state.find(p=>p.pid===Number(args[args.indexOf('-p')+1]));if(p)console.log(env.FAKE_REUSED==='1'?'/Applications/Other.app/Contents/MacOS/PeterFan':p.exe)}
 else for(const p of state)console.log('  '+p.pid+' '+p.exe);
 process.exit(0);
}
if(name==='cargo'){log('build '+args.join(' '));process.exit(env.FAKE_BUILD_FAIL==='1'?1:0)}
if(name==='fake-kill'){log('kill '+args.join(' '));state=state.filter(p=>p.pid!==Number(args.at(-1)));fs.writeFileSync(env.FAKE_STATE,JSON.stringify(state));process.exit(0)}
if(name==='open'){log('open');state.push({pid:555,exe:env.FAKE_OPEN_FOREIGN==='1'?'/Applications/PeterFan.app/Contents/MacOS/PeterFan':env.FAKE_APP});fs.writeFileSync(env.FAKE_STATE,JSON.stringify(state));process.exit(0)}
if(name==='sleep')process.exit(0);
throw Error('Unexpected fake tool '+name);
`);
    chmodSync(tool, 0o755);
    for (const name of ['uname', 'ps', 'cargo', 'fake-kill', 'open', 'sleep']) symlinkSync(tool, join(bin, name));
    const bashEnv = join(root, 'bash-env');
    writeFileSync(bashEnv, 'kill() { "$FAKE_BIN/fake-kill" "$@"; }\n');
    const bundle = join(root, 'scripts/bundle-macos.sh');
    writeFileSync(bundle, '#!/usr/bin/env bash\nset -eu\necho bundle >> "$FAKE_EVENTS"\nmkdir -p "$(dirname "$FAKE_APP")"\nprintf fixture > "$FAKE_APP"\nchmod +x "$FAKE_APP"\n');
    chmodSync(bundle, 0o755);
    const script = join(root, 'script/build_and_run.sh');
    writeFileSync(script, launcher);
    const result = spawnSync('bash', [script, mode], { encoding: 'utf8', timeout: 10000, env: {
      ...process.env, PATH: `${bin}:${process.env.PATH}`, BASH_ENV: bashEnv, FAKE_BIN: bin,
      FAKE_STATE: state, FAKE_EVENTS: events, FAKE_APP: app, FAKE_PLATFORM: platform,
      FAKE_BUILD_FAIL: buildFail ? '1' : '0', FAKE_OPEN_FOREIGN: openForeign ? '1' : '0',
      FAKE_PS_FAIL: psFail ? '1' : '0', FAKE_REUSED: reused ? '1' : '0',
    } });
    assert.ifError(result.error);
    return { ...result, events: readFileSync(events, 'utf8').trim().split('\n').filter(Boolean), remaining: JSON.parse(readFileSync(state)), app, raw };
  } finally { rmSync(root, { recursive: true, force: true }); }
}

test('build failure leaves own running app untouched', () => {
  const result = runCase({ own: ['bundle'], buildFail: true });
  assert.notEqual(result.status, 0);
  assert.equal(result.events.length, 1);
  assert.match(result.events[0], /^build \+stable build --locked/);
  assert.equal(result.remaining.length, 1);
});
test('installed app and other checkout are never stopped', () => {
  const result = runCase({ own: ['bundle'], foreign: ['/Applications/PeterFan.app/Contents/MacOS/PeterFan', '/another checkout/target/release/peterfan-menubar'] });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Quit that instance yourself/);
  assert.equal(result.events.length, 1);
  assert.equal(result.remaining.length, 3);
});
test('no-build validates executable before stopping any process', () => {
  const result = runCase({ mode: '--no-build', own: ['raw'], missing: true });
  assert.notEqual(result.status, 0);
  assert.deepEqual(result.events, []);
  assert.equal(result.remaining.length, 1);
});
test('successful verify stops only exact owned paths after build', () => {
  const result = runCase({ own: ['bundle', 'raw'], foreign: ['/Applications/PeterFan.app/Contents/MacOS/PeterFan-helper'] });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.events[0], /^build /);
  assert.deepEqual(result.events.slice(1), ['kill -TERM 100', 'kill -TERM 101', 'bundle', 'open']);
  assert.equal(result.remaining.filter(p => p.exe === result.app).length, 1);
  assert.equal(result.remaining.find(p => p.pid === 900).exe, '/Applications/PeterFan.app/Contents/MacOS/PeterFan-helper');
});
test('verify never counts an unrelated installed instance as success', () => {
  const result = runCase({ openForeign: true });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /from this checkout, found 0/);
});
test('help is available without macOS and performs no actions', () => {
  const result = runCase({ mode: '--help', platform: 'Linux' });
  assert.equal(result.status, 0);
  assert.match(result.stdout, /macOS only/);
  assert.deepEqual(result.events, []);
});
test('non-macOS launch is rejected before build or process actions', () => {
  const result = runCase({ platform: 'Linux' });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /requires macOS/);
  assert.deepEqual(result.events, []);
});
test('failed process inspection fails closed without signaling or opening', () => {
  const result = runCase({ own: ['bundle'], psFail: true });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /cannot inspect/);
  assert.equal(result.events.length, 1);
});
test('PID path recheck protects an executable that changed since inventory', () => {
  const result = runCase({ own: ['bundle'], reused: true });
  assert.notEqual(result.status, 0);
  assert.equal(result.events.length, 1);
  assert.match(result.stderr, /did not exit/);
});
