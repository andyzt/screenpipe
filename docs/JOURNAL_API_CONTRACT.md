# Journal and focus API contract (MVP)

<!-- doc-covers: crates/screenpipe-engine/src/routes/journal.rs, crates/screenpipe-engine/src/routes/focus.rs, crates/screenpipe-engine/src/focus, packages/screenpipe-mcp/src/index.ts, apps/screenpipe-app-tauri/components/journal -->
<!-- doc-verified: c26d61b14 -->
> **Current.** The shared contract between the engine routes, the desktop
> journal UI, the browser mock and the MCP tools. Change it here first; every
> consumer follows this file. Companion: `docs/MVP_IMPLEMENTATION_PLAN.md`.

Conventions: all timestamps are UTC RFC3339 strings (`2026-09-16T08:15:00Z`).
Minutes are `f64`, labelled estimates. Routes are behind the normal local API
auth (`Authorization: Bearer <local api key>`) and the history-access policy.
Errors use `{ "error": "<message>" }` with 400/404/500.

## Day boundary

A "day" starts at local 04:00 and ends at the next local 04:00. `date` is the
calendar date of that start. `today` resolves against the machine's local
time zone.

## GET /journal/day?date=YYYY-MM-DD

`date` defaults to today. Returns the day overview and all cards.

```json
{
  "date": "2026-09-16",
  "day_start": "2026-09-16T01:00:00Z",
  "day_end": "2026-09-17T01:00:00Z",
  "data_status": "ok | empty_but_recording | no_capture_in_range | not_recording | unknown",
  "generation": {
    "enabled": true,
    "provider_ready": true,
    "provider_message": null,
    "processing": false,
    "pending_windows": 0,
    "last_window_end_at": "2026-09-16T09:45:00Z",
    "last_error": null
  },
  "totals": {
    "active_minutes": 312.5,
    "wall_minutes": 480.0,
    "focus_minutes": 240.0,
    "distraction_minutes": 22.0,
    "idle_minutes": 60.0,
    "unknown_minutes": 0.0,
    "longest_focus_block_minutes": 84.0,
    "by_category": [
      { "category_id": "work", "name": "Work", "color_hex": "#B984FF", "minutes": 240.0 }
    ],
    "by_app": [
      { "name": "Code", "host": null, "minutes": 180.0 },
      { "name": "Chrome", "host": "github.com", "minutes": 60.0 }
    ]
  },
  "intentions": [ { "...": "Intention objects active during the day" } ],
  "activities": [ { "...": "ActivityCard, ascending by start_at" } ]
}
```

`focus_minutes` = minutes of cards whose category is not system/idle and not
named "Distraction", minus `distractions[]` sub-intervals.
`distraction_minutes` = minutes of "Distraction"-category cards plus
`distractions[]` sub-intervals inside other cards. `longest_focus_block_minutes`
merges focus intervals separated by less than 5 minutes.

`by_app` is every card's `apps` summed, top 12, descending by `minutes` with
ties broken by `name` then `host`.

### ActivityCard

```json
{
  "id": 42,
  "activity_key": "a1f3…",
  "start_at": "2026-09-16T08:15:00Z",
  "end_at": "2026-09-16T08:59:00Z",
  "active_minutes": 41.5,
  "state": "provisional | final",
  "producer": "deterministic-v1 | llm-v1 | idle-v1 | system",
  "title": "Investigated refresh-token failures in the auth service",
  "summary": "Read the failing test output, traced the retry path, patched the session store.",
  "detailed_summary": "[8:15 AM] - [8:40 AM]: … (nullable)",
  "category": {
    "id": "work", "name": "Work", "color_hex": "#B984FF",
    "is_system": false, "is_idle": false
  },
  "category_confidence": 0.86,
  "intention": { "id": 7, "title": "Ship auth fix" },
  "intention_relation": "supports_intention | other_work | break | possible_distraction | unknown",
  "relation_confidence": 0.8,
  "relation_reason": "Editor and test runner on the auth repo the whole time.",
  "app_primary": "code.visualstudio.com",
  "app_secondary": "github.com",
  "apps": [
    { "name": "Code", "host": null, "minutes": 30.0 },
    { "name": "Chrome", "host": "github.com", "minutes": 8.0 }
  ],
  "distractions": [
    { "start_at": "2026-09-16T08:30:00Z", "end_at": "2026-09-16T08:33:00Z",
      "title": "Checked X", "summary": "Scrolled the feed for three minutes." }
  ],
  "evidence_count": 18
}
```

### CardApp

```json
{ "name": "Chrome", "host": "github.com", "minutes": 60.0 }
```

