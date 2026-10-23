import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { resolveReleaseSource } from './check-release-source.mjs';

const script = fileURLToPath(new URL('./check-release-source.mjs', import.meta.url));
const releaseSource = (options) => resolveReleaseSource({ workflowRef: 'refs/heads/main', ...options });

function fixture(t) {
  const cwd = mkdtempSync(join(tmpdir(), 'peterfan-release-source-'));
  t.after(() => rmSync(cwd, { recursive: true, force: true }));
  const git = (...args) => execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
  git('init', '-b', 'main');
  git('config', 'user.email', 'release-test@example.invalid');
  git('config', 'user.name', 'Release Source Test');
  git('config', 'commit.gpgsign', 'false');
  git('config', 'tag.gpgsign', 'false');
  git('config', 'core.autocrlf', 'false');
  writeFileSync(join(cwd, 'fixture.txt'), 'reviewed\n');
  git('add', 'fixture.txt');
  git('commit', '-m', 'reviewed base');
  const base = git('rev-parse', 'HEAD');
  git('tag', 'v1.2.3');
  writeFileSync(join(cwd, 'fixture.txt'), 'reviewed main\n');
  git('commit', '-am', 'reviewed main');
  const main = git('rev-parse', 'HEAD');
  git('update-ref', 'refs/remotes/origin/main', main);
  return { cwd, git, base, main };
}

test('resolves an existing reviewed tag to one immutable commit', (t) => {
  const f = fixture(t);
  const result = releaseSource({ cwd: f.cwd, eventName: 'workflow_dispatch', tag: 'v1.2.3', required: true });
  assert.deepEqual(result, { release: 'true', tag: 'v1.2.3', sha: f.base, reviewed_sha: f.main });
  f.git('tag', '-f', 'v1.2.3', f.main);
  assert.equal(result.sha, f.base, 'resolved build source must not follow a subsequently moved tag');
});

test('accepts annotated tags and canonical prerelease/build identifiers', (t) => {
  const f = fixture(t);
  f.git('tag', '-a', 'v2.0.0-rc.1+build.01', '-m', 'release candidate');
  assert.equal(releaseSource({ cwd: f.cwd, eventName: 'workflow_dispatch', tag: 'v2.0.0-rc.1+build.01' }).sha, f.main);
});

test('rejects malformed SemVer, ref syntax, whitespace, and shell/output injection', (t) => {
  const f = fixture(t);
  const invalid = [
    '1.2.3', 'v1', 'v1.2', 'v01.2.3', 'v1.02.3', 'v1.2.03', 'v1.2.3.4',
    'v1.2.3-01', 'v1.2.3-rc..1', 'v1.2.3+', 'v1.2.3+build..1',
    ' v1.2.3', 'v1.2.3 ', 'v1.2.3\n', 'v1.2.3\r', 'refs/tags/v1.2.3', 'v1.2.3^{commit}', '--help',
    'v1.2.3; touch injected', 'v1.2.3$(touch injected)', 'v1.2.3`touch injected`',
    'v1.2.3\nsha=attacker', 'v1.2.3\r\nrelease=true', 'v1.2.3/other',
  ];
  for (const tag of invalid) {
    assert.throws(() => releaseSource({ cwd: f.cwd, eventName: 'workflow_dispatch', tag }), /canonical/);
  }
});

test('rejects unmerged tag commits, including descendants beyond reviewed main', (t) => {
  const f = fixture(t);
  f.git('switch', '-c', 'unreviewed');
  writeFileSync(join(f.cwd, 'fixture.txt'), 'unreviewed\n');
  f.git('commit', '-am', 'unreviewed change');
  f.git('tag', 'v9.0.0');
  assert.throws(() => releaseSource({ cwd: f.cwd, eventName: 'workflow_dispatch', tag: 'v9.0.0' }), /ancestor/);
});

