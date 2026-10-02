// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com
//! Local, user-authored contribution notes. Context labels constrain work
//! reports only; they do not change capture, LLM access, or recording retention.
use crate::server::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, NaiveDate, Utc};
use oasgen::{oasgen, OaSchema};
use screenpipe_db::WorkLogEntry;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

type Error = (StatusCode, Json<Value>);
fn bad(message: &str) -> Error {
    (StatusCode::BAD_REQUEST, Json(json!({"error":message})))
}
fn conflict() -> Error {
    (
        StatusCode::CONFLICT,
        Json(json!({"error":"Note changed or was deleted; reload before editing"})),
    )
}
fn internal(error: impl std::fmt::Display) -> Error {
    tracing::warn!("work log: local database error: {error}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error":"Could not access local work notes"})),
    )
}
fn date(value: &str) -> Result<NaiveDate, Error> {
    let parsed = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| bad("Use a valid YYYY-MM-DD date"))?;
    if parsed.format("%Y-%m-%d").to_string() != value {
        return Err(bad("Use YYYY-MM-DD"));
    }
    Ok(parsed)
}
/// Notes are keyed by the canonical lowercase hyphenated UUID. `parse_str`
/// also accepts braced, `urn:` and uppercase forms, which would otherwise let
/// one note exist as several rows.
fn valid_id(id: &str) -> Result<(), Error> {
    let parsed = uuid::Uuid::parse_str(id).map_err(|_| bad("Invalid note ID"))?;
    if parsed.hyphenated().to_string() != id {
        return Err(bad("Use the lowercase hyphenated note ID"));
    }
    Ok(())
}

#[derive(Deserialize, OaSchema)]
pub struct WorkLogRange {
    pub from: String,
    pub to: String,
}
#[derive(Deserialize, OaSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkLogInput {
    pub expected_revision: Option<i64>,
    pub date: String,
    pub context: String,
    pub project: String,
    pub outcome: String,
    pub status: String,
    pub role: String,
    /// Explicit confirmation on this save; never inherited from the old entry.
    #[serde(default)]
    pub approve: bool,
    pub source_activity_id: Option<i64>,
}
fn validate(input: &WorkLogInput) -> Result<(), Error> {
    date(&input.date)?;
    if input.expected_revision.is_some_and(|v| v < 1) {
        return Err(bad("Invalid note revision"));
    }
    if !["work", "personal", "mixed", "unspecified"].contains(&input.context.as_str())
        || !["in_progress", "completed", "blocked", "learning"].contains(&input.status.as_str())
        || !["owner", "contributor", "reviewer", "observer", "unknown"]
            .contains(&input.role.as_str())
    {
        return Err(bad("Invalid context, status or contribution role"));
    }
    if input.project.chars().count() > 120 || input.outcome.chars().count() > 2000 {
        return Err(bad(
            "Project must be at most 120 characters; outcome at most 2000",
        ));
    }
    if input.approve
        && (input.context != "work"
            || input.outcome.trim().is_empty()
            || !["owner", "contributor", "reviewer"].contains(&input.role.as_str()))
    {
        return Err(bad(
            "Approve only a work note with an authored outcome and a confirmed contribution role",
        ));
    }
    Ok(())
}

#[oasgen]
pub async fn get_work_log(
    State(state): State<Arc<AppState>>,
    Query(range): Query<WorkLogRange>,
) -> Result<Json<Value>, Error> {
    let days = (date(&range.to)? - date(&range.from)?).num_days();
    if !(0..366).contains(&days) {
        return Err(bad("Choose a range of 1–366 calendar dates"));
    }
    let mut entries = state
        .db
        .list_work_log(&range.from, &range.to)
        .await
        .map_err(internal)?;
    let truncated = entries.len() > 1000;
    entries.truncate(1000);
    Ok(Json(
        json!({"entries":entries,"truncated":truncated,"from":range.from,"to":range.to}),
    ))
}

