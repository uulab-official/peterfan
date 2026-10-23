import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';

const source = readFileSync(new URL('../packages/menubar/src/main.rs', import.meta.url), 'utf8');
const script = [
  source.slice(source.indexOf('function compareVersions(a,b)'), source.indexOf('function openLatestRelease(')),
  source.slice(source.indexOf('function updateUpdateChannel(d)'), source.indexOf('function toggleStartupItem(')),
  source.slice(source.indexOf('var RAIL_NAV_READY=false;'), source.indexOf('function setHealthValue(')),
].join('\n');
const now = 1_790_000_000;

function dashboard(language = 'en') {
  const node = () => ({
    textContent: '', disabled: false, value: '', style: {}, dataset: {},
    options: [{}, {}], setAttribute() {},
  });
  const nodes = Object.fromEntries([
    'update-current-version', 'update-latest-version', 'update-check-result',
    'update-last-attempt', 'update-last-success', 'update-next-check',
    'update-release-link', 'rail-update-install', 'rail-update-check',
    'update-release-notes-card', 'update-release-notes', 'rail-update-copy', 'update-channel',
  ].map(id => [id, node()]));
  const remembered = [];
  class Clock extends Date { static now() { return now * 1000; } }
  const context = vm.createContext({
    LANG: language, APP_UPDATE_STATUS: null, APP_UPDATE_CHECK_PENDING: false,
    APP_PERSISTED_UPDATE_KEY: '', Date: Clock,
    document: { getElementById: id => nodes[id] || null },
    window: {}, railView: () => 'settings',
    setText: (id, text) => { if (nodes[id]) nodes[id].textContent = text; },
    setPanelPill() {}, setButtonLabel() {}, updateHealthPanel() {},
    rememberUpdateStatus: value => remembered.push(value),
  });
  vm.runInContext(script, context);
  const update = (status = {}, receipt = null) => context.updateRail({
    app_version: '1.32.4', update_channel: 'stable', update_install_result: receipt,
    app_update_status: { phase: 'idle', channel: 'stable', auto_check_enabled: true, ...status },
  });
  return { context, nodes, update, remembered };
}

const receipt = (status, version = '1.32.4', age = 60) => ({ status, version, updated_at_unix: now - age });

test('automatic check failure shows its retry and last successful check without enabling an old release', () => {
  const ui = dashboard();
  ui.context.APP_UPDATE_STATUS = { latest: '9.0.0', install_ready: true, phase: 'available' };
  ui.update({ phase: 'failed', check_error: 'Network unavailable', last_attempt_unix: now, last_success_unix: now - 3600, next_check_unix: now + 300 });
  assert.match(ui.nodes['update-check-result'].textContent, /retry scheduled/);
  assert.match(ui.nodes['rail-update-copy'].textContent, /Network unavailable/);
  assert.notEqual(ui.nodes['update-last-success'].textContent, 'Not yet');
  assert.notEqual(ui.nodes['update-next-check'].textContent, '—');
  assert.equal(ui.nodes['rail-update-check'].disabled, false);
  assert.equal(ui.nodes['rail-update-install'].disabled, true);
});

test('first native snapshot replaces a previous-session or other-channel release claim', () => {
  const ui = dashboard();
  ui.context.APP_UPDATE_STATUS = { latest: '9.0.0', phase: 'current', channel: 'pre-release', cached: true };
  ui.update({ next_check_unix: now + 20 });
  assert.equal(ui.nodes['update-latest-version'].textContent, '—');
  assert.equal(ui.nodes['update-last-success'].textContent, 'Not yet');
  assert.match(ui.nodes['update-check-result'].textContent, /scheduled/);
});

test('incomplete verified release metadata leaves installation disabled', () => {
  const ui = dashboard();
  ui.update({ phase: 'available', latest: '1.33.0', install_ready: false, last_success_unix: now, next_check_unix: now + 300 });
  assert.equal(ui.nodes['rail-update-install'].disabled, true);
  assert.equal(ui.nodes['rail-update-install'].textContent, 'Preparing Update');
});

test('mock sessions disable check and install even with a cached available release', () => {
  const ui = dashboard();
  ui.update({ phase: 'available', latest: '1.33.0', install_ready: true, auto_check_enabled: false });
  assert.equal(ui.nodes['rail-update-check'].disabled, true);
  assert.equal(ui.nodes['rail-update-install'].disabled, true);
  assert.match(ui.nodes['rail-update-copy'].textContent, /mock mode/);
});

