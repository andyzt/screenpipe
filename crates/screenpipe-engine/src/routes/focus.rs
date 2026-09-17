// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com

//! `/focus/*` — the intention the user stated and how the last ten minutes
//! compare with it.
//!
//! Three writes and two reads, all small. The writes are the intention
//! lifecycle; the reads are the single `focus_state` row the tail detector
//! rewrites (`crate::focus::detector`) plus the intention list.
//!
//! Two rules this file exists to enforce:
//!
//! - **Stating or ending an intention resets the focus state.** Otherwise the
//!   UI keeps showing "distracted for 14 min" against an intention the user
//!   just replaced, which reads as an accusation about the wrong thing. The
//!   reset writes `unknown`; the next detector tick fills it in.
//! - **A missing state row is not an error.** Before the detector's first
//!   tick nothing is persisted, and the route synthesises the same `unknown`
//!   the detector would write, with a reason that says which of the two
//!   "unknowns" it is.
//! - **The user can always overrule the detector.** `POST
//!   /focus/state/override` is the one write that is not the detector's: it
//!   records "this is fine" or "I'm taking a break" against the task the state
//!   row currently names and stops the question being asked about that task for
//!   a while (`crate::focus::suppression`). An answer the product ignores is
//!   worse than never asking.
//!
//! Response shapes are fixed by `docs/JOURNAL_API_CONTRACT.md` (Focus).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json as JsonResponse,
};
use chrono::{DateTime, Utc};
use oasgen::{oasgen, OaSchema};
use screenpipe_db::{DatabaseManager, FocusStateDraft, FocusStateRecord, NewFocusIntention};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::error;

use crate::focus::suppression::{
    self, DEFAULT_OVERRIDE_MINUTES, MAX_OVERRIDE_MINUTES, OVERRIDE_REASON,
};
use crate::focus::{BREAK, OTHER_WORK, UNKNOWN};
use crate::routes::journal::JournalIntention;
use crate::server::AppState;

/// Title bounds. Long enough for a real sentence, short enough that the tray
/// line and the notification body can show it whole.
const MAX_TITLE_CHARS: usize = 200;

/// Where an intention came from. Mirrors the CHECK constraint on the column,
/// so a bad value is a 400 here rather than a 500 out of SQLite.
const SOURCES: [&str; 3] = ["app", "tray", "mcp"];

/// The only two relations a user may assert about themselves.
/// `supports_intention` would be the detector's job and `possible_distraction`
/// is nobody's business to be told; `unknown` is not an opinion.
const OVERRIDE_RELATIONS: [&str; 2] = [OTHER_WORK, BREAK];

type ApiError = (StatusCode, JsonResponse<Value>);

// ---------- GET /focus/status ----------

#[derive(Debug, Clone, Serialize, OaSchema)]
pub struct FocusStatusResponse {
    pub computed_at: String,
    pub intention: Option<JournalIntention>,
    pub relation: String,
    pub confidence: f64,
    pub divergence_started_at: Option<String>,
    /// Minutes since `divergence_started_at`, 0 when nothing is diverging.
    pub divergence_minutes: f64,
    pub dominant_task_title: Option<String>,
    pub dominant_app: Option<String>,
    pub evidence_ok: bool,
    pub reason: Option<String>,
}

#[oasgen]
pub async fn get_focus_status(
    State(state): State<Arc<AppState>>,
) -> Result<JsonResponse<FocusStatusResponse>, ApiError> {
    let now = Utc::now();
    let intention = state
        .db
        .active_focus_intention()
        .await
        .map_err(internal)?
        .map(JournalIntention::from);
    let persisted = state.db.get_focus_state().await.map_err(internal)?;

    Ok(JsonResponse(match persisted {
        Some(record) => status_from_record(record, intention, now),
        None => synthesised_status(intention, now),
    }))
}

/// The status served before the detector's first tick. Which of the two
/// "unknowns" this is matters to the reader, so it is said in the detector's
/// own words.
fn synthesised_status(
    intention: Option<JournalIntention>,
    now: DateTime<Utc>,
) -> FocusStatusResponse {
    let reason = if intention.is_some() {
        "detector has not run yet"
    } else {
        "no active intention"
    };
    FocusStatusResponse {
        computed_at: now.to_rfc3339(),
        intention,
        relation: UNKNOWN.to_string(),
        confidence: 0.0,
        divergence_started_at: None,
        divergence_minutes: 0.0,
        dominant_task_title: None,
        dominant_app: None,
        evidence_ok: false,
        reason: Some(reason.to_string()),
    }
}

