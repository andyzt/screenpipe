// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com

//! Storage for the user's judgments about the journal: a thumb on one card
//! (`journal_activity_feedback`) and a rating painted over a stretch of the
//! timeline (`journal_review_ratings`).
//!
//! Two invariants live here and nowhere else:
//!
//! - **Feedback outlives the card.** The row snapshots the card's key, day,
//!   span, title, category, producer and prompt version, so a rewrite of the
//!   window cannot erase what the user said. It carries no foreign key for the
//!   same reason; an orphaned row simply attaches to no card.
//! - **Review ratings never overlap.** [`DatabaseManager::apply_review_rating`]
//!   splits, trims or deletes whatever the new span covers before inserting,
//!   exactly as dropping a block on a calendar would, and does the whole thing
//!   in one `begin_immediate_with_retry` transaction. SQLite cannot express
//!   "no two rows intersect", so that function is the constraint.
//!
//! Split/replace semantics derived from Dayflow's timeline editing
//! (https://github.com/JerryZLiu/Dayflow), MIT, Copyright (c) 2025 Jerry Liu.

use super::journal::parse_ts;
use super::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::{BTreeMap, HashMap};

/// The two thumbs a card can carry.
pub const FEEDBACK_RATINGS: [&str; 2] = ["up", "down"];

/// The three ratings a reviewed span can carry.
pub const REVIEW_RATINGS: [&str; 3] = ["focused", "neutral", "distracted"];

/// Where a rating came from. Mirrors the CHECK constraint on the column.
pub const REVIEW_SOURCES: [&str; 2] = ["app", "mcp"];

/// What a card reports when more than one rating covers it and none of them
/// dominates.
pub const REVIEW_MIXED: &str = "mixed";

/// Share of a card's span a single rating must cover to name the whole card.
pub const REVIEW_DOMINANT_SHARE: f64 = 0.9;

/// Rows `GET /journal/feedback` returns without a `date`.
pub const FEEDBACK_RECENT_LIMIT: i64 = 200;

/// Longest span one `PUT /journal/reviews` call may cover.
pub const MAX_REVIEW_SPAN_HOURS: i64 = 24;

/// Note length the column accepts. Longer notes are a different feature.
pub const MAX_FEEDBACK_NOTE_CHARS: usize = 500;

// ---------- records ----------

/// The user's thumb on a card, as the card itself reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalCardFeedback {
    pub rating: String,
    pub note: Option<String>,
    pub created_at: String,
}

/// One feedback row with the snapshot that keeps it readable after the card it
/// was written against has been rewritten away.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalActivityFeedback {
    pub activity_id: i64,
    pub activity_key: String,
    pub rating: String,
    pub note: Option<String>,
    pub day: String,
    pub start_at: String,
    pub end_at: String,
    pub title: String,
    pub category_id: String,
    pub producer: String,
    pub prompt_version: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// One reviewed span. Never overlaps another row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalReviewRating {
    pub id: i64,
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,
    pub rating: String,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(FromRow)]
struct RawFeedback {
    activity_id: i64,
    activity_key: String,
    rating: String,
    note: Option<String>,
    day: String,
    start_at: String,
    end_at: String,
    title: String,
    category_id: String,
    producer: String,
    prompt_version: Option<String>,
    created_at: String,
    updated_at: String,
}

