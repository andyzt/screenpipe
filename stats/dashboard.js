// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com
'use strict';
const views=[['product','Product'],['coreloop','Core loop'],['client','Client health'],['models','Models & routing'],['spend','Spend'],['errors','Errors & latency'],['features','Features']];
const periods=[['yesterday','Yesterday'],['1','1 day'],['7','7 days'],['30','30 days']];
const params=new URLSearchParams(location.search);
let view=views.some(([v])=>v===params.get('view'))?params.get('view'):'product';
let period=periods.some(([v])=>v===params.get('period'))?params.get('period'):'1';
let product=null, charts=[], generation=0, breakdown='app_version', measure='actors';
const $=id=>document.getElementById(id),esc=v=>String(v??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const num=v=>v===null||v===undefined?'—':new Intl.NumberFormat('en-US',{maximumFractionDigits:2}).format(v);
const pct=v=>v==null?'—':num(v*100)+'%';
const color=i=>`hsl(${getComputedStyle(document.documentElement).getPropertyValue('--chart-'+(i%5+1))})`;
const plot=(id,title,note,wide=false)=>`<article class="panel ${wide?'wide':''}"><h2>${esc(title)}</h2><small>${esc(note)}</small><div class="plot"><canvas id="${id}" role="img" aria-label="${esc(title)}"></canvas><div id="empty-${id}" class="empty" hidden></div></div></article>`;
function draw(id,type,labels,series,emptyText='No observed events in this period. Waiting for opt-in client data.',horizontal=false){
  const has=series.some(s=>s.values.some(v=>v!==null&&v!==undefined&&(v!==0||s.zeroValid)));
  const ink=`hsl(${getComputedStyle(document.documentElement).getPropertyValue('--chart-ink')})`;
  const border=`hsl(${getComputedStyle(document.documentElement).getPropertyValue('--border')})`;
  charts.push(new Chart($(id),{type,data:{labels,datasets:series.map((s,i)=>({label:s.label,data:s.values,borderColor:color(i),backgroundColor:type==='line'?color(i):color(i).replace(')', ' / .7)'),borderWidth:2,tension:.15,pointRadius:labels.length>10?1:3,spanGaps:false}))},options:{responsive:true,maintainAspectRatio:false,animation:false,indexAxis:horizontal?'y':'x',interaction:{mode:'index',intersect:false},plugins:{legend:{position:'bottom',labels:{color:ink,boxWidth:14,font:{size:11}}},tooltip:{callbacks:{label:ctx=>`${ctx.dataset.label}: ${num(ctx.raw)}`}}},scales:{x:{grid:{color:type==='line'?'transparent':border},ticks:{color:ink,maxTicksLimit:10},beginAtZero:horizontal},y:{beginAtZero:true,grid:{color:border},ticks:{color:ink,precision:0}}}}}));
  if(!has){$('empty-'+id).hidden=false;$('empty-'+id).textContent=emptyText;}
}
const metrics=items=>`<div class="summary">${items.map(([k,v,h])=>`<div class="metric"><small>${esc(k)}</small><b>${esc(v)}</b><small>${esc(h)}</small></div>`).join('')}</div>`;
const table=(heads,rows)=>`<div class="table-wrap"><table><thead><tr>${heads.map(h=>`<th>${esc(h)}</th>`).join('')}</tr></thead><tbody>${rows.map(row=>`<tr>${row.map(c=>`<td>${esc(c)}</td>`).join('')}</tr>`).join('')}</tbody></table></div>`;
function render(){
  charts.forEach(c=>c.destroy());charts=[];
  document.querySelectorAll('#tabs button').forEach(b=>b.setAttribute('aria-pressed',String(b.dataset.view===view)));
  document.querySelectorAll('#periods button').forEach(b=>b.setAttribute('aria-pressed',String(b.dataset.period===period)));
  if(!product)return;
  const p=product, days=p.series.map(r=>r.date.slice(5)), line=key=>p.series.map(r=>r[key]);let html='';const jobs=[];
  const trend=(id,title,note,keys,wide=false,empty)=>{html+=plot(id,title,note,wide);jobs.push(()=>draw(id,'line',days,keys.map(([key,label])=>({label,values:line(key),zeroValid:['cost_usd','cpu_percent','rss_mb','disk_mb','llm_p95_ms','llm_attempts','llm_failures'].includes(key)})),empty));};
  const bars=(id,title,note,labels,series,empty)=>{html+=plot(id,title,note);jobs.push(()=>draw(id,'bar',labels,series,empty,true));};
  html+='<div class="charts">';
  if(view==='product'){
    trend('reach','Active installations over time','Explicit journal / evidence / MCP actions. Rolling reach is evaluated at each date.',[['dau','DAU'],['wau','WAU · 7 dates'],['mau','MAU · 30 dates']],true);
    trend('actions','Return to recorded work','Evidence clicks are navigation attempts; copied recaps confirm a successful clipboard action.',[['copies','Recaps copied'],['evidence','Evidence opened'],['actions','All explicit actions']]);
    bars('features','Feature reach','Distinct consenting installations for each action; one installation can use several features.',p.features.map(r=>r.name.replaceAll('_',' ')),[{label:'Installations',values:p.features.map(r=>r.actors)}]);
  }else if(view==='coreloop'){
    bars('stages','Core-loop stage reach','Stage reach, not sequential conversion. Native first-run senders are not connected yet.',p.stage_reach.map(r=>r.event.replaceAll('_',' ')),[{label:'Installations',values:p.stage_reach.map(r=>r.actors)}]);
    trend('feedback','Card feedback','Self-selected votes, not an unbiased model accuracy score.',[['feedback_up','Positive'],['feedback_down','Negative']]);
  }else if(view==='client'){
    trend('rss','Memory footprint · daily p95','RSS MB from explicit resource samples. Resource sender is not connected to production.',[['rss_mb','RSS · MB']]);
    trend('cpu','CPU usage · daily p95','Sampled CPU %, not battery drain. Values may exceed 100% across cores.',[['cpu_percent','CPU · %']]);
    const b=p.breakdowns[breakdown]||[];
    bars('versions',breakdown==='os'?'Operating systems':'App versions','Unknown remains visible when producers do not stamp metadata.',b.map(r=>r.label),[{label:measure==='actors'?'Installations':'Events',values:b.map(r=>r[measure])}]);
    const state=p.breakdowns.state||[];
    bars('capture','Observed capture states','Technical health observations do not increase DAU.',state.map(r=>r.label),[{label:'Observations',values:state.map(r=>r.events)}],'No capture-health sender is connected yet.');
  }else if(view==='models'){
    const b=p.breakdowns.route||[];
    bars('routes','LLM attempts by route','Screenpipe → vsellm → provider. The MultiTool / Memento gateway is not in this route.',b.map(r=>r.label),[{label:'Attempts',values:b.map(r=>r.events)}],'No LLM attempt sender is connected yet.');
    trend('latency','LLM latency · daily p95','End-to-end attempt time in milliseconds; retries remain separate attempts.',[['llm_p95_ms','p95 · ms']],false,'Latency has not been instrumented yet.');
  }else if(view==='spend'){
    trend('cost','Known model cost over time','USD only where confirmed cost is supplied. Missing prices are gaps, not zero cost.',[['cost_usd','Known cost · USD']],true,'No priced attempts received. This does not mean zero spend.');
    trend('attempts','Billable-attempt volume','Retry and fallback attempts count separately. Local inference cost is not API spend.',[['llm_attempts','Attempts'],['llm_failures','Failed attempts']],false,'No LLM attempt sender is connected yet.');
  }else if(view==='errors'){
    trend('failures','Errors over time','UI operation errors are observed; native and LLM coverage is incomplete.',[['errors','Operation errors'],['llm_failures','LLM failures']],true);
    bars('reasons','Error reasons','Bounded reason codes; no raw provider errors or recorded content.',Object.keys(p.reasons),[{label:'Errors',values:Object.values(p.reasons)}],'No observed error events in this period.');
    trend('latency','LLM latency · daily p95','Missing latency measurements remain blank.',[['llm_p95_ms','p95 · ms']],false,'Latency has not been instrumented yet.');
  }else if(view==='features'){
    bars('usage','Feature usage','Switch between distinct installations and action volume below.',p.features.map(r=>r.name.replaceAll('_',' ')),[{label:measure==='actors'?'Installations':'Events',values:p.features.map(r=>r[measure])}]);
    trend('outcomes','Observed useful actions','Copied recap is an outcome proxy. Evidence click alone does not prove retrieval success.',[['copies','Recaps copied'],['evidence','Evidence opened']]);
  }
  html+='</div>';
  html+=metrics([['Active in selected period',num(p.active_actors),'Anonymous installations'],['Explicit sessions',num(p.sessions),'5-minute inactivity boundary'],['Activation · 24 hours',pct(p.activation.rate),`${p.activation.numerator} / ${p.activation.denominator} mature setups; sender pending`],['Exact-day D7 retention',pct(p.retention_d7.rate),`${p.retention_d7.numerator} / ${p.retention_d7.denominator} mature cohort installations`]]);
  if(view==='spend')html+=metrics([['Known cost · USD',num(p.llm.known_cost_usd),'Not total cost with incomplete coverage'],['Pricing coverage',`${p.llm.priced_attempts} / ${p.llm.attempts}`,'Attempts with a known price']]);
  if(view==='client'||view==='features')html+=`<div class="section-controls"><label>Measure <select id="measure" aria-label="Measure"><option value="actors">Installations</option><option value="events">Events</option></select></label>${view==='client'?'<label>Breakdown <select id="breakdown" aria-label="Breakdown"><option value="app_version">App version</option><option value="os">Operating system</option></select></label>':''}</div>`;
  html+='<details class="notes"><summary>Measurement coverage and definitions</summary>'+table(['Signal','Coverage'],[['Journal, feedback, recap actions','Sender added in source; requires an updated app and explicit consent.'],['Setup, first frame, first card','Sender pending. Activation is not measurable yet.'],['LLM attempts, prices, MCP retrieval','Senders pending. Blank charts do not mean no usage or no spend.'],['Resource and capture samples','Manual local diagnostic exists; no automatic production sender.'],['DAU / WAU / MAU','Explicit actions over 1 / 7 / 30 Moscow calendar dates. Background capture excluded.'],['D7','Activity exactly seven dates after first activity; only completed D7 cohorts.']])+'</details>';
  $('content').innerHTML=html;jobs.forEach(fn=>fn());
  if($('measure')){$('measure').value=measure;$('measure').onchange=e=>{measure=e.target.value;render();};}
  if($('breakdown')){$('breakdown').value=breakdown;$('breakdown').onchange=e=>{breakdown=e.target.value;render();};}
  $('range').textContent=`${p.range.start} → ${p.range.end} · Europe/Moscow${p.range.partial?' · includes partial today':' · complete day'}`;
  $('coverage').textContent=p.events?'Opt-in events only. Background capture is excluded from active use; native / LLM instrumentation is incomplete.':'No opt-in events received yet. Chart layouts and breakdowns are ready; no sample data is mixed into production.';
}
async function refresh(){const id=++generation;$('refresh').disabled=true;try{const response=await fetch(`product?days=${period}`,{cache:'no-store',signal:AbortSignal.timeout(8000)});if(!response.ok)throw Error(`HTTP ${response.status}`);const p=await response.json();if(id!==generation)return;product=p;$('connection').textContent='Live · Screenpipe';$('freshness').textContent='Updated '+new Date(p.computed_at*1000).toLocaleTimeString('en-GB');$('error').hidden=!p.cache_error&&Date.now()/1000-p.computed_at<60;$('error').textContent='Background calculation is delayed. Showing the last available snapshot.';render();}catch(e){if(id!==generation)return;$('error').hidden=false;$('error').textContent='Update failed. Previous results may be stale. '+e.message;$('connection').textContent='Unavailable';}finally{if(id===generation)$('refresh').disabled=false;}}
function saveUrl(){const u=new URL(location.href);u.searchParams.set('view',view);u.searchParams.set('period',period);history.replaceState(null,'',u);}
$('tabs').innerHTML=views.map(([id,label])=>`<button data-view="${id}" aria-pressed="${id===view}">${label}</button>`).join('');
$('periods').innerHTML=periods.map(([id,label])=>`<button data-period="${id}" aria-pressed="${id===period}">${label}</button>`).join('');
document.querySelectorAll('#tabs button').forEach(b=>b.onclick=()=>{view=b.dataset.view;saveUrl();render();});
document.querySelectorAll('#periods button').forEach(b=>b.onclick=()=>{period=b.dataset.period;saveUrl();void refresh();});
$('theme').onclick=()=>{document.documentElement.classList.toggle('dark');try{localStorage.setItem('theme',document.documentElement.classList.contains('dark')?'dark':'light');}catch{}render();};
window.addEventListener('message',e=>{if(e.source!==window.parent||e.origin!==location.origin)return;const theme=e.data?.theme;if(theme==='dark'||theme==='light'){document.documentElement.classList.toggle('dark',theme==='dark');render();}});
$('refresh').onclick=refresh;void refresh();
