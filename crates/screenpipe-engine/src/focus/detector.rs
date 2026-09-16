// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com

//! The tail detector: every 60 seconds, "does the last ten minutes look like
//! the thing you said you were doing?".
//!
//! One pass, in this order, and every step can end it:
//!
//! 1. **No intention** → `unknown` / "no active intention". Nothing else is
//!    computed; without a stated intention there is nothing to diverge from.
//! 2. **No evidence** — capture stalled, or no frames in the tail window →
//!    `unknown` with `evidence_ok = false`. Missing evidence is never
//!    distraction; that asymmetry is deliberate and load-bearing.
//! 3. **Dominant task** from the deterministic ledger: the task with the most
//!    active minutes in the tail, measured with the shared active-minutes
//!    definition so "12 min" here and on a card mean the same thing.
//! 4. **Support set** — the apps, hosts and documents this intention has
//!    already been supported by, plus everything seen in the first `grace`
//!    minutes after it was stated (a user who just said what they are doing is
//!    presumed to be doing it). A dominant task inside that set is
//!    `supports_intention` and clears the divergence timer.
//! 5. **Otherwise a divergence candidate**, named deterministically from the
//!    most recent card overlapping the tail, with a timer that starts on the
//!    first divergent tick and survives across ticks in `focus_state`. Inside
//!    the grace period the candidate is reported with low confidence and the
//!    reason says so; past it, [`TailClassifier`] gets the final word.
//!
//! The classifier is a seam, not an implementation detail: see
//! [`super::select_tail_classifier`]. [`RulesClassifier`] is the only one
//! today and never exceeds [`DIVERGENCE_CONFIDENCE`], which is below the
//! nudge threshold on purpose.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use screenpipe_db::{
    DatabaseManager, FocusIntention, FocusStateDraft, FocusStateRecord, JournalActivity,
    JournalFrameSample, JournalLedgerInterval, JournalRunDraft,
};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::journal::compile::{attach_snippets, plan_window, snippet_frame_ids, CompiledWindow};
use crate::journal::day::DISTRACTION_CATEGORY_ID;
use crate::journal::settings::JournalSettings;
use crate::journal::time::{active_minutes_in_span, IDLE_GAP};
use crate::routes::activity_summary::load_recording_status;

use super::suppression::{self, OVERRIDE_REASON};
use super::{is_divergent, BREAK, OTHER_WORK, POSSIBLE_DISTRACTION, SUPPORTS_INTENTION, UNKNOWN};

/// How often the detector wakes up.
pub const TICK: Duration = Duration::seconds(60);

/// How far back "right now" reaches. Long enough that a glance at a chat
/// window does not redefine the tail, short enough that the answer is still
/// about the present.
pub const TAIL_WINDOW: Duration = Duration::minutes(10);

/// Minimum spacing between two calls to a classifier that leaves the machine.
/// A free, local classifier is not throttled — it is cheaper than the read
/// that feeds it.
pub const CLASSIFIER_INTERVAL: Duration = Duration::minutes(5);

/// Confidence for a dominant task inside the intention's support set.
pub const SUPPORT_CONFIDENCE: f64 = 0.7;

/// Ceiling while the divergence is still inside the grace period: enough to
/// show in the UI, never enough to act on.
pub const GRACE_CONFIDENCE: f64 = 0.5;

/// What the deterministic rules claim once the grace period has passed. Below
/// [`super::NUDGE_MIN_CONFIDENCE`] by design.
pub const DIVERGENCE_CONFIDENCE: f64 = 0.6;

/// How far back the support set reads cards. An intention left open over a
/// weekend must not turn the per-minute tick into a growing scan.
pub const SUPPORT_LOOKBACK: Duration = Duration::hours(24);

/// Category id treated as a deliberate pause rather than a divergence worth
/// naming. Seeded row; ordinary and user-editable, like `distraction`.
pub const PERSONAL_CATEGORY_ID: &str = "personal";

/// Screen text a tail may quote to a provider, across every interval in it.
/// The card pipeline budgets twelve thousand characters for a 45-minute
/// window; ten minutes asked every five minutes gets two orders of magnitude
/// less, because the question is "what kind of thing is this", not "what does
/// it say".
pub const TAIL_SNIPPET_BUDGET: usize = 600;

/// Most interval lines one tail prompt carries. A tail that fragmented into
/// thirty tasks is noise; the first dozen already say what kind of ten minutes
/// this was.
pub const TAIL_MAX_OBSERVATIONS: usize = 12;

static SPAWNED: AtomicBool = AtomicBool::new(false);

/// The task that owned most of the tail.
#[derive(Debug, Clone, PartialEq)]
pub struct DominantTask {
    pub task_key: String,
    /// Window title, document name or browser page title, whichever the
    /// ledger ranked highest.
    pub title: String,
    pub app: Option<String>,
    /// The ledger's parent task: the app-level or host-level grouping.
    pub parent_title: Option<String>,
    pub active_minutes: f64,
}

impl DominantTask {
    /// Every name this task can be recognised by, normalised.
    fn identities(&self) -> Vec<String> {
        [
            Some(self.title.clone()),
            self.app.clone(),
            self.parent_title.clone(),
        ]
        .into_iter()
        .flatten()
        .filter_map(|value| normalise(&value))
        .collect()
    }
}

/// Everything a classifier is allowed to know about this tick. Deliberately
/// small and owned: a classifier never touches the database.
#[derive(Debug, Clone)]
pub struct TailInput {
    pub now: DateTime<Utc>,
    pub tail_start: DateTime<Utc>,
    pub intention: FocusIntention,
    pub dominant: Option<DominantTask>,
    /// Apps, hosts and documents this intention has been supported by.
    pub support_set: Vec<String>,
    /// What the deterministic rules would say on their own.
    pub candidate: String,
    pub candidate_reason: String,
    /// How long the divergence has already lasted.
    pub divergence_minutes: f64,
    pub grace_minutes: i64,
    /// `journalWorkProfile`: role, projects, notes.
    pub work_profile: Value,
    /// The last ten minutes compiled into the same bounded observation set the
    /// card prompt reads: app, window title, host, document, input-event
    /// counts, and at most [`TAIL_SNIPPET_BUDGET`] characters of screen text
    /// across the whole tail.
    ///
    /// `None` on a tick that never reaches a provider-backed classifier.
    /// Compiling costs three extra reads and a text fetch, so it happens only
    /// when something is actually going to look at it.
    pub observations: Option<CompiledWindow>,
}

/// A classifier's answer. `relation` must be one of the contract's five.
#[derive(Debug, Clone, PartialEq)]
pub struct TailVerdict {
    pub relation: String,
    pub confidence: f64,
    pub reason: String,
}

/// The seam an LLM tail classifier implements.
///
/// Contract for that implementation:
///
/// - `classify` is called OUTSIDE any transaction, at most once per
///   [`CLASSIFIER_INTERVAL`] when [`Self::calls_provider`] is true, and only
///   after the divergence has outlived the grace period. It never runs while
///   the dominant task supports the intention.
/// - It may return any of the five relations. A relation it is not sure about
///   must come back as `unknown`, not as `possible_distraction` — the nudge
///   layer only ever reads the latter.
/// - Returning an error is fine: the detector falls back to the deterministic
///   candidate at [`GRACE_CONFIDENCE`] and records the failure in
///   `journal_runs`, so a provider outage degrades instead of lying.
/// - `calls_provider` must be true for anything that sends text off the
///   machine. It is what turns on the `journal_runs` audit row the user reads
///   to see exactly what left.
#[async_trait]
pub trait TailClassifier: Send + Sync {
    fn name(&self) -> &'static str;

    fn model(&self) -> Option<String> {
        None
    }

    fn prompt_version(&self) -> Option<String> {
        None
    }

