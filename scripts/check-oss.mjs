#!/usr/bin/env node
// Dependency-free checks for the public source tree; no network or app actions.
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, extname, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

export function checkRepository(root) {
  root = resolve(root);
  const failures = [];
  const read = (name) => readFileSync(resolve(root, name), 'utf8');
  const required = ['LICENSE', 'Cargo.lock', 'CONTRIBUTING.md', 'SECURITY.md',
    'CODE_OF_CONDUCT.md', 'SUPPORT.md', 'THIRD_PARTY_NOTICES.md', 'docs/DEVELOPMENT.md',
    '.github/pull_request_template.md', '.github/ISSUE_TEMPLATE/bug_report.yml',
    '.github/ISSUE_TEMPLATE/feature_request.yml', '.github/ISSUE_TEMPLATE/config.yml',
    '.github/dependabot.yml'];
  for (const name of required) {
    if (!existsSync(resolve(root, name)) || !read(name).trim()) failures.push(`Missing or empty: ${name}`);
  }
  const cargo = read('Cargo.toml');
  const msrv = cargo.match(/^rust-version\s*=\s*"([0-9.]+)"/m)?.[1];
  if (!msrv) failures.push('Workspace rust-version is missing');
  for (const name of ['README.md', 'README.ko.md', 'README.ja.md', 'README.zh.md']) {
    if (!existsSync(resolve(root, name)) || !read(name).includes(`rust-${msrv}%2B`)) {
      failures.push(`${name}: Rust badge must match workspace minimum ${msrv}`);
    }
  }

  const ignored = new Set(['.git', '.worktrees', 'target', 'dist', 'node_modules']);
  function markdownFiles(directory) {
    const result = [];
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (ignored.has(entry.name) || entry.isSymbolicLink()) continue;
      const path = resolve(directory, entry.name);
      if (entry.isDirectory()) result.push(...markdownFiles(path));
      else if (extname(path) === '.md') result.push(path);
    }
    return result;
  }
  for (const file of markdownFiles(root)) {
    // Fenced examples and inline code are not navigable Markdown links.
    const text = readFileSync(file, 'utf8').replace(/^(```|~~~)[\s\S]*?^\1.*$/gm, '')
      .replace(/`[^`\n]*`/g, '');
    for (const match of text.matchAll(/!?\[[^\]\n]*\]\((<[^>]+>|[^\s)]+)(?:\s+"[^"]*")?\)/g)) {
      const href = match[1].replace(/^<|>$/g, '');
      if (/^(?:[a-z][a-z0-9+.-]*:|\/\/|#)/i.test(href)) continue;
      let path;
      try { path = decodeURIComponent(href.split(/[?#]/)[0]); }
      catch { failures.push(`${relative(root, file)}: malformed link ${href}`); continue; }
      if (!path) continue;
      const destination = resolve(dirname(file), path);
      const local = relative(root, destination);
      if (local === '..' || local.startsWith(`..${sep}`) || !existsSync(destination)) {
        failures.push(`${relative(root, file)}: broken/outside local link ${href}`);
      }
    }
  }

  try {
    const manifest = JSON.parse(read('assets/runner/sources.json'));
    if (manifest.schema_version !== 1 || !Array.isArray(manifest.assets)) throw new Error('invalid schema');
    const pngs = new Set(readdirSync(resolve(root, 'assets/runner')).filter((name) => name.endsWith('.png')));
    const declared = new Map();
    for (const asset of manifest.assets) {
      if (!/^[a-zA-Z0-9-]+\.png$/.test(asset.file) || declared.has(asset.file)) {
        failures.push(`Invalid/duplicate asset name: ${asset.file}`); continue;
      }
      declared.set(asset.file, asset);
      if (asset.license !== 'CC0-1.0' || !asset.author || !/^https:\/\//.test(asset.source) ||
          !['runtime', 'reference'].includes(asset.usage)) failures.push(`Incomplete asset provenance: ${asset.file}`);
      if (!pngs.has(asset.file)) { failures.push(`Asset file missing: ${asset.file}`); continue; }
      const digest = createHash('sha256').update(readFileSync(resolve(root, 'assets/runner', asset.file))).digest('hex');
      if (digest !== asset.sha256) failures.push(`Asset hash changed: ${asset.file}`);
    }
    for (const name of pngs) if (!declared.has(name)) failures.push(`Undocumented PNG: ${name}`);
    const embedded = new Set([...read('packages/menubar/src/runner/assets.rs')
      .matchAll(/assets\/runner\/([a-zA-Z0-9-]+\.png)/g)].map((match) => match[1]));
    for (const name of embedded) if (declared.get(name)?.usage !== 'runtime') failures.push(`Embedded asset not marked runtime: ${name}`);
    for (const asset of declared.values()) {
      if (asset.usage === 'runtime' && !embedded.has(asset.file)) failures.push(`Runtime asset is not embedded: ${asset.file}`);
    }
  } catch (error) {
    failures.push(`Asset inventory could not be checked: ${error.message}`);
  }
  return failures;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const failures = checkRepository(fileURLToPath(new URL('..', import.meta.url)));
  for (const failure of failures) console.error(`FAIL ${failure}`);
  if (failures.length) process.exitCode = 1;
  else console.log('PASS community files, local Markdown links, Rust badges and runner provenance');
}
