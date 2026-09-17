// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com

//! `/journal/feedback`, `/journal/reviews` and the per-card feedback write —
//! the two places the user answers back.
//!
//! The rest of `/journal/*` is the machine's account of the day. These four
//! routes are the person's, and they follow two rules the read routes do not:
//!
//! - **What the user said is kept, whatever happens to the card.** A thumb is
//!   stored with a snapshot of the card it judged (`screenpipe_db`'s
//!   `journal_activity_feedback`), so a rewritten window cannot erase it and
//!   `journal-eval export` can still read it back as a label.
//! - **Ratings never overlap.** A `PUT /journal/reviews` splits, trims or
//!   replaces whatever it covers, exactly as dropping a block on a calendar
//!   would. The invariant is held in one database function; this file only
//!   validates the span before handing it over.
//!
//! Rating a span deliberately does not regenerate anything. The rating reaches
//! the next generation of the card through the compiler's `User review`
//! evidence block; until then the card's text is what the model wrote, and the
//! user asks for a rewrite with `POST /journal/regenerate` when they want the
//! words to follow.
//!
//! Response shapes are fixed by `docs/JOURNAL_API_CONTRACT.md` (*Card
//! feedback*, *Review ratings*).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json as JsonResponse,
};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use oasgen::{oasgen, OaSchema};
use screenpipe_db::{
    JournalActivityFeedback, JournalReviewRating, FEEDBACK_RATINGS, MAX_FEEDBACK_NOTE_CHARS,
    MAX_REVIEW_SPAN_HOURS, REVIEW_RATINGS, REVIEW_SOURCES,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::error;

use crate::journal::day::{attach_card_apps, to_card, ActivityCard};
use crate::journal::time::{day_bounds, day_of};
use crate::server::AppState;

/// Where a rating came from when the client does not say. The desktop app is
/// the overwhelming majority of writes; the MCP tool names itself.
pub const DEFAULT_REVIEW_SOURCE: &str = "app";

type ApiError = (StatusCode, JsonResponse<Value>);

// ---------- shapes ----------

/// One reviewed span, as every journal route serves it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, OaSchema)]
pub struct ReviewRatingItem {
    pub id: i64,
    pub start_at: String,
    pub end_at: String,
    pub rating: String,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, OaSchema)]
pub struct JournalReviewsResponse {
    pub items: Vec<ReviewRatingItem>,
}