test('fails closed for a missing tag, missing reviewed ref, or non-commit tag', (t) => {
  const f = fixture(t);
  assert.throws(() => releaseSource({ cwd: f.cwd, eventName: 'workflow_dispatch', tag: 'v9.9.9' }), /resolve release tag/);
  f.git('tag', 'v1.2.4', f.git('rev-parse', 'HEAD:fixture.txt'));
  assert.throws(() => releaseSource({ cwd: f.cwd, eventName: 'workflow_dispatch', tag: 'v1.2.4' }), /resolve release tag/);
  f.git('update-ref', '-d', 'refs/remotes/origin/main');
  assert.throws(() => releaseSource({ cwd: f.cwd, eventName: 'workflow_dispatch', tag: 'v1.2.3' }), /reviewed origin\/main/);
});

test('push and PR checks preserve their supplied HEAD/merge SHA without resolving tags', () => {
  const ciSha = 'a'.repeat(40);
  for (const eventName of ['push', 'pull_request']) {
    assert.deepEqual(releaseSource({ cwd: '/does-not-exist', eventName, ciSha, tag: 'untrusted ignored ref' }),
      { release: 'false', tag: '', sha: ciSha, reviewed_sha: '' });
  }
});

test('an optional empty dispatch is CI-only, while required or invalid sources fail', () => {
  const ciSha = 'b'.repeat(40);
  assert.equal(releaseSource({ eventName: 'workflow_dispatch', ciSha }).release, 'false');
  assert.throws(() => releaseSource({ eventName: 'workflow_dispatch', ciSha, required: true }), /required/);
  for (const invalidSha of ['main', `${ciSha}\n`, `${ciSha}\r`, ` ${ciSha}`]) {
    assert.throws(() => releaseSource({ eventName: 'pull_request', ciSha: invalidSha }), /CI commit SHA/);
  }
  assert.throws(() => releaseSource({ eventName: 'release', ciSha }), /Unsupported/);
});

test('CLI consumes environment data and writes only validated single-line outputs', (t) => {
  const f = fixture(t);
  const output = join(f.cwd, 'outputs');
  execFileSync(process.execPath, [script], { cwd: f.cwd, env: { ...process.env,
    RELEASE_EVENT_NAME: 'workflow_dispatch', RELEASE_WORKFLOW_REF: 'refs/heads/main', RELEASE_TAG: 'v1.2.3', RELEASE_REQUIRED: 'true', GITHUB_OUTPUT: output,
  } });
  assert.equal(readFileSync(output, 'utf8'), `release=true\ntag=v1.2.3\nsha=${f.base}\nreviewed_sha=${f.main}\n`);
  const rejectedOutput = join(f.cwd, 'rejected-outputs');
  const rejected = spawnSync(process.execPath, [script], { cwd: f.cwd, encoding: 'utf8', env: { ...process.env,
    RELEASE_EVENT_NAME: 'workflow_dispatch', RELEASE_WORKFLOW_REF: 'refs/heads/main', RELEASE_TAG: 'v1.2.3\nsha=attacker', GITHUB_OUTPUT: rejectedOutput,
  } });
  assert.equal(rejected.status, 1);
  assert.match(rejected.stderr, /Release source rejected/);
  assert.throws(() => readFileSync(rejectedOutput), { code: 'ENOENT' });
});

// Exercise the actual inline mutation scripts with an offline gh stub. These
// checks never contact GitHub or mutate the checkout's real tags/releases.
function workflowRun(workflow, job, step) {
  const lines = readFileSync(new URL(`../.github/workflows/${workflow}.yml`, import.meta.url), 'utf8').split('\n');
  const jobStart = lines.indexOf(`  ${job}:`);
  assert.ok(jobStart >= 0, `Missing job ${job}`);
  let end = lines.findIndex((line, i) => i > jobStart && /^  [a-zA-Z0-9_-]+:$/.test(line));
  if (end < 0) end = lines.length;
  const start = lines.findIndex((line, i) => i > jobStart && i < end && line === `      - name: ${step}`);
  assert.ok(start >= 0, `Missing step ${step}`);
  const run = lines.findIndex((line, i) => i > start && i < end && line === '        run: |');
  assert.ok(run > start, `Missing run script for ${step}`);
  const body = [];
  for (const line of lines.slice(run + 1, end)) {
    if (line && !line.startsWith('          ')) break;
    body.push(line.slice(10));
  }
  return body.join('\n');
}

