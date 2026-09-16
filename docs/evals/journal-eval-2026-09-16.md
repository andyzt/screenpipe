# Journal card eval — 2026-09-16

<!-- doc-covers: crates/screenpipe-engine/src/journal, crates/screenpipe-engine/src/focus -->
<!-- doc-verified: 3959328d6 -->
> **Current: the second run**, at the bottom of this file — `journal-cards-v3`
> plus the activity-ledger title fix, measured on 2026-09-16 against 3959328d6
> with those two changes in the working tree, on `deepseek/deepseek-v4-flash`
> through the team gateway.
>
> The first run below is kept because it is what the two fixes were written
> against; its numbers are `journal-cards-v2` and no longer describe the code.
> Re-run the commands before trusting any number here: a prompt change
> invalidates all of them, and this model's output is not deterministic — every
> figure below is one draw from a distribution that is also reported.

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

## First run — verdict

**The category gate fails at 75 % against a bar of 80 %.** Coverage validity is
100 %, no window fell back to a `System` card, and the relation labels — the
ones the distraction feature rests on — are right 4 times out of 7. Three
specific misses are described under "What the misses were", and one of them
(the twenty-minute YouTube block folded into a Work card) is the one worth
fixing before the journal ships classification to users.

## First run, part 1 — labelled synthetic corpus

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

## First run, part 2 — real capture from this machine, unlabelled

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

## First run — what the misses were

| fixture | expected | got | reading |
|---|---|---|---|
| `distraction-block` | `distraction` / `possible_distraction` | `work` / `supports_intention` | Twenty minutes of YouTube Shorts covering the whole window were folded into the preceding coding card. The prompt's merge-by-default rule and its "detours shorter than 5 minutes go in `distractions[]`" instruction between them leave a 20-minute detour with nowhere to go: too long to be a detour, and merged anyway. **This is the finding to act on** — it is the exact case the roadmap's distraction bar exists for. |
| `idle-heavy` | `idle` | `work` | A Notion draft left open with two clicks in fifteen minutes. The model called it work. In production this window never reaches a provider: `journal::idle` gates it deterministically and writes an Idle card with no call at all. The eval bypasses that gate, so this miss costs the score without costing a user anything — but it does say the model cannot recognise absence from input counts alone. |
| `focused-morning`, `split-attention` | `other_work` | `supports_intention` | Both are real work in the same repo or product that is not the stated intention (the auth crate; a Q4 launch plan in Notion). The model treats same-project as same-intention. The labels are strict on purpose: the roadmap's whole point is that other work is not a failure, and collapsing the two makes `supports_intention` mean "was working". |

Two labels are worth arguing about (`idle-heavy` and `split-attention`); none
was moved to make a gate pass.

## First run — cost and budget

Synthetic windows cost ~4.5k tokens each. Real windows cost ~14.9k — more than
three times as much — and needed 1.75 provider calls each instead of 1.12. The
cause is in the next section, and it is the same cause.

Against the plan's budget of 32–64 windows/day at 4–8k tokens, the synthetic
number is inside it and the real number is roughly double the ceiling.

## First run — what the live export exposed

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

## First run — what was stripped from the live fixtures

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

---

# Second run — `journal-cards-v3` and the ledger title fix

Both findings of the first run were acted on. Nothing in the labels was
touched; `synthetic.labels.json` is byte-for-byte what it was.

## What changed

| file | change |
|---|---|
| `crates/screenpipe-engine/src/journal/prompt.rs` | `PROMPT_VERSION` → `journal-cards-v3`. Three length tiers for an unrelated stretch, a fresh-mode exception that splits a long one out, sharper `other_work` vs `supports_intention`, and the same rules in the correction block. |
| `crates/screenpipe-engine/src/activity_ledger.rs` | `normalize_title` strips spinner/status decoration before a window title becomes a task key. |
| `crates/screenpipe-engine/src/journal/compile.rs` | the compiled `window_title` is normalized the same way, so `most_common` votes on titles instead of on spinner frames. |
| `crates/screenpipe-engine/tests/fixtures/journal/live/` | re-exported from the same day and re-scrubbed. |

## Finding 1 — the rules that replaced "merge by default"

Verbatim, from `prompt.rs`.

