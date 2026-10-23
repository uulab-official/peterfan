import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';

const source = readFileSync(new URL('../packages/menubar/src/main.rs', import.meta.url), 'utf8');
const settingsScript = source.slice(source.indexOf('var AUTOMATION_RULES=null;'), source.indexOf('function setSetupMenuOpen('));
const notificationScript = source.slice(source.indexOf('function notificationThreshold('), source.indexOf('var AUTOMATION_RULES=null;'));

function dashboard() {
  const makeNode = () => ({
    value: '', textContent: '', className: '', disabled: false, checked: false,
    children: [], appendChild(child) { this.children.push(child); }, setAttribute() {},
  });
  const nodes = Object.fromEntries([
    'automation-status', 'automation-rules', 'automation-add', 'automation-save',
    'config-transfer-status', 'config-transfer-text', 'config-copy', 'config-import',
    'notification-temp-toggle', 'notification-temp-threshold',
    'notification-fan-toggle', 'notification-update-toggle',
  ].map(id => [id, makeNode()]));
  const messages = [], timers = [];
  const document = {
    activeElement: null, getElementById: id => nodes[id] || null,
    createElement: makeNode, createTextNode: text => ({ textContent: text }),
  };
  const context = vm.createContext({
    LANG: 'en', document, setPanelPill() {},
    window: { ipc: { postMessage: message => messages.push(message) } },
    setTimeout: (callback, delay) => { timers.push({ callback, delay }); return timers.length; },
  });
  vm.runInContext(notificationScript + '\n' + settingsScript, context);
  const request = () => {
    const message = messages.at(-1);
    assert.ok(message?.startsWith('settingsrequest:'), 'Settings requests must carry a native request ID');
    return JSON.parse(message.slice('settingsrequest:'.length));
  };
  const result = (request, overrides = {}) => ({
    id: request.id, operation: request.operation, ok: true,
    daemon_reload: 'reloaded', message: 'Saved', ...overrides,
  });
  return { context, nodes, document, messages, timers, request, result };
}

const initialRules = [{ when: 'on_battery', profile: 'silent' }];

function startRuleSave(ui) {
  ui.context.renderAutomationRules({ rules: initialRules });
  ui.context.AUTOMATION_RULES[0].profile = 'performance';
  ui.context.automationMarkDirty();
  ui.context.saveAutomationRules();
}

test('a previous rules success cannot acknowledge a new save', () => {
  const ui = dashboard();
  startRuleSave(ui);
  ui.context.renderAutomationRules({ rules: initialRules, last_cmd_status: 'rules saved (1)' });
  assert.ok(ui.context.AUTOMATION_RULES_SAVE_PENDING);
  assert.equal(ui.context.AUTOMATION_RULES_DIRTY, true);
  assert.equal(ui.context.AUTOMATION_RULES[0].profile, 'performance');
});

test('rule edits made during a save survive its matching success', () => {
  const ui = dashboard();
  startRuleSave(ui);
  const request = ui.request();
  ui.context.AUTOMATION_RULES[0].profile = 'gaming';
  ui.context.automationMarkDirty();
  assert.ok(ui.context.AUTOMATION_RULES_SAVE_PENDING, 'Editing must not discard the in-flight request');
  ui.context.renderAutomationRules({ rules: [{ when: 'on_battery', profile: 'performance' }], settings_results: [ui.result(request)] });
  assert.equal(ui.context.AUTOMATION_RULES_SAVE_PENDING, null);
  assert.equal(ui.context.AUTOMATION_RULES_DIRTY, true);
  assert.equal(ui.context.AUTOMATION_RULES[0].profile, 'gaming');
  assert.match(ui.nodes['automation-status'].textContent, /unsaved/i);
});

test('repeated saves use different IDs and ignore older or unrelated operation results', () => {
  const ui = dashboard();
  startRuleSave(ui);
  const first = ui.request();
  const savedRules = [{ when: 'on_battery', profile: 'performance' }];
  ui.context.renderAutomationRules({ rules: savedRules, settings_results: [ui.result(first)] });
  assert.equal(ui.context.AUTOMATION_RULES_DIRTY, false);
  ui.context.AUTOMATION_RULES[0].profile = 'gaming';
  ui.context.automationMarkDirty();
  ui.context.saveAutomationRules();
  const second = ui.request();
  assert.notEqual(first.id, second.id);
  ui.context.renderAutomationRules({ rules: savedRules, settings_results: [ui.result(first), ui.result(second, { operation: 'import_config' })] });
  assert.ok(ui.context.AUTOMATION_RULES_SAVE_PENDING);
  assert.equal(ui.context.AUTOMATION_RULES[0].profile, 'gaming');
});

test('a failed rules save preserves the draft and allows retry', () => {
  const ui = dashboard();
  startRuleSave(ui);
  const request = ui.request();
  ui.context.renderAutomationRules({ rules: initialRules, settings_results: [ui.result(request, { ok: false, daemon_reload: 'not_attempted', message: 'Permission denied' })] });
  assert.equal(ui.context.AUTOMATION_RULES_SAVE_PENDING, null);
  assert.equal(ui.context.AUTOMATION_RULES_DIRTY, true);
  assert.equal(ui.context.AUTOMATION_RULES[0].profile, 'performance');
  assert.equal(ui.nodes['automation-save'].disabled, false);
  assert.match(ui.nodes['automation-status'].textContent, /Permission denied/);
});

