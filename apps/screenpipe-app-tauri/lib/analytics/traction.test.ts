// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com
import { describe, it, expect } from 'bun:test';
import { createTraction, safeTractionEndpoint } from './traction';
const cfg = { enabled:true, endpoint:'https://stats.example/p/screenpipe', token:'per-install-test-credential', actor:'anonymous-install' };
function storage() { const map = new Map<string,string>(); return {getItem:(k:string)=>map.get(k)??null,setItem:(k:string,v:string)=>{map.set(k,v)},removeItem:(k:string)=>{map.delete(k)},map}; }
describe('Traction consent, privacy and retry',()=>{
  it('stamps only bounded runtime metadata on consented events',async()=>{
    const sent:any[]=[];
    const c=createTraction(storage(),async(_u,_t,events)=>{sent.push(...events);return 200},Date.now,
      ()=>({app_version:'2.7.28',os:'macos',hostname:'private-machine'}));
    c.configure(cfg);c.track('journal_opened');await c.flush();
    expect(sent[0].properties).toEqual({app_version:'2.7.28',os:'macos'});
    c.configure({...cfg,enabled:false});
  });
  it('rejects unsafe transport destinations',()=>{
    expect(safeTractionEndpoint('http://example.com')).toBeNull();
    expect(safeTractionEndpoint('https://user:key@example.com')).toBeNull();
    expect(safeTractionEndpoint('https://example.com?token=x')).toBeNull();
    expect(safeTractionEndpoint('http://127.0.0.1:9911/')).toBe('http://127.0.0.1:9911');
  });
  it('does nothing without consent; sends only safe metadata; stable IDs on retry',async()=>{
    const store=storage();const sent:any[]=[];let status=503;
    const c=createTraction(store,async(_url,_token,events)=>{sent.push(structuredClone(events));return status});
    c.track('journal_opened');expect(c.size()).toBe(0);
    c.configure(cfg);c.track('card_feedback',{rating:'down',text:'secret',duration_ms:10});
    await c.flush();expect(c.size()).toBe(1);status=200;await c.flush();
    expect(sent[0][0].properties).toEqual({rating:'down',duration_ms:10});
    expect(sent[1][0].event_id).toBe(sent[0][0].event_id);expect(c.size()).toBe(0);
    c.configure({...cfg,enabled:false});
  });
  it('clears persisted pending events when consent is disabled after restart',()=>{
    const store=storage();store.setItem('screenpipe.traction.outbox.v1','private old queue');
    const c=createTraction(store,async()=>200);c.configure({...cfg,enabled:false});
    expect(store.map.size).toBe(0);
  });
  it('binds saved queue to destination and sanitizes persisted fields',async()=>{
    const store=storage();const c=createTraction(store,async()=>503);c.configure(cfg);c.track('journal_opened');
    const saved=JSON.parse(store.getItem('screenpipe.traction.outbox.v1')!);saved.events[0].properties={text:'private',view:'day'};
    store.setItem('screenpipe.traction.outbox.v1',JSON.stringify(saved));
    const sent:any[]=[];const next=createTraction(store,async(_u,_t,es)=>{sent.push(es);return 200});next.configure(cfg);await next.flush();
    expect(sent[0][0].properties).toEqual({view:'day'});
    store.setItem('screenpipe.traction.outbox.v1',JSON.stringify(saved));
    const other=createTraction(store,async()=>200);other.configure({...cfg,endpoint:'https://other.example'});expect(other.size()).toBe(0);
    [c,next,other].forEach(x=>x.configure({...cfg,enabled:false}));
  });
  it('drops a rejected batch but keeps sending, and stops only on a bad credential',async()=>{
    const statuses=[400,200,401];const sent:number[]=[];
    const c=createTraction(storage(),async(_u,_t,es)=>{sent.push(es.length);return statuses.shift()??200});c.configure(cfg);
    c.track('journal_opened');await c.flush();expect(c.size()).toBe(0);
    c.track('card_opened');await c.flush();expect(c.size()).toBe(0);
    c.track('week_opened');await c.flush();expect(c.size()).toBe(1);
    c.track('dashboard_opened');await c.flush();expect(sent).toEqual([1,1,1]);
    c.configure({...cfg,enabled:false});
  });
  it('bounds queue and drops expired data',async()=>{
    let now=Date.now();const c=createTraction(storage(),async()=>200,()=>now);c.configure(cfg);
    for(let i=0;i<110;i++)c.track('journal_opened');expect(c.size()).toBe(100);
    now+=8*86400*1000;await c.flush();expect(c.size()).toBe(0);c.configure({...cfg,enabled:false});
  });
});
