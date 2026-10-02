// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com

/** Opt-in, content-free Traction transport. Independent of upstream PostHog.
 * No capture-loop hooks. The bounded outbox contains only enum/numeric metadata.
 * Only a per-install credential belongs here; never the server ingest secret.
 */
export type TractionConfig = { enabled: boolean; endpoint: string; token: string; actor: string };
type Event = { event_id: string; device_id: string; name: string; ts: number; properties: Record<string, string | number> };
type Storage = Pick<globalThis.Storage, "getItem" | "setItem" | "removeItem">;
type Sender = (url: string, token: string, events: Event[], signal: AbortSignal) => Promise<number>;
const KEY = "screenpipe.traction.outbox.v1";
const names = new Set(["journal_opened", "card_opened", "evidence_opened", "week_opened", "dashboard_opened", "recap_copied", "card_feedback", "review_saved", "intention_started", "intention_ended", "focus_override", "regenerate_requested", "recap_requested", "operation_failed"]);
const enums: Record<string, Set<string>> = {
  os: new Set(["macos", "windows", "linux", "unknown"]),
  view: new Set(["day", "week", "dashboard"]),
  surface: new Set(["journal", "timeline", "settings"]),
  rating: new Set(["up", "down", "cleared", "focused", "neutral", "distracted"]),
  relation: new Set(["other_work", "break", "supports_intention", "possible_distraction", "unknown"]),
  operation: new Set(["recap", "regenerate", "feedback", "review", "intention", "focus", "retrieval", "cards", "capture"]),
  reason: new Set(["disk_pressure", "permission", "provider_unavailable", "timeout", "rate_limit", "auth", "network", "validation", "unknown"]),
  outcome: new Set(["success", "failed", "empty", "cancelled", "unknown"]),
};
export function safeTractionEndpoint(value: string): string | null {
  try {
    const u = new URL(value);
    if (u.username || u.password || u.search || u.hash) return null;
    if (u.protocol !== "https:" && !(u.protocol === "http:" && ["127.0.0.1", "localhost", "[::1]"].includes(u.hostname))) return null;
    return u.href.replace(/\/+$/, "");
  } catch { return null; }
}

function cleanProperties(properties: Record<string, unknown>): Event["properties"] {
  const clean: Event["properties"] = {};
  for (const [key, value] of Object.entries(properties || {})) {
    if (typeof value === "string" && enums[key]?.has(value)) clean[key] = value;
    if (key === "app_version" && typeof value === "string" && /^\d{1,3}\.\d{1,3}\.\d{1,10}$/.test(value)) clean[key] = value;
    if (key === "duration_ms" && typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= 3600000) clean[key] = value;
  }
  return clean;
}