/// The persisted row merged with the intention in force now.
fn status_from_record(
    record: FocusStateRecord,
    intention: Option<JournalIntention>,
    now: DateTime<Utc>,
) -> FocusStatusResponse {
    FocusStatusResponse {
        computed_at: record.computed_at,
        intention,
        relation: record.relation,
        confidence: record.confidence,
        divergence_minutes: divergence_minutes(record.divergence_started_at.as_deref(), now),
        divergence_started_at: record.divergence_started_at,
        dominant_task_title: record.dominant_task_title,
        dominant_app: record.dominant_app,
        evidence_ok: record.evidence_ok,
        reason: record.reason,
    }
}

// ---------- GET /focus/intentions ----------

#[derive(Debug, Deserialize, OaSchema)]
pub struct FocusIntentionsQuery {
    /// Only the intention that is still open. Default: false.
    #[serde(default)]
    pub active: bool,
    /// Newest first. Clamped to 1..=500.
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, OaSchema)]
pub struct FocusIntentionsResponse {
    pub intentions: Vec<JournalIntention>,
}

#[oasgen]
pub async fn list_focus_intentions(
    State(state): State<Arc<AppState>>,
    Query(query): Query<FocusIntentionsQuery>,
) -> Result<JsonResponse<FocusIntentionsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(20).clamp(1, 500);
    let now = Utc::now();
    let cutoff = state.history_access.cutoff(now);
    let intentions = state
        .db
        .list_focus_intentions(query.active, limit)
        .await
        .map_err(internal)?
        .into_iter()
        .filter(|intention| visible(intention.ended_at.as_deref(), cutoff))
        .map(JournalIntention::from)
        .collect();
    Ok(JsonResponse(FocusIntentionsResponse { intentions }))
}

/// Same clamp as every other journal read: an intention that ended before the
/// history cutoff is as invisible as the capture behind it. An open intention
/// is about now, so it always survives.
fn visible(ended_at: Option<&str>, cutoff: Option<DateTime<Utc>>) -> bool {
    match (cutoff, ended_at) {
        (Some(cutoff), Some(ended_at)) => parse(ended_at).is_none_or(|ended| ended >= cutoff),
        _ => true,
    }
}

// ---------- POST /focus/intentions ----------

#[derive(Debug, Deserialize, OaSchema)]
pub struct CreateFocusIntentionRequest {
    pub title: String,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    pub source: String,
}

#[oasgen]
pub async fn create_focus_intention(
    State(state): State<Arc<AppState>>,
    JsonResponse(payload): JsonResponse<CreateFocusIntentionRequest>,
) -> Result<JsonResponse<JournalIntention>, ApiError> {
    let draft = validate_intention(&payload)?;
    let now = Utc::now();
    let intention = state
        .db
        .create_focus_intention(&draft, now)
        .await
        .map_err(internal)?;

    reset_focus_state(
        &state.db,
        Some(intention.id),
        "detector has not run yet",
        now,
    )
    .await?;
    Ok(JsonResponse(JournalIntention::from(intention)))
}

/// Validate the body before it reaches SQLite: the title bounds and the
/// `source` CHECK constraint are both 400s, not 500s.
fn validate_intention(
    payload: &CreateFocusIntentionRequest,
) -> Result<NewFocusIntention, ApiError> {
    let title = payload.title.trim();
    if title.is_empty() || title.chars().count() > MAX_TITLE_CHARS {
        return Err(bad_request("title must be 1-200 characters"));
    }
    let source = payload.source.trim().to_lowercase();
    if !SOURCES.contains(&source.as_str()) {
        return Err(bad_request("source must be one of app, tray, mcp"));
    }
    Ok(NewFocusIntention {
        title: title.to_string(),
        project: trimmed(payload.project.as_deref()),
        notes: trimmed(payload.notes.as_deref()),
        source,
    })
}

// ---------- POST /focus/intentions/{id}/end ----------

