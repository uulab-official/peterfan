import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';

const path = new URL('../packages/menubar/src/cpu_widget.js', import.meta.url);
function ui() {
  const nodes = Object.fromEntries(['cpu-widget-setting','cpu-widget-enabled','cpu-widget-style','cpu-widget-percentage','cpu-widget-status','cpu-widget-preview','cpu-widget-preview-number','cpu-widget-preview-bar'].map(id=>[id,{checked:false,disabled:false,hidden:false,value:'',textContent:'',style:{},dataset:{},setAttribute(k,v){this[k]=v;}}]));
  const messages=[];
  let sequence=0,now=1000;
  const context=vm.createContext({LANG:'en',Date:{now:()=>now},document:{getElementById:id=>nodes[id]||null},window:{ipc:{postMessage:m=>messages.push(m)}},settingsRequest:(operation,payload)=>({id:'cpu-test-'+(++sequence),operation,payload}),settingsResult:(d,r)=>d.settings_results?.find(v=>v.id===r.id&&v.operation===r.operation)||null});
  vm.runInContext(existsSync(path)?readFileSync(path,'utf8'):'',context);
  assert.equal(typeof context.renderCpuWidget,'function','CPU widget settings must be implemented');
  const data=(cfg={},results=[])=>({cpu_widget_supported:true,cpu_widget:{enabled:false,style:'bar',show_percentage:true,...cfg},cpu_pct:42,settings_results:results});
  const request=()=>JSON.parse(messages.at(-1).slice('settingsrequest:'.length));
  return {context,nodes,messages,data,request,advance:n=>{now+=n;}};
}

test('CPU widget defaults off, preserves runner settings, and sends typed native requests',()=>{
  const u=ui();u.context.renderCpuWidget(u.data());
  assert.equal(u.nodes['cpu-widget-enabled'].checked,false);
  assert.equal(u.nodes['cpu-widget-style'].disabled,true);
  u.context.setCpuWidget('enabled',true);
  const r=u.request();assert.equal(r.operation,'cpu_widget');
  assert.deepEqual(JSON.parse(r.payload),{enabled:true,style:'bar',show_percentage:true});
  assert.equal(u.nodes['cpu-widget-enabled'].disabled,true);
  assert.equal(u.messages.length,1);
});

test('CPU widget waits for its own save acknowledgement and restores actual values on failure',()=>{
  const u=ui();u.context.renderCpuWidget(u.data());u.context.setCpuWidget('enabled',true);const r=u.request();
  u.context.renderCpuWidget(u.data({},[{id:'older',operation:'cpu_widget',ok:true}]));
  assert.equal(u.nodes['cpu-widget-enabled'].disabled,true);
  u.context.renderCpuWidget(u.data({},[{...r,ok:false,message:'disk full'}]));
  assert.equal(u.nodes['cpu-widget-enabled'].disabled,false);
  assert.equal(u.nodes['cpu-widget-enabled'].checked,false);
  assert.match(u.nodes['cpu-widget-status'].textContent,/disk full/);
  u.context.setCpuWidget('enabled',true);const retry=u.request();assert.notEqual(retry.id,r.id);
  u.context.renderCpuWidget(u.data({enabled:true},[{...retry,ok:true}]));
  assert.equal(u.nodes['cpu-widget-enabled'].checked,true);
  assert.equal(u.nodes['cpu-widget-style'].disabled,false);
});

test('CPU widget rejects unsupported platforms and invalid edits, and number mode always remains visible',()=>{
  const u=ui();u.context.renderCpuWidget({cpu_widget_supported:false});
  u.context.setCpuWidget('enabled',true);assert.equal(u.messages.length,0);
  u.context.renderCpuWidget(u.data({enabled:true,style:'number',show_percentage:false}));
  assert.equal(u.nodes['cpu-widget-percentage'].disabled,true);
  assert.match(u.nodes['cpu-widget-preview-number'].textContent,/CPU.*42%/);
  u.context.setCpuWidget('style','arbitrary');u.context.setCpuWidget('unrelated',true);assert.equal(u.messages.length,0);
  u.context.renderCpuWidget({...u.data({enabled:true}),cpu_pct:null});
  assert.match(u.nodes['cpu-widget-preview-number'].textContent,/--%/);
});

test('CPU widget timeout enables retry while stale results cannot acknowledge a newer request',()=>{
  const u=ui();u.context.renderCpuWidget(u.data());u.context.setCpuWidget('enabled',true);const first=u.request();
  u.advance(16000);u.context.renderCpuWidget(u.data());assert.equal(u.nodes['cpu-widget-enabled'].disabled,false);
  u.context.setCpuWidget('enabled',true);const second=u.request();
  u.context.renderCpuWidget(u.data({},[{...first,ok:true}]));assert.equal(u.nodes['cpu-widget-enabled'].disabled,true);
  u.context.renderCpuWidget(u.data({enabled:true},[{...second,ok:true}]));assert.equal(u.nodes['cpu-widget-enabled'].disabled,false);
});

test('CPU history preview keeps a fixed percentage axis and gaps within 60 samples',()=>{
  const u=ui();
  assert.equal(u.context.cpuWidgetHistoryPath([0,50,100]),'M59 33V32M60 33V17M61 33V3');
  assert.equal(u.context.cpuWidgetHistoryPath([null,NaN,Infinity,-1,101]),'');
  const many=Array(1000).fill(50);
  assert.equal((u.context.cpuWidgetHistoryPath(many).match(/M/g)||[]).length,60);
});

test('CPU widget reload and external menu changes use the native confirmed configuration',()=>{
  const u=ui();u.context.renderCpuWidget(u.data({enabled:true,style:'history',show_percentage:false}));
  assert.equal(u.nodes['cpu-widget-style'].value,'history');
  assert.equal(u.nodes['cpu-widget-percentage'].checked,false);
  assert.equal(u.nodes['cpu-widget-preview-number'].hidden,true);
  u.context.renderCpuWidget(u.data({enabled:false,style:'number'}));
  assert.equal(u.nodes['cpu-widget-enabled'].checked,false);
  assert.equal(u.nodes['cpu-widget-style'].disabled,true);
  assert.match(u.nodes['cpu-widget-preview-number'].textContent,/CPU/);
});
