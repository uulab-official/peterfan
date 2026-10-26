// Complete selections are confirmed only by matching native acknowledgements.
var MENU_ITEMS_CONFIRMED={cpu:false,runner:true,metric:true};
var MENU_ITEMS_PENDING=null;
var MENU_ITEMS_SUPPORTED=false;
var MENU_ITEMS_LAST_DATA={};
var MENU_ITEMS_MESSAGE='';
function menuItemsConfig(value){
  if(!value||typeof value.cpu!=='boolean'||typeof value.runner!=='boolean'||typeof value.metric!=='boolean')return {cpu:false,runner:true,metric:true};
  return {cpu:value.cpu,runner:value.runner,metric:value.metric};
}
function setMenuItem(field,value){
  if(!MENU_ITEMS_SUPPORTED||MENU_ITEMS_PENDING||(typeof CPU_WIDGET_PENDING!=='undefined'&&CPU_WIDGET_PENDING)||!window.ipc)return;
  if(['cpu','runner','metric'].indexOf(field)<0||typeof value!=='boolean')return;
  var next=menuItemsConfig(MENU_ITEMS_CONFIRMED);next[field]=value;
  if(!next.cpu&&!next.runner&&!next.metric){renderMenuItems(MENU_ITEMS_LAST_DATA);return;}
  var request=settingsRequest('menu_items',JSON.stringify(next),0);
  MENU_ITEMS_PENDING={request:request,config:next,at:Date.now()};MENU_ITEMS_MESSAGE='';
  renderMenuItems(MENU_ITEMS_LAST_DATA);
  if(typeof renderCpuWidget==='function')renderCpuWidget(MENU_ITEMS_LAST_DATA);
  window.ipc.postMessage('settingsrequest:'+JSON.stringify(request));
}
function renderMenuItems(d){
  d=d||{};MENU_ITEMS_LAST_DATA=d;
  MENU_ITEMS_SUPPORTED=d.menu_items_supported===true;
  MENU_ITEMS_CONFIRMED=menuItemsConfig(d.menu_items);
  if(MENU_ITEMS_PENDING){
    var result=settingsResult(d,MENU_ITEMS_PENDING.request);
    if(result){
      MENU_ITEMS_MESSAGE=result.ok?(LANG==='ko'?'저장했습니다':'Saved'):(result.message||(LANG==='ko'?'저장하지 못했습니다. 다시 시도해 주세요.':'Could not save. Please retry.'));
      MENU_ITEMS_PENDING=null;
    }else if(Date.now()-MENU_ITEMS_PENDING.at>15000){
      MENU_ITEMS_MESSAGE=LANG==='ko'?'저장 응답이 없습니다. 다시 시도해 주세요.':'Save timed out. Please retry.';MENU_ITEMS_PENDING=null;
    }
  }
  var config=MENU_ITEMS_PENDING?MENU_ITEMS_PENDING.config:MENU_ITEMS_CONFIRMED;
  var count=Number(config.cpu)+Number(config.runner)+Number(config.metric);
  var busy=!!MENU_ITEMS_PENDING||(typeof CPU_WIDGET_PENDING!=='undefined'&&!!CPU_WIDGET_PENDING);
  var names=LANG==='ko'?{'menu-items-title':'메뉴 막대 표시 항목','menu-items-copy':'표시할 항목을 각각 선택합니다. 숨긴 항목의 설정은 유지됩니다.','menu-items-cpu-label':'CPU 위젯','menu-items-runner-label':'러너 캐릭터','menu-items-metric-label':'측정값 텍스트'}:{'menu-items-title':'Menu-bar items','menu-items-copy':'Choose each item independently. Hidden items keep their settings.','menu-items-cpu-label':'CPU widget','menu-items-runner-label':'Runner character','menu-items-metric-label':'Metric text'};
  Object.keys(names).forEach(function(id){var node=document.getElementById(id);if(node)node.textContent=names[id];});
  var row=document.getElementById('menu-items-setting');if(row)row.hidden=!MENU_ITEMS_SUPPORTED;
  var legacy=document.getElementById('menubar-display-setting');if(legacy)legacy.hidden=MENU_ITEMS_SUPPORTED;
  ['cpu','runner','metric'].forEach(function(key){var node=document.getElementById('menu-items-'+key);if(node){node.checked=config[key];node.disabled=!MENU_ITEMS_SUPPORTED||busy||(count===1&&config[key]);}});
  var help=document.getElementById('menu-items-help');if(help)help.textContent=LANG==='ko'?'대시보드를 열려면 하나 이상의 항목을 표시해야 합니다.':'Keep at least one item visible to open the dashboard.';
  var status=document.getElementById('menu-items-status');if(status)status.textContent=MENU_ITEMS_PENDING?(LANG==='ko'?'저장 중…':'Saving…'):(MENU_ITEMS_MESSAGE||d.cpu_widget_error||'');
}
