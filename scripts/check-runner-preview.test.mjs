import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, copyFileSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';

function freshSource(t) {
  const root = mkdtempSync(join(tmpdir(), 'peterfan-preview-source-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const name of ['scripts/check-runner-preview.mjs', 'packages/menubar/src/main.rs']) {
    mkdirSync(dirname(join(root, name)), { recursive: true });
    copyFileSync(new URL(`../${name}`, import.meta.url), join(root, name));
  }
  return root;
}

test('frontend checks run in a fresh source tree without native build artifacts', (t) => {
  const result = spawnSync(process.execPath, ['scripts/check-runner-preview.mjs', '--web-only'],
    { cwd: freshSource(t), encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /native cadence comparison was not run/);
  assert.match(result.stdout, /CPU response/);
});

test('the full cadence gate refuses missing evidence instead of silently passing', (t) => {
  const result = spawnSync(process.execPath, ['scripts/check-runner-preview.mjs'],
    { cwd: freshSource(t), encoding: 'utf8' });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Missing native cadence evidence/);
});
