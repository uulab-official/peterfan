import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';

function ui() {
  const ids = ['menu-items-setting','menu-items-title','menu-items-copy','menu-items-cpu','menu-items-runner','menu-items-metric','menu-items-cpu-label','menu-items-runner-label','menu-items-metric-label','menu-items-help','menu-items-status','menubar-display-setting','cpu-widget-setting','cpu-widget-enabled','cpu-widget-style','cpu-widget-percentage','cpu-widget-status'];
  const nodes=Object.fromEntries(ids.map(id=>[id,{hidden:false,checked:false,disabled:false,value:'',textContent:'',style:{},dataset:{},setAttribute(k,v){this[k]=v;}}]));
  const messages=[];let now=1000,sequence=0;
  const context=vm.createContext({LANG:'en',Date:{now:()=>now},document:{getElementById:id=>nodes[id]||null},window:{ipc:{postMessage:m=>messages.push(m)}},settingsRequest:(operation,payload)=>({id:'test-'+(++sequence),operation,payload}),settingsResult:(d,r)=>d.settings_results?.find(v=>v.id===r.id&&v.operation===r.operation)||null});
  for (const name of ['cpu_widget.js','menu_items.js']) {
    const p=new URL('../packages/menubar/src/'+name,import.meta.url);
    vm.runInContext(existsSync(p)?readFileSync(p,'utf8'):'',context);
  }
  assert.equal(typeof context.renderMenuItems,'function','shared menu items controls must be implemented');
  const data=(visibility={cpu:true,runner:true,metric:true},results=[],cpu={})=>({menu_items_supported:true,menu_items:visibility,cpu_widget_supported:true,cpu_widget:{enabled:visibility.cpu,style:'history',show_percentage:false,...cpu},settings_results:results});
  const render=d=>{context.renderMenuItems(d);context.renderCpuWidget(d);};
  const request=()=>JSON.parse(messages.at(-1).slice('settingsrequest:'.length));
  return {context,nodes,messages,data,render,request,advance:n=>{now+=n;}};
}

test('all seven native states render independent checkboxes and guard the last active item',()=>{
  const u=ui();
  for(const [cpu,runner,metric] of [[true,false,false],[false,true,false],[false,false,true],[true,true,false],[true,false,true],[false,true,true],[true,true,true]]){
    const v={cpu,runner,metric};u.render(u.data(v));const count=Object.values(v).filter(Boolean).length;
    for(const key of ['cpu','runner','metric']){
      const node=u.nodes['menu-items-'+key];assert.equal(node.checked,v[key]);assert.equal(node.disabled,count===1&&v[key]);
    }
    if(count===1){const before=u.messages.length;u.context.setMenuItem(Object.keys(v).find(k=>v[k]),false);assert.equal(u.messages.length,before);assert.match(u.nodes['menu-items-help'].textContent,/at least one/i);}
  }
});

test('visibility is macOS gated and retains the Windows display controls',()=>{
  const u=ui();u.render({...u.data(),menu_items_supported:false,cpu_widget_supported:false});
  assert.equal(u.nodes['menu-items-setting'].hidden,true);assert.equal(u.nodes['menubar-display-setting'].hidden,false);
  u.context.setMenuItem('cpu',false);assert.equal(u.messages.length,0);
  u.render(u.data());assert.equal(u.nodes['menubar-display-setting'].hidden,true);
});

test('complete visibility request waits for exact operation and ID before confirming',()=>{
  const u=ui();u.render(u.data());u.context.setMenuItem('runner',false);const r=u.request();
  assert.equal(r.operation,'menu_items');assert.deepEqual(JSON.parse(r.payload),{cpu:true,runner:false,metric:true});
  for(const result of [{...r,id:'older',ok:true},{...r,operation:'cpu_widget',ok:true}]){u.render(u.data(undefined,[result]));assert.equal(u.nodes['menu-items-cpu'].disabled,true);}
  u.render(u.data({cpu:true,runner:false,metric:true},[{...r,ok:true}]));
  assert.equal(u.nodes['menu-items-runner'].checked,false);assert.equal(u.nodes['menu-items-cpu'].disabled,false);assert.match(u.nodes['menu-items-status'].textContent,/saved/i);
});

test('save failure restores confirmed items and supports explicit retry without stale acknowledgement',()=>{
  const u=ui();u.render(u.data());u.context.setMenuItem('metric',false);const first=u.request();
  u.render(u.data(undefined,[{...first,ok:false,message:'disk full'}]));
  assert.equal(u.nodes['menu-items-metric'].checked,true);assert.match(u.nodes['menu-items-status'].textContent,/disk full/);
  u.context.setMenuItem('metric',false);const second=u.request();assert.notEqual(first.id,second.id);
  u.render(u.data(undefined,[{...first,ok:true}]));assert.equal(u.nodes['menu-items-runner'].disabled,true);
  u.render(u.data({cpu:true,runner:true,metric:false},[{...second,ok:true}]));assert.equal(u.nodes['menu-items-metric'].checked,false);
});

test('timeout permits manual retry and older success cannot settle the retry',()=>{
  const u=ui();u.render(u.data());u.context.setMenuItem('runner',false);const first=u.request();
  u.advance(16000);u.render(u.data());assert.match(u.nodes['menu-items-status'].textContent,/timed out/i);assert.equal(u.messages.length,1);
  u.context.setMenuItem('runner',false);const second=u.request();u.render(u.data(undefined,[{...first,ok:true}]));assert.equal(u.nodes['menu-items-cpu'].disabled,true);
  u.render(u.data({cpu:true,runner:false,metric:true},[{...second,ok:true}]));assert.equal(u.nodes['menu-items-cpu'].disabled,false);
});

test('CPU style and visibility edits serialize until their own acknowledgement',()=>{
  const u=ui();u.render(u.data());u.context.setCpuWidget('style','cores');const style=u.request();
  u.context.setMenuItem('cpu',false);assert.equal(u.messages.length,1);assert.equal(u.nodes['menu-items-cpu'].disabled,true);
  u.render(u.data(undefined,[{...style,ok:true}],{style:'cores',show_percentage:false}));
  u.context.setMenuItem('runner',false);const visibility=u.request();u.context.setCpuWidget('show_percentage',true);assert.equal(u.messages.length,2);assert.equal(u.nodes['cpu-widget-style'].disabled,true);
  u.render(u.data({cpu:true,runner:false,metric:true},[{...visibility,ok:true}],{style:'cores',show_percentage:false}));
  assert.equal(u.nodes['cpu-widget-style'].value,'cores');assert.equal(u.nodes['cpu-widget-percentage'].checked,false);
});

test('reload preserves hidden appearance and metric settings, rejects malformed edits and localizes guard',()=>{
  const u=ui();u.context.LANG='ko';u.render(u.data({cpu:false,runner:false,metric:true},[],{style:'cores',show_percentage:true}));
  assert.equal(u.nodes['cpu-widget-style'].value,'cores');assert.equal(u.nodes['cpu-widget-percentage'].checked,true);assert.equal(u.nodes['cpu-widget-style'].disabled,true);
  assert.match(u.nodes['menu-items-help'].textContent,/하나/);assert.match(u.nodes['menu-items-title'].textContent,/메뉴/);
  u.context.setMenuItem('fan',true);u.context.setMenuItem('cpu','yes');assert.equal(u.messages.length,0);
  u.render(u.data({cpu:true,runner:false,metric:true},[],{style:'cores',show_percentage:true}));assert.equal(u.nodes['cpu-widget-style'].disabled,false);assert.equal(u.nodes['cpu-widget-percentage'].checked,true);
});