test('running ahead of the published release is described accurately', () => {
  const ui = dashboard();
  ui.update({ phase: 'current', latest: '1.32.2', install_ready: true });
  assert.match(ui.nodes['update-check-result'].textContent, /ahead/);
  assert.equal(ui.nodes['rail-update-install'].disabled, true);
});

test('installed receipt only claims success for the current running version within one day', () => {
  const ui = dashboard();
  for (const invalid of [receipt('installed', '1.32.2'), receipt('installed', '1.32.4', 86401), receipt('installed', '1.32.4', -1)]) {
    ui.update({}, invalid);
    assert.doesNotMatch(ui.nodes['update-check-result'].textContent, /installed successfully/);
  }
  ui.update({}, receipt('installed'));
  assert.equal(ui.nodes['update-check-result'].textContent, 'installed successfully');
});

test('valid installation receipts survive initial browser cache reconciliation', () => {
  const ui = dashboard();
  ui.context.APP_UPDATE_STATUS = { phase: 'current', latest: '1.32.2', cached: true };
  ui.update({ next_check_unix: now + 20 }, receipt('installed'));
  assert.equal(ui.nodes['update-check-result'].textContent, 'installed successfully');
  assert.notEqual(ui.nodes['update-next-check'].textContent, '—');
});

test('stale, future, invalid, or already-running pending receipts never imply ongoing installation', () => {
  for (const invalid of [receipt('pending', '1.33.0', 301), receipt('pending', '1.33.0', -1), receipt('pending', 'invalid'), receipt('pending', '1.32.4')]) {
    const ui = dashboard();
    ui.update({}, invalid);
    assert.doesNotMatch(ui.nodes['update-check-result'].textContent, /installation in progress/);
    assert.equal(ui.nodes['rail-update-check'].disabled, false);
  }
});

test('a recent pending receipt expires and never blocks fresh metadata checks', () => {
  const ui = dashboard();
  ui.update({}, receipt('pending', '1.33.0', 20));
  assert.equal(ui.nodes['update-check-result'].textContent, 'installation in progress');
  assert.equal(ui.nodes['rail-update-check'].disabled, false);
  ui.update({}, receipt('pending', '1.33.0', 301));
  assert.match(ui.nodes['update-check-result'].textContent, /scheduled/);
});

test('native request channel wins over a stale window channel and cannot change during a check', () => {
  const ui = dashboard();
  ui.update({ phase: 'checking', channel: 'pre-release' });
  assert.equal(ui.nodes['update-channel'].value, 'pre-release');
  assert.equal(ui.nodes['update-channel'].disabled, true);
  assert.equal(ui.nodes['rail-update-check'].disabled, true);
});

test('Korean automatic retry state and absent check history are localized', () => {
  const ui = dashboard('ko');
  ui.update({ phase: 'failed', check_error: 'offline', next_check_unix: now + 300 });
  assert.match(ui.nodes['update-check-result'].textContent, /재시도 예정/);
  assert.equal(ui.nodes['update-last-success'].textContent, '아직 없음');
  assert.match(ui.nodes['rail-update-copy'].textContent, /자동으로 다시 시도/);
});

test('failed installation copy does not promise an unchanged or restored app', () => {
  const ui = dashboard('ko');
  ui.update({}, receipt('failed', '1.33.0'));
  assert.match(ui.nodes['rail-update-copy'].textContent, /설치 상태를 확인/);
  assert.doesNotMatch(ui.nodes['rail-update-copy'].textContent, /변경하지|복원/);
});

test('Korean asset preparation explains the disabled install action', () => {
  const ui = dashboard('ko');
  ui.update({ phase: 'available', latest: '1.33.0', install_ready: false, message: 'Files not ready' });
  assert.match(ui.nodes['rail-update-copy'].textContent, /설치 파일.*자동으로 다시 확인/);
  assert.equal(ui.nodes['rail-update-install'].disabled, true);
});

test('invalid metadata cannot describe the running app as current', () => {
  const ui = dashboard('ko');
  ui.update({ phase: 'current', latest: 'invalid', install_ready: true });
  assert.match(ui.nodes['rail-update-copy'].textContent, /확인할 수 없습니다/);
  assert.doesNotMatch(ui.nodes['rail-update-copy'].textContent, /사용 중/);
  assert.equal(ui.nodes['rail-update-install'].disabled, true);
});
