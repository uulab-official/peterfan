#!/usr/bin/env node
// Generate a private local comparison from the same cached alpha frames as AppKit.
import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'node:fs';
import { parseArgs } from 'node:util';
import { resolve, dirname } from 'node:path';
const {values:options}=parseArgs({options:{before:{type:'string'},output:{type:'string'},'before-label':{type:'string'},'after-label':{type:'string'}}});
const htmlText=value=>String(value).replace(/[&<>"]/g,ch=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[ch]));
const beforeLabel=htmlText(options['before-label']||'이전 · 1.32.2');
const afterLabel=htmlText(options['after-label']||'개선안 · 1.32.3');
const frames = JSON.parse(readFileSync(new URL('../target/runner-motion-frames.json', import.meta.url)));
const beforeUrl = options.before?resolve(options.before):new URL('../target/runner-motion-before-1.32.2.json', import.meta.url);
const hasBefore = existsSync(beforeUrl);
if(options.before&&!hasBefore)throw new Error('Missing requested before frames: '+options.before);
const before = hasBefore ? JSON.parse(readFileSync(beforeUrl)) : frames;
const source = readFileSync(new URL('../packages/menubar/src/main.rs', import.meta.url), 'utf8');
const start = source.indexOf('var RUNNER_CHARACTERS=');
const end = source.indexOf('function positionRunnerCharacterMenu()', start);
const runtime = source.slice(start, end)
  .replace('__RUNNER_CHARACTERS__', JSON.stringify(Object.keys(frames)))
  .replace('__RUNNER_PREVIEWS__', '{}').replace('__RUNNER_PREVIEW_FRAMES__', '{}')
  .replace('__RUNNER_SELECTED_PREVIEW__', '[]').replace('__RUNNER_SELECTED_PREVIEW_CHARACTER__', '');
const html = `<!doctype html><html lang="ko"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>PeterFan · Motion study</title>
<style>*{box-sizing:border-box}body{margin:0;background:#111315;color:#e8e9ea;font:14px system-ui,-apple-system,sans-serif}main{max-width:1100px;margin:auto;padding:42px 32px}header{display:flex;justify-content:space-between;gap:30px;align-items:end;margin-bottom:32px}small{color:#a2aca8;letter-spacing:.1em;font-size:11px}h1{font-size:28px;font-weight:550;letter-spacing:-.9px;margin:8px 0}p{color:#9fa5aa;line-height:1.6}header>button{flex-shrink:0;white-space:nowrap}button[aria-pressed="true"]{border-color:#8cae9d;background:#8cae9d1a}button{font:inherit;cursor:pointer;color:inherit;border:1px solid #343a3e;background:transparent;border-radius:8px;padding:9px 14px}.light{background:#f5f6f7;color:#1d2226}.load-jumps{display:flex;gap:8px;flex-wrap:wrap;margin:0 0 16px}.load-jumps a{font-size:12px;color:inherit;text-decoration:none;border:1px solid #737d84;border-radius:7px;padding:8px 10px}.card{scroll-margin-top:16px}.grid{display:grid;grid-template-columns:repeat(3,1fr);gap:12px}.card{border:1px solid #303538;border-radius:12px;overflow:hidden}.card>header{padding:16px 18px;margin:0;border-bottom:1px solid #303538;font-variant-numeric:tabular-nums}.card h2{font-size:13px;margin:0}.pace{color:#8cae9d;font:12px ui-monospace,monospace}.runner{display:grid;grid-template-columns:65px 1fr 62px;align-items:center;min-height:62px;padding:8px 16px;border-bottom:1px solid #262b2e;gap:10px}.runner:last-child{border:0}.runner>span{font-size:11px;line-height:1.5;color:#91999e;text-transform:capitalize}.slot{height:30px;display:flex;align-items:center}.slot canvas{width:27px;height:18px}.enlarged{width:60px;height:40px;image-rendering:pixelated}.light canvas{filter:invert(1)}.light .card,.light .card>header,.light .runner,.light .pose-cell{border-color:#d1d8dd}.light .runner>span,.light p,.light small,.light .pose-study summary,.light .pace{color:#57656e}.pose-study{margin:18px 0 24px}.pose-study summary{cursor:pointer;color:#9fa5aa;font-size:12px}.pose-study h2{font-size:18px;margin-bottom:8px}.pose-grid{display:grid;grid-template-columns:repeat(8,minmax(0,1fr));gap:8px}.pose-cell{border:1px solid #303538;border-radius:10px;padding:12px 4px;text-align:center}.pose-cell canvas{width:96px;max-width:100%;height:auto;image-rendering:pixelated}.pose-cell span{display:block;font-size:11px;color:#91999e;margin-top:6px}.note{font-size:12px;margin-top:22px;color:#858e93}@media(max-width:760px){.pose-grid{grid-template-columns:repeat(4,minmax(0,1fr))}.grid{grid-template-columns:1fr}header{align-items:start}main{padding:24px 16px}}</style>
<main><header><div><small>PETERFAN / MOTION STUDY</small><h1>작은 크기에서도 읽히는 동작.</h1><p>20종의 실루엣·관절·접지 동작을 다듬었습니다. CPU에 따른 속도는 그대로입니다.<br>왼쪽은 실제 메뉴바 크기, 오른쪽은 자세를 확인하는 확대 보기입니다.</p></div><button id="theme">밝은 배경</button></header><details class="pose-study"><summary>팔굽혀펴기 · 여덟 자세 보기</summary><p>손·발은 제자리에, 몸은 한 줄로 내려갔다 올라옵니다.</p><div class="pose-grid" id="pushup-poses"></div></details><details class="pose-study"><summary>유령 · 여덟 자세 보기</summary><div class="pose-grid" id="ghost-poses"></div></details><details class="pose-study"><summary>파티 댄서 · 여덟 자세 보기</summary><div class="pose-grid" id="party-poses"></div></details><nav class="load-jumps"><button id="before-version"${hasBefore ? '' : ' disabled'}>${hasBefore ? beforeLabel : '비교 원본 없음'}</button><button id="after-version" aria-pressed="true">${afterLabel}</button></nav><nav class="load-jumps"><a href="#load-5">가벼운 작업 · 5%</a><a href="#load-45">일반 작업 · 45%</a><a href="#load-95">높은 부하 · 95%</a></nav><nav class="load-jumps"><button id="motion-toggle" aria-pressed="false">동작 정지</button><label>자세 <input id="motion-frame" aria-label="러너 자세" type="range" min="0" max="7" step="1" value="0"></label></nav><div class="grid" id="grid"></div><p class="note">실제 측정값이 아닌 5% · 45% · 95% 고정 부하 비교입니다. 앱과 동일한 프레임·속도 함수 사용. 시스템의 동작 줄이기 설정을 따릅니다.</p></main>
<script>
function railView(){return 'settings'}
${runtime}
const FRAMES=${JSON.stringify(frames)};
const BEFORE=${JSON.stringify(before)};
let activeFrames=FRAMES;
const contexts=[];
const labels={slime:'슬라임',sushi:'달리는 초밥',bird:'날갯짓',duck:'뒤뚱 오리',sheep:'통통 양',fish:'헤엄 물고기',sausage:'소시지 조깅',party:'파티 댄서',orbit:'작은 우주',city:'밤의 도시',pushup:'팔굽혀펴기',cat:'고양이',dog:'강아지',rabbit:'토끼',fox:'여우',penguin:'펭귄',dinosaur:'공룡',robot:'로봇',ghost:'유령',human:'러너'};
const poseLabels=['팔 펴기','내려가기 1','내려가기 2','내려가기 3','가장 낮은 자세','올라오기 1','올라오기 2','올라오기 3'];
for(const character of ['pushup','ghost','party']){
 FRAMES[character].slice(8,16).forEach((alpha,index)=>{
  const cell=document.createElement('div');cell.className='pose-cell';const label=character==='pushup'?poseLabels[index]:'자세 '+(index+1);cell.innerHTML='<canvas width="48" height="32" data-pose-character="'+character+'" data-pose-index="'+index+'" aria-label="'+labels[character]+' '+label+'"></canvas><span>'+label+'</span>';document.getElementById(character+'-poses').appendChild(cell);
  const data=new ImageData(48,32);for(let i=0;i<alpha.length;i+=2){const o=i*2;data.data[o]=data.data[o+1]=data.data[o+2]=255;data.data[o+3]=parseInt(alpha.slice(i,i+2),16)}cell.querySelector('canvas').getContext('2d').putImageData(data,0,0);
 });
}
const grid=document.getElementById('grid');
for(const [load,label] of [[5,'가벼운 작업'],[45,'일반 작업'],[95,'높은 부하']]){
 const card=document.createElement('section');card.className='card';card.id='load-'+load;card.innerHTML='<header><h2>'+label+' · '+load+'%</h2><span class="pace">'+runnerPreviewInterval(load,0)+' ms / 프레임</span></header>';
 for(const character of ['pushup','party','ghost','slime','sushi','bird','duck','sheep','fish','sausage','orbit','city',...Object.keys(FRAMES).filter(name=>!['pushup','party','ghost','slime','sushi','bird','duck','sheep','fish','sausage','orbit','city'].includes(name))]){
  const row=document.createElement('div');row.className='runner';row.innerHTML='<span>'+labels[character]+'</span><div class="slot"><canvas width="48" height="32"></canvas></div><canvas class="enlarged" width="48" height="32"></canvas>';card.appendChild(row);
  const images=FRAMES[character].slice(runnerPreviewBand(load)*8,runnerPreviewBand(load)*8+8).map(alpha=>{const data=new ImageData(48,32);for(let i=0;i<alpha.length;i+=2){const o=i*2;data.data[o]=data.data[o+1]=data.data[o+2]=255;data.data[o+3]=parseInt(alpha.slice(i,i+2),16)}return data});
  contexts.push({canvases:[...row.querySelectorAll('canvas')],images,load,character,frame:0,phase:0});
 }grid.appendChild(card);
}
let paused=false;let last=performance.now();const media=matchMedia('(prefers-reduced-motion: reduce)');
function paint(now){let elapsed=Math.min(250,now-last);last=now;for(const state of contexts){if(!paused&&!media.matches&&!document.hidden){state.phase+=elapsed;while(state.phase>=runnerPreviewInterval(state.load,state.frame,state.character)){state.phase-=runnerPreviewInterval(state.load,state.frame,state.character);state.frame=(state.frame+1)%8}}for(const c of state.canvases)c.getContext('2d').putImageData(state.images[state.frame],0,0)}requestAnimationFrame(paint)}if(location.hash!=='#poses')requestAnimationFrame(paint);
if(location.hash==='#poses'){document.querySelectorAll('.pose-study').forEach(section=>section.open=true);document.getElementById('grid').style.display='none';document.querySelectorAll('a[href^="#load-"]').forEach(link=>link.parentElement.style.display='none');document.getElementById('motion-toggle').parentElement.style.display='none';}
function selectVersion(useBefore){
 activeFrames=useBefore?BEFORE:FRAMES;
 document.querySelectorAll('[data-pose-character]').forEach(canvas=>{const alpha=activeFrames[canvas.dataset.poseCharacter][8+Number(canvas.dataset.poseIndex)];const data=new ImageData(48,32);for(let i=0;i<alpha.length;i+=2){const o=i*2;data.data[o]=data.data[o+1]=data.data[o+2]=255;data.data[o+3]=parseInt(alpha.slice(i,i+2),16)}canvas.getContext('2d').putImageData(data,0,0);});
 document.getElementById('before-version').setAttribute('aria-pressed',String(useBefore));
 document.getElementById('after-version').setAttribute('aria-pressed',String(!useBefore));
 for(const state of contexts){state.images=activeFrames[state.character].slice(runnerPreviewBand(state.load)*8,runnerPreviewBand(state.load)*8+8).map(alpha=>{const data=new ImageData(48,32);for(let i=0;i<alpha.length;i+=2){const o=i*2;data.data[o]=data.data[o+1]=data.data[o+2]=255;data.data[o+3]=parseInt(alpha.slice(i,i+2),16)}return data});}
}
document.getElementById('motion-toggle').onclick=()=>{paused=!paused;document.getElementById('motion-toggle').setAttribute('aria-pressed',String(paused));document.getElementById('motion-toggle').textContent=paused?'동작 재생':'동작 정지';};
document.getElementById('motion-frame').oninput=e=>{paused=true;for(const state of contexts){state.frame=Number(e.target.value);state.phase=0}document.getElementById('motion-toggle').setAttribute('aria-pressed','true');document.getElementById('motion-toggle').textContent='동작 재생';};
document.getElementById('before-version').onclick=()=>selectVersion(true);
document.getElementById('after-version').onclick=()=>selectVersion(false);
document.getElementById('theme').onclick=()=>{document.body.classList.toggle('light');document.getElementById('theme').textContent=document.body.classList.contains('light')?'어두운 배경':'밝은 배경'};
</script></html>`;
const url = options.output?resolve(options.output):new URL('../target/runner-motion-qa.html', import.meta.url);
if(options.output)mkdirSync(dirname(url),{recursive:true});
writeFileSync(url, html);
console.log(options.output?url:url.pathname);
