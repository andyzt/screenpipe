// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com
import { beforeEach, describe, expect, it, vi } from 'vitest';
const mocks=vi.hoisted(()=>({fetch:vi.fn(),track:vi.fn()}));
vi.mock('@/lib/api',()=>({localFetch:mocks.fetch}));
vi.mock('@/lib/analytics/traction',()=>({trackTraction:mocks.track}));
import { fetchJournalDay, generateRecap, putCardFeedback } from './api';
beforeEach(()=>{vi.clearAllMocks()});
describe('Journal product analytics boundary',()=>{
  it('does not count polling as activity',async()=>{
    mocks.fetch.mockResolvedValue(new Response(JSON.stringify({cards:[]})));
    await fetchJournalDay();expect(mocks.track).not.toHaveBeenCalled();
  });
  it('does not forward feedback text or card IDs',async()=>{
    mocks.fetch.mockResolvedValue(new Response(JSON.stringify({id:45})));
    await putCardFeedback(45,{rating:'down',note:'private diary'});
    expect(mocks.track).toHaveBeenCalledWith('card_feedback',{operation:'feedback',outcome:'success',rating:'down'});
    expect(JSON.stringify(mocks.track.mock.calls)).not.toContain('private');
  });
  it('counts failed HTTP as error without raw provider body',async()=>{
    mocks.fetch.mockResolvedValue(new Response(JSON.stringify({error:'private provider context'}),{status:503}));
    await expect(generateRecap('2026-09-22')).rejects.toThrow();
    expect(mocks.track).toHaveBeenCalledWith('operation_failed',{operation:'recap',reason:'provider_unavailable'});
  });
  it('does not mark logical failure inside HTTP 200 as success',async()=>{
    mocks.fetch.mockResolvedValue(new Response(JSON.stringify({status:'failed'})));
    await generateRecap('2026-09-22');
    expect(mocks.track).toHaveBeenCalledTimes(1);
    expect(mocks.track).toHaveBeenCalledWith('operation_failed',{operation:'recap',reason:'unknown'});
  });
});
