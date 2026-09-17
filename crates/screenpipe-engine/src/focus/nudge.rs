// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com

//! The nudge: one notification, off by default, on top of the detector's row.
//!
//! Detection is the deliverable; this file is the thin layer at the end of it,
//! and it is written to be easy to prove silent. Eight conditions must all
//! hold, every one of them is a separate [`NudgeBlock`], and the confidence
//! threshold ([`NUDGE_MIN_CONFIDENCE`]) is above anything the deterministic
//! classifier will ever claim — so a user without a usable AI preset gets a
//! detector that cannot interrupt them at all, by construction. With
//! [`super::classifier::LlmTailClassifier`] installed the layer is reachable,
//! but only through a `possible_distraction` the model was at least 0.8 sure
//! of, past the grace period, outside a meeting, with `focusNudgesEnabled`
//! switched on — and it is off by default.
//!
//! Delivery goes to the desktop notify daemon (`POST 127.0.0.1:11435/notify`),
//! the same door `packages/screenpipe-mcp/src/notification-request.ts` knocks
//! on, so the app's existing gate applies: master switch, snooze, quiet hours,
//! repeat suppression. Nothing is delivered when the app is not running, and
//! nothing about that is an error — the CLI engine simply has no one to tell.
//!
//! The three actions never do anything on their own, and none of them touches
//! an app. "Back to it" opens the journal. The other two are the user
//! answering the question: they post their own verdict to
//! `POST /focus/state/override` as an `api` action, which the desktop routes
//! back into the local API with the user's bearer key
//! (`apps/screenpipe-app-tauri/lib/notifications/actions.ts`, `case "api"`).
//! That records `other_work` or `break` and stops asking about this task for
//! half an hour. Nothing is closed, nothing is blocked, no score is kept.

use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Duration, Utc};
use screenpipe_db::{DatabaseManager, FocusIntention, FocusStateDraft};
use serde_json::{json, Value};
use tracing::debug;

use crate::journal::settings::JournalSettings;

use super::detector::{FocusTick, TAIL_WINDOW};
use super::suppression::DEFAULT_OVERRIDE_MINUTES;
use super::{BREAK, OTHER_WORK, POSSIBLE_DISTRACTION};

/// The notify daemon the desktop app runs. Identical to the MCP package's
/// `NOTIFICATION_DAEMON_URL`.
pub const NOTIFY_URL: &str = "http://127.0.0.1:11435/notify";

/// Matches the MCP package's `NOTIFICATION_DAEMON_TIMEOUT_MS`. The detector
/// tick must never be held up by a daemon that is not answering.
pub const NOTIFY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Nothing below this fires. The deterministic classifier tops out at 0.6.
pub const NUDGE_MIN_CONFIDENCE: f64 = 0.8;

/// Minimum spacing between two nudges, whatever changes in between.
pub const NUDGE_COOLDOWN: Duration = Duration::minutes(30);

/// Notification type. Deliberately not `pipe`: this is not a pipe, and typing
/// it as one would put it behind the "Pipe notifications" toggle and the
/// per-pipe mute list. An unrecognised type still passes through the master /
/// snooze / quiet-hours gate, which is the gate the plan asks for.
pub const NOTIFY_TYPE: &str = "focus";

/// Where the two acknowledgement buttons post the user's own answer. A
/// relative path on purpose: the desktop's `api` action resolves it against
/// the local API base and refuses anything that is not local, so a
/// notification can never be talked into calling out to the internet with the
/// user's key attached.
pub const OVERRIDE_PATH: &str = "/focus/state/override";

/// Why a nudge did not happen. One variant per condition, so a test can prove
/// each condition blocks on its own and a log line says which one it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NudgeBlock {
    /// `focusNudgesEnabled` is off. The default.
    Disabled,
    NoIntention,
    NotDistraction,
    LowConfidence,
    WithinGrace,
    NoEvidence,
    MeetingInProgress,
    Cooldown,
    /// The daemon is not there — usually the CLI engine with no desktop app.
    NotDelivered,
}

impl std::fmt::Display for NudgeBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Disabled => "nudges are disabled",
            Self::NoIntention => "no active intention",
            Self::NotDistraction => "relation is not possible_distraction",
            Self::LowConfidence => "confidence below the nudge threshold",
            Self::WithinGrace => "divergence is still inside the grace period",
            Self::NoEvidence => "evidence is missing",
            Self::MeetingInProgress => "a meeting is in progress",
            Self::Cooldown => "a nudge was already shown recently",
            Self::NotDelivered => "the notification daemon did not accept it",
        };
        f.write_str(message)
    }
}