function offlineWorkflow(t) {
  const cwd = mkdtempSync(join(tmpdir(), 'peterfan-release-job-'));
  t.after(() => rmSync(cwd, { recursive: true, force: true }));
  const bin = join(cwd, 'bin');
  mkdirSync(bin);
  mkdirSync(join(cwd, 'dist'));
  const gh = join(bin, 'gh');
  writeFileSync(gh, `#!/usr/bin/env node
const { readFileSync, writeFileSync, copyFileSync } = require('node:fs');
const { join } = require('node:path');
const args = process.argv.slice(2);
const e = process.env;
if (args[0] === 'api') {
  if (e.TEST_API_FAIL) process.exit(1);
  if (args[1] !== 'repos/example/PeterFan/commits/v1.2.3') process.exit(2);
  console.log(e.TEST_REMOTE_SHA);
} else if (args[0] === 'release' && args[1] === 'view') {
  if (e.TEST_VIEW_FAIL) process.exit(1);
  console.log('checksums.txt');
} else if (args[0] === 'release' && args[1] === 'download') {
  if (e.TEST_DOWNLOAD_FAIL) process.exit(1);
  copyFileSync(e.TEST_EXISTING_CHECKSUMS, join(args[args.indexOf('--dir') + 1], 'checksums.txt'));
} else if (args[0] === 'release' && args[1] === 'upload') {
  writeFileSync(e.TEST_UPLOAD_RECORD, JSON.stringify(args));
  writeFileSync(e.TEST_UPLOADED_CHECKSUMS, readFileSync(args[4]));
} else { process.exit(3); }
`);
  chmodSync(gh, 0o755);
  const archiveName = 'peterfan-v1.2.3-x86_64-pc-windows-msvc.zip';
  const archive = join(cwd, 'dist', archiveName);
  writeFileSync(archive, 'verified workflow artifact fixture\n');
  const existing = join(cwd, 'existing-checksums.txt');
  writeFileSync(existing, `mac-hash  peterfan-v1.2.3-universal-apple-darwin.tar.gz\ndmg-hash  PeterFan-v1.2.3.dmg\nold-hash  ${archiveName}\n`);
  const env = { ...process.env, PATH: `${bin}:${process.env.PATH}`, RUNNER_TEMP: cwd,
    GH_REPO: 'example/PeterFan', GH_TOKEN: 'offline-fixture', RELEASE_TAG: 'v1.2.3', RELEASE_SHA: 'a'.repeat(40),
    TEST_REMOTE_SHA: 'a'.repeat(40), TEST_EXISTING_CHECKSUMS: existing,
    TEST_UPLOAD_RECORD: join(cwd, 'upload.json'), TEST_UPLOADED_CHECKSUMS: join(cwd, 'uploaded-checksums.txt'),
  };
  const run = (body, overrides = {}) => spawnSync('bash', ['-euo', 'pipefail', '-c', body], {
    cwd, encoding: 'utf8', env: { ...env, ...overrides },
  });
  return { env, run, archive, archiveName };
}

const bashOnly = { skip: process.platform === 'win32' ? 'Inline publication scripts run on Ubuntu.' : false };

test('publication and attachment scripts reject moved tags and API failures', bashOnly, (t) => {
  const f = offlineWorkflow(t);
  for (const [workflow, job] of [['release', 'publish'], ['windows', 'attach-release']]) {
    const body = workflowRun(workflow, job, 'Confirm release tag still points to the verified commit');
    assert.equal(f.run(body).status, 0);
    assert.notEqual(f.run(body, { TEST_REMOTE_SHA: 'b'.repeat(40) }).status, 0);
    assert.notEqual(f.run(body, { TEST_API_FAIL: '1' }).status, 0);
  }
});