    /// Whether a call leaves the machine. Drives both the 5-minute throttle
    /// and the `journal_runs` audit row.
    fn calls_provider(&self) -> bool {
        false
    }

    async fn classify(&self, input: &TailInput) -> anyhow::Result<TailVerdict>;
}

/// Rules only: repeat the deterministic candidate at [`DIVERGENCE_CONFIDENCE`].
///
/// This is the whole "LLM" of this milestone, and it is honest about it: the
/// confidence is capped below the nudge threshold, so with this classifier
/// installed the notification layer is unreachable by construction.
pub struct RulesClassifier;

#[async_trait]
impl TailClassifier for RulesClassifier {
    fn name(&self) -> &'static str {
        "rules-v1"
    }

    async fn classify(&self, input: &TailInput) -> anyhow::Result<TailVerdict> {
        Ok(TailVerdict {
            relation: input.candidate.clone(),
            confidence: DIVERGENCE_CONFIDENCE,
            reason: input.candidate_reason.clone(),
        })
    }
}

/// What one tick decided. Returned so tests can assert on the rules without
/// reading the row back.
#[derive(Debug, Clone, PartialEq)]
pub struct FocusTick {
    pub state: FocusStateDraft,
    pub dominant: Option<DominantTask>,
    /// The classifier was actually invoked this tick.
    pub classifier_ran: bool,
    /// A `journal_runs` row of kind `tail` was written.
    pub run_recorded: bool,
}

/// Start the engine-owned focus detector. Spawned from
/// `server.rs::create_router_inner` next to the journal worker, for the same
/// reason: both entrypoints build their router through that path. Guarded so
/// it starts once per process.
pub fn spawn_focus_detector(
    db: Arc<DatabaseManager>,
    screenpipe_dir: PathBuf,
    power_manager: Option<Arc<crate::power::PowerManagerHandle>>,
    shutdown: CancellationToken,
) {
    if SPAWNED.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async move {
        // Let capture settle: a detector that runs before the first frame of
        // the session lands would report a stall that is really a cold start.
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(45)) => {}
            _ = shutdown.cancelled() => return,
        }
        info!("focus detector started");

        loop {
            let settings = JournalSettings::load(&screenpipe_dir);
            let paused = power_manager
                .as_ref()
                .is_some_and(|manager| manager.current_profile().capture_paused);

            if !settings.enabled {
                debug!("focus detector: journal disabled in settings, skipping tick");
            } else if paused {
                // Paused capture produces no evidence; a tick would only
                // write `unknown` over a still-useful answer.
                debug!("focus detector: power manager paused capture, skipping tick");
            } else {
                let classifier = super::select_tail_classifier(&settings, &screenpipe_dir);
                let outcome = tokio::select! {
                    result = run_focus_tick(&db, &settings, classifier.as_ref(), Utc::now()) => result,
                    _ = shutdown.cancelled() => {
                        info!("focus detector shutting down");
                        return;
                    }
                };
                match outcome {
                    Ok(tick) => {
                        debug!(
                            relation = %tick.state.relation,
                            confidence = tick.state.confidence,
                            "focus tick"
                        );
                        if let Err(error) =
                            super::nudge::maybe_nudge(&db, &settings, &tick, Utc::now()).await
                        {
                            debug!(%error, "focus nudge not delivered");
                        }
                    }
                    Err(error) => warn!(%error, "focus detector tick failed"),
                }
            }

            tokio::select! {
                _ = tokio::time::sleep(TICK.to_std().expect("positive tick")) => {}
                _ = shutdown.cancelled() => {
                    info!("focus detector shutting down");
                    return;
                }
            }
        }
    });
}

/// One pass of the detector. Public so tests drive it directly against a
/// temporary database instead of waiting on the tick.
pub async fn run_focus_tick(
    db: &DatabaseManager,
    settings: &JournalSettings,
    classifier: &dyn TailClassifier,
    now: DateTime<Utc>,
) -> anyhow::Result<FocusTick> {
    let tail_start = now - TAIL_WINDOW;
    let previous = db.get_focus_state().await?;

    // 1. Nothing to diverge from.
    let Some(intention) = db.active_focus_intention().await? else {
        let evidence_ok = evidence(db, tail_start, now).await.is_some();
        return persist(
            db,
            FocusStateDraft {
                computed_at: now,
                intention_id: None,
                relation: UNKNOWN.to_string(),
                confidence: 0.0,
                divergence_started_at: None,
                dominant_task_title: None,
                dominant_app: None,
                evidence_ok,
                reason: Some("no active intention".to_string()),
            },
            None,
            false,
            false,
        )
        .await;
    };

    // 2. No evidence is `unknown`, never distraction.
    let Some(samples) = evidence(db, tail_start, now).await else {
        return persist(
            db,
            FocusStateDraft {
                computed_at: now,
                intention_id: Some(intention.id),
                relation: UNKNOWN.to_string(),
                confidence: 0.0,
                divergence_started_at: carried_divergence(previous.as_ref(), intention.id),
                dominant_task_title: None,
                dominant_app: None,
                evidence_ok: false,
                reason: Some(
                    "capture data is missing for the last 10 minutes, so there is nothing to compare"
                        .to_string(),
                ),
            },
            None,
            false,
            false,
        )
        .await;
    };

    // 3. Dominant task over the tail.
    let frames: Vec<DateTime<Utc>> = samples.iter().map(|sample| sample.timestamp).collect();
    let intervals = db.list_journal_ledger_intervals(tail_start, now).await?;
    let Some(dominant) = dominant_task(&intervals, &frames, tail_start, now) else {
        return persist(
            db,
            FocusStateDraft {
                computed_at: now,
                intention_id: Some(intention.id),
                relation: UNKNOWN.to_string(),
                confidence: 0.0,
                divergence_started_at: carried_divergence(previous.as_ref(), intention.id),
                dominant_task_title: None,
                dominant_app: None,
                evidence_ok: true,
                reason: Some(
                    "the activity ledger has not caught up with the last 10 minutes yet"
                        .to_string(),
                ),
            },
            None,
            false,
            false,
        )
        .await;
    };

    // 4. Support set: what this intention has looked like so far.
    let grace = Duration::minutes(settings.focus_grace_minutes.max(0));
    let support_set = support_set(db, &intention, grace, now).await?;
    if dominant
        .identities()
        .iter()
        .any(|identity| support_set.contains(identity))
    {
        let draft = FocusStateDraft {
            computed_at: now,
            intention_id: Some(intention.id),
            relation: SUPPORTS_INTENTION.to_string(),
            confidence: SUPPORT_CONFIDENCE,
            divergence_started_at: None,
            dominant_task_title: Some(dominant.title.clone()),
            dominant_app: dominant.app.clone(),
            evidence_ok: true,
            reason: Some(format!(
                "{} is one of the apps and pages this intention has been worked on in",
                dominant.app.clone().unwrap_or_else(|| dominant.title.clone())
            )),
        };
        return persist(db, draft, Some(dominant), false, false).await;
    }

    // 5. The user's own judgment wins. "This is fine" / "Take a break"
    //    (`POST /focus/state/override`) suppresses this task for a while: the
    //    relation they chose, at full confidence, with the timer cleared and
    //    no classifier call. Because neither relation is
    //    `possible_distraction`, this also makes the nudge unreachable for
    //    the task — arguing with someone who already answered is the fastest
    //    way to get the whole feature switched off.
    let task_key = suppression::task_key(Some(&dominant.title), dominant.app.as_deref());
    if let Some(relation) = task_key
        .as_deref()
        .and_then(|key| suppression::active(intention.id, key, now))
    {
        let draft = FocusStateDraft {
            computed_at: now,
            intention_id: Some(intention.id),
            relation,
            confidence: 1.0,
            divergence_started_at: None,
            dominant_task_title: Some(dominant.title.clone()),
            dominant_app: dominant.app.clone(),
            evidence_ok: true,
            reason: Some(OVERRIDE_REASON.to_string()),
        };
        return persist(db, draft, Some(dominant), false, false).await;
    }

    // 6. Divergence: name a candidate, run the timer, and only ask the
    //    classifier once the grace period has been outlived.
    let (candidate, candidate_reason) = candidate_relation(db, &dominant, tail_start, now).await?;
    let divergence_started_at =
        carried_divergence(previous.as_ref(), intention.id).unwrap_or(now);
    let divergence_minutes =
        (now - divergence_started_at).num_milliseconds() as f64 / 60_000.0;

    let mut input = TailInput {
        now,
        tail_start,
        intention: intention.clone(),
        dominant: Some(dominant.clone()),
        support_set: support_set.into_iter().collect(),
        candidate: candidate.clone(),
        candidate_reason: candidate_reason.clone(),
        divergence_minutes,
        grace_minutes: settings.focus_grace_minutes,
        work_profile: settings.work_profile.clone(),
        observations: None,
    };

    let mut classifier_ran = false;
    let mut run_recorded = false;
    let verdict = if divergence_minutes <= settings.focus_grace_minutes.max(0) as f64 {
        // Inside the grace period nothing is asked and nothing is claimed.
        TailVerdict {
            relation: candidate.clone(),
            confidence: GRACE_CONFIDENCE,
            reason: format!("{candidate_reason}; within grace period"),
        }
    } else if classifier.calls_provider() && !provider_call_due(intention.id, now) {
        // Throttled: keep the last answer rather than spending a call a
        // minute on a divergence that has not changed.
        carried_verdict(previous.as_ref(), intention.id).unwrap_or(TailVerdict {
            relation: candidate.clone(),
            confidence: DIVERGENCE_CONFIDENCE,
            reason: candidate_reason.clone(),
        })
    } else {
        classifier_ran = true;
        // Compiling the tail costs three reads and a text fetch. Only a
        // classifier that is going to send it anywhere is worth paying that
        // for; the rules never look at it.
        if classifier.calls_provider() {
            match tail_observations(db, &intervals, &samples, tail_start, now).await {
                Ok(observations) => input.observations = observations,
                Err(error) => warn!(%error, "focus tail observations could not be compiled"),
            }
        }
        let started = std::time::Instant::now();
        let answer = classifier.classify(&input).await;
        let latency_ms = started.elapsed().as_millis() as i64;
        let verdict = match &answer {
            Ok(verdict) => verdict.clone(),
            Err(error) => {
                warn!(%error, classifier = classifier.name(), "tail classifier failed");
                TailVerdict {
                    relation: candidate.clone(),
                    confidence: GRACE_CONFIDENCE,
                    reason: format!("{candidate_reason}; classifier unavailable"),
                }
            }
        };
        // `journal_runs` audits what left the machine. A local rules
        // evaluation is not a provider call, and writing a row for it every
        // few minutes would also bury the journal's last card error, which
        // `/journal/status` reads from the same table.
        if classifier.calls_provider() {
            record_tail_run(db, classifier, &input, &answer, latency_ms).await;
            run_recorded = true;
        }
        verdict
    };

    let relation = normalise_relation(&verdict.relation);
    let draft = FocusStateDraft {
        computed_at: now,
        intention_id: Some(intention.id),
        relation: relation.clone(),
        confidence: verdict.confidence.clamp(0.0, 1.0),
        // A verdict that walks back to "supports" or "unknown" stops the
        // clock; anything still divergent keeps the original start.
        divergence_started_at: is_divergent(&relation).then_some(divergence_started_at),
        dominant_task_title: Some(dominant.title.clone()),
        dominant_app: dominant.app.clone(),
        evidence_ok: true,
        reason: Some(verdict.reason),
    };
    persist(db, draft, Some(dominant), classifier_ran, run_recorded).await
}