**Card structure**, replacing steps 3–4 and the closing line:

```
3. Is there a brief unrelated detour (<5 min)? → Log it in distractions[], keep the card going.
4. Is the unrelated stretch 5–10 minutes, with the same task running before it and resumed after it? → Still a detour: distractions[], keep the card going.
5. Is the unrelated stretch 10 minutes or longer? → Its own card, with its own category. Give it that card even when it lands under 15 minutes: ten minutes of YouTube is a ten-minute card, not a footnote on a coding card.
6. Has the focus genuinely shifted for 10+ minutes? → New card.
```

```
DEFAULT TO MERGING WITHIN ONE WORK STREAM. Two 15-minute cards about the same work stream should almost never exist, and if you're unsure whether to merge two parts of one work stream, merge. The default stops at the edge of the stream: an unrelated block of 10 minutes or more is never merged into the work around it, however short its own card turns out to be.
```

**Distractions**, replacing the old "<5 min" definition:

```
HARD RULE: no entry in distractions[] may last 10 minutes or more. Ever. distractions[] is only for interruptions that stay INSIDE a card, and length decides, in three tiers:

- Under 5 minutes and unrelated → a distraction. Checking a feed for 2 minutes while debugging goes in distractions[] and the card keeps running.
- 5–10 minutes and unrelated → still a distraction, but only when it sits inside one task: the same work was running before it and resumes after it. If the work does not resume, it is a card.
- 10 minutes or more and unrelated → never a distraction. It is its own card with its own category, even when that card is shorter than the usual 15–60 minutes. Twenty minutes of YouTube in the middle of a coding session is a twenty-minute card, not a line in the coding card's summary.

Pick that card's category by matching what the block actually was against the category descriptions above, not by what is nearby: a feed, a video, a game, endless scrolling → the label whose description covers feeds, videos and detours — watching videos is not an errand and does not belong under a personal/admin label; a real errand, admin or a message thread → the personal label; work that is simply different work → the work label.
```

**Fresh segment mode.** This block, not the card-structure rules, is what
decided the `distraction-block` miss: the old wording said "Return exactly ONE
new card covering the entire supplied observation span, regardless of internal
activity or goal changes", and that sentence beat every other rule in the
prompt. It now reads:

```
FRESH SEGMENT MODE — SPLIT ONLY ON A LONG UNRELATED BLOCK:
No previous card belongs to this batch's contiguous source-evidence segment. Nearby history separated by a genuine gap is left untouched. Build this batch's cards in two steps.

Step 1. Find every stretch of 10 minutes or more that is unrelated to the rest of the span — leisure, entertainment, social, or an activity of a different kind altogether. Each one is its own card, with its own start time, end time, title and category, even when that card is shorter than 15 minutes. Twenty minutes of YouTube inside a coding session is a twenty-minute card categorized as what it was: not a distractions[] entry, not a clause in a Work card's title, not a line in its detailed summary. This is the one split this mode makes, and it is not optional.

Step 2. Each remaining contiguous stretch is ONE card, regardless of internal activity or goal changes. Do not split it by topic, project, goal or app. Title and categorize its dominant activity, and put unrelated activity shorter than 10 minutes in distractions[], the summary and the detailed summary.

So: exactly one card when nothing unrelated ran for 10 minutes — the normal case — and one extra card for each long unrelated block when something did. Never return an empty cards array: every batch of observations produces at least one card. These cards are provisional; later sliding-window passes may split them once each resulting activity has at least 10 minutes of supporting evidence. Apart from step 1, this rule overrides all other coherence and splitting guidance for this call.
```

**Ongoing segmentation**, one sentence replaced:

```
Absorb unrelated interruptions under five minutes, and unrelated 5–9-minute episodes that sit inside one task which resumes afterwards. An unrelated stretch of ten minutes or more is always its own card with its own category and is never absorbed, even if that card lands at the ten-minute floor. A 5–9-minute episode that is not absorbed may borrow the minimum neighbouring minutes to reach ten if the neighbouring cards remain at least ten.
```

**The correction block**, same rule, both modes:

```
- This is a fresh segment. Split out every unrelated stretch of 10 minutes or more (leisure, entertainment, social, or an activity of a different kind) as its own card with its own category, even if that card is under 15 minutes; return exactly ONE card for each remaining contiguous stretch of the supplied observation span. Nothing shorter than 10 minutes is ever split out: measure it, then leave it inside the card it interrupted.
- The duration rule overrides semantic purity below ten minutes. When unrelated activity shorter than ten minutes must be merged, title and categorize the dominant activity and move the shorter activity into distractions[], the summary and the detailed summary.
- An unrelated stretch of ten minutes or more is never merged away: it keeps its own card and its own category, even when that card sits at the ten-minute floor. Merging by default applies inside one work stream only.
```

**The last thing the model reads**, appended to the output contract:

```
LAST CHECK, before you return: is any entry in distractions[] 10 minutes or longer, or does any card's text describe an unrelated block of 10 minutes or more? Then that block is not a distraction. Remove it from distractions[], give it its own card with its own start time, end time, title and category, and move the neighbouring card's boundary to meet it. Do this even in fresh segment mode, and even when the new card is under 15 minutes.
```

**Relation rules** (`RELATION_RULES`, shared verbatim with the live tail
classifier), the `other_work` / `supports_intention` pair:

```
- other_work: real work or a real errand that is simply not the stated intention. Other work is NOT a distraction. Use this whenever the activity is purposeful. Same project, different task is other_work: the intention names a task, not a repository, a product, an employer or a tool. Another crate, another feature, another document, the quarterly plan, a different ticket in the same tracker — all other_work, however real the work is.
- supports_intention: the activity advances the stated task itself, including reading documentation, searching an error message, or reviewing material that task needs. Research that serves the task supports it: the documentation for the API being used, the error being debugged, the document being written. Background reading about the field, the market or tooling in general is not that — it is other_work. Ask: would finishing this move the stated task forward? If the honest answer is "no, but it is still work", the answer is other_work, not supports_intention. Do not stretch the intention to cover whatever the person happened to be working on.
```

`possible_distraction` gained `Check break first: a timer, the weather, a walk
away from the desk is a break, however long it ran.` — a first attempt put the
card-length reasoning in the shared rules instead, and the tail classifier then
called a ten-minute coffee break a distraction, which is the one error class
its gate exists to prevent. Card-length reasoning now lives in the journal's
own intention section:

```
The title, and the notes when there are any, are the boundary of this intention. Work they do not cover is other_work, even in the same repository, the same product or the same company. A card whose own activity is leisure, social or entertainment and which ran for 10 minutes or more is possible_distraction, not a footnote on the work around it.
```

## Second run, part 1 — labelled synthetic corpus

```bash
set -a; . apps/screenpipe-app-tauri/.env.ai.local; set +a
export DEEPSEEK_API_KEY="$SCREENPIPE_DEEPSEEK_API_KEY"

cargo run -p screenpipe-engine --bin journal-eval -- run \
  --fixtures crates/screenpipe-engine/tests/fixtures/journal \
  --labels   crates/screenpipe-engine/tests/fixtures/journal/synthetic.labels.json \
  --data-dir ~/.screenpipe-dev
```

- run at: 2026-09-16T22:21:51.321758+00:00
- prompt: `journal-cards-v3`, model `deepseek/deepseek-v4-flash`, preset `default (deepseek)`
- intention: "Ship the journal MVP"

| fixture | valid | cards | attempts | repairs | category (expected → actual) | relation (expected → actual) | latency | tokens |
|---|---|---|---|---|---|---|---|---|
| `call-and-gap` | yes | 2 | 1 | 0 | work → work ✓ | other_work → other_work ✓ | 4.5 s | 5053 |
| `deep-work` | yes | 1 | 1 | 0 | work → work ✓ | supports_intention → supports_intention ✓ | 3.6 s | 5105 |
| `distraction-block` | yes | 2 | 2 | 0 | distraction → **distraction ✓** | possible_distraction → **possible_distraction ✓** | 7.9 s | 10715 |
| `focused-morning` | yes | 1 | 1 | 0 | work → work ✓ | other_work → supports_intention ✗ | 3.4 s | 5067 |
| `idle-heavy` | yes | 2 | 2 | 2 | idle → work ✗ | — → supports_intention | 6.5 s | 10377 |
| `meeting-audio` | yes | 1 | 1 | 0 | work → work ✓ | supports_intention → supports_intention ✓ | 3.1 s | 4917 |
| `research-spread` | yes | 1 | 1 | 0 | work → work ✓ | supports_intention → supports_intention ✓ | 3.5 s | 5004 |
| `split-attention` | yes | 1 | 1 | 0 | work → work ✓ | other_work → other_work ✓ | 3.8 s | 5075 |

