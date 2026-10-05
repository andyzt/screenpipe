// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com
/** Day history is never evidence of present capture. Use the live health stream. */
export type JournalCaptureState = 'recording' | 'paused' | 'unavailable' | 'checking';
export function journalCaptureState(health: {frame_status: string; vision_reason?: string | null} | null, serverDown: boolean): JournalCaptureState {
  if (serverDown) return 'unavailable';
  if (!health) return 'checking';
  if (health.frame_status === 'ok') return 'recording';
  if (health.frame_status === 'disabled' || health.frame_status === 'not_started') return 'paused';
  return 'unavailable';
}