#[oasgen]
pub async fn end_focus_intention(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<JsonResponse<JournalIntention>, ApiError> {
    let now = Utc::now();
    let Some(intention) = state
        .db
        .end_focus_intention(id, now)
        .await
        .map_err(internal)?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            JsonResponse(json!({"error": "intention not found"})),
        ));
    };

    // Ending the last open intention leaves nothing to diverge from. Ending an
    // older, already-closed one must not disturb a live state row.
    let active = state.db.active_focus_intention().await.map_err(internal)?;
    if active.is_none() {
        reset_focus_state(&state.db, None, "no active intention", now).await?;
    }
    Ok(JsonResponse(JournalIntention::from(intention)))
}

// ---------- POST /focus/state/override ----------

#[derive(Debug, Deserialize, OaSchema)]
pub struct FocusOverrideRequest {
    /// `other_work` or `break`.
    pub relation: String,
    /// How long to hold that answer for the task the state row names.
    /// 1..=240, default 30.
    #[serde(default)]
    pub minutes: Option<i64>,
}

/// Record the user's own judgment about what they are doing now.
///
/// Three effects, and the third is the point: the `focus_state` row says what
/// they said at confidence 1.0, the divergence timer is cleared, and the task
/// is suppressed for `minutes` so the detector stops re-litigating it. Without
/// the third, the next tick — sixty seconds later — would overwrite the answer
/// with the same verdict the user just rejected.
#[oasgen]
pub async fn override_focus_state(
    State(state): State<Arc<AppState>>,
    JsonResponse(payload): JsonResponse<FocusOverrideRequest>,
) -> Result<JsonResponse<FocusStatusResponse>, ApiError> {
    let (relation, minutes) = validate_override(&payload)?;
    apply_override(&state.db, &relation, minutes, Utc::now())
        .await
        .map(JsonResponse)
}

/// Everything the route does once the body is known good. Separated so the
/// whole effect — row, timer, suppression — is testable against a temporary
/// database without standing up an `AppState`.
async fn apply_override(
    db: &DatabaseManager,
    relation: &str,
    minutes: i64,
    now: DateTime<Utc>,
) -> Result<FocusStatusResponse, ApiError> {
    // Nothing to be "fine" about without an intention: the whole record is a
    // judgment relative to one, and the suppression is keyed by its id.
    let Some(intention) = db.active_focus_intention().await.map_err(internal)? else {
        return Err(bad_request("no active intention to override"));
    };

    // The task is whatever the detector last named. Reading it from the
    // persisted row rather than recomputing keeps the route free of the
    // ledger, and it is exactly the task the user was just shown.
    let previous = db.get_focus_state().await.map_err(internal)?;
    let (dominant_task_title, dominant_app) = previous
        .as_ref()
        .filter(|record| record.intention_id == Some(intention.id))
        .map(|record| {
            (
                record.dominant_task_title.clone(),
                record.dominant_app.clone(),
            )
        })
        .unwrap_or((None, None));

    if let Some(key) =
        suppression::task_key(dominant_task_title.as_deref(), dominant_app.as_deref())
    {
        suppression::suppress(
            intention.id,
            &key,
            relation,
            now,
            now + chrono::Duration::minutes(minutes),
        );
    }

    let draft = FocusStateDraft {
        computed_at: now,
        intention_id: Some(intention.id),
        relation: relation.to_string(),
        confidence: 1.0,
        divergence_started_at: None,
        dominant_task_title,
        dominant_app,
        // The user just told us what is happening. That is evidence.
        evidence_ok: true,
        reason: Some(OVERRIDE_REASON.to_string()),
    };
    db.set_focus_state(&draft).await.map_err(internal)?;

    Ok(FocusStatusResponse {
        computed_at: draft.computed_at.to_rfc3339(),
        intention: Some(JournalIntention::from(intention)),
        relation: draft.relation,
        confidence: draft.confidence,
        divergence_started_at: None,
        divergence_minutes: 0.0,
        dominant_task_title: draft.dominant_task_title,
        dominant_app: draft.dominant_app,
        evidence_ok: draft.evidence_ok,
        reason: draft.reason,
    })
}

/// Both bounds are 400s, and both messages name the accepted values: this
/// endpoint is called from a notification button and from MCP, and a caller
/// that guessed wrong should not have to read the source.
fn validate_override(payload: &FocusOverrideRequest) -> Result<(String, i64), ApiError> {
    let relation = payload.relation.trim().to_lowercase();
    if !OVERRIDE_RELATIONS.contains(&relation.as_str()) {
        return Err(bad_request("relation must be one of other_work, break"));
    }
    let minutes = payload.minutes.unwrap_or(DEFAULT_OVERRIDE_MINUTES);
    if !(1..=MAX_OVERRIDE_MINUTES).contains(&minutes) {
        return Err(bad_request("minutes must be between 1 and 240"));
    }
    Ok((relation, minutes))
}