| metric | first run (`v2`) | second run (`v3`) |
|---|---|---|
| coverage validity | 100.0 % (8/8) | 100.0 % (8/8) |
| error-card rate | 0.0 % | 0.0 % |
| attempts per window | 1.12 | 1.25 |
| repairs applied | 2 | 2 |
| cards written | 10 | 11 |
| **category accuracy** | **75.0 % (6/8)** | **87.5 % (7/8)** |
| relation accuracy | 57.1 % (4/7) | 85.7 % (6/7) |
| mean latency | 4.4 s | 4.5 s |
| total tokens (mean per window) | 35881 (4485) | 51313 (6414) |

| gate | measured | required | result |
|---|---|---|---|
| coverage validity | 100.0 % | ≥ 100 % | pass |
| category accuracy | 87.5 % | ≥ 80 % | pass |

**Both gates pass.** The twenty-minute YouTube block is now its own
`Distraction` card with `possible_distraction`, and the surrounding coding time
stays a Work card.

### How stable is that number

The model is not deterministic, so the run above is one draw. Six consecutive
runs of the same command on the final prompt:

| run | category | relation | attempts/window | `distraction-block` |
|---|---|---|---|---|
| 1 | 87.5 % | 100.0 % | 1.00 | ✓ |
| 2 | 87.5 % | 71.4 % | 1.00 | ✓ |
| 3 | 87.5 % | 100.0 % | 1.00 | ✓ |
| 4 | 87.5 % | 100.0 % | 1.00 | ✓ |
| 5 | 87.5 % | 71.4 % | 1.12 | ✓ |
| 6 | 87.5 % | 71.4 % | 1.00 | ✓ |

Category accuracy was 87.5 % in all six, plus the two recorded runs above and
the Russian run below: 6/8 is now a floor, not an average, and the split
happened in every run. Relation swings between 71 % and 100 % on the same
prompt — `focused-morning` and `split-attention` are the two that move, and
they move together. The relation axis is not gated, and this says why it should
not be until it is more stable than the thing it measures.

Earlier prompt revisions that did *not* survive are worth recording, because
each looked reasonable:

