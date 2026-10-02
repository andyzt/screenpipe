# Upstream sync, licence and telemetry decisions

<!-- doc-covers: none -->
<!-- doc-verified: 39ebfb765 -->
> **Current.** Process doc, written 2026-09-24 and revised 2026-10-02 on top of
> `39ebfb765`. It records
> the three decisions that gate any build leaving the team: how this fork
> follows upstream, under which licence it may be used, and what it reports
> home. Plan item 6 in `stats/QUALITY_DESIGN_RESEARCH.md`, section 4.

## 1. Fork base

| Fact | Value | How verified |
|---|---|---|
| Merge base with upstream | `1494836d8` — `fix(app): authenticate referral lookup to prevent sign-out (#6959)`, 2026-09-08 | `git log --oneline -1 1494836d8` |
| App version at the base / now | `2.7.26` / `2.7.28` (`apps/screenpipe-app-tauri/src-tauri/Cargo.toml`) | `git show 1494836d8:apps/screenpipe-app-tauri/src-tauri/Cargo.toml` |
| Fork commits on top of the base | 55 (15 first-parent) as of 2026-09-24 | `git rev-list --count 1494836d8..HEAD` |
| Licence boundary | Relicense commit `f390fa9d2` (2026-06-10) is an ancestor of the base; every commit after `892199f74` (2026-06-10), including all of ours, is under the Screenpipe Commercial License | `git merge-base --is-ancestor f390fa9d2 1494836d8` |
| Remotes | `origin` only, `https://github.com/andyzt/screenpipe.git`; no `upstream` remote | `git remote -v` |

### Adding the upstream remote (documented, not executed)

Nobody has added the remote yet. When the first cherry-pick is due:

```bash
git remote add upstream https://github.com/screenpipe/screenpipe.git
git fetch upstream main --no-tags
git merge-base upstream/main HEAD   # must print 1494836d8...
```

`--no-tags` is deliberate: upstream's `app-v*` and `app-beta-v*` tags must not
land in a clone from which anyone pushes. Tags of that shape are publication
events (`docs/human-only-app-publication.md`).

## 2. Policy: cherry-pick measured fixes, never merge upstream main

Upstream merged roughly 203 PRs in September 2026 alone (v2.7.28 → v2.7.68),
dominated by the SQLite+Parquet storage migration that reshapes the tables the
journal and the disk-reserve logic sit on. A merge would import that, their
build-time localization, and their telemetry defaults in one step, and would
make the licence question (section 4) impossible to reason about per change.
So:

- `upstream/main` is never merged or rebased onto. Each upstream change comes in
  as one `git cherry-pick -x` (or a hand port when the paths diverged) with the
  upstream PR number in the commit message.
- A cherry-pick is accepted only with the measurement from the PR body
  reproduced locally (checklist below). "Upstream says it is faster" is not a
  measurement.
- Rejections are recorded in the hold list with the reason, so the question is
  not reopened every sync.

### Pull list

None of these PR numbers exist in local history (`git log --oneline --all`
shows no `#7xxx` merge for any of them). The one-line descriptions come from
the September 2026 upstream survey in `stats/QUALITY_DESIGN_RESEARCH.md`,
section 2; verify each against the PR body at sync time before relying on it.

| Upstream PR | What it claims | Why we want it | What to measure here |
|---|---|---|---|
| #7218 | `sysinfo` refresh in analytics; partially closes the IOHID event leak (#7181) | Battery on the always-on path | The fork's `cpu.rs:242` / `idle_detector.rs:29` construct `System` once and the sampler reuses it, so the "80 IOHID events/min" may not reproduce. Measure first with `log stream` / Instruments; port only if it does. Pinned `sysinfo 0.29` has no `RefreshKind::nothing()` |
| #7133 | Warm ScreenCaptureKit streams stop on pause | Pause must mean zero capture cost | `ps`/energy impact of the app process while paused, before and after |
| #7194 | WAL backlog no longer restarts the engine | The fork still has the restart path (`write_queue.rs:484-562`); a restart is a data-loss event | Reproduce the backlog (large journal import or slow disk), confirm no restart in the engine log, and that the backlog drains |
| #7111 | `/activity-summary` frames-first query | Upstream reports 20–32 s → sub-second | Time `GET /activity-summary` for a full day on a real database before and after; the journal worker calls it |
| #7180 (with #7025) | Onboarding restart / retry | **Evaluate.** Overlaps our first-run verdict work (plan items 11–12) | Only if it does not conflict with `lib/journal/capture-state.ts` |
| #7167 | App parsers: field accuracy 16/33 → 33/33 | **Evaluate.** Touches `screenpipe-a11y` parsing on the hot path | Rerun the parser fixtures; hot-path review (no per-frame allocation) |

### Hold list