// ---------- helpers ----------

/// Clear the detector's answer so the UI cannot show a divergence measured
/// against an intention that is no longer the one in force.
async fn reset_focus_state(
    db: &DatabaseManager,
    intention_id: Option<i64>,
    reason: &str,
    now: DateTime<Utc>,
) -> Result<(), ApiError> {
    db.set_focus_state(&FocusStateDraft {
        computed_at: now,
        intention_id,
        relation: UNKNOWN.to_string(),
        confidence: 0.0,
        divergence_started_at: None,
        dominant_task_title: None,
        dominant_app: None,
        evidence_ok: false,
        reason: Some(reason.to_string()),
    })
    .await
    .map_err(internal)
}

fn divergence_minutes(divergence_started_at: Option<&str>, now: DateTime<Utc>) -> f64 {
    divergence_started_at
        .and_then(parse)
        .map(|started| ((now - started).num_milliseconds() as f64 / 60_000.0).max(0.0))
        .unwrap_or(0.0)
}

fn trimmed(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn parse(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn bad_request(message: &str) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        JsonResponse(json!({ "error": message })),
    )
}

fn internal(error: sqlx::Error) -> ApiError {
    error!(%error, "focus query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        JsonResponse(json!({"error": "focus query failed"})),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use screenpipe_config::DbConfig;

    async fn test_db() -> (tempfile::TempDir, DatabaseManager) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("focus-routes.db");
        for _ in 0..3 {
            match DatabaseManager::new(&path.to_string_lossy(), DbConfig::default()).await {
                Ok(db) => return (dir, db),
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
            }
        }
        panic!("test db failed to initialize");
    }

    fn at(value: &str) -> DateTime<Utc> {
        value.parse().unwrap()
    }

    fn request(title: &str, source: &str) -> CreateFocusIntentionRequest {
        CreateFocusIntentionRequest {
            title: title.to_string(),
            project: Some("  screenpipe  ".to_string()),
            notes: Some("   ".to_string()),
            source: source.to_string(),
        }
    }

    fn status(error: &ApiError) -> StatusCode {
        error.0
    }

    #[test]
    fn a_title_must_be_one_to_two_hundred_characters() {
        assert!(validate_intention(&request("Ship auth fix", "app")).is_ok());
        assert!(validate_intention(&request(&"a".repeat(200), "app")).is_ok());
        for bad in ["", "   ", &"a".repeat(201)] {
            let error = validate_intention(&request(bad, "app")).unwrap_err();
            assert_eq!(status(&error), StatusCode::BAD_REQUEST, "title {bad:?}");
        }
        // Characters, not bytes: a 200-character title of multi-byte glyphs is
        // still a 200-character title.
        assert!(validate_intention(&request(&"é".repeat(200), "app")).is_ok());
    }

    #[test]
    fn the_source_must_be_one_the_column_accepts() {
        for source in ["app", "tray", "mcp", "  APP  "] {
            let draft = validate_intention(&request("Ship auth fix", source)).unwrap();
            assert_eq!(draft.source, source.trim().to_lowercase());
        }
        for source in ["", "cli", "pipe"] {
            let error = validate_intention(&request("Ship auth fix", source)).unwrap_err();
            assert_eq!(status(&error), StatusCode::BAD_REQUEST, "source {source:?}");
        }
    }

    #[test]
    fn blank_optional_fields_become_null_rather_than_empty_strings() {
        let draft = validate_intention(&request("Ship auth fix", "app")).unwrap();
        assert_eq!(draft.title, "Ship auth fix");
        assert_eq!(draft.project.as_deref(), Some("screenpipe"));
        assert_eq!(draft.notes, None);
    }

    #[test]
    fn a_missing_state_row_is_answered_not_errored() {
        let now = at("2026-09-16T09:46:00Z");
        let synthesised = synthesised_status(None, now);
        assert_eq!(synthesised.relation, UNKNOWN);
        assert_eq!(synthesised.reason.as_deref(), Some("no active intention"));
        assert!(!synthesised.evidence_ok);
        assert_eq!(synthesised.divergence_minutes, 0.0);

        let intention = JournalIntention {
            id: 7,
            title: "Ship auth fix".to_string(),
            project: None,
            notes: None,
            started_at: "2026-09-16T08:00:00+00:00".to_string(),
            ended_at: None,
            source: "app".to_string(),
        };
        let waiting = synthesised_status(Some(intention), now);
        assert_eq!(waiting.relation, UNKNOWN);
        assert_eq!(waiting.reason.as_deref(), Some("detector has not run yet"));
        assert!(waiting.intention.is_some());
    }

    #[test]
    fn divergence_minutes_are_derived_from_the_persisted_start() {
        let now = at("2026-09-16T09:46:00Z");
        let record = FocusStateRecord {
            computed_at: "2026-09-16T09:45:00+00:00".to_string(),
            intention_id: Some(7),
            relation: "possible_distraction".to_string(),
            confidence: 0.6,
            divergence_started_at: Some("2026-09-16T09:31:00+00:00".to_string()),
            dominant_task_title: Some("r/rust".to_string()),
            dominant_app: Some("Arc".to_string()),
            evidence_ok: true,
            reason: Some("earlier".to_string()),
        };
        let response = status_from_record(record, None, now);
        assert_eq!(response.divergence_minutes, 15.0);
        assert_eq!(response.confidence, 0.6);
        assert_eq!(response.dominant_app.as_deref(), Some("Arc"));

        // A clock that moved backwards must not produce a negative duration.
        assert_eq!(
            divergence_minutes(Some("2026-09-16T10:00:00+00:00"), now),
            0.0
        );
        assert_eq!(divergence_minutes(None, now), 0.0);
        assert_eq!(divergence_minutes(Some("not a timestamp"), now), 0.0);
    }

    #[test]
    fn an_intention_that_ended_before_the_cutoff_is_invisible() {
        let cutoff = Some(at("2026-09-16T09:00:00Z"));
        assert!(
            visible(None, cutoff),
            "an open intention is always about now"
        );
        assert!(visible(Some("2026-09-16T09:30:00+00:00"), cutoff));
        assert!(!visible(Some("2026-09-16T08:30:00+00:00"), cutoff));
        // No policy, no clamp.
        assert!(visible(Some("2020-01-01T00:00:00+00:00"), None));
    }

    fn override_request(relation: &str, minutes: Option<i64>) -> FocusOverrideRequest {
        FocusOverrideRequest {
            relation: relation.to_string(),
            minutes,
        }
    }

    #[test]
    fn an_override_only_accepts_the_two_relations_a_user_can_assert() {
        for relation in ["other_work", "break", "  BREAK  "] {
            let (parsed, minutes) = validate_override(&override_request(relation, None)).unwrap();
            assert_eq!(parsed, relation.trim().to_lowercase());
            assert_eq!(minutes, DEFAULT_OVERRIDE_MINUTES);
        }
        // The detector's own verdicts are not the user's to assert, and an
        // empty string is not an opinion.
        for relation in [
            "",
            "supports_intention",
            "possible_distraction",
            "unknown",
            "nope",
        ] {
            let error = validate_override(&override_request(relation, None)).unwrap_err();
            assert_eq!(
                status(&error),
                StatusCode::BAD_REQUEST,
                "relation {relation:?}"
            );
        }
    }

    #[test]
    fn override_minutes_are_bounded() {
        for minutes in [1, 30, MAX_OVERRIDE_MINUTES] {
            let (_, parsed) = validate_override(&override_request("break", Some(minutes))).unwrap();
            assert_eq!(parsed, minutes);
        }
        for minutes in [0, -1, MAX_OVERRIDE_MINUTES + 1, 100_000] {
            let error = validate_override(&override_request("break", Some(minutes))).unwrap_err();
            assert_eq!(status(&error), StatusCode::BAD_REQUEST, "minutes {minutes}");
        }
    }

    #[tokio::test]
    async fn an_override_records_the_users_answer_and_suppresses_the_task() {
        let (_dir, db) = test_db().await;
        let now = at("2026-09-16T09:46:00Z");

        // Nothing to override before an intention exists.
        let error = apply_override(&db, "other_work", 30, now)
            .await
            .unwrap_err();
        assert_eq!(status(&error), StatusCode::BAD_REQUEST);

        let draft = validate_intention(&request("Ship auth fix", "app")).unwrap();
        let intention = db.create_focus_intention(&draft, now).await.unwrap();
        // The detector has been calling this a detour for twenty minutes.
        db.set_focus_state(&FocusStateDraft {
            computed_at: now,
            intention_id: Some(intention.id),
            relation: "possible_distraction".to_string(),
            confidence: 0.85,
            divergence_started_at: Some(now - chrono::Duration::minutes(20)),
            dominant_task_title: Some("route-override-task".to_string()),
            dominant_app: Some("Arc".to_string()),
            evidence_ok: true,
            reason: Some("a detour".to_string()),
        })
        .await
        .unwrap();

        let response = apply_override(&db, "other_work", 30, now).await.unwrap();
        assert_eq!(response.relation, "other_work");
        assert_eq!(response.confidence, 1.0);
        assert_eq!(response.reason.as_deref(), Some(OVERRIDE_REASON));
        assert_eq!(response.divergence_started_at, None);
        assert_eq!(response.divergence_minutes, 0.0);
        // The same shape `GET /focus/status` returns, task and intention kept.
        assert_eq!(
            response.dominant_task_title.as_deref(),
            Some("route-override-task")
        );
        assert_eq!(response.dominant_app.as_deref(), Some("Arc"));
        assert_eq!(response.intention.map(|i| i.id), Some(intention.id));
        assert!(response.evidence_ok);

        let persisted = db.get_focus_state().await.unwrap().unwrap();
        assert_eq!(persisted.relation, "other_work");
        assert_eq!(persisted.confidence, 1.0);
        assert!(persisted.divergence_started_at.is_none());

        // …and the detector is told to stop asking about that task.
        let key = suppression::task_key(Some("route-override-task"), Some("Arc")).unwrap();
        assert_eq!(
            suppression::active(intention.id, &key, now).as_deref(),
            Some("other_work")
        );
        assert!(
            suppression::active(intention.id, &key, now + chrono::Duration::minutes(30)).is_none(),
            "the suppression expires with the window the caller asked for"
        );
    }

    #[tokio::test]
    async fn stating_and_ending_an_intention_clears_a_stale_divergence() {
        let (_dir, db) = test_db().await;
        let now = at("2026-09-16T09:46:00Z");

        let draft = validate_intention(&request("Ship auth fix", "app")).unwrap();
        let intention = db.create_focus_intention(&draft, now).await.unwrap();
        reset_focus_state(&db, Some(intention.id), "detector has not run yet", now)
            .await
            .unwrap();

        // The detector then finds a divergence.
        db.set_focus_state(&FocusStateDraft {
            computed_at: now,
            intention_id: Some(intention.id),
            relation: "possible_distraction".to_string(),
            confidence: 0.6,
            divergence_started_at: Some(now),
            dominant_task_title: Some("r/rust".to_string()),
            dominant_app: Some("Arc".to_string()),
            evidence_ok: true,
            reason: Some("a detour".to_string()),
        })
        .await
        .unwrap();

        // Replacing the intention must not leave that divergence on screen.
        let second = db
            .create_focus_intention(&draft, now + chrono::Duration::minutes(5))
            .await
            .unwrap();
        reset_focus_state(&db, Some(second.id), "detector has not run yet", now)
            .await
            .unwrap();
        let state = db.get_focus_state().await.unwrap().unwrap();
        assert_eq!(state.relation, UNKNOWN);
        assert_eq!(state.intention_id, Some(second.id));
        assert!(state.divergence_started_at.is_none());

        // Ending is idempotent, and the second end must not move `ended_at`.
        let ended = db
            .end_focus_intention(second.id, now + chrono::Duration::minutes(10))
            .await
            .unwrap()
            .unwrap();
        let again = db
            .end_focus_intention(second.id, now + chrono::Duration::minutes(20))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ended.ended_at, again.ended_at);
        assert!(db.active_focus_intention().await.unwrap().is_none());

        reset_focus_state(&db, None, "no active intention", now)
            .await
            .unwrap();
        let state = db.get_focus_state().await.unwrap().unwrap();
        assert_eq!(state.relation, UNKNOWN);
        assert_eq!(state.intention_id, None);
        assert_eq!(state.reason.as_deref(), Some("no active intention"));

        // Ending an id that does not exist is a 404, not a reset.
        assert!(db.end_focus_intention(9_999, now).await.unwrap().is_none());
    }
}
