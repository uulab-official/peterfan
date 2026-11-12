import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const source = readFileSync(new URL('../packages/menubar/src/main.rs', import.meta.url), 'utf8');
const focusStart = source.indexOf('function focusRunnerCharacterOption(');
const start = focusStart >= 0 ? focusStart : source.indexOf('function closeRunnerCharacterMenu(');
const end = source.indexOf('function chooseRunnerCharacter(', start);
assert.ok(start >= 0 && end > start, 'runner picker code must exist');

function setup() {
  const calls = [], focusOptions = [];
  const options = ['cat', 'dog', 'bird'].map(character => ({
    dataset: { character }, focus(options) { calls.push(character); focusOptions.push(options); },
    getBoundingClientRect() { return { top: 5, bottom: 45 }; },
  }));
  const menu = { hidden: false, classList: { remove() {}, add() {} },
    scrollTop: 0, getBoundingClientRect() { return { top: 0, bottom: 100 }; },
    querySelector: () => options[0] };
  const trigger = { setAttribute() {}, focus() { calls.push('trigger'); }, getClientRects() { return [1]; }, getAttribute() { return null; }, closest() { return null; } };
  const before = { focus() { calls.push('before'); }, getClientRects() { return [1]; }, getAttribute() { return null; }, closest() { return null; } };
  const after = { focus() { calls.push('after'); }, getClientRects() { return [1]; }, getAttribute() { return null; }, closest() { return null; } };
  const context = { document: {
    getElementById(id) { return id === 'runner-character-select' ? trigger : menu; },
    querySelectorAll(selector) { return selector === '.character-option' ? options : [before, trigger, after]; },
  }, runnerScrollPane: { scrollTop: 100 }, RUNNER_OPEN_SCROLL_TOP: 100,
    positionRunnerCharacterMenu() {}, chooseRunnerCharacter(character) { calls.push('chosen:' + character); } };
  vm.createContext(context);
  vm.runInContext(source.slice(start, end), context);
  return { context, options, menu, calls, focusOptions };
}

test('picker options do not add twenty tab stops', () => {
  const start = source.indexOf('fn runner_picker_options(');
  const end = source.indexOf('\n}', start);
  const markup = source.slice(start, end);
  assert.equal((markup.match(/role="option" tabindex="-1"/g) || []).length, 2);
});

test('Tab and Shift+Tab close the picker and resume from its trigger', () => {
  for (const shiftKey of [false, true]) {
    const { context, options, menu, calls } = setup();
    let prevented = false;
    context.handleRunnerOptionKey({ key:'Tab', shiftKey, currentTarget:options[1], preventDefault() { prevented = true; } });
    assert.equal(menu.hidden, true);
    assert.deepEqual(calls, [shiftKey ? 'before' : 'after']);
    assert.equal(prevented, true, 'focus moves relative to the picker trigger');
  }
});

test('Tab from a mouse-opened trigger closes the picker without stopping native navigation', () => {
  const { context, menu } = setup();
  let prevented = false;
  context.handleRunnerTriggerKey({ key:'Tab', preventDefault() { prevented = true; } });
  assert.equal(menu.hidden, true);
  assert.equal(prevented, false);
});

test('arrow navigation, home/end, escape and selection remain available', () => {
  const { context, options, calls, menu, focusOptions } = setup();
  let prevented = 0;
  const press = (key, currentTarget) => context.handleRunnerOptionKey({ key, currentTarget, preventDefault() { prevented++; } });
  press('ArrowDown', options[0]);
  press('End', options[1]);
  press('Home', options[2]);
  press('Enter', options[2]);
  press('Escape', options[0]);
  assert.deepEqual(calls, ['dog', 'bird', 'cat', 'chosen:bird', 'trigger']);
  assert.ok(focusOptions.every(options => options && options.preventScroll === true));
  assert.equal(prevented, 5);
  assert.equal(menu.hidden, true);
});

test('stale opening scroll is ignored but subsequent pane scroll closes the picker', () => {
  const { context, menu } = setup();
  assert.equal(typeof context.handleRunnerPaneScroll, 'function');
  context.handleRunnerPaneScroll();
  assert.equal(menu.hidden, false);
  context.runnerScrollPane.scrollTop = 110;
  context.handleRunnerPaneScroll();
  assert.equal(menu.hidden, true);
});