`name` is the app the ledger attributed the time to (the interval's
`app_name`, falling back to its parent task's title). `host` is the browser
host when the ledger named that interval after a site, and `null` otherwise —
including for a browser interval the ledger named after a window or document
title. `minutes` is an estimate.

A card's `apps` is the top 6, descending by `minutes` with ties broken by
`name` then `host`. They come from the ledger intervals linked to the card
through `journal_activity_intervals`, clipped to the card's own span, and the
minutes are wall-clock overlap: a ledger interval is already gap-free, because
the segmenter emits a separate `unobserved` interval for every unobserved gap
and those are excluded here. Idle and system cards always report `[]`.

`intention`, `intention_relation`, `relation_confidence`, `relation_reason`
are `null` when no intention was active during the card.
`category` is never null; deterministic cards use the seeded fallback category
(first non-system category) with `category_confidence` 0.

## GET /journal/week?start=YYYY-MM-DD

`start` is the first day of the week and defaults to the Monday of the current
local journal week; the desktop client passes the Monday explicitly. Exactly
seven days are returned, starting at `start`, whatever weekday it names. Each
entry of `days` is the complete `GET /journal/day` body for that date, in
order, and each is clamped by the history-access policy on its own.

```json
{
  "start": "2026-09-14",
  "end": "2026-09-20",
  "days": [ { "...": "the full GET /journal/day response for each date, in order" } ],
  "totals": {
    "active_minutes": 1840.0,
    "wall_minutes": 2760.0,
    "focus_minutes": 1420.0,
    "distraction_minutes": 130.0,
    "idle_minutes": 310.0,
    "unknown_minutes": 0.0,
    "longest_focus_block_minutes": 96.0,
    "by_category": [ { "category_id": "work", "name": "Work", "color_hex": "#B984FF", "minutes": 1420.0 } ],
    "by_app": [ { "name": "Code", "host": null, "minutes": 900.0 } ]
  }
}
```

The week's minute totals are the seven days' totals summed;
`longest_focus_block_minutes` is the largest of the seven, not a block merged
across the 04:00 boundary. `by_app` is the top 12 across the week, summed from
the cards themselves (deduplicated by card id, because a card straddling local
04:00 is served by both of its days) rather than from the per-day top-12 lists.
An unparseable `start` is a 400.

## GET /journal/activities/{id}?include_evidence=true

Returns one ActivityCard plus:

```json
{
  "...": "ActivityCard fields",
  "interval_keys": ["ledger interval keys"],
  "evidence": [
    { "source_type": "frame | audio", "source_id": 12345,
      "occurred_at": "2026-09-16T08:16:10Z",
      "frame_id": 12345, "app_name": "Code", "window_title": "auth.rs — screenpipe",
      "browser_url": null }
  ]
}
```

`apps` is filled here exactly as it is on the day and week routes. Evidence is
sampled (at most 24 rows, evenly spaced) and ordered by `occurred_at`. `frame_id` is set for frame evidence so a client can call the
existing `GET /frames/{id}/context` or open the timeline at that moment.

## GET /journal/status

```json
{
  "enabled": true,
  "worker_running": true,
  "preset": { "id": "deepseek", "provider": "deepseek", "model": "deepseek/deepseek-v4-flash-vision-exp" },
  "provider_ready": true,
  "provider_message": null,
  "windows": { "pending": 0, "processing": 0, "done": 31, "failed": 1, "skipped_short": 2, "idle": 4 },
  "last_window_end_at": "2026-09-16T09:45:00Z",
  "last_run": { "kind": "cards", "ok": true, "latency_ms": 8400, "model": "…", "created_at": "…", "error": null },
  "prompt_version": "journal-cards-v1"
}
```

`preset.id` is the configured `journalAiPresetId` (null means "the default
preset"). `preset.provider` and `preset.model` come from the resolved preset and
are both null when it does not resolve — `provider_message` then says why.

## POST /journal/regenerate

Body `{ "date": "YYYY-MM-DD" }` resets that day's windows to `pending`
(cards are replaced as windows complete). Body `{ "activity_id": 4105 }`
resets only the windows that one card spans (`404` when the card is gone;
`date` is ignored). Rate-limited to one call per target — a day or a card —
per minute (`429`). Returns `{ "reset_windows": 12 }`.

## GET /journal/categories · PUT /journal/categories

```json
{ "categories": [
  { "id": "work", "name": "Work", "description": "Building, writing, analysing for your job or studies",
    "color_hex": "#B984FF", "is_system": false, "is_idle": false, "sort_order": 0 }
] }
```

PUT replaces the non-system list; system rows (`idle`, `system`) cannot be
removed or renamed. Seed on first run: Work, Personal, Distraction, Idle
(system, idle). Ids are lowercase slugs.

## Focus

### Intention

```json
{ "id": 7, "title": "Ship auth fix", "project": "screenpipe", "notes": null,
  "started_at": "2026-09-16T08:00:00Z", "ended_at": null, "source": "app | tray | mcp" }
```

- `GET /focus/intentions?active=true&limit=20` → `{ "intentions": [ … ] }`,
  newest first.
- `POST /focus/intentions` body `{ "title", "project"?, "notes"?, "source" }`
  → the new Intention. Ends any currently active intention first.
- `POST /focus/intentions/{id}/end` → the ended Intention (idempotent).

### GET /focus/status

```json
{
  "computed_at": "2026-09-16T09:46:00Z",
  "intention": { "…": "Intention or null" },
  "relation": "supports_intention | other_work | break | possible_distraction | unknown",
  "confidence": 0.7,
  "divergence_started_at": null,
  "divergence_minutes": 0.0,
  "dominant_task_title": "auth.rs — screenpipe",
  "dominant_app": "Code",
  "evidence_ok": true,
  "reason": "Same repository and editor as the intention's supporting cards."
}
```

With no active intention: `relation = "unknown"`, `reason = "no active
intention"`. With stalled or missing capture: `relation = "unknown"`,
`evidence_ok = false`.

Past the grace period the relation comes from the user's own AI preset
(prompt `focus-tail-v1`, audited in `journal_runs` with `kind = "tail"`), at
most one call per intention per 5 min. With no usable preset the engine falls
back to deterministic rules, which never exceed `confidence = 0.6`. A model
answer of `possible_distraction` below `0.6` is recorded as `unknown`.

### POST /focus/state/override

The user's own judgment about what they are doing now. Body:

```json
{ "relation": "other_work | break", "minutes": 30 }
```

`minutes` is optional, `1..=240`, default `30`. Any other `relation` (including
`supports_intention`, `possible_distraction` and `unknown`) is a 400, as is a
`minutes` outside the range. With no active intention: 400 `no active intention
to override`.

Effects: `focus_state` is written with that relation, `confidence = 1.0`,
`reason = "set by you"`, and `divergence_started_at = null`, so the divergence
timer stops. The task the row currently names (`dominant_task_title` +
`dominant_app`, normalised) is then suppressed for `minutes`: the detector
reports the chosen relation for it at confidence 1.0 and asks no classifier,
which also makes a nudge impossible for it (the nudge only reads
`possible_distraction`). The suppression is held in the engine process, keyed by
intention id + task key, and is deliberately not persisted — it expires with
`minutes` and a restart forgets it, like the nudge cooldown.

Returns the same body as `GET /focus/status`.

**Notification actions.** The nudge's two acknowledgement buttons carry this
route as an `api` action, which
`apps/screenpipe-app-tauri/lib/notifications/actions.ts` (`case "api"`) calls
with the local bearer key. The exact payloads:

```json
{ "id": "focus-this-is-fine", "label": "This is fine", "type": "api",
  "url": "/focus/state/override", "method": "POST",
  "body": { "relation": "other_work", "minutes": 30 } }
{ "id": "focus-take-a-break", "label": "Take a break", "type": "api",
  "url": "/focus/state/override", "method": "POST",
  "body": { "relation": "break", "minutes": 30 } }
```

The URL is relative on purpose: the desktop's `isLocalApiUrl` guard resolves it
against the local API and refuses anything off-box. The third button,
`focus-back-to-it`, stays a deep link to `screenpipe://home?section=journal`.

## Settings (desktop, frontend `extra` keys; engine reads the same store)

| Key | Default | Meaning |
|---|---|---|
| `journalEnabled` | `true` | Worker on/off |
| `journalAiPresetId` | unset → default preset | Preset used for card generation |
| `journalWorkProfile` | `{ "role": "", "projects": [], "notes": "" }` | Fed to prompts; `projects: [{ "name", "keywords": [] }]` |
| `focusNudgesEnabled` | `false` | Notification nudge layer |
| `focusGraceMinutes` | `10` | Divergence grace period before classification |

## MCP tools

| Tool | Input | Calls |
|---|---|---|
| `journal-day` | `{ date?: "YYYY-MM-DD" }` | `GET /journal/day` |
| `journal-activity` | `{ id: number, include_evidence?: boolean }` | `GET /journal/activities/{id}` |
| `focus-status` | `{}` | `GET /focus/status` |
| `set-intention` | `{ title?: string, project?: string, notes?: string, end?: boolean }` | `POST /focus/intentions` (or `…/end` when `end: true` and an intention is active) |