/// Everything the decision reads. Pure input: no clock, no database, no
/// network, so the whole condition matrix is a table test.
#[derive(Debug, Clone)]
pub struct NudgeConditions<'a> {
    pub nudges_enabled: bool,
    pub grace_minutes: i64,
    pub intention: Option<&'a FocusIntention>,
    pub state: &'a FocusStateDraft,
    pub meeting_in_progress: bool,
    pub last_nudged_at: Option<DateTime<Utc>>,
    pub now: DateTime<Utc>,
}

/// What would be shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NudgePayload {
    pub title: String,
    pub body: String,
}

/// Every condition, in the order the plan lists them.
pub fn decide_nudge(conditions: &NudgeConditions<'_>) -> Result<NudgePayload, NudgeBlock> {
    if !conditions.nudges_enabled {
        return Err(NudgeBlock::Disabled);
    }
    let intention = conditions.intention.ok_or(NudgeBlock::NoIntention)?;
    let state = conditions.state;
    if state.intention_id != Some(intention.id) {
        return Err(NudgeBlock::NoIntention);
    }
    if state.relation != POSSIBLE_DISTRACTION {
        return Err(NudgeBlock::NotDistraction);
    }
    if state.confidence < NUDGE_MIN_CONFIDENCE {
        return Err(NudgeBlock::LowConfidence);
    }
    if !state.evidence_ok {
        return Err(NudgeBlock::NoEvidence);
    }
    let divergence_started_at = state.divergence_started_at.ok_or(NudgeBlock::WithinGrace)?;
    let grace = Duration::minutes(conditions.grace_minutes.max(0));
    if conditions.now - divergence_started_at <= grace {
        return Err(NudgeBlock::WithinGrace);
    }
    if conditions.meeting_in_progress {
        return Err(NudgeBlock::MeetingInProgress);
    }
    if conditions
        .last_nudged_at
        .is_some_and(|at| conditions.now - at < NUDGE_COOLDOWN)
    {
        return Err(NudgeBlock::Cooldown);
    }

    Ok(NudgePayload {
        title: "Still on your intention?".to_string(),
        body: format!(
            "You set out to: {}. Last {} min: {}.",
            intention.title.trim(),
            TAIL_WINDOW.num_minutes(),
            state
                .dominant_task_title
                .clone()
                .or_else(|| state.dominant_app.clone())
                .unwrap_or_else(|| "something else".to_string()),
        ),
    })
}

/// The `/notify` body, in the daemon's own vocabulary
/// (`apps/screenpipe-app-tauri/src-tauri/src/notifications/routes.rs`).
pub fn notify_payload(payload: &NudgePayload) -> Value {
    json!({
        "title": payload.title,
        "body": payload.body,
        "type": NOTIFY_TYPE,
        "priority": "normal",
        "actions": [
            {
                "id": "focus-back-to-it",
                "label": "Back to it",
                "type": "deeplink",
                "url": "screenpipe://home?section=journal",
                "primary": true,
            },
            {
                "id": "focus-this-is-fine",
                "label": "This is fine",
                "type": "api",
                "url": OVERRIDE_PATH,
                "method": "POST",
                "body": { "relation": OTHER_WORK, "minutes": DEFAULT_OVERRIDE_MINUTES },
            },
            {
                "id": "focus-take-a-break",
                "label": "Take a break",
                "type": "api",
                "url": OVERRIDE_PATH,
                "method": "POST",
                "body": { "relation": BREAK, "minutes": DEFAULT_OVERRIDE_MINUTES },
            },
        ],
    })
}

/// Evaluate the conditions for this tick and, if they all hold, deliver.
///
/// Returns `Ok(true)` when the daemon accepted the notification. The two
/// cheap gates come first so a tick with nudges off (the default) costs
/// nothing at all.
pub async fn maybe_nudge(
    db: &DatabaseManager,
    settings: &JournalSettings,
    tick: &FocusTick,
    now: DateTime<Utc>,
) -> Result<bool, NudgeBlock> {
    if !settings.nudges_enabled {
        return Err(NudgeBlock::Disabled);
    }
    if tick.state.relation != POSSIBLE_DISTRACTION {
        return Err(NudgeBlock::NotDistraction);
    }

    let intention = db
        .active_focus_intention()
        .await
        .ok()
        .flatten()
        .ok_or(NudgeBlock::NoIntention)?;
    // Read-only, the same question `/meetings` answers: a nudge during a call
    // is an interruption of the call, not of a distraction.
    let meeting_in_progress = db.has_active_meeting().await.unwrap_or(false);

    let payload = decide_nudge(&NudgeConditions {
        nudges_enabled: settings.nudges_enabled,
        grace_minutes: settings.focus_grace_minutes,
        intention: Some(&intention),
        state: &tick.state,
        meeting_in_progress,
        last_nudged_at: last_nudged_at(),
        now,
    })?;

    // Recorded before delivery, not after: a daemon that hangs or refuses
    // must not turn into a nudge attempt every 60 seconds.
    record_nudge(now);
    deliver(&payload).await
}

