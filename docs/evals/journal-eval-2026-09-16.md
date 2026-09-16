# Journal card eval — 2026-09-16

<!-- doc-covers: crates/screenpipe-engine/src/journal, crates/screenpipe-engine/src/focus -->
<!-- doc-verified: 435e48906 -->
> **Current.** Measured on 2026-09-16 against 435e48906 with
> `deepseek/deepseek-v4-flash` through the team gateway. Re-run the commands
> below before trusting any number here; a prompt change invalidates all of them.

## How this was produced

```bash
set -a; . apps/screenpipe-app-tauri/.env.ai.local; set +a
export DEEPSEEK_API_KEY="$SCREENPIPE_DEEPSEEK_API_KEY"

# labelled synthetic corpus (8 windows)
cargo run -p screenpipe-engine --bin journal-eval -- run \
  --fixtures crates/screenpipe-engine/tests/fixtures/journal \
  --labels   crates/screenpipe-engine/tests/fixtures/journal/synthetic.labels.json \
  --data-dir ~/.screenpipe-dev \
  --out docs/evals/journal-eval-2026-09-16.md

# real capture exported from this machine (4 windows, unlabelled)
cargo run -p screenpipe-engine --bin journal-eval -- run \
  --fixtures crates/screenpipe-engine/tests/fixtures/journal/live \
  --labels   crates/screenpipe-engine/tests/fixtures/journal/live/2026-09-16.labels.json \
  --data-dir ~/.screenpipe-dev
```

The live corpus itself came from:

```bash
cargo run -p screenpipe-engine --bin journal-eval -- export \
  --data-dir ~/.screenpipe-dev --date 2026-09-16 \
  --out crates/screenpipe-engine/tests/fixtures/journal/live --max-windows 10
```

`--price-in` / `--price-out` (USD per million tokens) turn the cost row into a
number; they were left out because the gateway's per-token price is not
published, so the harness reports `n/a` rather than inventing one.

## Verdict

**The category gate fails at 75 % against a bar of 80 %.** Coverage validity is
100 %, no window fell back to a `System` card, and the relation labels — the
ones the distraction feature rests on — are right 4 times out of 7. Three
specific misses are described under "What the misses were", and one of them
(the twenty-minute YouTube block folded into a Work card) is the one worth
fixing before the journal ships classification to users.

## Part 1 — labelled synthetic corpus

- run at: 2026-09-16T20:32:03.892263+00:00
- fixtures: `crates/screenpipe-engine/tests/fixtures/journal`
- labels: `crates/screenpipe-engine/tests/fixtures/journal/synthetic.labels.json`
- preset: `default (deepseek)`
- model: `deepseek/deepseek-v4-flash`
- prompt: `journal-cards-v2`
- intention: "Ship the journal MVP"

### Per fixture

| fixture | valid | cards | attempts | repairs | category (expected → actual) | relation (expected → actual) | latency | tokens | note |
|---|---|---|---|---|---|---|---|---|---|
| `call-and-gap` | yes | 2 | 1 | 1 | work → work ✓ | other_work → other_work ✓ | 3.9 s | 3827 | — |
| `deep-work` | yes | 1 | 1 | 0 | work → work ✓ | supports_intention → supports_intention ✓ | 3.7 s | 4027 | — |
| `distraction-block` | yes | 1 | 1 | 0 | distraction → work ✗ | possible_distraction → supports_intention ✗ | 2.9 s | 3862 | — |
| `focused-morning` | yes | 1 | 2 | 0 | work → work ✓ | other_work → supports_intention ✗ | 6.9 s | 8419 | — |
| `idle-heavy` | yes | 2 | 1 | 1 | idle → work ✗ | — → supports_intention | 3.1 s | 3799 | — |
| `meeting-audio` | yes | 1 | 1 | 0 | work → work ✓ | supports_intention → supports_intention ✓ | 3.1 s | 3862 | — |
| `research-spread` | yes | 1 | 1 | 0 | work → work ✓ | supports_intention → supports_intention ✓ | 3.8 s | 4078 | — |
| `split-attention` | yes | 1 | 1 | 0 | work → work ✓ | other_work → supports_intention ✗ | 7.8 s | 4007 | — |

### Summary

| metric | value |
|---|---|
| windows | 8 |
| coverage validity | 100.0 % (8/8) |
| error-card rate | 0.0 % (0/8) |
| attempts per window | 1.12 |
| repairs applied | 2 |
| cards written | 10 |
| category accuracy | 75.0 % (6/8) |
| relation accuracy | 57.1 % (4/7) |
| mean latency | 4.4 s |
| prompt / completion tokens | 33258 / 2623 |
| total tokens (mean per window) | 35881 (4485) |
| cost estimate | n/a (no price given) |

### Gates

| gate | measured | required | result |
|---|---|---|---|
| coverage validity | 100.0 % | ≥ 100 % | pass |
| category accuracy | 75.0 % | ≥ 80 % | FAIL |

**A gate failed — the harness exits non-zero.**

## Part 2 — real capture from this machine, unlabelled

Exported from `~/.screenpipe-dev` for 2026-09-16 (local 04:00 boundary), then
scrubbed: see "What was stripped from the live fixtures". Unlabelled, so only
validity, attempts, repairs, latency and tokens mean anything here.