| Upstream PR | What | Why held |
|---|---|---|
| #7000 and its fix series (2026-09-14 to 09-23, ~25 PRs) | SQLite+Parquet storage migration | Journal queries and the disk-reserve semantics depend on the tables it rewrites; `docs/JOURNAL_API_CONTRACT.md` would have to be re-verified end to end. Not this quarter (`stats/QUALITY_DESIGN_RESEARCH.md`, section 7) |
| #7137 | Build-time General Translation pipeline | Incompatible with `lib/i18n/{en,ru}.json`, which is our localization source of truth |
| #7078 | Remote support logs default on | Reverses this fork's telemetry stance (section 5) |
| #7073, #7227 | Windows pair | Only when a Windows tester exists; nobody can validate them today |

### Checklist for each cherry-pick

1. `git cherry-pick -x <sha>` (or port by hand; say so in the message with the
   upstream PR number).
2. Reproduce the measurement method from the upstream PR body locally and put
   the before/after numbers in the PR body. No numbers, no merge.
3. If the change touches `crates/screenpipe-engine/src/journal/`,
   `activity_ledger.rs` or `db/journal.rs`: run `journal-eval` and put the
   eval table in the PR body (see `docs/evals/` for the report shape).
4. If the change touches capture (`screenpipe-screen`, `-capture`, `-a11y`),
   audio callbacks or the DB writer: hot-path review per `AGENTS.md`
   ("Hot paths"), and say in the PR whether allocation or blocking changed.
5. Licence: confirm the commit is post-`892199f74` upstream code (it will be)
   and that section 4 below is settled before the build containing it leaves
   the team.
6. Telemetry: confirm the cherry-pick does not reintroduce an upstream DSN,
   PostHog key or enabled-by-default sender (section 5 inventory).
7. Add a row to the sync log (section 6).

## 3. Licence: what `LICENSE.md` says

Read the file itself; this is a summary of the parts that bind us.

- **Section 2, Free Use.** No-charge use, copying, modification and running
  for personal non-commercial use; non-profit, educational or research use;
  and "evaluation, development, and testing for up to seven (7) days, at any
  organization size". The seven-day cap means "internal evaluation" is not an
  open-ended option for a team.
- **Section 4, Commercial Use requires a paid licence.** Any Commercial Use
  (business or production environment; revenue-supporting; by or for a
  for-profit entity after the evaluation period, per section 1) needs a
  separate paid commercial licence, regardless of size, headcount, revenue or
  funding.
- **Section 5, Prohibited without a commercial licence.** Selling,
  sublicensing or distributing the work in a commercial product or service;
  hosting it for third parties; embedding it in a product offered to
  customers; using it to build or operate a competing product or service.
- **Section 6, Ownership.** The licensor retains all right, title and
  interest, "including any modifications or patches you make". Our commits on
  top of the base do not change who owns the work.
- Section 3 says official prebuilt builds are governed by the Terms of Service
  and subscriptions, not by this licence; section 7 terminates rights
  automatically on any use outside the terms. Versions previously released
  under MIT remain available under MIT.

### The three options (`docs/MVP_IMPLEMENTATION_PLAN.md`, section 1)

1. Fork the last MIT snapshot (everything up to and including `892199f74`,
   2026-06-10) and re-apply our work on it. Loses three months of upstream
   fixes and the base this fork was built on.