- Repeating the ten-minute card floor inside the fresh-mode split step ("under
  10 minutes there is no split") made the model fold the twenty-minute block
  back into the Work card in 2 runs out of 4. The floor already lives in the
  card-structure block and in the correction round; saying it next to the split
  rule reads as permission not to split.
- A closing "is any card except the last shorter than 10 minutes? merge it
  back" in the output contract did the same thing, from the last line of the
  prompt: 1 correct run in 4. Removing it took `distraction-block` to 6/6.
- An explicit "read idleness from the input counts" paragraph in the category
  section changed nothing on `idle-heavy` in 4 runs and was dropped rather than
  kept as unmeasured prompt weight.

## Second run, part 2 — real capture, unlabelled

### Finding 2 — the ledger no longer fragments on animated titles

`identity_for` now normalizes a window title before it becomes a task key:
leading and trailing spinner and status ornament (braille U+2800–28FF, the
quadrant and clock spinners ◐◑◒◓◴◵◶◷, ✱✲✳✴✵✶, box/block/arrow ornaments),
`[3/10]` progress fragments, `42%`, bracketed `(12)` unread counts, elapsed
`00:12` timers, and the dangling punctuation left behind by any of those.
Whitespace collapses. Only the edges are trimmed, so the task between the
ornaments survives exactly as the window reported it; when nothing survives,
the identity falls back to the app (`Using iTerm2`). `task_title` carries the
cleaned title, so the ledger, the prompt and the UI all read the same string.
`UNOBSERVED_GAP` and the interval semantics are untouched.

Re-exporting the same day with the same command:

```bash
cargo run -p screenpipe-engine --bin journal-eval -- export \
  --data-dir ~/.screenpipe-dev --date 2026-09-16 \
  --out crates/screenpipe-engine/tests/fixtures/journal/live --max-windows 10
```

| window | intervals before | intervals after | fixture size before | after |
|---|---|---|---|---|
| `2026-09-16-2035` | 224 | **26** | 100 KB | 16 KB |
| `2026-09-16-2123` | 1,715 | **42** | 737 KB | 29 KB |
| `2026-09-16-2148` | 1,134 | **83** | 488 KB | 49 KB |
| `2026-09-16-2217` | 672 | **92** | 290 KB | 53 KB |
| total | 3,745 | **243** | 1.6 MB | 147 KB |

93.5 % of the intervals were the same iTerm2 task under a different spinner
frame. In the fixtures, 3,512 intervals titled `◐ Screenpipe MVP
implementation plan` / `◑ …` / `✳ …` are now 44 intervals titled
`Screenpipe MVP implementation plan`. The export also stopped being size-bound:
all ten windows of the day now compile to 4–57 KB each (1, 26, 27, 27, 33, 42,
48, 83, 88, 92 intervals), where the first export could only keep four. The
corpus is deliberately still those same four windows, so the before/after
numbers compare like with like.

### The run

```bash
cargo run -p screenpipe-engine --bin journal-eval -- run \
  --fixtures crates/screenpipe-engine/tests/fixtures/journal/live \
  --labels   crates/screenpipe-engine/tests/fixtures/journal/live/2026-09-16.labels.json \
  --data-dir ~/.screenpipe-dev
```

- run at: 2026-09-16T22:24:02.718841+00:00

| fixture | valid | cards | attempts | repairs | latency | tokens |
|---|---|---|---|---|---|---|
| `2026-09-16-2035` | yes | 1 | 1 | 0 | 17.1 s | 5661 |
| `2026-09-16-2123` | yes | 2 | 3 | 5 | 52.0 s | 22351 |
| `2026-09-16-2148` | yes | 2 | 1 | 1 | 9.5 s | 7672 |
| `2026-09-16-2217` | yes | 2 | 3 | 5 | 51.0 s | 23056 |

| metric | first run (`v2`, fragmented) | second run (`v3`, normalized) |
|---|---|---|
| coverage validity | 100.0 % (4/4) | 100.0 % (4/4) |
| attempts per window | 1.75 | 2.00 |
| **total tokens (mean per window)** | **59472 (14868)** | **58740 (14685)** |
| mean latency | 7.2 s | 32.4 s |

Across four consecutive runs of the live corpus on the final prompt, tokens per
window ranged 10,172–14,827 and attempts per window 1.50–2.00, with coverage
validity 100 % in all four. The first run's 14,868 tokens/window is therefore
at the top of the new range rather than in the middle of it: call the saving
"up to a third", not a third. Two effects cancel out. Truncation is gone — the
observation list now fits inside `MAX_EVIDENCE_CHARS`, no window is cut off
with `[observation list truncated: too many intervals]`, and no window fails
its first attempt with a `TIME COVERAGE ERROR` for minutes it was never shown —
but the model now *sees* every real interval of a fragmented day, so its
correction rounds are spent on geometry it could previously not even attempt.
As a control, the old 1,715-interval fixtures were replayed through the new
prompt: 21,380–23,763 tokens per window and 2.25–2.50 attempts. The
normalization is worth roughly 6–9k tokens per live window at equal prompt.

Live windows are also where this model is least stable: over nine live-corpus
runs during this work, three finished at 75 % coverage validity (one window
exhausting its three attempts on `EMPTY OUTPUT` or a `DURATION ERROR` for a
four-minute card). Every such failure was on `2026-09-16-2123` or
`2026-09-16-2217`, the two windows with the most fragmented evidence and the
least observed time inside the window itself. The final prompt's last three
runs were 100 %, but a corpus this small cannot tell 100 % from 90 %.

### What was stripped from the re-exported live fixtures

Same treatment as the first export: capture-side redaction had already replaced
credentials in URLs (`[URL_WITH_CREDENTIALS]`) and IP addresses
(`[IP_ADDRESS]`); the rest is by hand, before the fixtures were saved.

| what | how many | replacement |
|---|---|---|
| Telegram window titles (private group names) | 20 intervals | `Telegram — private chat` |
| text snippets: a third party's product roadmap, an NDA channel, a personal presentation, a private chat list, a base64 image blob | 16 snippets | dropped entirely |
| presentation document paths and titles | 15 fields | `Presentation` / `/Users/user/Documents/redacted.pptx` |
| Finder window titles naming a third party's folders | 12 fields | `redacted folder` |
| the machine's macOS username, in paths and screen text | 5 fields | `user` |
| the team gateway's internal hostname in a terminal snippet | 1 | `gateway.internal` |

62 snippets survived across the four windows (8 / 10 / 13 / 15). A final scan
for the username, e-mail shapes, `sk-`/`Bearer`/`ghp_`/JWT token shapes, long
base64 runs and every term on the denylist returns zero hits.

## The tail classifier, because `RELATION_RULES` is shared

`crate::focus::classifier` renders `RELATION_RULES` verbatim, so a card-prompt
change is a tail-prompt change and the tail gate had to be re-measured.

```bash
cargo run -p screenpipe-engine --bin journal-eval -- tail \
  --fixtures crates/screenpipe-engine/tests/fixtures/focus/tail \
  --data-dir ~/.screenpipe-dev
```

- run at: 2026-09-16T22:24:14.540385+00:00

| metric | first run | second run | spread over 7 runs |
|---|---|---|---|
| relation accuracy | 83.3 % (5/6) | 66.7 % (4/6) | 66.7 – 100 % |
| **distraction precision** | **100.0 %** | **100.0 %** | 100 % in 6 of 7 |
| `unknown` rate on thin evidence | 100 % | 100 % | 100 % |

The gate (distraction precision ≥ 90 %) passes. The one run that failed it
scored 50 % and was the intermediate prompt described above, which put "a
leisure block of 10 minutes or more is possible_distraction" into the shared
rules and taught the classifier to call a coffee break a distraction; with that
sentence moved into the journal's own intention section the failure has not
recurred. `industry-report` is the fixture that moves: it is graded
`other_work` and the model calls it `supports_intention` or `unknown` depending
on the draw.

## Output language

Cards and tail reasons now follow the app's `uiLanguage` setting
(`system` | `en` | `ru`; `system` resolves from `LC_ALL`/`LC_MESSAGES`/`LANG`).
Only human-readable text moves: JSON keys, the five relation values, the
category labels and the timestamps are contract, and the prompt says so.

Same corpus, same commands, with `uiLanguage: "ru"` in the store the
`--data-dir` points at:

- run at: 2026-09-16T22:24:59.170275+00:00
- coverage validity 100.0 % (8/8), category accuracy **87.5 % (7/8)**, relation
  accuracy 85.7 % (6/7), attempts per window 1.00, 5,117 tokens per window.

Both gates pass in Russian, at the same numbers as English and about 100 tokens
per window more. The tail fixtures with `"language": "ru"` come back with
Russian reasons and unchanged English relation values (`break`,
`possible_distraction`, …), relation accuracy 83.3 % and distraction precision
100 %.

## What is still wrong

| what | reading |
|---|---|
| `idle-heavy` is still `work` | Unchanged from the first run, and still the one miss behind 87.5 %. In production this window never reaches a provider: `journal::idle` gates it deterministically and writes an Idle card with no call at all. An explicit "read idleness from the input counts" instruction was tried and measured no better, so it was not kept. |
| relation accuracy swings 71–100 % | `focused-morning` and `split-attention` — same project, different task — flip together between runs. The rule that names them is in the prompt; the model applies it about two runs in three. Not gated, and should not be until it is stable. |
| live windows cost 2 attempts | On the two most fragmented windows the model still needs a correction round, and occasionally exhausts three. Now that the evidence list is honest, this is a real geometry problem rather than an artefact of truncation: the next thing to try is deterministic repair of sub-ten-minute cards in `repair.rs`, which would cost no provider call at all. |
| the live corpus is 4 unlabelled windows | It measures validity and cost, never accuracy. Ten windows of the day now fit; labelling them is the cheapest available improvement to this eval. |