test('disk-only settings success reports deferred or failed daemon reload truthfully', () => {
  for (const reload of ['deferred', 'failed', 'not_applicable']) {
    const ui = dashboard();
    startRuleSave(ui);
    const request = ui.request();
    ui.context.renderAutomationRules({ rules: [{ when: 'on_battery', profile: 'performance' }], settings_results: [ui.result(request, { daemon_reload: reload })] });
    assert.equal(ui.context.AUTOMATION_RULES_DIRTY, false);
    assert.doesNotMatch(ui.nodes['automation-status'].textContent, /daemon reloaded/i, reload);
    if (reload !== 'not_applicable') assert.match(ui.nodes['automation-status'].textContent, /reload.*pending|reload.*failed/i, reload);
  }
});

test('only the matching import result completes an import and keeps newer text', () => {
  const ui = dashboard();
  ui.nodes['config-transfer-text'].value = 'profile = "silent"';
  ui.context.markConfigImportDirty();
  ui.context.importConfig();
  ui.context.updateConfigTransfer({ config_toml: 'old config', last_cmd_status: 'config imported (0 rules)' });
  assert.ok(ui.context.CONFIG_IMPORT_PENDING);
  const request = ui.request();
  ui.nodes['config-transfer-text'].value = 'profile = "gaming"';
  ui.context.markConfigImportDirty();
  ui.context.updateConfigTransfer({ config_toml: 'profile = "silent"', settings_results: [ui.result(request, { daemon_reload: 'deferred' })] });
  assert.equal(ui.context.CONFIG_IMPORT_PENDING, null);
  assert.equal(ui.context.CONFIG_IMPORT_DIRTY, true);
  assert.equal(ui.nodes['config-transfer-text'].value, 'profile = "gaming"');
  assert.match(ui.nodes['config-transfer-status'].textContent, /unsaved/i);
});

test('failed imports retain the pasted TOML and enable retry', () => {
  const ui = dashboard();
  const draft = 'invalid = [';
  ui.nodes['config-transfer-text'].value = draft;
  ui.context.markConfigImportDirty();
  ui.context.importConfig();
  const request = ui.request();
  ui.context.updateConfigTransfer({ config_toml: 'saved = true', settings_results: [ui.result(request, { ok: false, daemon_reload: 'not_attempted', message: 'Invalid TOML' })] });
  assert.equal(ui.context.CONFIG_IMPORT_PENDING, null);
  assert.equal(ui.context.CONFIG_IMPORT_DIRTY, true);
  assert.equal(ui.nodes['config-transfer-text'].value, draft);
  assert.equal(ui.nodes['config-import'].disabled, false);
  assert.match(ui.nodes['config-transfer-status'].textContent, /Invalid TOML/);
});

test('another window import response cannot complete this window request', () => {
  const ui = dashboard();
  ui.nodes['config-transfer-text'].value = 'profile = "silent"';
  ui.context.markConfigImportDirty();
  ui.context.importConfig();
  const request = ui.request();
  ui.context.updateConfigTransfer({ config_toml: 'other window config', settings_results: [ui.result(request, { id: 'other-window-request' })] });
  assert.ok(ui.context.CONFIG_IMPORT_PENDING);
  assert.equal(ui.nodes['config-transfer-text'].value, 'profile = "silent"');
});

test('unconfirmed saves and imports allow manual retry without erasing drafts', () => {
  const ui = dashboard();
  startRuleSave(ui);
  ui.nodes['config-transfer-text'].value = 'profile = "silent"';
  ui.context.markConfigImportDirty();
  ui.context.importConfig();
  assert.equal(ui.timers.length, 2);
  for (const timer of ui.timers) {
    assert.equal(timer.delay, 15_000);
    timer.callback();
  }
  assert.equal(ui.context.AUTOMATION_RULES_SAVE_PENDING, null);
  assert.equal(ui.context.CONFIG_IMPORT_PENDING, null);
  assert.equal(ui.context.AUTOMATION_RULES_DIRTY, true);
  assert.equal(ui.context.CONFIG_IMPORT_DIRTY, true);
  assert.equal(ui.nodes['automation-save'].disabled, false);
  assert.equal(ui.nodes['config-import'].disabled, false);
  assert.equal(ui.nodes['config-transfer-text'].value, 'profile = "silent"');
  assert.equal(ui.messages.length, 2, 'Timeout must not retry automatically');
  ui.context.saveAutomationRules();
  const request = ui.context.AUTOMATION_RULES_SAVE_PENDING;
  ui.timers[0].callback();
  assert.equal(ui.context.AUTOMATION_RULES_SAVE_PENDING, request, 'Old timeout must not cancel a new request');
});

test('temperature snapshots preserve focused typing and refresh after blur', () => {
  const ui = dashboard();
  const input = ui.nodes['notification-temp-threshold'];
  input.value = '9';
  ui.document.activeElement = input;
  ui.context.updateNotificationSettings({ notifications: { temperature_c: 85 } });
  assert.equal(input.value, '9');
  ui.document.activeElement = null;
  ui.context.updateNotificationSettings({ notifications: { temperature_c: 91 } });
  assert.equal(input.value, '91');
  ui.context.updateNotificationSettings({ notifications: { temperature_c: null } });
  assert.equal(input.disabled, true);
  assert.equal(ui.nodes['notification-temp-toggle'].checked, false);
});