#[oasgen]
pub async fn put_work_log(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<WorkLogInput>,
) -> Result<Json<Value>, Error> {
    valid_id(&id)?;
    validate(&input)?;
    let existing = state.db.work_log_entry(&id).await.map_err(internal)?;
    if existing.as_ref().map(|e| e.revision) != input.expected_revision {
        return Err(conflict());
    }
    // Source identity is snapshotted once. Editing a note cannot silently relink it
    // to a regenerated card. Manual fields remain editable after source deletion.
    let (source_id, source_key, source_start, source_end) = if let Some(old) = existing {
        if input.source_activity_id != old.source_activity_id {
            return Err(bad("A saved note's source cannot be changed"));
        }
        (
            old.source_activity_id,
            old.source_activity_key,
            old.source_start_at,
            old.source_end_at,
        )
    } else if let Some(source_id) = input.source_activity_id {
        let source = state
            .db
            .get_journal_activity(source_id, false)
            .await
            .map_err(internal)?
            .filter(|(a, _, _)| {
                state
                    .history_access
                    .cutoff(Utc::now())
                    .is_none_or(|cutoff| {
                        DateTime::parse_from_rfc3339(&a.end_at)
                            .is_ok_and(|end| end.with_timezone(&Utc) >= cutoff)
                    })
            });
        let Some((a, _, _)) = source else {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"error":"Journal source unavailable"})),
            ));
        };
        (
            Some(source_id),
            Some(a.activity_key),
            Some(a.start_at),
            Some(a.end_at),
        )
    } else {
        (None, None, None, None)
    };
    let entry = WorkLogEntry {
        id: id.clone(),
        revision: 1,
        date: input.date,
        context: input.context,
        project: input.project.trim().to_string(),
        outcome: input.outcome.trim().to_string(),
        status: input.status,
        role: input.role,
        approved: input.approve,
        source_activity_id: source_id,
        source_activity_key: source_key,
        source_start_at: source_start,
        source_end_at: source_end,
        updated_at: String::new(),
    };
    if !state
        .db
        .save_work_log(&entry, input.expected_revision)
        .await
        .map_err(internal)?
    {
        return Err(conflict());
    }
    Ok(Json(
        json!({"entry":state.db.work_log_entry(&id).await.map_err(internal)?}),
    ))
}

#[derive(Deserialize, OaSchema)]
pub struct DeleteRevision {
    pub revision: i64,
}

#[oasgen]
pub async fn delete_work_log(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<DeleteRevision>,
) -> Result<Json<Value>, Error> {
    valid_id(&id)?;
    if !state
        .db
        .delete_work_log(&id, query.revision)
        .await
        .map_err(internal)?
    {
        return Err(conflict());
    }
    Ok(Json(json!({"deleted":true})))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn work_log_approval_requires_authored_work_and_contribution() {
        for context in ["work", "personal", "mixed", "unspecified"] {
            for role in ["owner", "contributor", "reviewer", "observer", "unknown"] {
                let input = WorkLogInput {
                    expected_revision: None,
                    date: "2026-09-22".into(),
                    context: context.into(),
                    project: String::new(),
                    outcome: "Reviewed the migration".into(),
                    status: "completed".into(),
                    role: role.into(),
                    approve: true,
                    source_activity_id: None,
                };
                assert_eq!(
                    validate(&input).is_ok(),
                    context == "work" && ["owner", "contributor", "reviewer"].contains(&role)
                );
            }
        }
        assert!(date("2026-02-29").is_err());
        assert!(date("2024-02-29").is_ok());
        assert!(date("2026-9-2").is_err());
        assert!(valid_id("nope").is_err());
        assert!(valid_id("0b7c8f9e-1d2a-4c3b-9e8f-7a6b5c4d3e2f").is_ok());
        assert!(valid_id("0B7C8F9E-1D2A-4C3B-9E8F-7A6B5C4D3E2F").is_err());
        assert!(valid_id("{0b7c8f9e-1d2a-4c3b-9e8f-7a6b5c4d3e2f}").is_err());
    }
}
