#!/usr/bin/env node
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
const source=readFileSync(new URL('../packages/menubar/src/main.rs',import.meta.url),'utf8');
function definition(name){
 const start=source.indexOf('function '+name+'(');
 assert(start>=0,'Missing dashboard function '+name);
 const end=source.indexOf('\nfunction ',start+1);
 return source.slice(start,end<0?undefined:end);
}
const title={textContent:''},copy={textContent:''};
const empty={style:{},setAttribute:()=>{},querySelector:s=>s==='.empty-state-title'?title:copy};
const context={LANG:'ko',document:{getElementById:()=>empty}};
vm.createContext(context);
vm.runInContext(definition('compareVersions')+'\n'+definition('updateFanEmptyState'),context);
let failed=0,passed=0;
function test(name,fn){try{fn();passed++;console.log('PASS '+name);}catch(error){failed++;console.error('FAIL '+name+': '+error.message);}}
test('prerelease upgrades preserve patch and stable precedence',()=>{
 for(const [older,newer] of [['1.32.2','1.32.3-beta.1'],['1.32.3-beta.1','1.32.3'],['1.0.0-beta.2','1.0.0-beta.10'],['1.0.0-alpha.1','1.0.0-alpha.beta'],['1.0.0-99999999999999999999','1.0.0-100000000000000000000'],['v1.2','1.2.1']]){
  assert.equal(context.compareVersions(older,newer),-1,older+' -> '+newer);
  assert.equal(context.compareVersions(newer,older),1,newer+' -> '+older);
 }
 assert.equal(context.compareVersions('1.2.3+build.1','1.2.3+build.2'),0);
});
test('invalid release versions never produce an upgrade decision',()=>{
 for(const invalid of ['', 'garbage','1.02.3','1.2.3.4','1.2.3-beta.01',' 1.2.3','1.2.3+','18446744073709551616.0.0']){
  assert.equal(context.compareVersions('1.2.3',invalid),null,invalid);
  assert.equal(context.compareVersions(invalid,'1.2.3'),null,invalid);
 }
});
test('fan detection does not announce missing hardware before completion',()=>{
 context.updateFanEmptyState({fan_control_state_ready:false,fans:[]});
 assert(!title.textContent.includes('없습니다'));
 assert(title.textContent.includes('확인'));
});
test('completed empty and read-only fan states remain distinct',()=>{
 context.updateFanEmptyState({fan_control_state_ready:true,fans:[]});
 assert.equal(title.textContent,'팬 센서가 없습니다');
 context.updateFanEmptyState({fan_control_state_ready:true,fans:[{controllable:false}]});
 assert.equal(title.textContent,'읽기 전용 팬');
 context.updateFanEmptyState({fan_control_state_ready:true,fans:[{controllable:true}]});
 assert.equal(empty.style.display,'none');
});
test('unchanged live status text does not rewrite or reannounce the node',()=>{
 let value='최신 상태',writes=0;
 const node={get textContent(){return value;},set textContent(next){value=next;writes++;}};
 const scope={document:{getElementById:()=>node}};
 vm.createContext(scope);vm.runInContext(definition('setText'),scope);
 scope.setText('status','최신 상태');assert.equal(writes,0);
 scope.setText('status','업데이트 가능');assert.equal(writes,1);assert.equal(value,'업데이트 가능');
});
test('process snapshots render loading, empty and populated states without timer errors',()=>{
 const makeNode=()=>({style:{},classList:{contains:()=>false,toggle:()=>{}},setAttribute:()=>{},textContent:''});
 const nodes={'process-search':{value:''},'process-action-status':makeNode(),'procs-empty':makeNode(),'procs-list':makeNode()};
 const list=nodes['procs-list'];
 let children=[];
 Object.defineProperty(list,'innerHTML',{set:()=>{children=[];}});
 list.appendChild=row=>children.push(row);
 const scope={LANG:'en',PROCESS_ACTION_REVISION:0,PROCESS_CONFIRM:{pid:null,startTime:null,until:0},document:{
  getElementById:id=>nodes[id]||null,
  createElement:()=>Object.assign(makeNode(),{children:Array.from({length:5},makeNode)})
 }};
 vm.createContext(scope);
 vm.runInContext(['setVisible','processActionText','resetProcessConfirmation','renderProcesses'].map(definition).join('\n'),scope);
 scope.renderProcesses({slow_data_ready:false,procs:[]});
 assert.equal(nodes['procs-empty'].style.display,'none');
 scope.renderProcesses({slow_data_ready:true,procs:[]});
 assert.equal(nodes['procs-empty'].style.display,'');
 assert.equal(children.length,0);
 const snapshot={slow_data_ready:true,process_filter:'Editor',procs:[{name:'Editor',pid:123,start_time:456,cpu:'2.0%',mem:'40 MB',can_quit:true}]};
 scope.renderProcesses(snapshot);
 scope.renderProcesses(snapshot);
 assert.equal(nodes['procs-empty'].style.display,'none');
 assert.equal(nodes['process-search'].value,'Editor');
 assert.equal(children.length,1);
 assert.equal(children[0].children[0].textContent,'Editor');
 assert.equal(children[0].children[1].textContent,'PID 123');
 assert.equal(children[0].children[4].disabled,false);
});
function setupProgressHarness(){
 const nodes=Object.fromEntries(['setup-progress','setup-progress-label','setup-progress-percent','setup-progress-track','setup-progress-fill'].map(id=>[id,{style:{},attrs:{},textContent:'',classList:{toggle(){}},setAttribute(key,value){this.attrs[key]=value;}}]));
 let frames=new Map(),next=0,time=0;
 const scope={LANG:'en',view:'fan',reduced:false,SETUP_PROGRESS:{phase:'idle',value:0,target:0,frame:null,started:null,from:0},
  document:{hidden:false,getElementById:id=>nodes[id]},window:{matchMedia:()=>({matches:scope.reduced})},
  railView:()=>scope.view,requestAnimationFrame:fn=>{frames.set(++next,fn);return next;},cancelAnimationFrame:id=>frames.delete(id)};
 vm.createContext(scope);
 vm.runInContext(['setText','cancelSetupProgressFrame','setupProgressCanAnimate','paintSetupProgress','tickSetupProgress','refreshSetupProgressMotion','resetSetupProgress','renderSetupProgress'].map(definition).join('\n'),scope);
 return {scope,nodes,pending:()=>frames.size,step(ms=75){time+=ms;const batch=[...frames.values()];frames.clear();batch.forEach(fn=>fn(time));},finish(){for(let i=0;i<12&&frames.size;i++)this.step();}};
}
test('Set Up first opens settings without starting installation, then explicit activation installs',()=>{
 const messages=[];
 const scope={view:'fan',railView:()=>scope.view,setRailView:view=>{scope.view=view;messages.push('view:'+view);},
  startFanControlSetup:()=>messages.push('install'),updateSetup:()=>{},window:{__pf_pending:{}},
  closeSetupMenu:()=>{},requestAnimationFrame:fn=>fn(),document:{getElementById:()=>({focus:()=>{}})}};
 vm.createContext(scope);
 const handler=source.match(/id="setup-fan"[^>]*onclick="([^"]+)"/)[1];
 if(source.includes('function handleFanSetupAction('))vm.runInContext(definition('handleFanSetupAction'),scope);
 vm.runInContext(handler.replace(/this/g,'null'),scope);
 assert.equal(scope.view,'settings');assert(!messages.includes('install'));
 vm.runInContext(handler.replace(/this/g,'null'),scope);
 assert.equal(messages.filter(x=>x==='install').length,1);
});
test('fan setup and install progress remain visible on the settings destination',()=>{
 const node=()=>({style:{},classList:{contains:()=>false,toggle(){}},setAttribute(){},textContent:''});
 const nodes=Object.fromEntries(['setup-row','setup-fan'].map(id=>[id,node()]));
 const scope={LANG:'ko',FAN_CONTROL_FIX_PENDING:false,APP_UPDATE_CHECK_PENDING:false,LOGIN_ITEM_TOGGLE_PENDING:false,
  view:'settings',railView:()=>scope.view,renderSetupProgress:()=>{},document:{getElementById:id=>nodes[id]||null}};
 vm.createContext(scope);vm.runInContext(definition('updateSetup'),scope);
 const data={fan_control_state_ready:true,fan_control_supported:true,fan_count:2,controllable_fan_count:2,fan_setup_needed:true};
 scope.updateSetup(data);
 assert.equal(nodes['setup-row'].style.display,'flex');assert.equal(nodes['setup-fan'].disabled,false);
 assert.equal(nodes['setup-fan'].textContent,'팬 제어 활성화');
 scope.view='fan';scope.updateSetup(data);assert.equal(nodes['setup-fan'].textContent,'설정하기');
 scope.view='overview';scope.updateSetup(data);assert.equal(nodes['setup-row'].style.display,'none');
 scope.view='settings';scope.FAN_CONTROL_FIX_PENDING=true;scope.updateSetup(data);
 assert.equal(nodes['setup-fan'].disabled,true);assert.equal(nodes['setup-fan'].textContent,'설치 중…');
});
test('setup progress initializes before the initial navigation render',()=>{
 assert(source.indexOf("var SETUP_PROGRESS={")<source.indexOf('setRailView(railView());'));
});
test('installation stage progress interpolates monotonically toward actual milestones only',()=>{
 const ui=setupProgressHarness();
 ui.scope.renderSetupProgress({phase:'preflight',percent:10});ui.finish();
 assert.equal(ui.scope.SETUP_PROGRESS.value,10);assert.equal(ui.pending(),0);
 ui.scope.renderSetupProgress({phase:'installing',percent:30});
 let previous=10,sawIntermediate=false;
 for(let i=0;i<10;i++){ui.step();const value=ui.scope.SETUP_PROGRESS.value;assert(value>=previous&&value<=30);if(value>10&&value<30)sawIntermediate=true;previous=value;}
 assert(sawIntermediate);assert.equal(previous,30);assert.equal(ui.pending(),0);
 assert.equal(ui.nodes['setup-progress-percent'].textContent,'30%');
 assert.match(ui.nodes['setup-progress-track'].attrs['aria-valuetext'],/Installation stages 30%/);
});
test('installation failures stop animation, retries reset, and only healthy completion reaches100',()=>{
 const ui=setupProgressHarness();
 ui.scope.renderSetupProgress({phase:'verifying',percent:85});ui.step();ui.step();
 ui.scope.renderSetupProgress({phase:'failed',percent:85});assert.equal(ui.pending(),0);assert.equal(ui.scope.SETUP_PROGRESS.value,85);
 ui.scope.resetSetupProgress();ui.scope.renderSetupProgress({phase:'preflight',percent:10});ui.finish();assert.equal(ui.scope.SETUP_PROGRESS.value,10);
 ui.scope.renderSetupProgress({phase:'installing',percent:100});ui.finish();assert.equal(ui.scope.SETUP_PROGRESS.value,99);
 ui.scope.renderSetupProgress({phase:'complete',percent:100});ui.finish();assert.equal(ui.scope.SETUP_PROGRESS.value,100);assert.equal(ui.pending(),0);
 ui.scope.renderSetupProgress({phase:'idle',percent:0});assert.equal(ui.scope.SETUP_PROGRESS.value,0);assert.equal(ui.nodes['setup-progress'].style.display,'none');
});
test('hidden panels and Reduce Motion cancel frames and show the latest reported stage',()=>{
 const ui=setupProgressHarness();ui.scope.renderSetupProgress({phase:'installing',percent:30});assert.equal(ui.pending(),1);
 ui.scope.document.hidden=true;ui.scope.refreshSetupProgressMotion();assert.equal(ui.pending(),0);assert.equal(ui.scope.SETUP_PROGRESS.value,30);
 ui.scope.renderSetupProgress({phase:'verifying',percent:85});assert.equal(ui.pending(),0);
 ui.scope.document.hidden=false;ui.scope.refreshSetupProgressMotion();assert.equal(ui.scope.SETUP_PROGRESS.value,85);assert.equal(ui.pending(),0);
 ui.scope.resetSetupProgress();ui.scope.reduced=true;ui.scope.renderSetupProgress({phase:'preflight',percent:10});assert.equal(ui.pending(),0);assert.equal(ui.scope.SETUP_PROGRESS.value,10);
 ui.scope.reduced=false;ui.scope.renderSetupProgress({phase:'installing',percent:30});assert.equal(ui.pending(),1);
 ui.scope.view='settings';ui.scope.refreshSetupProgressMotion();assert.equal(ui.pending(),1);ui.finish();assert.equal(ui.scope.SETUP_PROGRESS.value,30);
 ui.scope.renderSetupProgress({phase:'verifying',percent:85});assert.equal(ui.pending(),1);
 ui.scope.view='system';ui.scope.refreshSetupProgressMotion();assert.equal(ui.pending(),0);
 ui.scope.renderSetupProgress({phase:'installing',percent:NaN});assert.equal(ui.scope.SETUP_PROGRESS.value,0);assert.equal(ui.pending(),0);
});