export function createTraction(storage: Storage, send: Sender, now = Date.now, context: () => Record<string, unknown> = () => ({})) {
  let config: TractionConfig | null = null;
  let queue: Event[] = [];
  let controller: AbortController | null = null;
  let epoch = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let retryMs = 1000;
  const persist = () => { try { storage.setItem(KEY, JSON.stringify({ endpoint: config?.endpoint, actor: config?.actor, events: queue })); } catch { /* capture never waits on analytics */ } };
  const schedule = (delay: number) => {
    if (!timer && config && queue.length) timer = setTimeout(() => { timer = null; void flush(); }, delay);
  };
  async function flush() {
    if (!config || controller || !queue.length) return;
    const current = config, generation = epoch;
    queue = queue.filter(e => e.ts >= now()/1000-7*86400);
    if (!queue.length) { persist(); return; }
    const batch = queue.slice(0, 50);
    controller = new AbortController();
    const abort = controller;
    const timeout = setTimeout(() => abort.abort(), 5000);
    try {
      const status = await send(current.endpoint + "/events", current.token, batch, abort.signal);
      if (generation !== epoch) return;
      if (status >= 200 && status < 300) {
        const ids = new Set(batch.map(e => e.event_id));
        queue = queue.filter(e => !ids.has(e.event_id)); retryMs = 1000; persist();
      } else if (status === 400 || status === 413) {
        // The server rejected this batch's content (e.g. a fast client clock or
        // an oversized payload). Drop it instead of resending it forever, and
        // keep sending later events.
        const ids = new Set(batch.map(e => e.event_id));
        queue = queue.filter(e => !ids.has(e.event_id)); persist();
      } else if (status === 401 || status === 403) {
        // Revoked or wrong credential: stop until the user saves settings again.
        config = null;
      } else retryMs = Math.min(retryMs * 2, 60000);
    } catch { if (generation === epoch) retryMs = Math.min(retryMs * 2, 60000); }
    finally {
      clearTimeout(timeout);
      if (generation === epoch) { controller = null; schedule(retryMs); }
    }
  }
  return {
    configure(next: TractionConfig) {
      const endpoint = safeTractionEndpoint(next.endpoint);
      const valid = next.enabled && endpoint && next.token.length >= 20 && /^[a-zA-Z0-9_-]{8,100}$/.test(next.actor);
      const normalized = valid ? { ...next, endpoint: endpoint! } : null;
      if (normalized && JSON.stringify(config) === JSON.stringify(normalized)) return;
      const changedOwner = config && normalized && (config.actor !== normalized.actor || config.endpoint !== normalized.endpoint);
      epoch++; controller?.abort(); controller = null;
      if (timer) clearTimeout(timer); timer = null;
      config = normalized;
      if (!config || changedOwner) { queue = []; try { storage.removeItem(KEY); } catch {} }
      else {
        try {
          const saved = JSON.parse(storage.getItem(KEY) || "null");
          queue = saved?.actor === config.actor && saved?.endpoint === config.endpoint && Array.isArray(saved.events)
            ? saved.events.filter((e: Event) => e && e.device_id === config!.actor && /^[a-zA-Z0-9_-]{8,100}$/.test(e.event_id) && Number.isFinite(e.ts) && e.ts >= now()/1000-7*86400 && e.ts <= now()/1000+60 && names.has(e.name))
              .slice(-100).map((e: Event) => ({ event_id: e.event_id, device_id: config!.actor, name: e.name, ts: e.ts, properties: cleanProperties(e.properties) })) : [];
          persist();
        } catch { queue = []; }
      }
      schedule(1000);
    },
    track(name: string, properties: Record<string, unknown> = {}) {
      if (!config || !names.has(name)) return false;
      const clean = cleanProperties({ ...context(), ...properties });
      queue.push({ event_id: crypto.randomUUID(), device_id: config.actor, name, ts: now()/1000, properties: clean });
      queue = queue.slice(-100); persist(); schedule(1000);
      return true;
    },
    flush,
    size: () => queue.length,
  };
}

let runtimeMetadata: Record<string, unknown> = {};
let metadataRequested = false;
const readyListeners = new Set<() => void>();
let client: ReturnType<typeof createTraction> | null = null;
export function configureTraction(config: TractionConfig) {
  if (typeof window === "undefined") return;
  try {
    if (config.enabled && !metadataRequested && "__TAURI_INTERNALS__" in window) {
      metadataRequested = true;
      void Promise.all([import("@tauri-apps/api/app"), import("@tauri-apps/plugin-os")])
        .then(async ([app, os]) => { runtimeMetadata = { app_version: await app.getVersion(), os: os.platform() }; })
        .catch(() => { /* Unknown version remains an explicit breakdown bucket. */ });
    }
    client ??= createTraction(window.localStorage, async (url, token, events, signal) => {
      const request = "__TAURI_INTERNALS__" in window ? (await import("@tauri-apps/plugin-http")).fetch : fetch;
      const response = await request(url, { method: "POST", headers: { "Content-Type": "application/json", "X-Ingest-Token": token }, body: JSON.stringify({ events }), signal });
      return response.status;
    }, Date.now, () => runtimeMetadata);
    client.configure(config);
    readyListeners.forEach(listener => listener());
  } catch { /* storage may be unavailable */ }
}
export function trackTraction(name: string, properties: Record<string, unknown> = {}) {
  try { client?.track(name, properties); } catch { /* never affect a product action */ }
}

/** View mount can precede async settings hydration; count it once after consent. */
export function observeTractionView(name: string, properties: Record<string, unknown>) {
  let tracked = false;
  const attempt = () => { try { if (!tracked) tracked = client?.track(name, properties) ?? false; } catch { /* Never affect view mounting. */ } };
  readyListeners.add(attempt);
  attempt();
  return () => { readyListeners.delete(attempt); };
}