test('attachment hashes the downloaded artifact and preserves other platform checksums', bashOnly, (t) => {
  const f = offlineWorkflow(t);
  const result = f.run(workflowRun('windows', 'attach-release', 'Attach archive and refresh release checksums'));
  assert.equal(result.status, 0, result.stderr);
  const uploaded = readFileSync(f.env.TEST_UPLOADED_CHECKSUMS, 'utf8').trim().split('\n');
  assert.equal(uploaded.length, 3);
  assert.ok(uploaded.includes('mac-hash  peterfan-v1.2.3-universal-apple-darwin.tar.gz'));
  assert.ok(uploaded.includes('dmg-hash  PeterFan-v1.2.3.dmg'));
  const digest = createHash('sha256').update(readFileSync(f.archive)).digest('hex');
  assert.ok(uploaded.includes(`${digest}  ${f.archiveName}`));
  assert.deepEqual(JSON.parse(readFileSync(f.env.TEST_UPLOAD_RECORD, 'utf8')).slice(0, 4),
    ['release', 'upload', 'v1.2.3', `dist/${f.archiveName}`]);
});

test('attachment never uploads after a checksum listing or download error', bashOnly, (t) => {
  const f = offlineWorkflow(t);
  const body = workflowRun('windows', 'attach-release', 'Attach archive and refresh release checksums');
  for (const failure of ['TEST_VIEW_FAIL', 'TEST_DOWNLOAD_FAIL']) {
    assert.notEqual(f.run(body, { [failure]: '1' }).status, 0);
    assert.throws(() => readFileSync(f.env.TEST_UPLOAD_RECORD), { code: 'ENOENT' });
  }
});

test('tagged dispatch requires the main workflow ref, including for a reviewed tag', (t) => {
  const f = fixture(t);
  const request = { cwd: f.cwd, eventName: 'workflow_dispatch', tag: 'v1.2.3' };
  assert.equal(resolveReleaseSource({ ...request, workflowRef: 'refs/heads/main' }).sha, f.base);
  for (const workflowRef of ['refs/heads/feature/release', 'refs/tags/v1.2.3', '', undefined]) {
    assert.throws(() => resolveReleaseSource({ ...request, workflowRef }), /must run from refs\/heads\/main/);
  }
});

test('workflow-ref restriction preserves push, PR, and empty-dispatch semantics', () => {
  const ciSha = 'c'.repeat(40);
  for (const eventName of ['push', 'pull_request', 'workflow_dispatch']) {
    for (const workflowRef of ['refs/heads/feature/ci', 'refs/tags/v1.2.3', undefined]) {
      assert.deepEqual(resolveReleaseSource({ eventName, workflowRef, ciSha }),
        { release: 'false', tag: '', sha: ciSha, reviewed_sha: '' });
    }
  }
});

test('CLI rejects missing and non-main workflow refs without release outputs', (t) => {
  const f = fixture(t);
  for (const workflowRef of ['refs/heads/feature/release', 'refs/tags/v1.2.3', '', undefined]) {
    const output = join(f.cwd, `rejected-ref-${workflowRef?.replaceAll('/', '-') ?? 'unset'}`);
    const env = { ...process.env,
      RELEASE_EVENT_NAME: 'workflow_dispatch', RELEASE_WORKFLOW_REF: workflowRef, RELEASE_TAG: 'v1.2.3', GITHUB_OUTPUT: output,
    };
    if (workflowRef === undefined) delete env.RELEASE_WORKFLOW_REF;
    const result = spawnSync(process.execPath, [script], { cwd: f.cwd, encoding: 'utf8', env });
    assert.equal(result.status, 1);
    assert.match(result.stderr, /must run from refs\/heads\/main/);
    assert.throws(() => readFileSync(output), { code: 'ENOENT' });
  }
});

test('privileged release jobs independently require the main workflow ref', () => {
  for (const [workflow, job] of [['release', 'package-macos'], ['release', 'publish'], ['windows', 'attach-release']]) {
    const lines = readFileSync(new URL(`../.github/workflows/${workflow}.yml`, import.meta.url), 'utf8').split('\n');
    const start = lines.indexOf(`  ${job}:`);
    assert.ok(start >= 0);
    let end = lines.findIndex((line, i) => i > start && /^  [a-zA-Z0-9_-]+:$/.test(line));
    if (end < 0) end = lines.length;
    assert.match(lines.slice(start, end).join('\n'), /^    if: github\.ref == 'refs\/heads\/main'(?: && .+)?$/m);
  }
});