/// POST to the desktop notify daemon. Absence of the daemon is not an error
/// worth logging loudly: the CLI engine runs without a desktop app.
async fn deliver(payload: &NudgePayload) -> Result<bool, NudgeBlock> {
    let client = reqwest::Client::builder()
        .timeout(NOTIFY_TIMEOUT)
        .build()
        .map_err(|_| NudgeBlock::NotDelivered)?;
    match client
        .post(NOTIFY_URL)
        .json(&notify_payload(payload))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => Ok(true),
        Ok(response) => {
            debug!(status = %response.status(), "focus nudge refused by the notify daemon");
            Err(NudgeBlock::NotDelivered)
        }
        Err(error) => {
            debug!(%error, "focus nudge could not reach the notify daemon");
            Err(NudgeBlock::NotDelivered)
        }
    }
}

/// When the last nudge was shown, in process. Deliberately not a column: the
/// cooldown is a politeness limit, not a durable guarantee, and a restart is
/// allowed to forget it.
fn nudge_clock() -> &'static Mutex<Option<DateTime<Utc>>> {
    static LAST: OnceLock<Mutex<Option<DateTime<Utc>>>> = OnceLock::new();
    LAST.get_or_init(|| Mutex::new(None))
}

fn last_nudged_at() -> Option<DateTime<Utc>> {
    *nudge_clock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn record_nudge(now: DateTime<Utc>) {
    *nudge_clock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(now);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::focus::detector::{DIVERGENCE_CONFIDENCE, GRACE_CONFIDENCE};
    use crate::focus::{OTHER_WORK, UNKNOWN};

    fn at(value: &str) -> DateTime<Utc> {
        value.parse().unwrap()
    }

    fn intention() -> FocusIntention {
        FocusIntention {
            id: 7,
            title: "Ship auth fix".to_string(),
            project: Some("screenpipe".to_string()),
            notes: None,
            started_at: "2026-09-16T08:00:00+00:00".to_string(),
            ended_at: None,
            source: "app".to_string(),
        }
    }

    /// A state that satisfies every condition, so each test can break exactly
    /// one of them.
    fn firing_state(now: DateTime<Utc>) -> FocusStateDraft {
        FocusStateDraft {
            computed_at: now,
            intention_id: Some(7),
            relation: POSSIBLE_DISTRACTION.to_string(),
            confidence: 0.85,
            divergence_started_at: Some(now - Duration::minutes(25)),
            dominant_task_title: Some("r/rust".to_string()),
            dominant_app: Some("Arc".to_string()),
            evidence_ok: true,
            reason: Some("the model says so".to_string()),
        }
    }

    fn conditions<'a>(
        intention: &'a FocusIntention,
        state: &'a FocusStateDraft,
        now: DateTime<Utc>,
    ) -> NudgeConditions<'a> {
        NudgeConditions {
            nudges_enabled: true,
            grace_minutes: 10,
            intention: Some(intention),
            state,
            meeting_in_progress: false,
            last_nudged_at: None,
            now,
        }
    }

    #[test]
    fn every_condition_together_produces_the_body_the_plan_specifies() {
        let now = at("2026-09-16T09:46:00Z");
        let intention = intention();
        let state = firing_state(now);
        let payload = decide_nudge(&conditions(&intention, &state, now)).unwrap();
        assert_eq!(
            payload.body,
            "You set out to: Ship auth fix. Last 10 min: r/rust."
        );
        assert!(!payload.title.is_empty());

        let json = notify_payload(&payload);
        assert_eq!(json["type"], NOTIFY_TYPE);
        let actions = json["actions"].as_array().unwrap();
        let labels: Vec<&str> = actions
            .iter()
            .map(|action| action["label"].as_str().unwrap())
            .collect();
        assert_eq!(labels, vec!["Back to it", "This is fine", "Take a break"]);
        // The first opens the journal; the other two record the user's own
        // answer through the route the contract documents. None of the three
        // does anything to an app.
        assert_eq!(actions[0]["type"], "deeplink");
        assert_eq!(actions[0]["url"], "screenpipe://home?section=journal");

        for (index, relation) in [(1, OTHER_WORK), (2, BREAK)] {
            let action = &actions[index];
            assert_eq!(action["type"], "api", "{relation}");
            assert_eq!(action["url"], OVERRIDE_PATH);
            assert_eq!(action["method"], "POST");
            assert_eq!(action["body"]["relation"], relation);
            assert_eq!(action["body"]["minutes"], DEFAULT_OVERRIDE_MINUTES);
            // Relative, so the desktop's `isLocalApiUrl` guard can never see
            // an off-box host here.
            assert!(
                action["url"].as_str().unwrap().starts_with('/'),
                "the override url must stay same-origin"
            );
        }
        assert_eq!(actions[1]["id"], "focus-this-is-fine");
        assert_eq!(actions[2]["id"], "focus-take-a-break");
    }

    #[test]
    fn each_condition_blocks_on_its_own() {
        let now = at("2026-09-16T09:46:00Z");
        let intention = intention();
        let state = firing_state(now);

        let case = |mutate: &dyn Fn(&mut NudgeConditions)| {
            let mut conditions = conditions(&intention, &state, now);
            mutate(&mut conditions);
            decide_nudge(&conditions).unwrap_err()
        };

        assert_eq!(
            case(&|c| c.nudges_enabled = false),
            NudgeBlock::Disabled,
            "off by default is the whole point"
        );
        assert_eq!(case(&|c| c.intention = None), NudgeBlock::NoIntention);
        assert_eq!(
            case(&|c| c.meeting_in_progress = true),
            NudgeBlock::MeetingInProgress
        );
        assert_eq!(
            case(&|c| c.last_nudged_at = Some(now - Duration::minutes(29))),
            NudgeBlock::Cooldown
        );
        // 30 minutes exactly is outside the cooldown.
        let mut fresh = conditions(&intention, &state, now);
        fresh.last_nudged_at = Some(now - NUDGE_COOLDOWN);
        assert!(decide_nudge(&fresh).is_ok());

        // State-shaped conditions need their own state values.
        let block = |state: FocusStateDraft| {
            let mut conditions = conditions(&intention, &state, now);
            conditions.state = &state;
            decide_nudge(&conditions).unwrap_err()
        };
        let mut other = firing_state(now);
        other.relation = OTHER_WORK.to_string();
        assert_eq!(block(other), NudgeBlock::NotDistraction);
        let mut unknown = firing_state(now);
        unknown.relation = UNKNOWN.to_string();
        assert_eq!(block(unknown), NudgeBlock::NotDistraction);
        let mut quiet = firing_state(now);
        quiet.confidence = DIVERGENCE_CONFIDENCE;
        assert_eq!(block(quiet), NudgeBlock::LowConfidence);
        let mut blind = firing_state(now);
        blind.evidence_ok = false;
        assert_eq!(block(blind), NudgeBlock::NoEvidence);
        let mut early = firing_state(now);
        early.divergence_started_at = Some(now - Duration::minutes(3));
        assert_eq!(block(early), NudgeBlock::WithinGrace);
        let mut timerless = firing_state(now);
        timerless.divergence_started_at = None;
        assert_eq!(block(timerless), NudgeBlock::WithinGrace);
        let mut stale = firing_state(now);
        stale.intention_id = Some(8);
        assert_eq!(block(stale), NudgeBlock::NoIntention);
    }

    /// The load-bearing property of this milestone: with the deterministic
    /// classifier installed the nudge layer is unreachable, whatever the user
    /// has switched on.
    #[test]
    fn the_rules_classifier_can_never_clear_the_confidence_bar() {
        assert!(DIVERGENCE_CONFIDENCE < NUDGE_MIN_CONFIDENCE);
        assert!(GRACE_CONFIDENCE < NUDGE_MIN_CONFIDENCE);

        let now = at("2026-09-16T09:46:00Z");
        let intention = intention();
        let mut state = firing_state(now);
        state.confidence = DIVERGENCE_CONFIDENCE;
        assert_eq!(
            decide_nudge(&conditions(&intention, &state, now)).unwrap_err(),
            NudgeBlock::LowConfidence
        );
    }
}