/// The ratings of one day, clipped to it: a rating that straddles local 04:00
/// belongs to both days and each day shows its own half, so a client can paint
/// the day's timeline without doing the arithmetic again.
pub fn review_items(
    ratings: &[JournalReviewRating],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Vec<ReviewRatingItem> {
    let mut items: Vec<ReviewRatingItem> = ratings
        .iter()
        .filter_map(|rating| {
            let start = rating.start_at.max(from);
            let end = rating.end_at.min(to);
            (end > start).then(|| ReviewRatingItem {
                id: rating.id,
                start_at: start.to_rfc3339(),
                end_at: end.to_rfc3339(),
                rating: rating.rating.clone(),
                source: rating.source.clone(),
                created_at: rating.created_at.clone(),
                updated_at: rating.updated_at.clone(),
            })
        })
        .collect();
    items.sort_by(|left, right| {
        left.start_at
            .cmp(&right.start_at)
            .then(left.id.cmp(&right.id))
    });
    items
}

#[derive(Debug, Clone, Serialize, OaSchema)]
pub struct JournalFeedbackItem {
    pub activity_id: i64,
    pub rating: String,
    pub note: Option<String>,
    pub title: String,
    pub start_at: String,
    pub end_at: String,
    pub category_id: String,
    pub created_at: String,
}

impl From<JournalActivityFeedback> for JournalFeedbackItem {
    fn from(row: JournalActivityFeedback) -> Self {
        Self {
            activity_id: row.activity_id,
            rating: row.rating,
            note: row.note,
            title: row.title,
            start_at: row.start_at,
            end_at: row.end_at,
            category_id: row.category_id,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, OaSchema)]
pub struct JournalFeedbackResponse {
    pub items: Vec<JournalFeedbackItem>,
}

#[derive(Debug, Clone, Default, Deserialize, OaSchema)]
pub struct JournalDateQuery {
    /// Calendar date of the day's local 04:00 start. Without it, feedback
    /// returns the most recent rows and reviews the current day.
    #[serde(default)]
    pub date: Option<String>,
}

// ---------- PUT /journal/activities/{id}/feedback ----------

#[derive(Debug, Clone, Default, Deserialize, OaSchema)]
pub struct JournalFeedbackRequest {
    /// `"up"`, `"down"`, or `null` to clear the rating.
    #[serde(default)]
    pub rating: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[oasgen]
pub async fn put_journal_activity_feedback(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    JsonResponse(payload): JsonResponse<JournalFeedbackRequest>,
) -> Result<JsonResponse<ActivityCard>, ApiError> {
    let rating = match payload.rating.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(rating) if FEEDBACK_RATINGS.contains(&rating) => Some(rating.to_string()),
        Some(_) => return Err(bad_request("rating must be \"up\", \"down\" or null")),
    };
    if let Some(note) = payload.note.as_deref() {
        if note.chars().count() > MAX_FEEDBACK_NOTE_CHARS {
            return Err(bad_request("note must be 500 characters or fewer"));
        }
    }

    match rating {
        Some(rating) => {
            if !state
                .db
                .upsert_journal_activity_feedback(id, &rating, payload.note.as_deref())
                .await
                .map_err(internal)?
            {
                return Err(not_found());
            }
        }
        // Clearing a card that was never rated is not an error; the 404 below
        // still covers a card that is gone.
        None => state
            .db
            .clear_journal_activity_feedback(id)
            .await
            .map_err(internal)?,
    }

    Ok(JsonResponse(card_response(&state, id).await?))
}

/// The card as `GET /journal/activities/{id}` would serve it, so a client can
/// swap the rated card in place instead of refetching the day.
async fn card_response(state: &Arc<AppState>, id: i64) -> Result<ActivityCard, ApiError> {
    let categories = state.db.list_journal_categories().await.map_err(internal)?;
    let Some((activity, _, _)) = state
        .db
        .get_journal_activity(id, false)
        .await
        .map_err(internal)?
    else {
        return Err(not_found());
    };
    let mut card = to_card(activity, &categories);
    let (Some(start), Some(end)) = (parse(&card.start_at), parse(&card.end_at)) else {
        return Ok(card);
    };
    // The card's own span decides visibility, exactly as on the read route: a
    // restricted account cannot reach a card it may not see by rating it.
    if let Some(cutoff) = state.history_access.cutoff(Utc::now()) {
        if end < cutoff {
            return Err(not_found());
        }
    }
    let linked = state
        .db
        .list_journal_activity_intervals(start, end)
        .await
        .map_err(internal)?;
    attach_card_apps(std::slice::from_mut(&mut card), &linked);
    Ok(card)
}

// ---------- GET /journal/feedback ----------

#[oasgen]
pub async fn get_journal_feedback(
    State(state): State<Arc<AppState>>,
    Query(query): Query<JournalDateQuery>,
) -> Result<JsonResponse<JournalFeedbackResponse>, ApiError> {
    let date = match query.date.as_deref() {
        Some(value) => Some(parse_date(value)?),
        None => None,
    };
    let items = state
        .db
        .list_journal_feedback(date.map(|date| date.to_string()).as_deref())
        .await
        .map_err(internal)?
        .into_iter()
        .map(JournalFeedbackItem::from)
        .collect();
    Ok(JsonResponse(JournalFeedbackResponse { items }))
}

// ---------- GET /journal/reviews ----------

#[oasgen]
pub async fn get_journal_reviews(
    State(state): State<Arc<AppState>>,
    Query(query): Query<JournalDateQuery>,
) -> Result<JsonResponse<JournalReviewsResponse>, ApiError> {
    let now = Utc::now();
    let date = match query.date.as_deref() {
        Some(value) => parse_date(value)?,
        None => day_of(now),
    };
    Ok(JsonResponse(JournalReviewsResponse {
        items: day_reviews(&state, date, now).await?,
    }))
}

// ---------- PUT /journal/reviews ----------

#[derive(Debug, Clone, Deserialize, OaSchema)]
pub struct JournalReviewRequest {
    pub start_at: String,
    pub end_at: String,
    /// One of the three ratings, or `null` to clear the span.
    #[serde(default)]
    pub rating: Option<String>,
    /// `"app"` (the default) or `"mcp"`.
    #[serde(default)]
    pub source: Option<String>,
}

#[oasgen]
pub async fn put_journal_reviews(
    State(state): State<Arc<AppState>>,
    JsonResponse(payload): JsonResponse<JournalReviewRequest>,
) -> Result<JsonResponse<JournalReviewsResponse>, ApiError> {
    let (start_at, end_at, rating, source) = validate_review(&payload)?;
    state
        .db
        .apply_review_rating(start_at, end_at, rating.as_deref(), &source)
        .await
        .map_err(internal)?;
    let now = Utc::now();
    Ok(JsonResponse(JournalReviewsResponse {
        items: day_reviews(&state, day_of(start_at), now).await?,
    }))
}

/// Everything `PUT /journal/reviews` refuses, in one pure function so the
/// rules are tested without a database.
fn validate_review(
    payload: &JournalReviewRequest,
) -> Result<(DateTime<Utc>, DateTime<Utc>, Option<String>, String), ApiError> {
    let start_at =
        parse(payload.start_at.trim()).ok_or_else(|| bad_request("start_at must be RFC3339"))?;
    let end_at =
        parse(payload.end_at.trim()).ok_or_else(|| bad_request("end_at must be RFC3339"))?;
    if end_at <= start_at {
        return Err(bad_request("start_at must be before end_at"));
    }
    if end_at - start_at > Duration::hours(MAX_REVIEW_SPAN_HOURS) {
        return Err(bad_request("a review span may not exceed 24 hours"));
    }
    let rating = match payload.rating.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(rating) if REVIEW_RATINGS.contains(&rating) => Some(rating.to_string()),
        Some(_) => {
            return Err(bad_request(
                "rating must be \"focused\", \"neutral\", \"distracted\" or null",
            ))
        }
    };
    let source = match payload.source.as_deref().map(str::trim) {
        None | Some("") => DEFAULT_REVIEW_SOURCE.to_string(),
        Some(source) if REVIEW_SOURCES.contains(&source) => source.to_string(),
        Some(_) => return Err(bad_request("source must be \"app\" or \"mcp\"")),
    };
    Ok((start_at, end_at, rating, source))
}

// ---------- helpers ----------

/// The ratings of one journal day, clamped by the history-access policy the
/// same way the day route is.
async fn day_reviews(
    state: &Arc<AppState>,
    date: NaiveDate,
    now: DateTime<Utc>,
) -> Result<Vec<ReviewRatingItem>, ApiError> {
    let (day_start, day_end) =
        day_bounds(date).ok_or_else(|| bad_request("date is not a representable local day"))?;
    let read_start = match state.history_access.cutoff(now) {
        Some(cutoff) if day_end < cutoff => return Ok(Vec::new()),
        Some(cutoff) => day_start.max(cutoff),
        None => day_start,
    };
    let ratings = state
        .db
        .list_review_ratings(read_start, day_end)
        .await
        .map_err(internal)?;
    Ok(review_items(&ratings, read_start, day_end))
}

fn parse_date(value: &str) -> Result<NaiveDate, ApiError> {
    value
        .trim()
        .parse::<NaiveDate>()
        .map_err(|_| bad_request("date must be YYYY-MM-DD"))
}

fn parse(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn not_found() -> ApiError {
    (
        StatusCode::NOT_FOUND,
        JsonResponse(json!({"error": "activity not found"})),
    )
}

fn bad_request(message: &str) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        JsonResponse(json!({ "error": message })),
    )
}

