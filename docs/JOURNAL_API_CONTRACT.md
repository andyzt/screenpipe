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
  "activities": [ { "...": "ActivityCard, ascending by start_at" } ],
  "reviews": [ { "...": "ReviewRating rows touching the day, ascending by start_at" } ],
  "review_totals": { "focused_minutes": 120.0, "neutral_minutes": 30.0, "distracted_minutes": 15.0, "unrated_minutes": 147.5 },
  "recap": { "status": "none | ready | stale | failed", "generated_at": "2026-09-16T18:05:00Z" }
}
```

`review_totals` split `wall_minutes` of non-idle cards by the review rating
covering each minute. `recap.status` is `stale` when cards changed after the
recap was written (see *Daily recap*); the recap body itself is fetched
separately.

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
  "evidence_count": 18,
  "feedback": { "rating": "up | down", "note": "Wrong category — this was a meeting", "created_at": "2026-09-16T10:02:00Z" },
  "review": "focused | neutral | distracted | mixed"
}
```

`feedback` is `null` until the user rates the card (see *Card feedback*).
`review` is the user's timeline review rating over the card's span
(see *Review ratings*): the single rating when one covers ≥ 90 % of the
span, `mixed` in every other case where at least one rating touches it (two
or more ratings, or a single one covering less than 90 %), `null` when none
does. Idle and system cards always report `null` and clients hide the
controls there.

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

## GET /journal/week/dashboard?start=YYYY-MM-DD

The analytical view of a week, computed by the engine from the same cards
and ledger intervals as `GET /journal/week`, so the two never disagree.
`start` follows the same rules as `/journal/week`. Idle and system cards are
excluded from every section; a card straddling 04:00 is counted once, in the
day of its `start_at`. `compare` is the same computation for the seven days
before `start`, with `null` when that week has no cards.

```json
{
  "start": "2026-09-14",
  "end": "2026-09-20",
  "days": [
    { "date": "2026-09-14", "active_minutes": 312.0, "focus_minutes": 240.0, "distraction_minutes": 22.0,
      "longest_focus_block_minutes": 84.0, "switches": 37, "first_active_at": "2026-09-14T06:05:00Z",
      "last_active_at": "2026-09-14T16:40:00Z", "review": { "focused_minutes": 120.0, "neutral_minutes": 0.0, "distracted_minutes": 15.0 } }
  ],
  "totals": { "active_minutes": 1840.0, "focus_minutes": 1420.0, "distraction_minutes": 130.0,
              "longest_focus_block_minutes": 96.0, "switches": 212, "focus_share": 0.77 },
  "compare": { "active_minutes": 1700.0, "focus_minutes": 1200.0, "distraction_minutes": 180.0,
               "longest_focus_block_minutes": 71.0, "switches": 260, "focus_share": 0.71 },
  "categories": [
    { "category_id": "work", "name": "Work", "color_hex": "#B984FF", "minutes": 1420.0, "share": 0.77,
      "compare_minutes": 1200.0, "apps": [ { "name": "Code", "host": null, "minutes": 900.0 } ] }
  ],
  "apps": [ { "name": "Code", "host": null, "minutes": 900.0, "share": 0.49, "compare_minutes": 800.0,
              "category_id": "work" } ],
  "flows": [ { "category_id": "work", "name": "Code", "host": null, "minutes": 900.0 } ],
  "heatmap": {
    "hours": [4, 5, "…", 27],
    "cells": [ { "day": 0, "hour": 9, "active_minutes": 55.0, "focus_minutes": 50.0, "distraction_minutes": 5.0 } ]
  },
  "workflow": {
    "rows": [ { "name": "Code", "host": null, "cells": [ { "day": 0, "hour": 9, "minutes": 40.0 } ] } ]
  },
  "focus_blocks": [ { "start_at": "…", "end_at": "…", "minutes": 96.0, "title": "Auth fix", "category_id": "work" } ],
  "intentions": [ { "id": 7, "title": "Ship auth fix", "supporting_minutes": 300.0, "other_minutes": 40.0,
                    "distraction_minutes": 12.0 } ]
}
```

Definitions:

- `switches` — the number of ledger intervals in the day whose `app_name`
  differs from the previous interval's, unobserved gaps excluded.
- `focus_share` — `focus_minutes / active_minutes`, `null` when active is 0.
- `categories[].share` — minutes over all categorised minutes of the week
  (sums to 1); `apps[].share` — minutes over all app minutes of the week, not
  just the top 12. `compare_minutes` is `null` when the compare week has no
  cards and `0.0` when it has cards but not that category or app.
- `categories` — descending by minutes, every category with minutes > 0,
  each with its top 6 apps; the donut and the sankey are drawn from this.
- `apps` — top 12 across the week; `category_id` is the category the app spent
  most of its time in; the treemap is drawn from this.
- `flows` — category → app minutes for the top 12 apps and every category,
  the sankey links; the rest is folded into `name: "Other", host: null`.
- `heatmap.hours` — local journal hours 4…27 (27 = 03:00 next day); `day` is
  the index into `days`. Cells with 0 active minutes are omitted.
- `workflow.rows` — the top 8 apps, minutes per (day, hour) cell; omitted
  when 0. It shows *when* each app is used across the week.
- `focus_blocks` — the five longest focus blocks of the week, merging
  focus intervals separated by less than 5 minutes exactly as
  `longest_focus_block_minutes` does; `title` is the longest card inside.
