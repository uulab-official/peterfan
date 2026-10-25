// Independent CPU-item preferences; native request IDs are the save authority.
var CPU_WIDGET_CONFIRMED={enabled:false,style:'bar',show_percentage:true};
var CPU_WIDGET_PENDING=null;
var CPU_WIDGET_SUPPORTED=false;
var CPU_WIDGET_LAST_DATA={};
var CPU_WIDGET_MESSAGE='';
function cpuWidgetConfig(value){
  value=value||{};
  return {enabled:value.enabled===true,style:/^(bar|history|number|cores)$/.test(value.style)?value.style:'bar',show_percentage:value.show_percentage!==false};
}
function setCpuWidget(field,value){
  if(!CPU_WIDGET_SUPPORTED||CPU_WIDGET_PENDING||!window.ipc)return;
  if(field==='style'&&!/^(bar|history|number|cores)$/.test(value))return;
  if(field!=='enabled'&&field!=='style'&&field!=='show_percentage')return;
  if(field!=='style'&&typeof value!=='boolean')return;
  var next=cpuWidgetConfig(CPU_WIDGET_CONFIRMED);next[field]=value;
  if(field==='style'&&value==='cores'&&CPU_WIDGET_CONFIRMED.style!=='cores')next.show_percentage=false;
  var request=settingsRequest('cpu_widget',JSON.stringify(next),0);
  CPU_WIDGET_PENDING={request:request,config:next,at:Date.now()};
  CPU_WIDGET_MESSAGE='';
  renderCpuWidget(CPU_WIDGET_LAST_DATA);
  window.ipc.postMessage('settingsrequest:'+JSON.stringify(request));
}
function cpuWidgetHistoryPath(values){
  if(!Array.isArray(values))return '';
  values=values.slice(-60);
  var offset=60-values.length,path='';
  values.forEach(function(value,index){
    if(typeof value!=='number'||!isFinite(value)||value<0||value>100)return;
    var x=2+offset+index,y=32-Math.round(29*value/100);
    path+='M'+x+' 33V'+y;
  });
  return path;
}
function cpuWidgetCorePaths(values){
  values=Array.isArray(values)?values:[];
  var result={width:values.length?4+8*values.length:64,load:'',missing:'',tracks:''};
  values.forEach(function(value,index){
    var x=5+8*index;
    result.tracks+='M'+x+' 33V3';
    if(typeof value==='number'&&isFinite(value)&&value>=0&&value<=100){
      result.load+='M'+x+' 33V'+(33-Math.round(30*value/100));
    }else{result.missing+='M'+(x-2)+' 18h4';}
  });
  if(!values.length)result.missing='M28 18h8';
  return result;
}
function renderCpuWidget(d){
  d=d||{};CPU_WIDGET_LAST_DATA=d;
  var names=LANG==='ko'
    ?{'cpu-widget-title':'CPU 위젯','cpu-widget-copy':'러너와 별도로 상단에 CPU 사용률을 표시합니다.','cpu-widget-on-label':'표시','cpu-widget-style-label':'스타일','cpu-widget-style-bar':'바','cpu-widget-style-history':'히스토리','cpu-widget-style-number':'숫자','cpu-widget-percent-label':'사용률 숫자 표시'}
    :{'cpu-widget-title':'CPU widget','cpu-widget-copy':'A separate CPU item alongside your runner.','cpu-widget-on-label':'Show','cpu-widget-style-label':'Display','cpu-widget-style-bar':'Bar','cpu-widget-style-history':'History','cpu-widget-style-number':'Number','cpu-widget-percent-label':'Show percentage'};
  names['cpu-widget-style-cores']=LANG==='ko'?'코어별 바':'Per-core bars';
  Object.keys(names).forEach(function(id){var node=document.getElementById(id);if(node)node.textContent=names[id];});
  CPU_WIDGET_SUPPORTED=d.cpu_widget_supported===true;
  CPU_WIDGET_CONFIRMED=cpuWidgetConfig(d.cpu_widget);
  if(CPU_WIDGET_PENDING){
    var result=settingsResult(d,CPU_WIDGET_PENDING.request);
    if(result){
      CPU_WIDGET_MESSAGE=result.ok?(LANG==='ko'?'저장했습니다':'Saved'):(result.message||(LANG==='ko'?'저장하지 못했습니다':'Could not save'));
      CPU_WIDGET_PENDING=null;
    } else if(Date.now()-CPU_WIDGET_PENDING.at>15000){
      CPU_WIDGET_MESSAGE=LANG==='ko'?'저장 응답이 없습니다. 다시 시도해 주세요.':'Save timed out. Please retry.';
      CPU_WIDGET_PENDING=null;
    }
  }
  var config=CPU_WIDGET_PENDING?CPU_WIDGET_PENDING.config:CPU_WIDGET_CONFIRMED;
  var corePaths=cpuWidgetCorePaths(d.cpu_widget_cores);
  var help=document.getElementById('cpu-widget-core-help');
  if(help){help.hidden=config.style!=='cores';help.textContent=LANG==='ko'?'논리 코어마다 막대 하나 · 각 0–100%. Intel에서는 가상 코어도 포함합니다.':'One bar per logical CPU · 0–100% each. Includes Intel Hyper-Threading CPUs.';}
  var pending=!!CPU_WIDGET_PENDING;
  var row=document.getElementById('cpu-widget-setting');
  if(row)row.hidden=!CPU_WIDGET_SUPPORTED;
  var enabled=document.getElementById('cpu-widget-enabled');
  if(enabled){enabled.checked=config.enabled;enabled.disabled=!CPU_WIDGET_SUPPORTED||pending;enabled.setAttribute('aria-label',LANG==='ko'?'메뉴 막대에 CPU 위젯 표시':'Show CPU widget in menu bar');}
  var style=document.getElementById('cpu-widget-style');
  if(style){style.value=config.style;style.disabled=!CPU_WIDGET_SUPPORTED||!config.enabled||pending;}
  var percentage=document.getElementById('cpu-widget-percentage');
  if(percentage){percentage.checked=config.style==='number'||config.show_percentage;percentage.disabled=!CPU_WIDGET_SUPPORTED||!config.enabled||config.style==='number'||pending;}
  var status=document.getElementById('cpu-widget-status');
  if(status)status.textContent=pending?(LANG==='ko'?'저장 중…':'Saving…'):(d.cpu_widget_error||CPU_WIDGET_MESSAGE);
  var preview=document.getElementById('cpu-widget-preview');
  if(preview){preview.dataset.style=config.style;preview.style.opacity=config.enabled?'1':'.45';preview.setAttribute('aria-label',LANG==='ko'?'CPU 메뉴 막대 미리보기':'CPU menu bar preview');}
  var raw=d.cpu_pct,valid=typeof raw==='number'&&isFinite(raw)&&raw>=0&&raw<=100;
  var number=document.getElementById('cpu-widget-preview-number');
  if(number){number.textContent=(config.style==='number'?'CPU ':'')+(valid?Math.round(raw):'--')+'%';number.hidden=config.style!=='number'&&!config.show_percentage;}
  var bar=document.getElementById('cpu-widget-preview-bar');
  if(bar){bar.setAttribute('width',valid?String(Math.round(raw*.52)):"0");bar.setAttribute('visibility',config.style==='bar'?'visible':'hidden');}
  var graph=document.getElementById('cpu-widget-preview-graph');
  if(graph){graph.setAttribute('d',cpuWidgetHistoryPath(d.cpu_widget_history));graph.setAttribute('visibility',config.style==='history'?'visible':'hidden');}
  var missing=document.getElementById('cpu-widget-preview-missing');
  if(missing)missing.setAttribute('visibility',!valid&&config.style==='bar'?'visible':'hidden');
  var svg=document.getElementById('cpu-widget-preview-svg');
  if(svg){svg.setAttribute('viewBox','0 0 '+(config.style==='cores'?corePaths.width:64)+' 36');svg.style.width=(config.style==='cores'?corePaths.width/2:32)+'px';}
  ['tracks','load','missing'].forEach(function(key){var node=document.getElementById('cpu-widget-core-'+key);if(node)node.setAttribute('d',corePaths[key]);});
}
