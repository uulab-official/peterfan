import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';

const source=readFileSync(new URL('../packages/menubar/src/main.rs',import.meta.url),'utf8');
const begin=source.indexOf('function renderFanCards(fans,enabled){');
const end=source.indexOf('function markFanPending(',begin);
assert.ok(begin>=0&&end>begin);

function render(lang,fan){
  const element=()=>({dataset:{},style:{},classList:{toggle(){}},setAttribute(){},textContent:''});
  const names=['.fn','.fv','input[type=range]','.fa-num','.fan-bar i','.fan-target-marker','.fan-rpm-text','.fa-auto','.fa-manual','.fa-min','.fa-max','.fan-rpm-row'];
  const nodes=Object.fromEntries(names.map(name=>[name,element()]));
  const card=element();card.getAttribute=()=>fan.id;card.querySelector=name=>nodes[name]||null;
  const container={children:[card],querySelector:()=>card};
  const document={activeElement:null,getElementById:id=>id==='fan-cards'?container:null};
  const context=vm.createContext({LANG:lang,document,Date,Math,Number,Array,parseInt,parseFloat,isFinite});
  vm.runInContext(source.slice(begin,end),context);
  context.renderFanCards([fan],true);
  return nodes['.fv'].textContent;
}

const base={id:'fan.0',l:'Fan 1',controllable:true,manual:true,min_rpm:2317,max_rpm:7826,cur_rpm:7826,pct:100,override_pct:8,target_pct:100,target_rpm:7826,readback_status:'settled'};

test('manual request differs from the reported target without claiming it was applied',()=>{
  const label=render('ko',base);
  assert.match(label,/요청 8%/);
  assert.match(label,/현재 목표 100%/);
  assert.match(label,/요청과 다름/);
  assert.doesNotMatch(label,/적용됨|임계|보호/);
  const en=render('en',base);
  assert.match(en,/requested 8%/i);
  assert.match(en,/current target 100%/i);
  assert.doesNotMatch(en,/applied|critical/i);
});

test('stale and adjusting state remain visible when request differs',()=>{
  assert.match(render('ko',{...base,readback_status:'stale'}),/응답 없음/);
  assert.match(render('ko',{...base,readback_status:'adjusting'}),/조정 중/);
});

test('matching override uses readback state and absent readback is not confirmed',()=>{
  const matching={...base,override_pct:8,target_pct:8,target_rpm:2758,cur_rpm:2758};
  assert.match(render('ko',matching),/적용됨/);
  assert.match(render('ko',{...matching,readback_status:'adjusting'}),/조정 중/);
  assert.match(render('ko',{...matching,readback_status:'stale'}),/응답 없음/);
  assert.doesNotMatch(render('ko',{...matching,readback_status:''}),/적용됨/);
});

test('global controls do not pretend to have a manual request',()=>{
  assert.doesNotMatch(render('ko',{...base,manual:false,override_pct:null}),/요청 8%/);
});
