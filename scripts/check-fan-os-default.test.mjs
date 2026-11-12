import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';

const source=readFileSync(new URL('../packages/menubar/src/main.rs',import.meta.url),'utf8');
const begin=source.indexOf('function setProfile(profile){');
const end=source.indexOf('function processActionText(action){',begin);
assert.ok(begin>=0&&end>begin,'fan control functions must exist');

function ui(lang='en'){
  const messages=[];let now=1000,sequence=0;
  const classes=()=>({toggle(){}});
  const button={dataset:{mode:'auto'},classList:classes(),setAttribute(k,v){this[k]=v;},disabled:false};
  const strip={classList:classes(),setAttribute(){},querySelectorAll:()=>[button]};
  const detail={textContent:''},title={textContent:''},status={textContent:'',style:{},className:''};
  const root={dataset:{}};
  const nodes={'profile-strip':strip,'profile-guide':root,'profile-guide-title':title,'profile-guide-detail':detail,'fan-apply-status':status};
  const context=vm.createContext({LANG:lang,Date:{now:()=>now},Math,Number,isFinite,Array,settingsRequest:()=>({id:'test-'+(++sequence)}),document:{getElementById:id=>nodes[id]||null,querySelectorAll:()=>[]},window:{ipc:{postMessage:m=>messages.push(m)}},FAN_CONTROL_PENDING:null,FAN_CONTROL_RESULT:null});
  vm.runInContext(source.slice(begin,end),context);
  const data=(extra={})=>({can_control:true,controllable_fan_count:2,active_control_mode:'auto',active_profile:'',fan_override_count:1,control_revision:8,applied_control_revision:8,last_cmd_status:'',os_auto_results:[],fans:[],...extra});
  const sentId=()=>JSON.parse(messages.at(-1).slice('osautorequest:'.length)).id;
  return {context,messages,data,detail,title,status,button,sentId,advance:ms=>{now+=ms;}};
}

test('daemon acknowledgement reports acceptance without attributing a stale revision',()=>{
  const u=ui();u.context.window.__pf_pending=u.data({control_revision:9,applied_control_revision:8});
  u.context.updateProfileStrip(u.data());assert.match(u.detail.textContent,/overrides remain/i);
  u.context.setAuto();assert.equal(u.messages.length,1);assert.match(u.messages[0],/^osautorequest:/);
  const id=u.sentId();u.context.setAuto();assert.equal(u.messages.length,1);
  const ok=[{id,ok:true,route:'daemon'}];
  u.context.updateProfileStrip(u.data({control_revision:9,applied_control_revision:9,fan_override_count:0,os_auto_results:ok}));
  assert.equal(u.context.FAN_CONTROL_PENDING,null);
  assert.equal(u.context.FAN_CONTROL_RESULT.route,'daemon');
  u.context.updateFanApplyStatus(u.data({control_revision:9,applied_control_revision:9,fan_override_count:0}));
  assert.match(u.status.textContent,/request accepted.*application and hardware mode unverified/i);
  assert.doesNotMatch(u.status.textContent,/applied/i);
});

test('direct or mock success requires the matching request ID, without daemon revisions',()=>{
  const u=ui('ko');const unknown=u.data({active_control_mode:'',fan_override_count:null,control_revision:null,applied_control_revision:null});
  u.context.window.__pf_pending=unknown;u.context.updateProfileStrip(unknown);
  assert.equal(u.button['aria-pressed'],'false');assert.match(u.title.textContent,/확인 불가/);
  u.context.setAuto();const id=u.sentId();
  u.context.updateProfileStrip({...unknown,os_auto_results:[{id:'older',ok:true,route:'local'}]});
  assert.ok(u.context.FAN_CONTROL_PENDING,'unrelated local result cannot acknowledge');
  u.context.updateProfileStrip({...unknown,os_auto_results:[{id,ok:true,route:'local'}]});
  assert.equal(u.context.FAN_CONTROL_RESULT.ok,true);
  u.context.updateFanApplyStatus(unknown);assert.match(u.status.textContent,/실제 팬 모드 확인 불가/);
});

test('a matching command failure wins over a newer stale-looking daemon snapshot',()=>{
  const u=ui();u.context.window.__pf_pending=u.data();u.context.setAuto();const id=u.sentId();
  const failed=u.data({control_revision:9,applied_control_revision:9,fan_override_count:0,os_auto_results:[{id,ok:false,route:'unknown',message:'error: denied'}]});
  u.context.updateProfileStrip(failed);u.context.updateFanApplyStatus(failed);
  assert.equal(u.context.FAN_CONTROL_RESULT.ok,false);assert.match(u.status.textContent,/denied/);
});

test('legacy daemon acknowledgement never claims application when override state or revisions are missing',()=>{
  for(const bad of [{fan_override_count:null},{fan_override_count:'0'},{fan_override_count:-1},{control_revision:null},{applied_control_revision:null}]){
    const u=ui();const initial=u.data(bad);u.context.window.__pf_pending=initial;u.context.setAuto();const id=u.sentId();
    const advanced=u.data({...bad,applied_control_revision:bad.applied_control_revision===null?null:10,os_auto_results:[{id,ok:true,route:'daemon'}]});
    u.context.updateProfileStrip(advanced);u.context.updateFanApplyStatus(advanced);
    assert.equal(u.context.FAN_CONTROL_RESULT.route,'daemon');
    assert.match(u.status.textContent,/application and hardware mode unverified/i);
    assert.doesNotMatch(u.status.textContent,/applied/i);
  }
});

test('an unrelated daemon result cannot acknowledge the pending request',()=>{
  const u=ui();u.context.window.__pf_pending=u.data();u.context.setAuto();
  u.context.updateProfileStrip(u.data({os_auto_results:[{id:'older',ok:true,route:'daemon'}]}));
  assert.ok(u.context.FAN_CONTROL_PENDING);
  u.advance(8001);u.context.updateProfileStrip(u.data());u.context.updateFanApplyStatus(u.data());
  assert.equal(u.context.FAN_CONTROL_RESULT.ok,false);assert.match(u.status.textContent,/acknowledgement unavailable/i);
});

test('profile revision floor includes desired revision already in flight',()=>{
  const u=ui();u.context.window.__pf_pending=u.data({active_control_mode:'profile',active_profile:'balanced',control_revision:9,applied_control_revision:8});
  u.context.setProfile('silent');
  u.context.updateProfileStrip(u.data({active_control_mode:'profile',active_profile:'silent',applied_control_revision:9}));
  assert.ok(u.context.FAN_CONTROL_PENDING);
  u.context.updateProfileStrip(u.data({active_control_mode:'profile',active_profile:'silent',applied_control_revision:10}));
  assert.equal(u.context.FAN_CONTROL_RESULT.ok,true);
});

test('per-fan follow mode is described as following the global setting',()=>{
  assert.match(source,/'전체 설정 따르기':'Follow global'/);
  assert.match(source,/Clear this fan’s override and follow the global fan setting/);
});
