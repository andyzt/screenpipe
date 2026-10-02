# Screenpipe: product directions for the team to choose

Decision draft · 22 September 2026. These are alternative bets, not a commitment
to build every feature. Estimates below describe relative effort against this
fork, not delivery promises. This document belongs outside the statistics UI.

For the subsequent detailed exploration of quarterly contribution reviews,
mixed personal/work context, role attribution and a screen-log evaluation corpus,
see [Quarterly work research](QUARTERLY_WORK_RESEARCH.md). The two manual features
implemented from that research and their build status are documented in
the PR description.

## Recommendation

Pilot **A: a verified work journal** as the shortest path to value, and test
**B: resume a specific task** as the stronger differentiation hypothesis.
Use C as the delivery channel if the pilot consists mainly of heavy agent users.
Treat E as a separate later bet: a wrong interruption destroys trust faster
than an imperfect retrospective summary.

The common product promise is: **recover useful work with sources you can check**.
A generated story or a rising activity graph does not demonstrate that promise.

## What the alternatives teach us

- [Dayflow](https://github.com/JerryZLiu/Dayflow) is the closest journal reference.
  Its [assistant integration guide](https://www.dayflow.so/integrations/) already
  covers timeline retrieval, weekly context and optional edits. It explicitly
  distinguishes confirmed facts, uncertain outcomes and what leaves the device.
  Therefore MCP connectivity alone is not our competitive advantage.
- [ActivityWatch](https://activitywatch.net/) provides local app/window activity,
  categories and browser/editor watchers. It is a useful baseline for time
  accounting and coverage; an LLM category should not be presented as a more
  precise clock merely because it reads fluently.
- [OpenRecall](https://github.com/openrecall/openrecall) centres on retrieving
  digital history through locally processed screenshots. Its narrow recovery
  task gives us a stronger success condition than “the summary looks plausible”.
- [Drifty's privacy policy](https://drifty.so/privacy/) describes activity metadata,
  AI classification and bounded intervention metrics. The relevant pattern is
  measuring whether an intervention helps and respecting local context. We have
  not run a controlled comparison of its effectiveness.

These are observations from primary documentation. The product choices below
are our hypotheses, not competitor claims or validated market demand.

## Five candidate tracks

| Track | Person and trigger | Smallest valuable result | Relative effort | Main risk |
|---|---|---|---|---|
| A. Verified journal | Consultant, PM, developer preparing an update | Reviewed summary with sources and corrected outcomes | Small–medium | Attractive prose still misrepresents completed work |
| B. Resume work | Person returning after a meeting, interruption or next morning | Last task, relevant artifacts, unresolved point and a suggested next step | Medium–large | Wrong task segmentation or invented next step |
| C. Context for an agent | Existing assistant user repeatedly explaining their work | A bounded, inspectable context pack for one task | Medium | Irrelevant history and unwanted cloud disclosure |
| D. Find something I saw | Researcher, support specialist, anyone who lost a page or decision | Correct source located and reopened quickly | Medium | Search retrieves a plausible but wrong item |
| E. Gentle focus support | Person who deliberately chooses a focus session | Useful, dismissible intervention at the right moment | Large | False interruptions and feeling monitored |

### A. Verified journal → reviewed daily or weekly update

**Concrete flow:** open Yesterday → inspect blocks → correct title/category and
“done / in progress / unknown” → confirm selected outcomes → copy a short update
with source links. Start with a human-triggered export; sending to a team is a
separate explicit action.

Variants to choose:

1. **Personal review:** “What did I finish, what remains, what should I remember?”
2. **Team update:** a stand-up or weekly draft grouped by outcomes and blockers.
3. **Client/project record:** a user-reviewed project breakdown. Billable time
   needs a separately designed approval process; activity minutes are estimates.

MVP work: stable edits, split/merge, preserve manual changes through regeneration,
source coverage and missing-data intervals. Existing recap, cards and feedback
provide a base. Avoid auto-publishing unreviewed work reports.

Experiment: 10–15 volunteers, ten working days. Compare the time and correction
burden of a manually prepared update with a reviewed generated draft. Collect
whether the draft was actually reused. Report completion errors separately from
stylistic edits, and disclose the small sample.

**Continue when** people repeatedly use the reviewed output without prompting and
the source-backed draft saves them effort. **Reconsider when** correcting it costs
as much as writing the update, or users never return after curiosity wears off.

### B. Resume a specific task → work continuity

**Concrete flow:** “Continue the task from yesterday afternoon” → choose among
candidate tasks → show last confirmed activity, related documents/discussions,
unresolved point → open a selected source → accept or rewrite the next step.
Show suggested next steps as suggestions. Observation alone cannot prove a task
is complete or a decision was accepted.

Variants:

1. **Next-morning brief:** one unfinished task and a minimal restart checklist.
2. **Interruption recovery:** a snapshot before/after a meeting or context switch.
3. **Project handover to self:** several sessions collected into a project trail.

MVP work: task boundaries, explicit user pinning, continuity across sessions,
reliable source reopening, stale-link handling, and editable unresolved items.
Start with user-selected intervals before trying autonomous cross-day clustering.

Experiment: each participant records a few interrupted tasks, then attempts to
resume comparable tasks using browser/editor history versus the prototype.
Measure time to identify the correct task and first useful action, incorrect
artifact opens, and whether the suggested step was accepted. This is an
exploratory within-person comparison, not proof from aggregate DAU.

**Continue when** it restores context faster with fewer wrong turns. This is my
preferred differentiation hypothesis, provided source correctness is strong.

### C. Bounded context for an agent → stop repeating the setup

**Concrete flow:** select task/time interval → preview included sources and excluded
private apps → build a compact context pack → let the chosen agent read it →
answer with citations. Distinguish the agent's LLM route from Screenpipe's vsellm
route. A local MCP server does not make the external agent local.

Variants: developer debugging context; PM discussion-to-decision context;
researcher's reading trail. Begin with read-only task-specific context. Write
operations require explicit review and a separate capability boundary.

MVP work: scoped retrieval, size budgets, source identifiers, predictable auth,
context preview and deletion, testing against several agent clients. Existing
journal MCP tools help, but relevance and trustworthy sources are the differentiator.

Experiment: run the same real task with manual context and with a reviewed pack.
Measure setup time, answer correctness, missing evidence, correction effort and
amount of unnecessary context included. Token savings alone are insufficient.

**Choose C first** if agent context preparation is a frequent pain for the recruited
pilot and people already work primarily inside an assistant.

### D. Find the missing page, fact or decision → personal retrieval

**Concrete flow:** describe what was seen → browse a small set of evidence-backed
hits → reopen the exact frame/page → confirm it answered the question.

Variants: “where did I read this?”, “what was agreed in that discussion?”, or
“show the screen where the error happened”. Source availability, timestamps and
capture gaps must be visible. Audio and screenshots may have different coverage.

MVP work: hybrid retrieval/ranking, deduplication, time/app filters, source previews
and graceful handling of unavailable originals. Reuse the existing search layer;
judge it on user tasks rather than broad semantic similarity scores.

Experiment: volunteers create a known-answer set from their own captured work.
Measure successful retrieval in the first few results, time to the correct source,
wrong confident answers and return usage. Keep personal content local; central
analytics receives only consented outcome metadata.

**Choose D first** if people frequently lose material but do not want another daily
review ritual. It offers a narrow proof of value with limited generation needs.

### E. Gentle focus support → deliberate sessions

**Concrete flow:** state an intention and allowed activities → start a session →
observe sufficient evidence → ask once when divergence seems likely → user marks
“other work / break / distraction” → continue, snooze or end.

Variants: reflective summary after a session; an opt-in mid-session check;
a quiet reminder after a long interruption. Start with retrospective labelling,
then enable live nudges only after measuring false positives.

MVP work: explicit intention, minimum evidence, low-confidence abstention,
user corrections, cooldowns, easy suspension and an audit trail of why a nudge
appeared. Existing nudges and intentions are infrastructure, not validation.

Experiment: first collect consented labels without notifications; then compare
quiet sessions with sparse nudges. Measure incorrect interruptions per session,
helpfulness ratings, return to the stated task and disable/snooze rates. A return
to an app does not necessarily mean progress on the task.

**Defer when** labels are ambiguous or people turn it off. A synthetic precision
result of 1/1 cannot justify a live intervention policy.

## Foundation required whichever track wins

1. **Honest capture state.** Paused, disconnected, permission-blocked and recording
   are separate from “cards are generating” and “not enough recent context”.
2. **Controllable storage.** Visible reserve, retained-history size and capture
   gaps; user changes the reserve and deliberately resumes after a stop.
3. **Provider clarity.** Screenpipe → vsellm by default in this fork; show endpoint,
   selected model, credential source and a working connectivity check.
4. **Source integrity.** Sources open correctly, manual edits survive, inferred
   completion is labelled, and missing recording is not reconstructed as fact.
5. **Measurement coverage.** Opt-in events; outcome, error and resource definitions;
   first-run, LLM and MCP producers must be tested before treating blank charts as
   meaningful zeroes. Record app version/OS without screen contents.
6. **Delivery discipline.** A tested Mac arm64 build first. Separate Windows/Intel/
   Linux validation and measured recording workloads before promising support.

## A practical sequence for the team

**Decision session:** pick one primary track, one secondary experiment and a named
pilot audience. Each teammate brings two recent examples of the pain, not a feature
wishlist. Recruit people who actually encounter those situations.

**Foundation checkpoint:** reproduce disk/permission/pause/resume failures on a clean
profile; verify source reopening, provider failure and consent. A truthful UI is
required before testing value.

**Two-week learning pilot:** run A for the whole group, or C if the audience is
agent-heavy. Prototype B or D with a smaller subset. Instrument the proposed
outcomes first. Agree on thresholds using a short baseline before seeing the
results; do not invent a success threshold after the pilot.

**Review:** compare repeated use, task success, correction time, missing evidence,
resource cost and reasons for stopping. Choose the next investment from observed
behaviour. Increased background recording or generated-card count is not success.

Relative priority I would take to the meeting: **A for near-term validation, B for
product differentiation, C for agent-heavy users, D as the narrower retrieval bet,
E only after trust and labels are established.**
