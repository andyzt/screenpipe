// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com
import {describe,it,expect} from 'vitest';
import {journalCaptureState} from './capture-state';
describe('live journal recording status',()=>{
  it('never treats missing health as recording',()=>expect(journalCaptureState(null,false)).toBe('checking'));
  it('overrides old healthy data after disconnection',()=>expect(journalCaptureState({frame_status:'ok'},true)).toBe('unavailable'));
  it.each(['disabled','not_started'])('shows paused for %s',frame_status=>expect(journalCaptureState({frame_status},false)).toBe('paused'));
  it.each(['stale','error'])('does not call a failed capture recording: %s',frame_status=>expect(journalCaptureState({frame_status},false)).toBe('unavailable'));
  it('tracks resume from current health',()=>expect(journalCaptureState({frame_status:'ok'},false)).toBe('recording'));
});
