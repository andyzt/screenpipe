// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com

/** True when the failure looks like the engine is not (yet) reachable rather
 * than a real answer: a transport error (the webview reports "Load failed" /
 * "Failed to fetch") or a 502/503 from the app's own proxy. */
export function isEngineUnavailable(error: unknown): boolean {
  // Duck-typed on purpose: component tests mock "@/lib/journal/api", which
  // turns the class into a stub that `instanceof` cannot use.
  const status = (error as { name?: string; status?: unknown } | null)?.status;
  if ((error as { name?: string } | null)?.name === "JournalApiError" || typeof status === "number") {
    return status === 502 || status === 503;
  }
  if (error instanceof DOMException && error.name === "AbortError") return false;
  return error instanceof TypeError || (error instanceof Error && /load failed|failed to fetch|network/i.test(error.message));
}

export const ENGINE_WAIT_ATTEMPTS = 20;
export const ENGINE_WAIT_DELAY_MS = 3000;

function delay(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(done, ms);
    function done() {
      signal.removeEventListener("abort", done);
      clearTimeout(timer);
      resolve();
    }
    signal.addEventListener("abort", done, { once: true });
  });
}

/** Run a journal request, retrying while the engine is still starting.
 *
 * At app launch the webview mounts a few seconds before the embedded engine
 * listens, so the first `/journal/*` call fails at the transport layer. That
 * is not an error the reader can act on; keep waiting (up to about a minute)
 * and only surface failures that survive it or that carry a real status. */
export async function withEngineWait<T>(
  request: () => Promise<T>,
  signal: AbortSignal,
  onWaiting?: (attempt: number) => void,
  attempts = ENGINE_WAIT_ATTEMPTS,
  delayMs = ENGINE_WAIT_DELAY_MS,
): Promise<T> {
  for (let attempt = 1; ; attempt++) {
    try {
      return await request();
    } catch (error) {
      if (signal.aborted || !isEngineUnavailable(error) || attempt >= attempts) throw error;
      onWaiting?.(attempt);
      await delay(delayMs, signal);
      if (signal.aborted) throw error;
    }
  }
}