// ---------- steps ----------

/// Frames in the tail, or `None` when there is no evidence to reason over.
///
/// Reuses `/activity-summary`'s recording-health query so "capture stalled"
/// means the same thing in both places, with one correction: that query
/// compares against its own `Utc::now()` truncated to whole seconds, so a
/// frame written a fraction of a second ago can come back "minus one second old"
/// and read as stale. Capture that produced a frame less than one
/// [`IDLE_GAP`] ago is live whatever the rounding says.
async fn evidence(
    db: &DatabaseManager,
    tail_start: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Option<Vec<JournalFrameSample>> {
    let frames = db.journal_frame_samples(tail_start, now).await.ok()?;
    let newest = frames.last()?.timestamp;
    let recently_recording = load_recording_status(
        db,
        &tail_start.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        &now.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        None,
    )
    .await
    .is_ok_and(|recording| recording.recent_capture);
    (recently_recording || now - newest < IDLE_GAP).then_some(frames)
}

/// The ledger task with the most active minutes in the tail. Active minutes
/// rather than wall time: a window left open while the user is away must not
/// win the tail.
fn dominant_task(
    intervals: &[JournalLedgerInterval],
    frames: &[DateTime<Utc>],
    tail_start: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Option<DominantTask> {
    let mut tasks: Vec<DominantTask> = Vec::new();
    for interval in intervals {
        // Task depth only. `unobserved` is the ledger's word for a gap — the
        // screen went quiet — and must never win the tail; `category` is the
        // app/host grouping above the task and would out-total its own
        // children.
        if interval.kind == "unobserved" || interval.kind == "category" {
            continue;
        }
        let start = interval.start_at.max(tail_start);
        let end = interval.end_at.min(now);
        if end <= start {
            continue;
        }
        let active = active_minutes_in_span(frames, start, end);
        let minutes = if active > 0.0 {
            active
        } else {
            // Audio-only or sparse capture: fall back to the clipped span so
            // a real interval is never worth exactly nothing.
            (end - start).num_milliseconds() as f64 / 60_000.0
        };
        match tasks
            .iter_mut()
            .find(|task| task.task_key == interval.task_key)
        {
            Some(task) => task.active_minutes += minutes,
            None => tasks.push(DominantTask {
                task_key: interval.task_key.clone(),
                title: interval.title.clone(),
                app: interval.app_name.clone(),
                parent_title: interval.parent_title.clone(),
                active_minutes: minutes,
            }),
        }
    }
    // Ties resolve by title so two ticks over identical data agree.
    tasks.sort_by(|a, b| {
        b.active_minutes
            .partial_cmp(&a.active_minutes)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.title.cmp(&b.title))
    });
    tasks.into_iter().next()
}

/// The tail compiled into the card pipeline's own observation format, clipped
/// to the tail and budgeted far below it.
///
/// Reuses [`plan_window`] rather than growing a second renderer: a live verdict
/// and the card written about the same ten minutes twenty minutes later must be
/// reasoning over evidence that looks identical, or "why does the strip say
/// something else?" has no answer. The clip is explicit because `plan_window`
/// widens to a 45-minute context horizon, which is right for a card and wrong
/// for a tail.
async fn tail_observations(
    db: &DatabaseManager,
    intervals: &[JournalLedgerInterval],
    samples: &[JournalFrameSample],
    tail_start: DateTime<Utc>,
    now: DateTime<Utc>,
) -> anyhow::Result<Option<CompiledWindow>> {
    let clipped: Vec<JournalLedgerInterval> = intervals
        .iter()
        .filter(|interval| interval.kind != "unobserved" && interval.kind != "category")
        .filter_map(|interval| {
            let start = interval.start_at.max(tail_start);
            let end = interval.end_at.min(now);
            (end > start).then(|| JournalLedgerInterval {
                start_at: start,
                end_at: end,
                ..interval.clone()
            })
        })
        .collect();
    if clipped.is_empty() {
        return Ok(None);
    }

    let ui_events = db.journal_ui_event_marks(tail_start, now).await?;
    let audio = db.journal_audio_marks(tail_start, now).await?;
    let mut window = plan_window(tail_start, now, &clipped, samples, &ui_events, &audio);
    window.context_start = tail_start;

    let sample_ids = snippet_frame_ids(&window, samples);
    let wanted: Vec<i64> = sample_ids.values().flatten().copied().collect();
    let texts = db.journal_frame_texts(&wanted).await?;
    attach_snippets(&mut window, &sample_ids, &texts);

    window.intervals.sort_by_key(|interval| (interval.start_at, interval.end_at));
    window.intervals.truncate(TAIL_MAX_OBSERVATIONS);
    trim_snippets(&mut window, TAIL_SNIPPET_BUDGET);
    Ok(Some(window))
}

/// Spend the tail's screen-text budget oldest interval first, then stop. A
/// truncated snippet is still evidence; a prompt that quietly grew to a
/// twelve-thousand-character question asked every five minutes is a cost bug.
fn trim_snippets(window: &mut CompiledWindow, budget: usize) {
    let mut spent = 0usize;
    for interval in window.intervals.iter_mut() {
        let mut kept: Vec<String> = Vec::new();
        for snippet in std::mem::take(&mut interval.text_snippets) {
            let room = budget.saturating_sub(spent);
            if room == 0 {
                break;
            }
            let trimmed: String = snippet.chars().take(room).collect();
            spent += trimmed.chars().count();
            kept.push(trimmed);
        }
        interval.text_snippets = kept;
    }
}

/// Apps, hosts and documents that count as "on task" for this intention:
/// everything a `supports_intention` card of this intention named, plus
/// everything observed in the first `grace` minutes after it was stated.
///
/// The second half is what makes the detector usable on the first day: before
/// any card has been classified, the user's own behaviour right after stating
/// the intention is the only definition of the task there is.
async fn support_set(
    db: &DatabaseManager,
    intention: &FocusIntention,
    grace: Duration,
    now: DateTime<Utc>,
) -> anyhow::Result<HashSet<String>> {
    let mut set = HashSet::new();
    let Some(started_at) = parse(&intention.started_at) else {
        return Ok(set);
    };

    // An intention nobody ended can be days old; the detector runs every
    // minute, so the card scan is bounded rather than growing without limit.
    let cards_from = started_at.max(now - SUPPORT_LOOKBACK);
    for card in db.list_journal_activities(cards_from, now).await? {
        if !supports(&card, intention.id) {
            continue;
        }
        for value in [card.app_primary, card.app_secondary].into_iter().flatten() {
            if let Some(value) = normalise(&value) {
                set.insert(value);
            }
        }
    }

    let grace_end = (started_at + grace).min(now);
    if grace_end > started_at {
        for interval in db
            .list_journal_ledger_intervals(started_at, grace_end)
            .await?
        {
            for value in [
                Some(interval.title),
                interval.app_name,
                interval.parent_title,
            ]
            .into_iter()
            .flatten()
            {
                if let Some(value) = normalise(&value) {
                    set.insert(value);
                }
            }
        }
    }
    Ok(set)
}

fn supports(card: &JournalActivity, intention_id: i64) -> bool {
    card.intention_relation.as_deref() == Some(SUPPORTS_INTENTION)
        && card.intention_id.unwrap_or(intention_id) == intention_id
}

/// Name the divergence from the most recent card overlapping the tail. A card
/// the journal already classified is better evidence than anything this tick
/// can infer on its own; with no card at all, "other work" is the honest
/// answer, not "distraction".
async fn candidate_relation(
    db: &DatabaseManager,
    dominant: &DominantTask,
    tail_start: DateTime<Utc>,
    now: DateTime<Utc>,
) -> anyhow::Result<(String, String)> {
    let cards = db.list_journal_activities(tail_start, now).await?;
    let latest = cards.last();
    let Some(card) = latest else {
        return Ok((
            OTHER_WORK.to_string(),
            format!("{} is not part of this intention so far", dominant.title),
        ));
    };

    let category_id = card.category_id.as_str();
    let category_name = card
        .category
        .as_ref()
        .map(|category| category.name.clone())
        .unwrap_or_default();
    let relation = card.intention_relation.as_deref().unwrap_or("");

    if category_id == DISTRACTION_CATEGORY_ID
        || category_name.eq_ignore_ascii_case("distraction")
        || relation == POSSIBLE_DISTRACTION
    {
        return Ok((
            POSSIBLE_DISTRACTION.to_string(),
            format!("the last card, \"{}\", reads as a detour", card.title),
        ));
    }
    if category_id == PERSONAL_CATEGORY_ID
        || category_name.eq_ignore_ascii_case("personal")
        || relation == BREAK
    {
        return Ok((
            BREAK.to_string(),
            format!("the last card, \"{}\", reads as a break", card.title),
        ));
    }
    Ok((
        OTHER_WORK.to_string(),
        format!(
            "{} is real work, just not the intention",
            dominant
                .app
                .clone()
                .unwrap_or_else(|| dominant.title.clone())
        ),
    ))
}

// ---------- state plumbing ----------

async fn persist(
    db: &DatabaseManager,
    state: FocusStateDraft,
    dominant: Option<DominantTask>,
    classifier_ran: bool,
    run_recorded: bool,
) -> anyhow::Result<FocusTick> {
    db.set_focus_state(&state).await?;
    Ok(FocusTick {
        state,
        dominant,
        classifier_ran,
        run_recorded,
    })
}

/// The divergence timer, carried across ticks. A row written for a different
/// intention is ignored: stating a new intention starts a fresh clock.
fn carried_divergence(
    previous: Option<&FocusStateRecord>,
    intention_id: i64,
) -> Option<DateTime<Utc>> {
    let previous = previous?;
    if previous.intention_id != Some(intention_id) {
        return None;
    }
    previous.divergence_started_at.as_deref().and_then(parse)
}

/// The last verdict, reused while a provider-backed classifier is throttled.
fn carried_verdict(previous: Option<&FocusStateRecord>, intention_id: i64) -> Option<TailVerdict> {
    let previous = previous?;
    if previous.intention_id != Some(intention_id) || !is_divergent(&previous.relation) {
        return None;
    }
    Some(TailVerdict {
        relation: previous.relation.clone(),
        confidence: previous.confidence,
        reason: previous.reason.clone().unwrap_or_default(),
    })
}

/// At most one provider call per intention per [`CLASSIFIER_INTERVAL`]. In
/// process on purpose: this is a cost limiter, not a durable guarantee, and a
/// restart is allowed to ask again.
fn provider_call_due(intention_id: i64, now: DateTime<Utc>) -> bool {
    let mut guard = provider_throttle()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match *guard {
        Some((id, at)) if id == intention_id && now - at < CLASSIFIER_INTERVAL => false,
        _ => {
            *guard = Some((intention_id, now));
            true
        }
    }
}

/// Forget the throttle. Tests only: the map is process-global, so two tests
/// that both drive a provider-backed classifier would otherwise throttle each
/// other rather than the thing they are testing.
#[cfg(test)]
fn reset_provider_throttle() {
    provider_throttle()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
}

async fn record_tail_run(
    db: &DatabaseManager,
    classifier: &dyn TailClassifier,
    input: &TailInput,
    answer: &anyhow::Result<TailVerdict>,
    latency_ms: i64,
) {
    let request_chars = input.intention.title.len()
        + input
            .dominant
            .as_ref()
            .map(|task| task.title.len())
            .unwrap_or(0);
    let draft = JournalRunDraft {
        window_id: None,
        kind: "tail".to_string(),
        model: classifier.model(),
        prompt_version: classifier.prompt_version(),
        request_chars: request_chars as i64,
        response_chars: answer.as_ref().map(|v| v.reason.len() as i64).unwrap_or(0),
        latency_ms,
        ok: answer.is_ok(),
        error: answer.as_ref().err().map(|error| error.to_string()),
    };
    if let Err(error) = db.record_journal_run(&draft).await {
        warn!(%error, "focus tail run audit could not be written");
    }
}

fn provider_throttle() -> &'static Mutex<Option<(i64, DateTime<Utc>)>> {
    static LAST: OnceLock<Mutex<Option<(i64, DateTime<Utc>)>>> = OnceLock::new();
    LAST.get_or_init(|| Mutex::new(None))
}

