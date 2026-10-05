// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com
//! User-authored work notes, independent of generated cards. All writes join
//! the existing SQLite coordinator; no capture hot-path work or model calls.
use super::*;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WorkLogEntry {
    pub id: String,
    pub revision: i64,
    pub date: String,
    pub context: String,
    pub project: String,
    pub outcome: String,
    pub status: String,
    pub role: String,
    pub approved: bool,
    pub source_activity_id: Option<i64>,
    pub source_activity_key: Option<String>,
    pub source_start_at: Option<String>,
    pub source_end_at: Option<String>,
    pub updated_at: String,
}

impl DatabaseManager {
    pub async fn work_log_entry(&self, id: &str) -> Result<Option<WorkLogEntry>, SqlxError> {
        sqlx::query_as::<_, WorkLogEntry>("SELECT * FROM journal_work_log WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    /// One extra row lets callers detect truncation and refuse partial exports.
    pub async fn list_work_log(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Vec<WorkLogEntry>, SqlxError> {
        sqlx::query_as::<_, WorkLogEntry>("SELECT * FROM journal_work_log WHERE date >= ?1 AND date <= ?2 ORDER BY date, id LIMIT 1001")
            .bind(from).bind(to).fetch_all(&self.pool).await
    }

    pub async fn save_work_log(
        &self,
        entry: &WorkLogEntry,
        expected: Option<i64>,
    ) -> Result<bool, SqlxError> {
        let mut tx = self.begin_immediate_with_retry().await?;
        let changed = if let Some(revision) = expected {
            sqlx::query("UPDATE journal_work_log SET date=?2,context=?3,project=?4,outcome=?5,status=?6,role=?7,approved=?8,revision=revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1 AND revision=?9")
                .bind(&entry.id).bind(&entry.date).bind(&entry.context).bind(&entry.project)
                .bind(&entry.outcome).bind(&entry.status).bind(&entry.role).bind(entry.approved).bind(revision)
                .execute(&mut **tx.conn()).await?.rows_affected()
        } else {
            sqlx::query("INSERT INTO journal_work_log (id,date,context,project,outcome,status,role,approved,source_activity_id,source_activity_key,source_start_at,source_end_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12) ON CONFLICT(id) DO NOTHING")
                .bind(&entry.id).bind(&entry.date).bind(&entry.context).bind(&entry.project)
                .bind(&entry.outcome).bind(&entry.status).bind(&entry.role).bind(entry.approved)
                .bind(entry.source_activity_id).bind(&entry.source_activity_key)
                .bind(&entry.source_start_at).bind(&entry.source_end_at)
                .execute(&mut **tx.conn()).await?.rows_affected()
        };
        tx.commit().await?;
        Ok(changed == 1)
    }

    pub async fn delete_work_log(&self, id: &str, expected: i64) -> Result<bool, SqlxError> {
        let mut tx = self.begin_immediate_with_retry().await?;
        let changed = sqlx::query("DELETE FROM journal_work_log WHERE id = ?1 AND revision = ?2")
            .bind(id)
            .bind(expected)
            .execute(&mut **tx.conn())
            .await?
            .rows_affected();
        tx.commit().await?;
        Ok(changed == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use screenpipe_config::DbConfig;
    #[tokio::test]
    async fn work_log_roundtrip_approval_and_independent_retention() {
        let dir = tempfile::tempdir().unwrap();
        let db = DatabaseManager::new(
            dir.path().join("db.sqlite").to_str().unwrap(),
            DbConfig::default(),
        )
        .await
        .unwrap();
        let mut entry = WorkLogEntry {
            id: "92bf75e4-f4c7-416a-a916-ce15fb593ef1".into(),
            revision: 1,
            date: "2026-09-22".into(),
            context: "work".into(),
            project: "Migration".into(),
            outcome: "Reviewed rollback".into(),
            status: "in_progress".into(),
            role: "reviewer".into(),
            approved: true,
            source_activity_id: None,
            source_activity_key: None,
            source_start_at: None,
            source_end_at: None,
            updated_at: String::new(),
        };
        assert!(db.save_work_log(&entry, None).await.unwrap());
        assert!(
            db.work_log_entry(&entry.id)
                .await
                .unwrap()
                .unwrap()
                .approved
        );
        entry.context = "personal".into();
        assert!(db.save_work_log(&entry, Some(1)).await.is_err());
        entry.approved = false;
        assert!(db.save_work_log(&entry, Some(1)).await.unwrap());
        assert!(!db.save_work_log(&entry, Some(1)).await.unwrap());
        assert!(!db.delete_work_log(&entry.id, 1).await.unwrap());
        assert_eq!(
            db.list_work_log("2026-07-01", "2026-09-30")
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(db
            .list_work_log("2026-10-01", "2026-12-31")
            .await
            .unwrap()
            .is_empty());
        // No card is required: a user can record offline work, and capture
        // regeneration cannot cascade-delete these independently authored notes.
        assert!(db.delete_work_log(&entry.id, 2).await.unwrap());
        assert!(!db.save_work_log(&entry, Some(2)).await.unwrap());
        assert!(db.work_log_entry(&entry.id).await.unwrap().is_none());
    }
}
