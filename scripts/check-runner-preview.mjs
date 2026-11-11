#!/usr/bin/env node
// Execute the shipped WebView runner code with a deterministic browser clock.
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
const webOnly = process.argv[2] === '--web-only';
if (process.argv.length > (webOnly ? 3 : 2)) throw Error('Usage: node scripts/check-runner-preview.mjs [--web-only]');
import vm from 'node:vm';
const source = readFileSync(new URL('../packages/menubar/src/main.rs', import.meta.url), 'utf8');
const start = source.indexOf('var RUNNER_CHARACTERS=');
const end = source.indexOf('function positionRunnerCharacterMenu()', start);
const code = source.slice(start, end).replace(/__[A-Z_]+__/g, (token) => token === '__RUNNER_SELECTED_PREVIEW_CHARACTER__' ? 'cat' : token === '__RUNNER_CHARACTERS__' ? '["cat"]' : token.includes('ALPHA') || token === '__RUNNER_PREVIEWS__' ? '{}' : '[]');
let now = 0, nextId = 0, allocations = 0, paints = 0, reduced = false, view = 'settings';
const timers = new Map(), listeners = {};
const context = { console, Map, WeakMap, Uint8ClampedArray, atob,
  performance: { now: () => now },
  document: { hidden: false, addEventListener: (event, callback) => { listeners[event] = callback; }, querySelectorAll: () => [], getElementById: () => null },
  window: { matchMedia: () => ({ matches: reduced, addEventListener: (event, callback) => { listeners[`media-${event}`] = callback; } }) },
  railView: () => view,
  setTimeout(callback, delay) { const id = ++nextId; timers.set(id, { at: now + delay, callback }); return id; },
  clearTimeout(id) { timers.delete(id); },
};
vm.createContext(context);
vm.runInContext(code, context);
const alpha = 'ff'.repeat(48 * 32);
context.RUNNER_SELECTED_PREVIEW = Array(32).fill(alpha);
context.RUNNER_SELECTED_PREVIEW_CHARACTER = 'cat';
const canvas = { id: 'runner-character-preview', width: 48, height: 32,
  getContext: () => ({ createImageData(w,h) { allocations++; return { data: new Uint8ClampedArray(w*h*4) }; },
    clearRect() {}, putImageData() { paints++; }, fillRect() {} }) };