impl From<RawFeedback> for JournalActivityFeedback {
    fn from(row: RawFeedback) -> Self {
        Self {
            activity_id: row.activity_id,
            activity_key: row.activity_key,
            rating: row.rating,
            note: row.note,
            day: row.day,
            start_at: row.start_at,
            end_at: row.end_at,
            title: row.title,
            category_id: row.category_id,
            producer: row.producer,
            prompt_version: row.prompt_version,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(FromRow)]
struct RawReviewRating {
    id: i64,
    start_at: String,
    end_at: String,
    rating: String,
    source: String,
    created_at: String,
    updated_at: String,
}

fn review_from_row(row: RawReviewRating) -> Option<JournalReviewRating> {
    Some(JournalReviewRating {
        id: row.id,
        start_at: parse_ts(&row.start_at)?,
        end_at: parse_ts(&row.end_at)?,
        rating: row.rating,
        source: row.source,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

/// The rating a card's span carries, per `docs/JOURNAL_API_CONTRACT.md`:
/// the single rating when one covers at least [`REVIEW_DOMINANT_SHARE`] of the
/// span, [`REVIEW_MIXED`] in every other case where a rating touches the span
/// — two or more ratings, or one rating over less than that share — and `None`
/// when nothing touches it.
///
/// Ratings never overlap each other, so the shares are simply added up. The
/// share is what the label claims: "focused" on a card the user only marked
/// for ten of its sixty minutes would put words in their mouth.
pub fn review_label_for_span(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    ratings: &[JournalReviewRating],
) -> Option<String> {
    let span_ms = (end - start).num_milliseconds();
    if span_ms <= 0 {
        return None;
    }
    let mut covered: BTreeMap<&str, i64> = BTreeMap::new();
    for rating in ratings {
        let from = rating.start_at.max(start);
        let to = rating.end_at.min(end);
        let overlap = (to - from).num_milliseconds();
        if overlap > 0 {
            *covered.entry(rating.rating.as_str()).or_default() += overlap;
        }
    }
    let (label, overlap) = covered
        .iter()
        .max_by_key(|(_, overlap)| **overlap)
        .map(|(label, overlap)| ((*label).to_string(), *overlap))?;
    if overlap as f64 / span_ms as f64 >= REVIEW_DOMINANT_SHARE {
        Some(label)
    } else {
        Some(REVIEW_MIXED.to_string())
    }
}

impl DatabaseManager {
    // ---------- card feedback ----------

    /// Write the user's thumb on a card, snapshotting the card as it stands.
    /// `false` when the card is gone, which the route turns into a 404.
    pub async fn upsert_journal_activity_feedback(
        &self,
        activity_id: i64,
        rating: &str,
        note: Option<&str>,
    ) -> Result<bool, SqlxError> {
        let note = note
            .map(str::trim)
            .filter(|note| !note.is_empty())
            .map(|note| {
                note.chars()
                    .take(MAX_FEEDBACK_NOTE_CHARS)
                    .collect::<String>()
            });
        let mut tx = self.begin_immediate_with_retry().await?;
        let affected = sqlx::query(
            r#"INSERT INTO journal_activity_feedback
               (activity_id, activity_key, rating, note, day, start_at, end_at, title,
                category_id, producer, prompt_version)
               SELECT a.id, a.activity_key, ?2, ?3, a.day, a.start_at, a.end_at, a.title,
                      a.category_id, a.producer, a.prompt_version
                 FROM journal_activities a
                WHERE a.id = ?1 AND a.deleted_at IS NULL
               ON CONFLICT(activity_id) DO UPDATE SET
                 activity_key = excluded.activity_key,
                 rating = excluded.rating,
                 note = excluded.note,
                 day = excluded.day,
                 start_at = excluded.start_at,
                 end_at = excluded.end_at,
                 title = excluded.title,
                 category_id = excluded.category_id,
                 producer = excluded.producer,
                 prompt_version = excluded.prompt_version,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')"#,
        )
        .bind(activity_id)
        .bind(rating)
        .bind(note)
        .execute(&mut **tx.conn())
        .await?
        .rows_affected();
        tx.commit().await?;
        Ok(affected > 0)
    }

    /// Drop the thumb on a card. Idempotent: clearing an unrated card is a
    /// no-op, not an error.
    pub async fn clear_journal_activity_feedback(&self, activity_id: i64) -> Result<(), SqlxError> {
        let mut tx = self.begin_immediate_with_retry().await?;
        sqlx::query("DELETE FROM journal_activity_feedback WHERE activity_id = ?1")
            .bind(activity_id)
            .execute(&mut **tx.conn())
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Feedback rows for one journal day, or the most recent
    /// [`FEEDBACK_RECENT_LIMIT`] when `day` is `None`.
    pub async fn list_journal_feedback(
        &self,
        day: Option<&str>,
    ) -> Result<Vec<JournalActivityFeedback>, SqlxError> {
        let rows = match day {
            Some(day) => {
                sqlx::query_as::<_, RawFeedback>(
                    r#"SELECT activity_id, activity_key, rating, note, day, start_at, end_at,
                              title, category_id, producer, prompt_version, created_at, updated_at
                         FROM journal_activity_feedback
                        WHERE day = ?1
                        ORDER BY start_at, activity_id"#,
                )
                .bind(day)
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query_as::<_, RawFeedback>(
                    r#"SELECT activity_id, activity_key, rating, note, day, start_at, end_at,
                              title, category_id, producer, prompt_version, created_at, updated_at
                         FROM journal_activity_feedback
                        ORDER BY created_at DESC, activity_id DESC
                        LIMIT ?1"#,
                )
                .bind(FEEDBACK_RECENT_LIMIT)
                .fetch_all(&self.pool)
                .await?
            }
        };
        Ok(rows.into_iter().map(Into::into).collect())
    }

    /// The thumbs on a batch of cards, keyed by activity id. One query for a
    /// whole day's cards: the read path must never go card by card.
    pub async fn journal_feedback_for_activities(
        &self,
        activity_ids: &[i64],
    ) -> Result<HashMap<i64, JournalCardFeedback>, SqlxError> {
        if activity_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders = activity_ids
            .iter()
            .enumerate()
            .map(|(index, _)| format!("?{}", index + 1))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT activity_id, rating, note, created_at FROM journal_activity_feedback \
             WHERE activity_id IN ({placeholders})"
        );
        let mut query =
            sqlx::query_as::<_, (i64, String, Option<String>, String)>(sqlx::AssertSqlSafe(sql));
        for id in activity_ids {
            query = query.bind(id);
        }
        Ok(query
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(|(activity_id, rating, note, created_at)| {
                (
                    activity_id,
                    JournalCardFeedback {
                        rating,
                        note,
                        created_at,
                    },
                )
            })
            .collect())
    }

    // ---------- review ratings ----------

    /// Rate `[start_at, end_at)`, or clear it with `rating = None`.
    ///
    /// The new span always wins. Every stored row it touches is trimmed to the
    /// part outside it, split in two when the new span sits strictly inside it,
    /// and deleted when it is covered whole — so the table never holds two rows
    /// that intersect, and the day view never has to decide which of two
    /// ratings a minute carries.
    pub async fn apply_review_rating(
        &self,
        start_at: DateTime<Utc>,
        end_at: DateTime<Utc>,
        rating: Option<&str>,
        source: &str,
    ) -> Result<(), SqlxError> {
        if end_at <= start_at {
            return Ok(());
        }
        let start = start_at.to_rfc3339();
        let end = end_at.to_rfc3339();

        let mut tx = self.begin_immediate_with_retry().await?;
        let overlapping = sqlx::query_as::<_, RawReviewRating>(
            "SELECT id, start_at, end_at, rating, source, created_at, updated_at \
             FROM journal_review_ratings \
             WHERE end_at > ?1 AND start_at < ?2 ORDER BY start_at, id",
        )
        .bind(&start)
        .bind(&end)
        .fetch_all(&mut **tx.conn())
        .await?;

        for row in overlapping {
            let Some(existing) = review_from_row(row) else {
                continue;
            };
            let keeps_head = existing.start_at < start_at;
            let keeps_tail = existing.end_at > end_at;
            match (keeps_head, keeps_tail) {
                // The new span sits strictly inside this one: keep the head in
                // place and re-open the tail as its own row.
                (true, true) => {
                    sqlx::query(
                        "UPDATE journal_review_ratings SET end_at = ?2, \
                         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
                    )
                    .bind(existing.id)
                    .bind(&start)
                    .execute(&mut **tx.conn())
                    .await?;
                    sqlx::query(
                        "INSERT INTO journal_review_ratings \
                         (start_at, end_at, rating, source, created_at) \
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                    )
                    .bind(&end)
                    .bind(existing.end_at.to_rfc3339())
                    .bind(&existing.rating)
                    .bind(&existing.source)
                    .bind(&existing.created_at)
                    .execute(&mut **tx.conn())
                    .await?;
                }
                (true, false) => {
                    sqlx::query(
                        "UPDATE journal_review_ratings SET end_at = ?2, \
                         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
                    )
                    .bind(existing.id)
                    .bind(&start)
                    .execute(&mut **tx.conn())
                    .await?;
                }
                (false, true) => {
                    sqlx::query(
                        "UPDATE journal_review_ratings SET start_at = ?2, \
                         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
                    )
                    .bind(existing.id)
                    .bind(&end)
                    .execute(&mut **tx.conn())
                    .await?;
                }
                (false, false) => {
                    sqlx::query("DELETE FROM journal_review_ratings WHERE id = ?1")
                        .bind(existing.id)
                        .execute(&mut **tx.conn())
                        .await?;
                }
            }
        }

        if let Some(rating) = rating {
            sqlx::query(
                "INSERT INTO journal_review_ratings (start_at, end_at, rating, source) \
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .bind(&start)
            .bind(&end)
            .bind(rating)
            .bind(source)
            .execute(&mut **tx.conn())
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Ratings overlapping `[start_at, end_at)`, ascending. Rows are returned
    /// whole, not clipped: a caller that needs the clipped span knows its own
    /// bounds, and the card label needs the real one.
    pub async fn list_review_ratings(
        &self,
        start_at: DateTime<Utc>,
        end_at: DateTime<Utc>,
    ) -> Result<Vec<JournalReviewRating>, SqlxError> {
        let rows = sqlx::query_as::<_, RawReviewRating>(
            "SELECT id, start_at, end_at, rating, source, created_at, updated_at \
             FROM journal_review_ratings \
             WHERE end_at > ?1 AND start_at < ?2 ORDER BY start_at, id",
        )
        .bind(start_at.to_rfc3339())
        .bind(end_at.to_rfc3339())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().filter_map(review_from_row).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use screenpipe_config::DbConfig;

    async fn test_db() -> (DatabaseManager, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = DatabaseManager::new(
            dir.path().join("db.sqlite").to_str().unwrap(),
            DbConfig::default(),
        )
        .await
        .unwrap();
        (db, dir)
    }

    fn at(value: &str) -> DateTime<Utc> {
        value.parse().unwrap()
    }

    async fn rate(db: &DatabaseManager, start: &str, end: &str, rating: &str) {
        db.apply_review_rating(at(start), at(end), Some(rating), "app")
            .await
            .unwrap();
    }

    /// Every row, as `(start, end, rating)` clock strings, ascending.
    async fn spans(db: &DatabaseManager) -> Vec<(String, String, String)> {
        db.list_review_ratings(at("2000-01-01T00:00:00Z"), at("2100-01-01T00:00:00Z"))
            .await
            .unwrap()
            .into_iter()
            .map(|row| {
                (
                    row.start_at.format("%H:%M").to_string(),
                    row.end_at.format("%H:%M").to_string(),
                    row.rating,
                )
            })
            .collect()
    }

    fn rating(start: &str, end: &str, label: &str) -> JournalReviewRating {
        JournalReviewRating {
            id: 1,
            start_at: at(start),
            end_at: at(end),
            rating: label.to_string(),
            source: "app".to_string(),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[tokio::test]
    async fn an_adjacent_rating_is_left_alone() {
        let (db, _dir) = test_db().await;
        rate(
            &db,
            "2026-09-16T09:00:00Z",
            "2026-09-16T10:00:00Z",
            "focused",
        )
        .await;
        rate(
            &db,
            "2026-09-16T10:00:00Z",
            "2026-09-16T11:00:00Z",
            "distracted",
        )
        .await;
        assert_eq!(
            spans(&db).await,
            vec![
                ("09:00".into(), "10:00".into(), "focused".into()),
                ("10:00".into(), "11:00".into(), "distracted".into()),
            ]
        );
    }

    #[tokio::test]
    async fn a_contained_rating_is_replaced_whole() {
        let (db, _dir) = test_db().await;
        rate(
            &db,
            "2026-09-16T09:30:00Z",
            "2026-09-16T09:45:00Z",
            "neutral",
        )
        .await;
        rate(
            &db,
            "2026-09-16T09:00:00Z",
            "2026-09-16T10:00:00Z",
            "focused",
        )
        .await;
        assert_eq!(
            spans(&db).await,
            vec![("09:00".into(), "10:00".into(), "focused".into())]
        );
    }

    #[tokio::test]
    async fn a_rating_inside_an_existing_one_splits_it_in_two() {
        let (db, _dir) = test_db().await;
        rate(
            &db,
            "2026-09-16T09:00:00Z",
            "2026-09-16T12:00:00Z",
            "focused",
        )
        .await;
        rate(
            &db,
            "2026-09-16T10:00:00Z",
            "2026-09-16T10:30:00Z",
            "distracted",
        )
        .await;
        assert_eq!(
            spans(&db).await,
            vec![
                ("09:00".into(), "10:00".into(), "focused".into()),
                ("10:00".into(), "10:30".into(), "distracted".into()),
                ("10:30".into(), "12:00".into(), "focused".into()),
            ]
        );
    }

    #[tokio::test]
    async fn overlapping_the_left_and_the_right_edge_trims_the_neighbour() {
        let (db, _dir) = test_db().await;
        rate(
            &db,
            "2026-09-16T09:00:00Z",
            "2026-09-16T10:00:00Z",
            "focused",
        )
        .await;
        rate(
            &db,
            "2026-09-16T11:00:00Z",
            "2026-09-16T12:00:00Z",
            "neutral",
        )
        .await;
        // Runs into the tail of the first row and the head of the second.
        rate(
            &db,
            "2026-09-16T09:30:00Z",
            "2026-09-16T11:30:00Z",
            "distracted",
        )
        .await;
        assert_eq!(
            spans(&db).await,
            vec![
                ("09:00".into(), "09:30".into(), "focused".into()),
                ("09:30".into(), "11:30".into(), "distracted".into()),
                ("11:30".into(), "12:00".into(), "neutral".into()),
            ]
        );
    }

    #[tokio::test]
    async fn rating_the_exact_same_span_again_replaces_it() {
        let (db, _dir) = test_db().await;
        rate(
            &db,
            "2026-09-16T09:00:00Z",
            "2026-09-16T10:00:00Z",
            "focused",
        )
        .await;
        rate(
            &db,
            "2026-09-16T09:00:00Z",
            "2026-09-16T10:00:00Z",
            "distracted",
        )
        .await;
        let rows = db
            .list_review_ratings(at("2026-09-16T00:00:00Z"), at("2026-09-17T00:00:00Z"))
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].rating, "distracted");
    }

    #[tokio::test]
    async fn clearing_a_span_cuts_it_out_and_adds_nothing() {
        let (db, _dir) = test_db().await;
        rate(
            &db,
            "2026-09-16T09:00:00Z",
            "2026-09-16T12:00:00Z",
            "focused",
        )
        .await;
        db.apply_review_rating(
            at("2026-09-16T10:00:00Z"),
            at("2026-09-16T10:30:00Z"),
            None,
            "app",
        )
        .await
        .unwrap();
        assert_eq!(
            spans(&db).await,
            vec![
                ("09:00".into(), "10:00".into(), "focused".into()),
                ("10:30".into(), "12:00".into(), "focused".into()),
            ]
        );

        // Clearing everything leaves an empty table, not a zero-length row.
        db.apply_review_rating(
            at("2026-09-16T08:00:00Z"),
            at("2026-09-16T13:00:00Z"),
            None,
            "mcp",
        )
        .await
        .unwrap();
        assert!(spans(&db).await.is_empty());
    }

    #[tokio::test]
    async fn a_listing_is_clipped_by_overlap_not_by_containment() {
        let (db, _dir) = test_db().await;
        rate(
            &db,
            "2026-09-16T03:30:00Z",
            "2026-09-16T04:30:00Z",
            "focused",
        )
        .await;
        let rows = db
            .list_review_ratings(at("2026-09-16T04:00:00Z"), at("2026-09-17T04:00:00Z"))
            .await
            .unwrap();
        assert_eq!(rows.len(), 1, "a rating straddling the boundary is served");
        assert_eq!(rows[0].start_at, at("2026-09-16T03:30:00Z"));

        let rows = db
            .list_review_ratings(at("2026-09-16T04:30:00Z"), at("2026-09-17T04:00:00Z"))
            .await
            .unwrap();
        assert!(
            rows.is_empty(),
            "a rating ending at the start is not in range"
        );
    }

    fn card_draft(key: &str, start: &str, end: &str) -> JournalActivityDraft {
        JournalActivityDraft {
            activity_key: key.to_string(),
            day: "2026-09-16".to_string(),
            start_at: at(start),
            end_at: at(end),
            active_minutes: 45.0,
            state: "final".to_string(),
            title: format!("card {key}"),
            summary: "summary".to_string(),
            detailed_summary: None,
            category_id: "work".to_string(),
            category_confidence: 0.8,
            intention_id: None,
            intention_relation: None,
            relation_confidence: None,
            relation_reason: None,
            app_primary: Some("Code".to_string()),
            app_secondary: None,
            producer: "llm-v1".to_string(),
            prompt_version: Some("journal-cards-v4".to_string()),
            model: Some("deepseek".to_string()),
            window_id: None,
            distractions: Vec::new(),
            interval_keys: Vec::new(),
            evidence: Vec::new(),
        }
    }

    #[tokio::test]
    async fn feedback_snapshots_the_card_and_survives_the_card_being_rewritten() {
        let (db, _dir) = test_db().await;
        let ids = db
            .replace_activities_in_range(
                at("2026-09-16T09:00:00Z"),
                at("2026-09-16T10:00:00Z"),
                &[card_draft(
                    "k1",
                    "2026-09-16T09:00:00Z",
                    "2026-09-16T10:00:00Z",
                )],
            )
            .await
            .unwrap();
        let id = ids[0];

        assert!(db
            .upsert_journal_activity_feedback(id, "down", Some("  wrong category  "))
            .await
            .unwrap());
        let items = db.list_journal_feedback(Some("2026-09-16")).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].rating, "down");
        assert_eq!(items[0].note.as_deref(), Some("wrong category"));
        assert_eq!(items[0].activity_key, "k1");
        assert_eq!(items[0].title, "card k1");
        assert_eq!(items[0].category_id, "work");
        assert_eq!(items[0].producer, "llm-v1");
        assert_eq!(items[0].prompt_version.as_deref(), Some("journal-cards-v4"));

        // A later PUT replaces the row rather than adding one.
        assert!(db
            .upsert_journal_activity_feedback(id, "up", None)
            .await
            .unwrap());
        let items = db.list_journal_feedback(None).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].rating, "up");
        assert_eq!(items[0].note, None);

        // The card is rewritten out from under the rating; the row stays.
        db.replace_activities_in_range(
            at("2026-09-16T09:00:00Z"),
            at("2026-09-16T10:00:00Z"),
            &[card_draft(
                "k2",
                "2026-09-16T09:00:00Z",
                "2026-09-16T09:30:00Z",
            )],
        )
        .await
        .unwrap();
        let items = db.list_journal_feedback(Some("2026-09-16")).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].activity_key, "k1");

        // Rating a card that never existed is a miss, not a row.
        assert!(!db
            .upsert_journal_activity_feedback(9_000_001, "up", None)
            .await
            .unwrap());

        db.clear_journal_activity_feedback(id).await.unwrap();
        assert!(db.list_journal_feedback(None).await.unwrap().is_empty());
        // Clearing twice is a no-op, not an error.
        db.clear_journal_activity_feedback(id).await.unwrap();
    }

    #[test]
    fn a_card_takes_the_only_rating_that_touches_it_and_mixes_two() {
        let start = at("2026-09-16T09:00:00Z");
        let end = at("2026-09-16T10:00:00Z");
        assert_eq!(review_label_for_span(start, end, &[]), None);

        // One rating over a sixth of the card is not a verdict on the card.
        let partial = rating("2026-09-16T09:50:00Z", "2026-09-16T10:00:00Z", "distracted");
        assert_eq!(
            review_label_for_span(start, end, &[partial]),
            Some(REVIEW_MIXED.to_string())
        );

        // One rating covering the whole card names it.
        let whole = rating("2026-09-16T08:00:00Z", "2026-09-16T11:00:00Z", "distracted");
        assert_eq!(
            review_label_for_span(start, end, &[whole]),
            Some("distracted".to_string())
        );

        // Two ratings, neither dominant.
        let halves = [
            rating("2026-09-16T09:00:00Z", "2026-09-16T09:30:00Z", "focused"),
            rating("2026-09-16T09:30:00Z", "2026-09-16T10:00:00Z", "neutral"),
        ];
        assert_eq!(
            review_label_for_span(start, end, &halves),
            Some(REVIEW_MIXED.to_string())
        );

        // Two ratings, one covering 95 % of the card.
        let dominant = [
            rating("2026-09-16T09:00:00Z", "2026-09-16T09:57:00Z", "focused"),
            rating("2026-09-16T09:57:00Z", "2026-09-16T10:00:00Z", "neutral"),
        ];
        assert_eq!(
            review_label_for_span(start, end, &dominant),
            Some("focused".to_string())
        );

        // A rating that ends where the card starts does not touch it.
        let before = rating("2026-09-16T08:00:00Z", "2026-09-16T09:00:00Z", "focused");
        assert_eq!(review_label_for_span(start, end, &[before]), None);
    }
}