2. Buy a commercial licence from the licensor
   (https://screenpi.pe/commercial-license).
3. Reuse only MIT-era crates and rebuild the rest.

**Decision: pending — owner: product owner; deadline: before any build leaves
the team.** Until it is signed, builds stay on the machines of the people who
made them and within the section 2 free-use terms.

```
Decision: ______________________   Option: 1 / 2 / 3
Signed:   ______________________   Date: __________
```

## 4. Telemetry

### What was found (2026-09-24)

Every sender in the tree pointed at upstream's own projects. Nothing here was
ours to receive.

| Location | What | Gate | Status |
|---|---|---|---|
| `packages/screenpipe-mcp/src/telemetry.ts:12` | `DEFAULT_MCP_SENTRY_DSN` = upstream Sentry org `o4505591122886656`, project `4510761360949248` (same DSN as the engine binary below) | `SCREENPIPE_DISABLE_TELEMETRY`, `SCREENPIPE_TELEMETRY_DISABLED`, `SCREENPIPE_MCP_SENTRY_DISABLED`, `SENTRY_DISABLED`; override with `SCREENPIPE_MCP_SENTRY_DSN` / `SENTRY_DSN` | **Changed**: default is now `""`, so `initMcpTelemetry` returns false and every capture is a no-op unless a DSN env var is set |
| `apps/screenpipe-app-tauri/lib/hooks/use-settings.tsx` (`createDefaultSettingsObject`, `analyticsEnabled`) | Frontend default `true` | Settings toggle; `is_telemetry_disabled_by_env` from Rust | **Changed**: default is now `false` for new installs. Existing installs keep the value already in `store.bin`; nothing is backfilled |
| `apps/screenpipe-app-tauri/app/providers.tsx:95-96` | `posthog.init("phc_z7FZ…", api_host "https://us.i.posthog.com")` | capture follows `analyticsEnabled` + env; flag requests do not (see Still open) | Capture now opted out unless explicitly enabled |
| `apps/screenpipe-app-tauri/src-tauri/src/main.rs:702` | App Sentry DSN, upstream org, project `4510761355116544` | `store_bool("analyticsEnabled")`; a missing key means enabled (`unwrap_or(false)` on "disabled"); `telemetry_disabled_by_env` (CI/`SCREENPIPE_DISABLE_TELEMETRY`) | Unchanged (Rust) |
| `apps/screenpipe-app-tauri/src-tauri/src/main.rs:1423`, `src-tauri/src/analytics.rs:122` | PostHog key `phc_z7FZ…`, host `https://us.i.posthog.com` | `store.recording.analytics_enabled`; serde default `true` when the key is absent (`crates/screenpipe-config/src/recording.rs:982`) | Unchanged (Rust) |
| `crates/screenpipe-engine/src/bin/screenpipe-engine.rs:513` | Engine/CLI Sentry DSN (same as the old MCP default) | Engine telemetry flags | Unchanged (Rust) |
| `crates/screenpipe-engine/src/analytics.rs:18`, `resource_monitor.rs:629` | Engine PostHog key (same `phc_z7FZ…`) | stored `analytics_enabled` | Off by default now that the stored default is off |
| `packages/cli/screenpipe/lib/telemetry.js:9`, `scripts/postinstall.{js,sh}`, `packages/sdk/session/telemetry-core.js:21`, `packages/sdk/tauri/rust/src/telemetry.rs:31` | Same PostHog key | Package-specific | Unchanged (outside scope) |
| `packages/screenpipe-mcp/src/team-config.ts:19` | `HOSTED_TEAM_API = https://screenpi.pe/api/enterprise/v1` | Used only by `team-*` tools with an enterprise token | Not telemetry; unchanged |
| `packages/screenpipe-mcp/src/index.ts:1544` | `POST /internal/telemetry/mcp-value` | Local engine on `localhost:3030` | Not upstream; unchanged |
| `apps/screenpipe-app-tauri/lib/analytics-id.ts`, `lib/auth-guard.tsx:309` | Per-install id key; `api_host` is an event property of the local API, not an endpoint | — | No endpoint; unchanged |

### What this fork changed (merge review, 2026-10-02)

| Sender | Before | Now |
|---|---|---|
| MCP Sentry (`packages/screenpipe-mcp/src/telemetry.ts`) | upstream DSN by default | no default DSN; sends only if `SCREENPIPE_MCP_SENTRY_DSN` / `SENTRY_DSN` is set |
| Frontend default (`use-settings.tsx`) | `analyticsEnabled: true` | `false` for a new install |
| Stored app default (`src-tauri/src/store.rs`, `SettingsStore::default`) | inherited `analytics_enabled: true` from `RecordingSettings::default()` and wrote it to `store.bin` on first launch, before the webview loaded | `analytics_enabled: false`; the CLI sets its own value and is unaffected |
| App Sentry gate (`src-tauri/src/main.rs`) | a missing key meant enabled, so the very first launch reported | a missing key means not opted in |
| Webview PostHog capture (`app/providers.tsx`) | no cached preference → `opt_in_capturing()`, sending `$opt_in` on a fresh install | only an explicit cached `true` opts in; everything else opts out |
| Engine PostHog (`crates/screenpipe-engine/src/analytics.rs:18`, `resource_monitor.rs`) | gated on the stored setting | unchanged code; now off by default because the stored default is off |

Existing installs that already stored `analyticsEnabled: true` keep it; nothing
is backfilled.

### Still open

- **PostHog feature flags.** `posthog.init` runs in every webview regardless of
  the setting; opting out stops capture, not flag requests. An opted-out
  install still asks upstream's project for flags (trial activation,
  `pipe_advisories`, experimental features, ACP rollout) with its bootstrapped
  id, and upstream can change fork behaviour through them. Decide before a
  build leaves the team: `advanced_disable_flags`, or remove the init.
- **Enterprise policy default.** `lib/managed-settings.ts:211` still defaults
  the managed analytics policy to `true`.
- **Our own sender.** The fork's opt-in Traction client
  (`lib/analytics/traction.ts`) is off by default and only sends after an
  explicit per-install credential is saved in Settings; its endpoint is
  prefilled in `components/settings/traction-settings.tsx`.

### Before any build leaves the team

1. Remove or replace the three Sentry DSNs and the PostHog key listed above,
   and decide the feature-flag question.
2. Add the build-script grep from the plan's measure: the build fails if any
   of `o4505591122886656`, `phc_z7FZXE8vmXtdTQ78LMy3j1BQWW4zP6PGDUP46rgcdnb`,
   or `analyticsEnabled: true` / `analytics_enabled: true` as an app default is
   present in the shipped tree. Not implemented yet.
3. Record the outcome in the sync log.

## 5. Sync log

| Date | Upstream PR | Local commit | Measurement | Notes |
|---|---|---|---|---|
| — | — | — | — | No cherry-picks yet; `upstream` remote not added |