fn internal(error: sqlx::Error) -> ApiError {
    error!(%error, "journal review query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        JsonResponse(json!({"error": "journal query failed"})),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(value: &str) -> DateTime<Utc> {
        value.parse().unwrap()
    }

    fn request(start: &str, end: &str, rating: Option<&str>) -> JournalReviewRequest {
        JournalReviewRequest {
            start_at: start.to_string(),
            end_at: end.to_string(),
            rating: rating.map(str::to_string),
            source: None,
        }
    }

    fn rating(id: i64, start: &str, end: &str, label: &str) -> JournalReviewRating {
        JournalReviewRating {
            id,
            start_at: at(start),
            end_at: at(end),
            rating: label.to_string(),
            source: "app".to_string(),
            created_at: "2026-09-16T10:00:00Z".to_string(),
            updated_at: "2026-09-16T10:00:00Z".to_string(),
        }
    }

    #[test]
    fn a_review_span_must_be_ordered_and_no_longer_than_a_day() {
        let (start, end, rating, source) = validate_review(&request(
            "2026-09-16T09:00:00Z",
            "2026-09-16T10:30:00Z",
            Some("focused"),
        ))
        .unwrap();
        assert_eq!(start, at("2026-09-16T09:00:00Z"));
        assert_eq!(end, at("2026-09-16T10:30:00Z"));
        assert_eq!(rating.as_deref(), Some("focused"));
        assert_eq!(source, "app");

        // Clearing is a rating of null, not a missing span.
        let (_, _, rating, _) = validate_review(&request(
            "2026-09-16T09:00:00Z",
            "2026-09-16T10:00:00Z",
            None,
        ))
        .unwrap();
        assert_eq!(rating, None);

        for (start, end) in [
            ("2026-09-16T10:00:00Z", "2026-09-16T09:00:00Z"),
            ("2026-09-16T09:00:00Z", "2026-09-16T09:00:00Z"),
            // Longer than 24 hours.
            ("2026-09-16T09:00:00Z", "2026-09-17T09:00:01Z"),
            ("yesterday", "2026-09-16T10:00:00Z"),
            ("2026-09-16T09:00:00Z", "noon"),
        ] {
            let (status, _) = validate_review(&request(start, end, Some("focused"))).unwrap_err();
            assert_eq!(status, StatusCode::BAD_REQUEST, "accepted {start}..{end}");
        }

        // Exactly 24 hours is allowed.
        assert!(validate_review(&request(
            "2026-09-16T09:00:00Z",
            "2026-09-17T09:00:00Z",
            Some("neutral")
        ))
        .is_ok());

        // Only the three ratings.
        let (status, _) = validate_review(&request(
            "2026-09-16T09:00:00Z",
            "2026-09-16T10:00:00Z",
            Some("great"),
        ))
        .unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn the_source_defaults_to_the_app_and_refuses_anything_but_mcp() {
        let mut payload = request(
            "2026-09-16T09:00:00Z",
            "2026-09-16T10:00:00Z",
            Some("focused"),
        );
        payload.source = Some("mcp".to_string());
        let (_, _, _, source) = validate_review(&payload).unwrap();
        assert_eq!(source, "mcp");

        payload.source = Some("  ".to_string());
        let (_, _, _, source) = validate_review(&payload).unwrap();
        assert_eq!(source, DEFAULT_REVIEW_SOURCE);

        for bad in ["tray", "APP", "cli"] {
            payload.source = Some(bad.to_string());
            let (status, _) = validate_review(&payload).unwrap_err();
            assert_eq!(status, StatusCode::BAD_REQUEST, "accepted source {bad}");
        }
    }

    #[test]
    fn a_day_shows_its_own_half_of_a_rating_that_straddles_the_boundary() {
        let day_start = at("2026-09-16T04:00:00Z");
        let day_end = at("2026-09-17T04:00:00Z");
        let items = review_items(
            &[
                rating(2, "2026-09-16T09:00:00Z", "2026-09-16T10:00:00Z", "focused"),
                rating(
                    1,
                    "2026-09-16T03:00:00Z",
                    "2026-09-16T05:00:00Z",
                    "distracted",
                ),
                // Entirely outside the day.
                rating(3, "2026-09-18T09:00:00Z", "2026-09-18T10:00:00Z", "neutral"),
            ],
            day_start,
            day_end,
        );
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, 1);
        assert_eq!(items[0].start_at, day_start.to_rfc3339());
        assert_eq!(items[0].end_at, at("2026-09-16T05:00:00Z").to_rfc3339());
        assert_eq!(items[1].id, 2);
        assert_eq!(items[1].rating, "focused");
    }

    #[test]
    fn a_feedback_row_keeps_the_card_it_judged() {
        let item = JournalFeedbackItem::from(JournalActivityFeedback {
            activity_id: 42,
            activity_key: "a1f3".to_string(),
            rating: "down".to_string(),
            note: Some("wrong category".to_string()),
            day: "2026-09-16".to_string(),
            start_at: "2026-09-16T08:15:00Z".to_string(),
            end_at: "2026-09-16T08:59:00Z".to_string(),
            title: "Investigated refresh-token failures".to_string(),
            category_id: "work".to_string(),
            producer: "llm-v1".to_string(),
            prompt_version: Some("journal-cards-v4".to_string()),
            created_at: "2026-09-16T10:02:00Z".to_string(),
            updated_at: "2026-09-16T10:02:00Z".to_string(),
        });
        assert_eq!(item.activity_id, 42);
        assert_eq!(item.rating, "down");
        assert_eq!(item.title, "Investigated refresh-token failures");
        assert_eq!(item.category_id, "work");
        assert_eq!(item.created_at, "2026-09-16T10:02:00Z");
    }
}