- `intentions` — every intention active during the week with minutes of
  cards by `intention_relation` (`supports_intention` → supporting;
  `other_work`/`break` → other; `possible_distraction` → distraction).

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
  "preset": { "id": "deepseek", "provider": "deepseek", "model": "deepseek/deepseek-v4.1-flash" },
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

## Card feedback

`PUT /journal/activities/{id}/feedback` with body
`{ "rating": "up" | "down" | null, "note": "optional, ≤ 500 chars" }`.
`null` clears the rating. Returns the updated `ActivityCard`. `404` when the
card is gone. One row per card; a later PUT replaces it.

The row also snapshots `activity_key`, `day`, `start_at`, `end_at`, `title`,
`category_id`, `producer`, `prompt_version` so the rating survives a rewrite
of the card and can be exported as an eval label. When a window is rewritten,
feedback whose `activity_key` is no longer present stays in the table but is
not attached to any card.

`GET /journal/feedback?date=YYYY-MM-DD` → `{ "items": [ { "activity_id": 42,
"rating": "down", "note": "…", "title": "…", "start_at": "…", "end_at": "…",
"category_id": "work", "created_at": "…" } ] }`. Without `date`, the last 200
rows. `journal-eval export` writes rows with rating `down` into the live
fixture as `disputed: true` cards, and `up` rows as confirmed labels.

## Review ratings

The user marks a span of the day as `focused`, `neutral` or `distracted`.
Ratings never overlap: a new one splits or replaces what it covers, exactly
as a calendar block would.

```json
{ "id": 3, "start_at": "2026-09-16T09:00:00Z", "end_at": "2026-09-16T10:30:00Z",
  "rating": "focused | neutral | distracted", "source": "app | mcp", "created_at": "…", "updated_at": "…" }
```

`GET /journal/reviews?date=YYYY-MM-DD` → `{ "items": [ ReviewRating… ] }`
ascending by `start_at`, clipped to the day.

`PUT /journal/reviews` body `{ "start_at", "end_at", "rating" | null, "source"? }`.
`rating: null` clears the span. `source` is `app` (default) or `mcp`.
`start_at < end_at`, span ≤ 24 h, else `400`.
Returns `{ "items": [...] }` for the journal day containing `start_at`
after the write.

Effects: (1) `ActivityCard.review` and `review_totals` in the day response;
(2) the compiler adds the ratings that overlap a window to the evidence as a
`User review` block ("09:00–10:30 marked focused") and the card prompt
instructs the model to treat them as ground truth for the relation and the
category (a span marked `distracted` is a Distraction card or a
`distractions[]` sub-interval; `focused` is never `possible_distraction`);
(3) `journal-eval export` writes ratings as expected labels for the covered
cards. Rating a span does not itself trigger a rewrite; the user regenerates
the card (per-card `POST /journal/regenerate`) when they want the text to
follow.

## Daily recap

One LLM call over the day's final cards; stored per day, regenerated on
demand.

```json
{
  "date": "2026-09-16",
  "status": "none | ready | stale | failed",
  "generated_at": "2026-09-16T18:05:00Z",
  "summary": "A focused morning on the auth fix, an afternoon split between review and email.",
  "done": [ "Shipped the refresh-token fix (PR #412)", "Reviewed two PRs for the billing team" ],
  "next": [ "Re-run the flaky session test on CI", "Reply to the design thread" ],
  "focus_note": "One 20-minute detour to news around 15:00.",
  "source_cards": 11,
  "model": "deepseek/deepseek-v4-flash",
  "prompt_version": "journal-recap-v1",
  "error": null,
  "markdown": "## 2026-09-16\n\n**Done**\n- …\n\n**Next**\n- …"
}
```

`GET /journal/recap?date=YYYY-MM-DD` returns the stored recap, or
`status: "none"` with `done`/`next` empty, strings `""`, `source_cards` 0 and
`generated_at`/`model`/`prompt_version`/`error` null. `stale` means a card in the day was
written after `generated_at`. `failed` carries `error` and the previous
successful body when one exists.

`POST /journal/recap/generate` body `{ "date": "YYYY-MM-DD" }` runs the call
synchronously (same client, model, timeout and language as cards; no
`thinking`) and returns the recap. `409` when the day has no final cards,
`503` when no provider is ready, `429` more than once per day per minute.
A provider or validation failure is a `200` with `status: "failed"`, `error`
set and the previous successful body preserved, so the client can still show
it. `focus_note` is always a string (`""` when absent). Over-long bullet lists
are clamped to the limits and bullets over 140 chars are shortened at a word
boundary; an empty `summary` or `done` fails the run.
Output rules: `done` 1–6 bullets and `next` 0–4 bullets, each ≤ 140 chars, in
the UI language; `next` only from evidence in the cards (open work, unfinished
threads) — never invented tasks. `markdown` is rendered by the engine so the
app and the MCP tool copy the identical text.

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
| `journal-recap` | `{ date?: "YYYY-MM-DD", regenerate?: boolean }` | `GET /journal/recap`; `POST /journal/recap/generate` when `regenerate` or `status` is `none`/`stale` |
| `journal-week` | `{ start?: "YYYY-MM-DD" }` | `GET /journal/week/dashboard`, formatted as a text digest (totals with week-over-week deltas, top categories/apps, longest focus blocks, per-day table) |
| `journal-review` | `{ start: ISO-8601, end: ISO-8601, rating: "focused" \| "neutral" \| "distracted" \| null }` | `PUT /journal/reviews` with `source: "mcp"` |