- run at: 2026-09-16T20:32:59.931985+00:00
- fixtures: `crates/screenpipe-engine/tests/fixtures/journal/live`
- labels: `crates/screenpipe-engine/tests/fixtures/journal/live/2026-09-16.labels.json`
- preset: `default (deepseek)`
- model: `deepseek/deepseek-v4-flash`
- prompt: `journal-cards-v2`
- intention: none

### Per fixture

| fixture | valid | cards | attempts | repairs | category (expected → actual) | relation (expected → actual) | latency | tokens | note |
|---|---|---|---|---|---|---|---|---|---|
| `2026-09-16-2035` | yes | 1 | 1 | 0 | — → work | — | 4.6 s | 8071 | — |
| `2026-09-16-2123` | yes | 2 | 2 | 1 | — → work | — | 10.5 s | 17508 | — |
| `2026-09-16-2148` | yes | 2 | 2 | 1 | — → work | — | 5.3 s | 16836 | — |
| `2026-09-16-2217` | yes | 2 | 2 | 1 | — → work | — | 8.3 s | 17057 | — |

### Summary

| metric | value |
|---|---|
| windows | 4 |
| coverage validity | 100.0 % (4/4) |
| error-card rate | 0.0 % (0/4) |
| attempts per window | 1.75 |
| repairs applied | 3 |
| cards written | 7 |
| category accuracy | n/a (unlabelled) |
| relation accuracy | n/a (unlabelled) |
| mean latency | 7.2 s |
| prompt / completion tokens | 56727 / 2745 |
| total tokens (mean per window) | 59472 (14868) |
| cost estimate | n/a (no price given) |

### Gates

| gate | measured | required | result |
|---|---|---|---|
| coverage validity | 100.0 % | ≥ 100 % | pass |
| category accuracy | n/a | ≥ 80 % | pass |

All gates passed.

## What the misses were

| fixture | expected | got | reading |
|---|---|---|---|
| `distraction-block` | `distraction` / `possible_distraction` | `work` / `supports_intention` | Twenty minutes of YouTube Shorts covering the whole window were folded into the preceding coding card. The prompt's merge-by-default rule and its "detours shorter than 5 minutes go in `distractions[]`" instruction between them leave a 20-minute detour with nowhere to go: too long to be a detour, and merged anyway. **This is the finding to act on** — it is the exact case the roadmap's distraction bar exists for. |
| `idle-heavy` | `idle` | `work` | A Notion draft left open with two clicks in fifteen minutes. The model called it work. In production this window never reaches a provider: `journal::idle` gates it deterministically and writes an Idle card with no call at all. The eval bypasses that gate, so this miss costs the score without costing a user anything — but it does say the model cannot recognise absence from input counts alone. |
| `focused-morning`, `split-attention` | `other_work` | `supports_intention` | Both are real work in the same repo or product that is not the stated intention (the auth crate; a Q4 launch plan in Notion). The model treats same-project as same-intention. The labels are strict on purpose: the roadmap's whole point is that other work is not a failure, and collapsing the two makes `supports_intention` mean "was working". |

Two labels are worth arguing about (`idle-heavy` and `split-attention`); none
was moved to make a gate pass.

## Cost and budget

Synthetic windows cost ~4.5k tokens each. Real windows cost ~14.9k — more than
three times as much — and needed 1.75 provider calls each instead of 1.12. The
cause is in the next section, and it is the same cause.

Against the plan's budget of 32–64 windows/day at 4–8k tokens, the synthetic
number is inside it and the real number is roughly double the ceiling.

## What the live export exposed

`export` compiled four real windows and the activity ledger produced **224 to
1,715 intervals each** — 9,546 of about 9,900 intervals across the four windows
are one activity: an agent CLI running in iTerm2 whose window title carries an
animated spinner glyph (`◐`/`◑`). The glyph changes about once a second, the
ledger ranks window title above app for task identity, and so every second
becomes its own task and its own interval.

Consequences visible in the numbers above:

- `prompt::build_prompt` truncates at `MAX_EVIDENCE_CHARS` (14k) and appends
  `[observation list truncated: too many intervals]`, so the model never sees
  most of the window. Every live window failed its first attempt with a
  `TIME COVERAGE ERROR` naming 15–40 minutes it had no observations for, and
  passed only after the correction round.
- Tokens per window triple, because the truncated list is 14k characters of
  near-identical one-second lines rather than a few dozen real ones.
- A compiled window serialises to 100–740 KB, which is why the live corpus is
  four windows and not ten.

None of this is a journal bug — the journal survived it, at 100 % validity —
but it is an `activity_ledger` bug with a direct cost in provider tokens, and
it is filed under open issues in the plan's §11.

## What was stripped from the live fixtures

The export goes through the same redaction the worker uses, so the capture-side
guards had already replaced credentials in URLs (`[URL_WITH_CREDENTIALS]`) and
IP addresses (`[IP_ADDRESS]`) before anything was written. On top of that, and
by hand, before the fixtures were saved:

| what | how many | replacement |
|---|---|---|
| Telegram window titles (private group names, unread counts) | 104 | `Telegram — private chat` |
| text snippets naming a third party's product roadmap, an NDA channel, a personal presentation, or carrying a base64 image blob | 10 | dropped entirely |
| document paths and window titles of two presentations | 11 | `Presentation` / `/Users/user/Documents/redacted.pptx` |
| the machine's macOS username in paths | 3 | `user` |

A final scan over the saved files for the username, e-mail addresses, long
token-shaped strings and every term on the denylist returns zero hits. No API
key, password or access token was found in the export at any point.