function batteryHarness(lang='en'){
 const node=()=>({style:{},dataset:{},textContent:'',className:'',classList:{contains:()=>false,toggle(){}},setAttribute(){}});
 const nodes=Object.fromEntries(['sec-batt','batt-val','batt-sub','batt-bar','batt-details','batt-health','batt-design','batt-full','batt-cycles','batt-source-note'].map(id=>[id,node()]));
 for(const value of Object.values(nodes))Object.defineProperty(value,'innerHTML',{set(){throw Error('Battery data must use textContent');}});
 const scope={LANG:lang,document:{getElementById:id=>nodes[id]||null}};
 vm.createContext(scope);
 vm.runInContext(['setText','setVisible','renderBattery'].map(definition).join('\n'),scope);
 return {scope,nodes,render(data){scope.renderBattery(data);}};
}
function batterySnapshot(details={}){
 return {batt_present:true,batt_pct:76,batt_text:'76%',batt_sub:'discharging',batt_details:{
  charge_percent:76,state:'discharging',health_percent:96,health_source:'reported',
  design_capacity_mah:6249,full_charge_capacity_mah:5655,cycle_count:0,...details}};
}
test('battery reported health96 remains distinct from current charge76',()=>{
 const ui=batteryHarness();ui.render(batterySnapshot());
 assert.equal(ui.nodes['batt-val'].textContent,'76%');
 assert.equal(ui.nodes['batt-health'].textContent,'96%');
 assert.equal(ui.nodes['batt-design'].textContent,'6,249 mAh');
 assert.equal(ui.nodes['batt-full'].textContent,'5,655 mAh');
 assert.equal(ui.nodes['batt-cycles'].textContent,'0');
 assert.equal(ui.nodes['batt-details'].style.display,'');
});
test('battery capacity-derived health is explicitly estimated in both languages',()=>{
 for(const [lang,label] of [['en','91% (estimated)'],['ko','91% (추정)']]){
  const ui=batteryHarness(lang);ui.render(batterySnapshot({health_percent:90.5,health_source:'estimated'}));
  assert.equal(ui.nodes['batt-health'].textContent,label);
 }
});
for(const [lang,estimated,unavailable] of [['en','estimated','Unavailable'],['ko','추정','확인 불가']]){
 for(const [source,health,want] of [
  ['estimated',105,'105% ('+estimated+')'],
  ['estimated',120,'120% ('+estimated+')'],
  ['estimated',121,unavailable],
  ['reported',105,unavailable]
 ]){
  test('battery '+lang+' '+source+' health '+health+' respects its source range',()=>{
   const ui=batteryHarness(lang);
   ui.render(batterySnapshot({health_percent:health,health_source:source,design_capacity_mah:null,full_charge_capacity_mah:null}));
   assert.equal(ui.nodes['batt-health'].textContent,want);
   assert.equal(ui.nodes['batt-val'].textContent,'76%');
   assert.equal(ui.nodes['batt-bar'].style.width,'76%');
   assert.equal(ui.nodes['batt-source-note'].style.display,'none');
  });
 }
 test('battery '+lang+' estimated health does not widen the charge range',()=>{
  const ui=batteryHarness(lang);
  for(const charge of [105,120,121]){
   ui.render({...batterySnapshot({health_percent:120,health_source:'estimated'}),batt_pct:charge});
   assert.equal(ui.nodes['batt-val'].textContent,unavailable);
   assert.equal(ui.nodes['batt-bar'].style.width,'0%');
   assert.equal(ui.nodes['batt-health'].textContent,'120% ('+estimated+')');
  }
  ui.render({...batterySnapshot({health_percent:120,health_source:'estimated'}),batt_pct:0});
  assert.equal(ui.nodes['batt-val'].textContent,'0%');
  assert.equal(ui.nodes['batt-cycles'].textContent,'0');
 });
}
test('missing or invalid battery details stay unavailable per field',()=>{
 const ui=batteryHarness();
 for(const invalid of [null,undefined,0,NaN,Infinity,'96',true,-1]){
  ui.render(batterySnapshot({health_percent:invalid,design_capacity_mah:invalid,full_charge_capacity_mah:invalid,cycle_count:invalid}));
  for(const id of ['batt-health','batt-design','batt-full'])assert.equal(ui.nodes[id].textContent,'Unavailable',id+' '+invalid);
  assert.equal(ui.nodes['batt-cycles'].textContent,invalid===0?'0':'Unavailable');
 }
 ui.render(batterySnapshot({health_percent:96,health_source:null,design_capacity_mah:null}));
 assert.equal(ui.nodes['batt-health'].textContent,'Unavailable');
 assert.equal(ui.nodes['batt-design'].textContent,'Unavailable');
 assert.equal(ui.nodes['batt-full'].textContent,'5,655 mAh');
 const ko=batteryHarness('ko');ko.render(batterySnapshot({health_percent:null}));
 assert.equal(ko.nodes['batt-health'].textContent,'확인 불가');
});
test('battery presence changes clear old data and retain a no-battery label',()=>{
 const ui=batteryHarness();ui.render(batterySnapshot());
 ui.render({batt_present:false,batt_details:batterySnapshot().batt_details});
 assert.equal(ui.nodes['batt-val'].textContent,'No battery');
 assert.equal(ui.nodes['batt-sub'].textContent,'—');
 assert.equal(ui.nodes['batt-details'].style.display,'none');
 for(const id of ['batt-health','batt-design','batt-full','batt-cycles'])assert.equal(ui.nodes[id].textContent,'Unavailable');
 ui.render(batterySnapshot({health_percent:null,cycle_count:null}));
 assert.equal(ui.nodes['batt-details'].style.display,'');
 assert.equal(ui.nodes['batt-health'].textContent,'Unavailable');
 assert.equal(ui.nodes['batt-cycles'].textContent,'Unavailable');
 const ko=batteryHarness('ko');ko.render({batt_present:false});assert.equal(ko.nodes['batt-val'].textContent,'배터리 없음');
});
test('reported battery health explains the different physical capacity ratio in both languages',()=>{
 for(const [lang,note] of [['en','Health is reported by macOS and may differ from the capacity ratio.'],['ko','건강도는 macOS 보고값이며 용량의 단순 비율과 다를 수 있습니다.']]){
  const ui=batteryHarness(lang);ui.render(batterySnapshot());
  assert.equal(ui.nodes['batt-source-note'].textContent,note);
  assert.equal(ui.nodes['batt-source-note'].style.display,'');
  ui.render(batterySnapshot({health_source:'estimated'}));
  assert.equal(ui.nodes['batt-source-note'].style.display,'none');
  ui.render({batt_present:false});assert.equal(ui.nodes['batt-source-note'].style.display,'none');
 }
});
test('battery text is inserted as text and never interpreted as HTML',()=>{
 const ui=batteryHarness();const text='<img src=x onerror=alert(1)>';
 ui.render({...batterySnapshot({state:text}),batt_sub:text});
 assert.equal(ui.nodes['batt-sub'].textContent,text);
});

console.log(`${passed} dashboard behavior checks passed, ${failed} failed`);
process.exitCode=failed?1:0;