const colorFrames = Array.from({length: 8}, (_, i) => 'rgba:' + [i+1, 20, 30, 255].map(x => x.toString(16).padStart(2, '0')).join('').repeat(48*32));
assert.equal(context.runnerSelectedPreviewMatchesCharacter({runner_selected_preview:colorFrames,runner_selected_preview_character:'cat'},'cat'),true,'compact color cycles must be accepted');
assert.equal(context.runnerSelectedPreviewMatchesCharacter({runner_selected_preview:colorFrames.slice(0,7),runner_selected_preview_character:'cat'},'cat'),false,'incomplete compact cycles must be rejected');
let paintedFirstPixel;
const colorCanvas = { id:'runner-character-preview', width:48, height:32, getContext:()=>({createImageData(w,h){return {data:new Uint8ClampedArray(w*h*4)};}, clearRect(){}, putImageData(img){paintedFirstPixel=img.data[0];}}) };
const packed = Buffer.from([0x80,77,33,11,255,...Array(11).fill(127),126]);
context.RUNNER_PREVIEW_FRAMES={cat:['rgba-rle:'+packed.toString('base64')]};
const pickerCanvas={...colorCanvas,id:'picker-preview'};
context.paintRunnerPreview(pickerCanvas,'cat',0,0);
assert.equal(paintedFirstPixel,77,'packed picker frames must decode literal and transparent runs');
paintedFirstPixel=undefined;
context.RUNNER_PREVIEW_FRAMES={cat:['rgba-rle:'+Buffer.from([0x80,77]).toString('base64')]};
context.paintRunnerPreview(pickerCanvas,'cat',0,0);
assert.equal(paintedFirstPixel,undefined,'truncated packed frames must not paint');
context.RUNNER_SELECTED_PREVIEW=colorFrames;
context.paintRunnerPreview(colorCanvas,'cat',3,3);
assert.equal(paintedFirstPixel,4,'compact cycle must select pose in every CPU band');
assert.equal(context.runnerSelectedPreviewMatchesCurrent({runner_selected_preview:colorFrames,runner_selected_preview_character:'cat'},'cat'),true);
const legacyFrames=Array.from({length:32},(_,i)=>'rgba:'+ [i+1,20,30,255].map(x=>x.toString(16).padStart(2,'0')).join('').repeat(48*32));
assert.equal(context.runnerSelectedPreviewMatchesCharacter({runner_selected_preview:legacyFrames,runner_selected_preview_character:'cat'},'cat'),true,'legacy cycles must still be accepted');
assert.equal(context.runnerSelectedPreviewMatchesCurrent({runner_selected_preview:legacyFrames,runner_selected_preview_character:'cat'},'cat'),false,'different pack lengths must trigger refresh');
context.RUNNER_SELECTED_PREVIEW=legacyFrames;
context.paintRunnerPreview(colorCanvas,'cat',3,3);
assert.equal(paintedFirstPixel,28,'32-frame cycle must preserve CPU band selection');
context.RUNNER_SELECTED_PREVIEW=Array(32).fill(alpha);
function advance(ms) {
  const until = now + ms;
  for (let turns = 0; turns < 1000; turns++) {
    const next = [...timers].sort((a,b) => a[1].at-b[1].at)[0];
    if (!next || next[1].at > until) { now = until; return; }
    now = next[1].at; timers.delete(next[0]); next[1].callback();
  }
  throw new Error('preview timer runaway');
}
context.startRunnerPreview(canvas, 'cat', 5);
advance(1000);
assert.equal(allocations, 1, 'repeated alpha frames must reuse decoded image data');
assert.ok(context.runnerPreviewInterval(95,0) <= 35, 'busy preview must visibly sprint');
const poseBefore = context.RUNNER_PREVIEW_STATE.frame;
context.setRunnerPreviewCpu(95);
advance(240);
assert.ok(context.RUNNER_PREVIEW_STATE.motionCpu > 90, 'preview acceleration lags the CPU spike');
assert.notEqual(context.RUNNER_PREVIEW_STATE.frame, poseBefore, 'spike must advance the pose');
context.setRunnerPreviewVisibility(false);
const count = paints;
advance(1000);
assert.equal(paints, count, 'closed popover must not paint');
assert.equal(timers.size, 0, 'closed popover must not wake');
context.setRunnerPreviewVisibility(true);
context.setRunnerPreviewVisibility(true);
assert.equal(timers.size, 1, 'repeated show must keep one timer');
view = 'overview';
advance(1000);
assert.equal(timers.size, 0, 'non-settings view must stop preview work');
view = 'settings';
context.setRunnerPreviewVisibility(true);
reduced = true;
listeners['media-change']?.();
advance(1000);
assert.equal(timers.size, 0, 'Reduce Motion must stop preview timers');
reduced = false;
listeners['media-change']?.();
assert.equal(timers.size, 1, 'disabling Reduce Motion must resume one timer');
context.document.hidden = true;
listeners.visibilitychange();
assert.equal(timers.size, 0, 'hidden document must stop preview timers');
context.document.hidden = false;
listeners.visibilitychange();
assert.equal(timers.size, 1, 'visible document must resume one timer');
assert.equal(context.runnerPreviewInterval(95,0,'pushup'),21,'exercise cadence must remain readable at full load');
context.RUNNER_SELECTED_PREVIEW_CHARACTER='pushup';
context.startRunnerPreview(canvas,'pushup',95);
advance(500);
assert.equal(context.RUNNER_PREVIEW_STATE.frame,7,'exercise preview must use its character cadence');
if (!webOnly) {
 const report = new URL('../target/runner-motion-cadence.json',import.meta.url);
 if (!existsSync(report)) throw Error('Missing native cadence evidence. Run scripts/check-runner-quality.sh first, or use --web-only for frontend-only checks.');
 const cadence=JSON.parse(readFileSync(report,'utf8'));
 assert.equal(Object.keys(cadence).length,20,'native cadence evidence must cover the full catalog');
 for(let cpu=0;cpu<=100;cpu++)for(const character of Object.keys(cadence))for(let pose=0;pose<8;pose++){
  assert.equal(context.runnerPreviewInterval(cpu,pose,character),context.runnerPreviewInterval(cpu,0,'cat'),`character-specific speed cap returned: ${character}`);
 }
 for(const [character,loads] of Object.entries(cadence))for(let cpu=0;cpu<=100;cpu++)for(let pose=0;pose<8;pose++){
  assert.ok(Math.abs(context.runnerPreviewInterval(cpu,pose,character)-loads[cpu][pose])<=1,`native/preview cadence differs: ${character} ${cpu}% pose ${pose}`);
 }
} else {
 console.log('Frontend-only mode: native cadence comparison was not run.');
}
for(const sample of [NaN,Infinity,-Infinity])assert.equal(context.runnerPreviewInterval(sample,0,'slime'),180);
console.log('Runner preview: cache, cadence, CPU response, panel visibility, Reduce Motion and timer lifecycle passed.');
