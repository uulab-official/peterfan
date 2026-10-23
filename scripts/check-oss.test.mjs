import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { checkRepository } from './check-oss.mjs';

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'peterfan-oss-test-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const write = (name, content) => {
    mkdirSync(dirname(join(root, name)), { recursive: true });
    writeFileSync(join(root, name), content);
  };
  for (const name of ['LICENSE', 'Cargo.lock', 'CONTRIBUTING.md', 'SECURITY.md',
    'CODE_OF_CONDUCT.md', 'SUPPORT.md', 'THIRD_PARTY_NOTICES.md', 'docs/DEVELOPMENT.md',
    '.github/pull_request_template.md', '.github/ISSUE_TEMPLATE/bug_report.yml',
    '.github/ISSUE_TEMPLATE/feature_request.yml', '.github/ISSUE_TEMPLATE/config.yml',
    '.github/dependabot.yml']) write(name, 'Fixture documentation\n');
  write('Cargo.toml', '[workspace.package]\nrust-version = "1.88"\n');
  for (const name of ['README.md', 'README.ko.md', 'README.ja.md', 'README.zh.md']) {
    write(name, '[Rust](https://example.org/rust-1.88%2B)\n[Guide](docs/DEVELOPMENT.md#setup)\n');
  }
  const png = readFileSync(new URL('../assets/runner/dino-run-1-cc0.png', import.meta.url));
  write('assets/runner/dino.png', png);
  const inventory = { schema_version: 1, assets: [{ file: 'dino.png', author: 'Fixture author',
    license: 'CC0-1.0', source: 'https://example.org/art', usage: 'runtime',
    sha256: createHash('sha256').update(png).digest('hex') }] };
  const saveInventory = () => write('assets/runner/sources.json', JSON.stringify(inventory));
  saveInventory();
  write('packages/menubar/src/runner/assets.rs', 'include_bytes!("assets/runner/dino.png");');
  return { root, write, inventory, saveInventory };
}

test('complete tree passes without checking external URLs or anchor targets', (t) => {
  const { root } = fixture(t);
  assert.deepEqual(checkRepository(root), []);
});

test('a broken local link fails, while fenced examples are ignored', (t) => {
  const { root, write } = fixture(t);
  write('docs/DEVELOPMENT.md', '```md\n[Example](missing.md)\n```\n[Actual](absent.md)\n');
  const failures = checkRepository(root);
  assert.equal(failures.length, 1);
  assert.match(failures[0], /absent\.md/);
});

test('outside-tree links and malformed escapes are rejected', (t) => {
  const { root, write } = fixture(t);
  write('docs/DEVELOPMENT.md', '[Outside](../../secret)\n[Bad](%ZZ)\n');
  const failures = checkRepository(root);
  assert.equal(failures.length, 2);
  assert(failures.some((failure) => failure.includes('outside')));
  assert(failures.some((failure) => failure.includes('malformed')));
});

test('a changed asset or an added undocumented PNG blocks the gate', (t) => {
  const { root, write } = fixture(t);
  write('assets/runner/dino.png', 'modified image bytes');
  write('assets/runner/unknown.png', 'unreviewed bytes');
  const failures = checkRepository(root);
  assert(failures.some((failure) => failure.includes('Asset hash changed')));
  assert(failures.some((failure) => failure.includes('Undocumented PNG')));
});

test('runtime/reference usage drift and malformed inventory fail closed', (t) => {
  const { root, inventory, saveInventory, write } = fixture(t);
  inventory.assets[0].usage = 'reference';
  saveInventory();
  assert(checkRepository(root).some((failure) => failure.includes('not marked runtime')));
  write('assets/runner/sources.json', '{');
  assert(checkRepository(root).some((failure) => failure.includes('could not be checked')));
});

test('missing public policy and a stale localized Rust badge are detected', (t) => {
  const { root, write } = fixture(t);
  rmSync(join(root, 'SECURITY.md'));
  write('README.ja.md', '[Rust](https://example.org/rust-1.80%2B)');
  const failures = checkRepository(root);
  assert(failures.some((failure) => failure.includes('SECURITY.md')));
  assert(failures.some((failure) => failure.includes('README.ja.md')));
});
