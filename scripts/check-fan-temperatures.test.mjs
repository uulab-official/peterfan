import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';

const source=readFileSync(new URL('../packages/menubar/src/main.rs',import.meta.url),'utf8');
const rail=source.slice(source.indexOf('var RAIL_NAV_READY=false;'),source.indexOf('function setHealthValue('));
const temperatures=source.slice(source.indexOf('function tempValue(value){'),source.indexOf('function updateHealthPanel(d){'));

test('fan view refreshes daemon temperatures and clears expired values',()=>{
  const values={};
  const ctx=vm.createContext({
    LANG:'ko', document:{getElementById:()=>null},railView:()=> 'fan',
    setText:(id,value)=>{values[id]=value;}, updateUpdateChannel(){},
  });
  vm.runInContext(temperatures+'\n'+rail,ctx);
  ctx.RAIL_NAV_READY=true;
  ctx.updateRail({fan_curve_input_temp_c:61,fan_safety_temp_c:63,fan_critical_temp_c:90,fan_temp_source:'daemon',fan_temp_age_secs:1});
  assert.equal(values['fan-curve-input'],'61°C');
  ctx.updateRail({fan_curve_input_temp_c:88,fan_safety_temp_c:96,fan_critical_temp_c:90,fan_temp_source:'daemon',fan_temp_age_secs:2});
  assert.equal(values['fan-curve-input'],'88°C');
  assert.equal(values['fan-safety-hottest'],'96°C');
  assert.match(values['fan-temperature-source'],/데몬 최근 측정/);
  ctx.updateRail({fan_curve_input_temp_c:null,fan_safety_temp_c:null,fan_critical_temp_c:90,fan_temp_source:'daemon',fan_temp_age_secs:null});
  assert.equal(values['fan-curve-input'],'—');
  assert.match(values['fan-temperature-source'],/데몬 온도 확인 불가/);
  ctx.updateRail({fan_curve_input_temp_c:62,fan_safety_temp_c:64,fan_critical_temp_c:90,fan_temp_source:'provider',fan_temp_age_secs:1});
  assert.equal(values['fan-curve-input'],'62°C');
  assert.match(values['fan-temperature-source'],/기기 최근 측정/);
});