fn normalise_relation(relation: &str) -> String {
    match relation {
        SUPPORTS_INTENTION | OTHER_WORK | BREAK | POSSIBLE_DISTRACTION => relation.to_string(),
        _ => UNKNOWN.to_string(),
    }
}

/// Identity for matching an app, host or document name. Case and surrounding
/// whitespace are noise; a `www.` prefix is the same host.
fn normalise(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_start_matches("www.").trim();
    (!trimmed.is_empty()).then(|| trimmed.to_lowercase())
}

fn parse(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::focus::NUDGE_MIN_CONFIDENCE;
    use screenpipe_config::DbConfig;
    use screenpipe_db::{JournalActivityDraft, NewFocusIntention};

    async fn test_db() -> (tempfile::TempDir, DatabaseManager) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("focus.db");
        for _ in 0..3 {
            match DatabaseManager::new(&path.to_string_lossy(), DbConfig::default()).await {
                Ok(db) => return (dir, db),
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
            }
        }
        panic!("test db failed to initialize");
    }

    fn settings(grace_minutes: i64) -> JournalSettings {
        JournalSettings {
            focus_grace_minutes: grace_minutes,
            ..JournalSettings::default()
        }
    }

    /// Capture is checked against the wall clock (`recent_capture`), so every
    /// fixture is anchored on the real now and counts backwards.
    async fn seed_frames(db: &DatabaseManager, from: DateTime<Utc>, until: DateTime<Utc>) {
        seed_frames_as(db, from, until, "Code", "auth.rs", "screen text").await;
    }

    /// Frames that say what was on screen. The compiled observations read
    /// window titles and screen text from here, not from the ledger, so a test
    /// about what a classifier sees has to seed them accordingly.
    async fn seed_frames_as(
        db: &DatabaseManager,
        from: DateTime<Utc>,
        until: DateTime<Utc>,
        app: &str,
        window: &str,
        text: &str,
    ) {
        let mut sql = String::new();
        let mut at = from;
        let mut index = 0;
        while at < until {
            sql.push_str(&format!(
                "INSERT INTO frames (timestamp, app_name, window_name, focused, content_hash, \
                 simhash, full_text) VALUES ('{}', '{app}', '{window}', 1, {index}, {index}, \
                 '{text}');",
                at.to_rfc3339()
            ));
            at += Duration::minutes(1);
            index += 1;
        }
        db.execute_raw_sql_write(&sql).await.unwrap();
    }

    /// A task-depth ledger interval, written directly so a rules test can
    /// state exactly what the tail looked like.
    async fn seed_interval(
        db: &DatabaseManager,
        key: &str,
        title: &str,
        app: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) {
        let sql = format!(
            "INSERT OR IGNORE INTO activity_tasks (task_key, kind, title, app_name, confidence, \
             producer) VALUES ('task-{key}', 'task', '{title}', '{app}', 1.0, 'test');\
             INSERT INTO activity_intervals (interval_key, task_id, start_at, end_at, state, \
             confidence, producer) VALUES ('{key}', \
             (SELECT id FROM activity_tasks WHERE task_key = 'task-{key}'), '{}', '{}', 'final', \
             1.0, 'test');",
            start.to_rfc3339(),
            end.to_rfc3339()
        );
        db.execute_raw_sql_write(&sql).await.unwrap();
    }

    #[allow(clippy::too_many_arguments)]
    async fn seed_card(
        db: &DatabaseManager,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        title: &str,
        category_id: &str,
        relation: Option<&str>,
        app_primary: Option<&str>,
        intention_id: Option<i64>,
    ) {
        let draft = JournalActivityDraft {
            activity_key: format!("{title}-{}", start.timestamp()),
            day: "2026-09-16".to_string(),
            start_at: start,
            end_at: end,
            active_minutes: (end - start).num_minutes() as f64,
            state: "final".to_string(),
            title: title.to_string(),
            summary: String::new(),
            detailed_summary: None,
            category_id: category_id.to_string(),
            category_confidence: 0.9,
            intention_id,
            intention_relation: relation.map(str::to_string),
            relation_confidence: relation.map(|_| 0.8),
            relation_reason: None,
            app_primary: app_primary.map(str::to_string),
            app_secondary: None,
            producer: "test".to_string(),
            prompt_version: None,
            model: None,
            window_id: None,
            distractions: Vec::new(),
            interval_keys: Vec::new(),
            evidence: Vec::new(),
        };
        db.replace_activities_in_range(start, end, &[draft])
            .await
            .unwrap();
    }

    async fn intention(db: &DatabaseManager, title: &str, started_at: DateTime<Utc>) -> i64 {
        db.create_focus_intention(
            &NewFocusIntention {
                title: title.to_string(),
                project: None,
                notes: None,
                source: "app".to_string(),
            },
            started_at,
        )
        .await
        .unwrap()
        .id
    }

    /// A classifier that claims to leave the machine, so the audit row and the
    /// throttle can be observed. Never used in production.
    struct ProviderClassifier;

    #[async_trait]
    impl TailClassifier for ProviderClassifier {
        fn name(&self) -> &'static str {
            "test-provider"
        }
        fn model(&self) -> Option<String> {
            Some("test-model".to_string())
        }
        fn calls_provider(&self) -> bool {
            true
        }
        async fn classify(&self, _input: &TailInput) -> anyhow::Result<TailVerdict> {
            Ok(TailVerdict {
                relation: POSSIBLE_DISTRACTION.to_string(),
                confidence: 0.9,
                reason: "the model says so".to_string(),
            })
        }
    }

    #[tokio::test]
    async fn without_an_intention_nothing_is_classified() {
        let (_dir, db) = test_db().await;
        let now = Utc::now();
        seed_frames(&db, now - Duration::minutes(9), now).await;

        let tick = run_focus_tick(&db, &settings(10), &RulesClassifier, now)
            .await
            .unwrap();
        assert_eq!(tick.state.relation, UNKNOWN);
        assert_eq!(tick.state.intention_id, None);
        assert_eq!(tick.state.reason.as_deref(), Some("no active intention"));
        assert!(tick.state.evidence_ok, "capture is live, only the intention is missing");
        assert!(!tick.classifier_ran);

        let persisted = db.get_focus_state().await.unwrap().unwrap();
        assert_eq!(persisted.relation, UNKNOWN);
    }

    #[tokio::test]
    async fn a_stalled_capture_is_unknown_never_distraction() {
        let (_dir, db) = test_db().await;
        let now = Utc::now();
        intention(&db, "Ship auth fix", now - Duration::minutes(60)).await;
        // No frames at all: capture is not producing anything.

        let tick = run_focus_tick(&db, &settings(10), &RulesClassifier, now)
            .await
            .unwrap();
        assert_eq!(tick.state.relation, UNKNOWN);
        assert!(!tick.state.evidence_ok);
        assert!(tick.state.reason.unwrap().contains("capture data is missing"));
    }

    #[tokio::test]
    async fn live_capture_with_an_empty_ledger_is_unknown_not_a_divergence() {
        let (_dir, db) = test_db().await;
        let now = Utc::now();
        intention(&db, "Ship auth fix", now - Duration::minutes(60)).await;
        seed_frames(&db, now - Duration::minutes(9), now).await;

        let tick = run_focus_tick(&db, &settings(10), &RulesClassifier, now)
            .await
            .unwrap();
        assert_eq!(tick.state.relation, UNKNOWN);
        assert!(tick.state.evidence_ok);
        assert!(tick.state.reason.unwrap().contains("ledger"));
    }

    #[tokio::test]
    async fn the_support_set_covers_what_was_observed_right_after_the_intention() {
        let (_dir, db) = test_db().await;
        let now = Utc::now();
        let started = now - Duration::minutes(40);
        intention(&db, "Ship auth fix", started).await;
        seed_frames(&db, now - Duration::minutes(9), now).await;
        // Five minutes inside the grace period, then the same app in the tail.
        seed_interval(&db, "grace", "auth.rs", "Code", started, started + Duration::minutes(5))
            .await;
        seed_interval(&db, "tail", "session.rs", "Code", now - Duration::minutes(8), now).await;

        let tick = run_focus_tick(&db, &settings(10), &RulesClassifier, now)
            .await
            .unwrap();
        assert_eq!(tick.state.relation, SUPPORTS_INTENTION);
        assert_eq!(tick.state.confidence, SUPPORT_CONFIDENCE);
        assert_eq!(tick.state.divergence_started_at, None);
        assert_eq!(tick.state.dominant_app.as_deref(), Some("Code"));
        assert_eq!(tick.state.dominant_task_title.as_deref(), Some("session.rs"));
        assert!(!tick.classifier_ran, "a supported tail never asks a classifier");
    }

    #[tokio::test]
    async fn a_supporting_card_also_defines_the_support_set() {
        let (_dir, db) = test_db().await;
        let now = Utc::now();
        let started = now - Duration::minutes(90);
        let id = intention(&db, "Ship auth fix", started).await;
        seed_frames(&db, now - Duration::minutes(9), now).await;
        seed_card(
            &db,
            started,
            started + Duration::minutes(30),
            "Worked on the auth service",
            "work",
            Some(SUPPORTS_INTENTION),
            Some("Zed"),
            Some(id),
        )
        .await;
        seed_interval(&db, "tail", "auth.rs", "Zed", now - Duration::minutes(8), now).await;

        let tick = run_focus_tick(&db, &settings(1), &RulesClassifier, now)
            .await
            .unwrap();
        assert_eq!(tick.state.relation, SUPPORTS_INTENTION);
    }

    #[tokio::test]
    async fn a_distraction_card_names_the_candidate_and_grace_holds_the_confidence_down() {
        let (_dir, db) = test_db().await;
        let now = Utc::now();
        let started = now - Duration::minutes(40);
        intention(&db, "Ship auth fix", started).await;
        seed_frames(&db, now - Duration::minutes(9), now).await;
        seed_interval(&db, "grace", "auth.rs", "Code", started, started + Duration::minutes(5))
            .await;
        seed_interval(&db, "tail", "r/rust", "Arc", now - Duration::minutes(8), now).await;
        seed_card(
            &db,
            now - Duration::minutes(8),
            now,
            "Scrolled Reddit",
            "distraction",
            None,
            Some("reddit.com"),
            None,
        )
        .await;

        let tick = run_focus_tick(&db, &settings(10), &RulesClassifier, now)
            .await
            .unwrap();
        assert_eq!(tick.state.relation, POSSIBLE_DISTRACTION);
        assert_eq!(tick.state.confidence, GRACE_CONFIDENCE);
        assert!(tick.state.reason.unwrap().contains("within grace period"));
        // The timer starts on the first divergent tick and is persisted.
        assert!(tick.state.divergence_started_at.is_some());
        assert!(!tick.classifier_ran, "inside the grace period nothing is asked");
    }

    #[tokio::test]
    async fn a_personal_card_is_a_break_and_no_card_at_all_is_other_work() {
        let (_dir, db) = test_db().await;
        let now = Utc::now();
        let started = now - Duration::minutes(40);
        intention(&db, "Ship auth fix", started).await;
        seed_frames(&db, now - Duration::minutes(9), now).await;
        seed_interval(&db, "grace", "auth.rs", "Code", started, started + Duration::minutes(5))
            .await;
        seed_interval(&db, "tail", "Inbox", "Mail", now - Duration::minutes(8), now).await;

        // No card overlapping the tail: other work, not a distraction.
        let tick = run_focus_tick(&db, &settings(10), &RulesClassifier, now)
            .await
            .unwrap();
        assert_eq!(tick.state.relation, OTHER_WORK);

        seed_card(
            &db,
            now - Duration::minutes(8),
            now,
            "Booked a flight",
            "personal",
            None,
            Some("Mail"),
            None,
        )
        .await;
        let tick = run_focus_tick(&db, &settings(10), &RulesClassifier, now)
            .await
            .unwrap();
        assert_eq!(tick.state.relation, BREAK);
    }

    #[tokio::test]
    async fn the_divergence_timer_survives_ticks_and_outliving_the_grace_period_raises_confidence() {
        let (_dir, db) = test_db().await;
        let now = Utc::now();
        let started = now - Duration::minutes(60);
        let id = intention(&db, "Ship auth fix", started).await;
        seed_frames(&db, now - Duration::minutes(9), now).await;
        seed_interval(&db, "grace", "auth.rs", "Code", started, started + Duration::minutes(5))
            .await;
        seed_interval(&db, "tail", "r/rust", "Arc", now - Duration::minutes(9), now).await;
        seed_card(
            &db,
            now - Duration::minutes(9),
            now,
            "Scrolled Reddit",
            "distraction",
            None,
            Some("reddit.com"),
            None,
        )
        .await;

        // A previous tick already saw this divergence, fifteen minutes ago.
        let divergence_started_at = now - Duration::minutes(15);
        db.set_focus_state(&FocusStateDraft {
            computed_at: now - Duration::minutes(1),
            intention_id: Some(id),
            relation: POSSIBLE_DISTRACTION.to_string(),
            confidence: GRACE_CONFIDENCE,
            divergence_started_at: Some(divergence_started_at),
            dominant_task_title: Some("r/rust".to_string()),
            dominant_app: Some("Arc".to_string()),
            evidence_ok: true,
            reason: Some("earlier tick".to_string()),
        })
        .await
        .unwrap();

        let tick = run_focus_tick(&db, &settings(10), &RulesClassifier, now)
            .await
            .unwrap();
        assert_eq!(tick.state.relation, POSSIBLE_DISTRACTION);
        assert_eq!(tick.state.confidence, DIVERGENCE_CONFIDENCE);
        assert!(tick.classifier_ran, "past the grace period the classifier decides");
        assert!(!tick.run_recorded, "a local rules run is not a provider call");
        assert_eq!(
            tick.state
                .divergence_started_at
                .unwrap()
                .timestamp(),
            divergence_started_at.timestamp(),
            "the timer keeps its original start"
        );
        assert!(tick.state.confidence < NUDGE_MIN_CONFIDENCE);
    }

    #[tokio::test]
    async fn stating_a_new_intention_restarts_the_divergence_timer() {
        let (_dir, db) = test_db().await;
        let now = Utc::now();
        let first = intention(&db, "Ship auth fix", now - Duration::minutes(90)).await;
        db.set_focus_state(&FocusStateDraft {
            computed_at: now - Duration::minutes(1),
            intention_id: Some(first),
            relation: POSSIBLE_DISTRACTION.to_string(),
            confidence: DIVERGENCE_CONFIDENCE,
            divergence_started_at: Some(now - Duration::minutes(45)),
            dominant_task_title: Some("r/rust".to_string()),
            dominant_app: Some("Arc".to_string()),
            evidence_ok: true,
            reason: Some("earlier tick".to_string()),
        })
        .await
        .unwrap();

        let started = now - Duration::minutes(30);
        intention(&db, "Write the plan", started).await;
        seed_frames(&db, now - Duration::minutes(9), now).await;
        seed_interval(&db, "grace", "plan.md", "Obsidian", started, started + Duration::minutes(5))
            .await;
        seed_interval(&db, "tail", "r/rust", "Arc", now - Duration::minutes(8), now).await;

        let tick = run_focus_tick(&db, &settings(10), &RulesClassifier, now)
            .await
            .unwrap();
        assert_eq!(tick.state.relation, OTHER_WORK);
        assert_eq!(tick.state.confidence, GRACE_CONFIDENCE);
        let divergence = tick.state.divergence_started_at.unwrap();
        assert!(
            (now - divergence).num_seconds() < 5,
            "the previous intention's clock must not carry over: {divergence}"
        );
    }

    #[tokio::test]
    #[serial_test::serial(focus_provider_throttle)]
    async fn a_provider_classifier_is_audited_and_throttled() {
        let (_dir, db) = test_db().await;
        let now = Utc::now();
        let started = now - Duration::minutes(60);
        let id = intention(&db, "Ship auth fix", started).await;
        seed_frames(&db, now - Duration::minutes(9), now).await;
        seed_interval(&db, "grace", "auth.rs", "Code", started, started + Duration::minutes(5))
            .await;
        seed_interval(&db, "tail", "r/rust", "Arc", now - Duration::minutes(9), now).await;
        db.set_focus_state(&FocusStateDraft {
            computed_at: now - Duration::minutes(1),
            intention_id: Some(id),
            relation: OTHER_WORK.to_string(),
            confidence: DIVERGENCE_CONFIDENCE,
            divergence_started_at: Some(now - Duration::minutes(20)),
            dominant_task_title: Some("r/rust".to_string()),
            dominant_app: Some("Arc".to_string()),
            evidence_ok: true,
            reason: Some("earlier tick".to_string()),
        })
        .await
        .unwrap();

        reset_provider_throttle();
        let tick = run_focus_tick(&db, &settings(10), &ProviderClassifier, now)
            .await
            .unwrap();
        assert!(tick.classifier_ran);
        assert!(tick.run_recorded);
        assert_eq!(tick.state.relation, POSSIBLE_DISTRACTION);
        assert_eq!(tick.state.confidence, 0.9);
        let run = db.last_journal_run().await.unwrap().unwrap();
        assert_eq!(run.kind, "tail");
        assert_eq!(run.model.as_deref(), Some("test-model"));
        assert!(run.ok);

        // A second tick a minute later must not spend another call; the last
        // answer is carried forward instead.
        let again = run_focus_tick(
            &db,
            &settings(10),
            &ProviderClassifier,
            now + Duration::minutes(1),
        )
        .await
        .unwrap();
        assert!(!again.classifier_ran, "throttled to one provider call per 5 min");
        assert_eq!(again.state.relation, POSSIBLE_DISTRACTION);
        assert_eq!(again.state.confidence, 0.9, "the previous verdict is carried");
    }

    /// A classifier that records what it was handed, so a detector test can
    /// assert on the input the provider would have seen.
    struct RecordingClassifier {
        seen: Arc<Mutex<Option<TailInput>>>,
    }

    #[async_trait]
    impl TailClassifier for RecordingClassifier {
        fn name(&self) -> &'static str {
            "test-recording"
        }
        fn calls_provider(&self) -> bool {
            true
        }
        async fn classify(&self, input: &TailInput) -> anyhow::Result<TailVerdict> {
            *self.seen.lock().unwrap() = Some(input.clone());
            Ok(TailVerdict {
                relation: OTHER_WORK.to_string(),
                confidence: 0.7,
                reason: "recorded".to_string(),
            })
        }
    }

    /// A classifier that always fails, like a provider that is down or a key
    /// that has expired.
    struct FailingClassifier;

    #[async_trait]
    impl TailClassifier for FailingClassifier {
        fn name(&self) -> &'static str {
            "test-failing"
        }
        fn model(&self) -> Option<String> {
            Some("test-model".to_string())
        }
        fn calls_provider(&self) -> bool {
            true
        }
        async fn classify(&self, _input: &TailInput) -> anyhow::Result<TailVerdict> {
            Err(anyhow::anyhow!("the ai provider rejected the credentials: 401"))
        }
    }

    #[tokio::test]
    #[serial_test::serial(focus_provider_throttle)]
    async fn a_provider_classifier_is_handed_the_compiled_tail() {
        let (_dir, db) = test_db().await;
        let now = Utc::now();
        let started = now - Duration::minutes(60);
        let id = intention(&db, "Ship auth fix", started).await;
        seed_frames_as(
            &db,
            now - Duration::minutes(9),
            now,
            "Arc",
            "r/observed-tail",
            "borrow checker memes",
        )
        .await;
        seed_interval(&db, "grace", "auth.rs", "Code", started, started + Duration::minutes(5))
            .await;
        // Deliberately wider than the tail: the compiled line must be clipped
        // to the ten minutes being classified, not to plan_window's 45-minute
        // card horizon.
        seed_interval(
            &db,
            "tail",
            "r/observed-tail",
            "Arc",
            now - Duration::minutes(40),
            now,
        )
        .await;
        db.set_focus_state(&FocusStateDraft {
            computed_at: now - Duration::minutes(1),
            intention_id: Some(id),
            relation: OTHER_WORK.to_string(),
            confidence: DIVERGENCE_CONFIDENCE,
            divergence_started_at: Some(now - Duration::minutes(20)),
            dominant_task_title: Some("r/observed-tail".to_string()),
            dominant_app: Some("Arc".to_string()),
            evidence_ok: true,
            reason: Some("earlier tick".to_string()),
        })
        .await
        .unwrap();

        reset_provider_throttle();
        let seen = Arc::new(Mutex::new(None));
        let classifier = RecordingClassifier { seen: seen.clone() };
        let tick = run_focus_tick(&db, &settings(10), &classifier, now)
            .await
            .unwrap();
        assert!(tick.classifier_ran);

        let input = seen.lock().unwrap().clone().expect("the classifier was called");
        assert_eq!(input.intention.title, "Ship auth fix");
        assert_eq!(input.candidate, OTHER_WORK);
        assert!(input.divergence_minutes >= 19.0);
        let window = input.observations.expect("a provider call gets observations");
        let line = crate::journal::prompt::render_observations(&window, 6);
        assert!(line.contains("r/observed-tail"), "{line}");
        assert!(line.contains("Arc"), "{line}");
        let earliest = window
            .intervals
            .iter()
            .map(|interval| interval.start_at)
            .min()
            .unwrap();
        assert!(
            earliest >= now - TAIL_WINDOW - Duration::seconds(1),
            "observations are clipped to the tail, got {earliest}"
        );
        let snippet_chars: usize = window
            .intervals
            .iter()
            .flat_map(|interval| interval.text_snippets.iter())
            .map(|snippet| snippet.chars().count())
            .sum();
        assert!(
            snippet_chars <= TAIL_SNIPPET_BUDGET,
            "screen text is budgeted: {snippet_chars}"
        );
    }

    #[tokio::test]
    #[serial_test::serial(focus_provider_throttle)]
    async fn a_classifier_error_degrades_to_the_candidate_and_is_audited() {
        let (_dir, db) = test_db().await;
        let now = Utc::now();
        let started = now - Duration::minutes(60);
        let id = intention(&db, "Ship auth fix", started).await;
        seed_frames(&db, now - Duration::minutes(9), now).await;
        seed_interval(&db, "grace", "auth.rs", "Code", started, started + Duration::minutes(5))
            .await;
        seed_interval(&db, "tail", "r/failing", "Arc", now - Duration::minutes(9), now).await;
        seed_card(
            &db,
            now - Duration::minutes(9),
            now,
            "Scrolled Reddit",
            "distraction",
            None,
            Some("reddit.com"),
            None,
        )
        .await;
        db.set_focus_state(&FocusStateDraft {
            computed_at: now - Duration::minutes(1),
            intention_id: Some(id),
            relation: POSSIBLE_DISTRACTION.to_string(),
            confidence: GRACE_CONFIDENCE,
            divergence_started_at: Some(now - Duration::minutes(25)),
            dominant_task_title: Some("r/failing".to_string()),
            dominant_app: Some("Arc".to_string()),
            evidence_ok: true,
            reason: Some("earlier tick".to_string()),
        })
        .await
        .unwrap();

        reset_provider_throttle();
        let tick = run_focus_tick(&db, &settings(10), &FailingClassifier, now)
            .await
            .unwrap();
        assert!(tick.classifier_ran);
        assert!(tick.run_recorded);
        // The deterministic candidate survives, at the grace ceiling — well
        // under the nudge bar, so a dead provider can never interrupt anyone.
        assert_eq!(tick.state.relation, POSSIBLE_DISTRACTION);
        assert_eq!(tick.state.confidence, GRACE_CONFIDENCE);
        assert!(tick.state.confidence < NUDGE_MIN_CONFIDENCE);
        assert!(tick.state.reason.unwrap().contains("classifier unavailable"));

        let run = db.last_journal_run().await.unwrap().unwrap();
        assert_eq!(run.kind, "tail");
        assert!(!run.ok);
        assert!(run.error.unwrap().contains("rejected the credentials"));
    }

    #[tokio::test]
    async fn an_override_holds_the_relation_and_stops_the_timer_until_it_expires() {
        let (_dir, db) = test_db().await;
        let now = Utc::now();
        let started = now - Duration::minutes(60);
        let id = intention(&db, "Ship auth fix", started).await;
        seed_frames(&db, now - Duration::minutes(9), now).await;
        seed_interval(&db, "grace", "auth.rs", "Code", started, started + Duration::minutes(5))
            .await;
        seed_interval(&db, "tail", "r/overridden", "Arc", now - Duration::minutes(9), now).await;
        seed_card(
            &db,
            now - Duration::minutes(9),
            now,
            "Scrolled Reddit",
            "distraction",
            None,
            Some("reddit.com"),
            None,
        )
        .await;
        db.set_focus_state(&FocusStateDraft {
            computed_at: now - Duration::minutes(1),
            intention_id: Some(id),
            relation: POSSIBLE_DISTRACTION.to_string(),
            confidence: GRACE_CONFIDENCE,
            divergence_started_at: Some(now - Duration::minutes(25)),
            dominant_task_title: Some("r/overridden".to_string()),
            dominant_app: Some("Arc".to_string()),
            evidence_ok: true,
            reason: Some("earlier tick".to_string()),
        })
        .await
        .unwrap();

        // Without an override the tail reads as a detour and the classifier
        // decides.
        let before = run_focus_tick(&db, &settings(10), &RulesClassifier, now)
            .await
            .unwrap();
        assert_eq!(before.state.relation, POSSIBLE_DISTRACTION);
        assert!(before.classifier_ran);

        // The user says it is fine. Two minutes rather than the route's
        // default thirty, so the expiry is observable without seeding an hour
        // of capture.
        let key = suppression::task_key(Some("r/overridden"), Some("Arc")).unwrap();
        suppression::suppress(id, &key, OTHER_WORK, now, now + Duration::minutes(2));

        let during = run_focus_tick(&db, &settings(10), &RulesClassifier, now)
            .await
            .unwrap();
        assert_eq!(during.state.relation, OTHER_WORK);
        assert_eq!(during.state.confidence, 1.0);
        assert_eq!(during.state.reason.as_deref(), Some(OVERRIDE_REASON));
        assert_eq!(during.state.divergence_started_at, None, "the timer is cleared");
        assert!(!during.classifier_ran, "an answered question is not asked again");
        // The nudge reads `possible_distraction` only, so this also silences it.
        assert!(matches!(
            super::super::nudge::maybe_nudge(
                &db,
                &JournalSettings { nudges_enabled: true, ..settings(10) },
                &during,
                now,
            )
            .await,
            Err(super::super::NudgeBlock::NotDistraction)
        ));

        // Once it expires the detector resumes where it left off.
        let after = run_focus_tick(
            &db,
            &settings(10),
            &RulesClassifier,
            now + Duration::minutes(3),
        )
        .await
        .unwrap();
        assert_eq!(after.state.relation, POSSIBLE_DISTRACTION);
        assert!(after.state.divergence_started_at.is_some(), "the clock restarts");

        // A different task under the same intention was never covered.
        let other = suppression::task_key(Some("r/something-else"), Some("Arc")).unwrap();
        assert!(suppression::active(id, &other, now).is_none());
    }

    #[tokio::test]
    async fn a_reconciled_ledger_produces_a_focus_state_row() {
        let (_dir, db) = test_db().await;
        let now = Utc::now();
        let started = now - Duration::minutes(10);
        intention(&db, "Ship auth fix", started).await;
        seed_frames(&db, started, now).await;
        // The real deterministic builder, not hand-written intervals.
        crate::activity_ledger::reconcile_range(&db, started - Duration::minutes(1), now)
            .await
            .unwrap();
        assert!(
            !db.list_journal_ledger_intervals(now - TAIL_WINDOW, now)
                .await
                .unwrap()
                .is_empty(),
            "the ledger must have something to read"
        );

        let tick = run_focus_tick(&db, &settings(10), &RulesClassifier, now)
            .await
            .unwrap();
        let persisted = db.get_focus_state().await.unwrap().unwrap();
        assert_eq!(persisted.relation, tick.state.relation);
        assert_eq!(persisted.relation, SUPPORTS_INTENTION);
        assert!(persisted.evidence_ok);
        assert_eq!(persisted.dominant_app.as_deref(), Some("Code"));
        assert!(persisted.divergence_started_at.is_none());
    }

    #[test]
    fn names_match_regardless_of_case_and_www() {
        assert_eq!(normalise("  GitHub.com "), Some("github.com".to_string()));
        assert_eq!(normalise("www.github.com"), Some("github.com".to_string()));
        assert_eq!(normalise("   "), None);
        let task = DominantTask {
            task_key: "t".to_string(),
            title: "auth.rs".to_string(),
            app: Some("Code".to_string()),
            parent_title: Some("screenpipe".to_string()),
            active_minutes: 4.0,
        };
        assert_eq!(task.identities(), vec!["auth.rs", "code", "screenpipe"]);
    }

    #[test]
    fn a_relation_outside_the_contract_degrades_to_unknown() {
        assert_eq!(normalise_relation("break"), BREAK);
        assert_eq!(normalise_relation("procrastinating"), UNKNOWN);
        assert!(is_divergent(POSSIBLE_DISTRACTION));
        assert!(!is_divergent(SUPPORTS_INTENTION));
        assert!(!is_divergent(UNKNOWN));
    }

    #[test]
    fn a_timer_written_for_another_intention_is_ignored() {
        let record = FocusStateRecord {
            computed_at: "2026-09-16T09:00:00+00:00".to_string(),
            intention_id: Some(7),
            relation: POSSIBLE_DISTRACTION.to_string(),
            confidence: 0.6,
            divergence_started_at: Some("2026-09-16T08:40:00+00:00".to_string()),
            dominant_task_title: None,
            dominant_app: None,
            evidence_ok: true,
            reason: Some("earlier".to_string()),
        };
        assert!(carried_divergence(Some(&record), 7).is_some());
        assert!(carried_divergence(Some(&record), 8).is_none());
        assert!(carried_divergence(None, 7).is_none());
        assert!(carried_verdict(Some(&record), 7).is_some());
        assert!(carried_verdict(Some(&record), 8).is_none());
    }
}
