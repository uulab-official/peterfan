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
console.log(`${passed} dashboard behavior checks passed, ${failed} failed`);
process.exitCode=failed?1:0;
