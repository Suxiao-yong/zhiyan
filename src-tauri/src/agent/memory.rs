// Rust Memory (M3 Part 3): structured long-term memory.
//
// Memories carry one of the seven spec §11 types, a source, a confidence, and
// a status gate: model- or inference-derived memories start as `candidate`
// and must be confirmed by the user; explicit user statements are confirmed
// automatically. Users can edit, deactivate, or delete every memory. Content
// is plain text and never vector-embedded; `relevant` picks a few confirmed
// records by exam, recency of use, and creation time.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::agent::error::AgentError;

/// The seven spec §11 memory types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    SchedulePreference,
    DailyCapacity,
    SubjectPreference,
    LearningConstraint,
    ReminderPreference,
    StrategyPreference,
    ConfirmedWeakness,
}

impl MemoryType {
    pub const ALL: [MemoryType; 7] = [
        MemoryType::SchedulePreference,
        MemoryType::DailyCapacity,
        MemoryType::SubjectPreference,
        MemoryType::LearningConstraint,
        MemoryType::ReminderPreference,
        MemoryType::StrategyPreference,
        MemoryType::ConfirmedWeakness,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryType::SchedulePreference => "schedule_preference",
            MemoryType::DailyCapacity => "daily_capacity",
            MemoryType::SubjectPreference => "subject_preference",
            MemoryType::LearningConstraint => "learning_constraint",
            MemoryType::ReminderPreference => "reminder_preference",
            MemoryType::StrategyPreference => "strategy_preference",
            MemoryType::ConfirmedWeakness => "confirmed_weakness",
        }
    }

    pub fn parse(value: &str) -> Option<MemoryType> {
        MemoryType::ALL
            .into_iter()
            .find(|memory_type| memory_type.as_str() == value)
    }
}

/// Where a memory came from; decides auto-confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySource {
    /// An explicit user statement; confirmed automatically on creation.
    UserStatement,
    /// Inferred from behavior; must be confirmed by the user.
    BehaviorInferred,
    /// Proposed by the model; must be confirmed by the user.
    ModelCandidate,
}

impl MemorySource {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemorySource::UserStatement => "user_statement",
            MemorySource::BehaviorInferred => "behavior_inferred",
            MemorySource::ModelCandidate => "model_candidate",
        }
    }

    pub fn parse(value: &str) -> Option<MemorySource> {
        match value {
            "user_statement" => Some(MemorySource::UserStatement),
            "behavior_inferred" => Some(MemorySource::BehaviorInferred),
            "model_candidate" => Some(MemorySource::ModelCandidate),
            _ => None,
        }
    }
}

/// candidate -> confirmed -> inactive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    Candidate,
    Confirmed,
    Inactive,
}

impl MemoryStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryStatus::Candidate => "candidate",
            MemoryStatus::Confirmed => "confirmed",
            MemoryStatus::Inactive => "inactive",
        }
    }

    pub fn parse(value: &str) -> Option<MemoryStatus> {
        match value {
            "candidate" => Some(MemoryStatus::Candidate),
            "confirmed" => Some(MemoryStatus::Confirmed),
            "inactive" => Some(MemoryStatus::Inactive),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: String,
    pub exam_id: Option<String>,
    pub memory_type: MemoryType,
    pub content: String,
    pub source: MemorySource,
    pub confidence: f64,
    pub status: MemoryStatus,
    pub created_at: String,
    pub updated_at: String,
    pub last_used_at: Option<String>,
}

#[derive(Clone)]
pub struct MemoryRepository {
    pool: SqlitePool,
}

impl MemoryRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a memory. Explicit user statements start `confirmed`; everything
    /// else starts as a `candidate` awaiting user confirmation.
    pub async fn create(
        &self,
        exam_id: Option<&str>,
        memory_type: MemoryType,
        content: &str,
        source: MemorySource,
        confidence: f64,
    ) -> Result<MemoryRecord, AgentError> {
        let id = uuid::Uuid::new_v4().to_string();
        let status = if source == MemorySource::UserStatement {
            MemoryStatus::Confirmed
        } else {
            MemoryStatus::Candidate
        };
        sqlx::query(
            "INSERT INTO agent_memories \
             (id, exam_id, memory_type, content, source, confidence, status) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(exam_id)
        .bind(memory_type.as_str())
        .bind(content)
        .bind(source.as_str())
        .bind(confidence)
        .bind(status.as_str())
        .execute(&self.pool)
        .await
        .map_err(map_sqlx)?;
        self.get(&id).await
    }

    /// Confirm a candidate memory (candidate -> confirmed).
    pub async fn confirm(&self, id: &str) -> Result<MemoryRecord, AgentError> {
        let changed = sqlx::query(
            "UPDATE agent_memories SET status = 'confirmed' \
             WHERE id = ? AND status = 'candidate'",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx)?
        .rows_affected();
        if changed == 0 {
            return match self.get(id).await {
                Ok(_) => Err(AgentError::Conflict),
                Err(error) => Err(error),
            };
        }
        self.get(id).await
    }

    /// Edit a memory's content.
    pub async fn update_content(
        &self,
        id: &str,
        content: &str,
    ) -> Result<MemoryRecord, AgentError> {
        let changed = sqlx::query("UPDATE agent_memories SET content = ? WHERE id = ?")
            .bind(content)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx)?
            .rows_affected();
        if changed == 0 {
            return Err(AgentError::NotFound(id.to_owned()));
        }
        self.get(id).await
    }

    /// Deactivate a memory (candidate/confirmed -> inactive). Users can stop a
    /// memory from being offered without deleting it.
    pub async fn deactivate(&self, id: &str) -> Result<MemoryRecord, AgentError> {
        let changed = sqlx::query(
            "UPDATE agent_memories SET status = 'inactive' \
             WHERE id = ? AND status != 'inactive'",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx)?
        .rows_affected();
        if changed == 0 {
            return Err(AgentError::NotFound(id.to_owned()));
        }
        self.get(id).await
    }

    /// Permanently delete a memory.
    pub async fn delete(&self, id: &str) -> Result<(), AgentError> {
        let changed = sqlx::query("DELETE FROM agent_memories WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx)?
            .rows_affected();
        if changed == 0 {
            return Err(AgentError::NotFound(id.to_owned()));
        }
        Ok(())
    }

    /// Every memory of an exam (or all exams when `exam_id` is None), newest
    /// first. `include_inactive` controls whether deactivated memories appear.
    pub async fn list(
        &self,
        exam_id: Option<&str>,
        include_inactive: bool,
    ) -> Result<Vec<MemoryRecord>, AgentError> {
        let mut sql = String::from(
            "SELECT id, exam_id, memory_type, content, source, confidence, status, \
             created_at, updated_at, last_used_at FROM agent_memories",
        );
        let mut conditions = Vec::new();
        let mut binds: Vec<String> = Vec::new();
        if let Some(exam_id) = exam_id {
            conditions.push("(exam_id = ? OR exam_id IS NULL)");
            binds.push(exam_id.to_owned());
        }
        if !include_inactive {
            conditions.push("status != 'inactive'");
        }
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        sql.push_str(" ORDER BY created_at DESC, id DESC");
        let mut query = sqlx::query_as::<_, MemoryRow>(&sql);
        for bind in &binds {
            query = query.bind(bind);
        }
        let rows = query.fetch_all(&self.pool).await.map_err(map_sqlx)?;
        rows.into_iter().map(|row| row.try_into()).collect()
    }

    /// The few confirmed memories a context builder should offer, ordered by
    /// last use (most recent first) then creation. Exam-scoped first, then
    /// exam-independent (`exam_id IS NULL`) fallbacks.
    pub async fn relevant(
        &self,
        exam_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<MemoryRecord>, AgentError> {
        let rows = sqlx::query_as::<_, MemoryRow>(
            "SELECT id, exam_id, memory_type, content, source, confidence, status, \
             created_at, updated_at, last_used_at FROM agent_memories \
             WHERE status = 'confirmed' AND (? IS NULL OR exam_id = ? OR exam_id IS NULL) \
             ORDER BY (exam_id IS NULL), (last_used_at IS NULL), last_used_at DESC, created_at DESC \
             LIMIT ?",
        )
        .bind(exam_id)
        .bind(exam_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx)?;
        rows.into_iter().map(|row| row.try_into()).collect()
    }

    /// Mark a memory as used now (called when the context builder includes it).
    pub async fn touch(&self, id: &str) -> Result<(), AgentError> {
        let changed = sqlx::query(
            "UPDATE agent_memories SET last_used_at = datetime('now','localtime') WHERE id = ?",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx)?
        .rows_affected();
        if changed == 0 {
            return Err(AgentError::NotFound(id.to_owned()));
        }
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<MemoryRecord, AgentError> {
        let row = sqlx::query_as::<_, MemoryRow>(
            "SELECT id, exam_id, memory_type, content, source, confidence, status, \
             created_at, updated_at, last_used_at FROM agent_memories WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(row) => row.try_into(),
            None => Err(AgentError::NotFound(id.to_owned())),
        }
    }
}

#[derive(sqlx::FromRow)]
struct MemoryRow {
    id: String,
    exam_id: Option<String>,
    memory_type: String,
    content: String,
    source: String,
    confidence: f64,
    status: String,
    created_at: String,
    updated_at: String,
    last_used_at: Option<String>,
}

impl TryFrom<MemoryRow> for MemoryRecord {
    type Error = AgentError;

    fn try_from(row: MemoryRow) -> Result<Self, Self::Error> {
        let memory_type = MemoryType::parse(&row.memory_type)
            .ok_or_else(|| AgentError::Persistence("invalid memory type".to_owned()))?;
        let source = MemorySource::parse(&row.source)
            .ok_or_else(|| AgentError::Persistence("invalid memory source".to_owned()))?;
        let status = MemoryStatus::parse(&row.status)
            .ok_or_else(|| AgentError::Persistence("invalid memory status".to_owned()))?;
        Ok(MemoryRecord {
            id: row.id,
            exam_id: row.exam_id,
            memory_type,
            content: row.content,
            source,
            confidence: row.confidence,
            status,
            created_at: row.created_at,
            updated_at: row.updated_at,
            last_used_at: row.last_used_at,
        })
    }
}

fn map_sqlx(_error: sqlx::Error) -> AgentError {
    AgentError::Persistence("memory operation failed".to_owned())
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    async fn memory_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        for migration in crate::db::migrations() {
            sqlx::raw_sql(migration.sql).execute(&pool).await.unwrap();
        }
        pool
    }

    #[tokio::test]
    async fn user_statement_is_confirmed_automatically_and_others_stay_candidate() {
        let pool = memory_pool().await;
        let memory = MemoryRepository::new(pool);

        let explicit = memory
            .create(
                None,
                MemoryType::DailyCapacity,
                "每天最多学习两小时",
                MemorySource::UserStatement,
                1.0,
            )
            .await
            .unwrap();
        assert_eq!(explicit.status, MemoryStatus::Confirmed);
        assert_eq!(explicit.source, MemorySource::UserStatement);

        let inferred = memory
            .create(
                None,
                MemoryType::SubjectPreference,
                "晚上更常做数学",
                MemorySource::BehaviorInferred,
                0.6,
            )
            .await
            .unwrap();
        assert_eq!(inferred.status, MemoryStatus::Candidate);

        let proposed = memory
            .create(
                None,
                MemoryType::StrategyPreference,
                "先做错题再写新题",
                MemorySource::ModelCandidate,
                0.4,
            )
            .await
            .unwrap();
        assert_eq!(proposed.status, MemoryStatus::Candidate);
    }

    #[tokio::test]
    async fn confirm_requires_a_candidate_and_rejects_other_states() {
        let pool = memory_pool().await;
        let memory = MemoryRepository::new(pool);

        let candidate = memory
            .create(
                None,
                MemoryType::ConfirmedWeakness,
                "二次函数压轴题",
                MemorySource::ModelCandidate,
                0.5,
            )
            .await
            .unwrap();
        let confirmed = memory.confirm(&candidate.id).await.unwrap();
        assert_eq!(confirmed.status, MemoryStatus::Confirmed);

        // Confirming an already-confirmed memory is a conflict, not a no-op.
        let conflict = memory.confirm(&candidate.id).await.unwrap_err();
        assert_eq!(conflict, AgentError::Conflict);

        // Unknown id is not found.
        let missing = memory.confirm("does-not-exist").await.unwrap_err();
        assert_eq!(missing, AgentError::NotFound("does-not-exist".to_owned()));
    }

    #[tokio::test]
    async fn edit_deactivate_and_delete_cover_the_full_management_flow() {
        let pool = memory_pool().await;
        let memory = MemoryRepository::new(pool);

        let created = memory
            .create(
                None,
                MemoryType::ReminderPreference,
                "晚上八点提醒",
                MemorySource::UserStatement,
                1.0,
            )
            .await
            .unwrap();

        let edited = memory
            .update_content(&created.id, "晚上九点提醒")
            .await
            .unwrap();
        assert_eq!(edited.content, "晚上九点提醒");
        assert_eq!(edited.status, MemoryStatus::Confirmed);

        let deactivated = memory.deactivate(&created.id).await.unwrap();
        assert_eq!(deactivated.status, MemoryStatus::Inactive);
        assert_eq!(
            memory.deactivate(&created.id).await.unwrap_err(),
            AgentError::NotFound(created.id.clone())
        );

        memory.delete(&created.id).await.unwrap();
        assert_eq!(
            memory.delete(&created.id).await.unwrap_err(),
            AgentError::NotFound(created.id.clone())
        );
        assert!(memory.list(None, true).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn list_filters_by_exam_and_hides_inactive_unless_requested() {
        let pool = memory_pool().await;
        sqlx::raw_sql(
            "INSERT INTO exams (id, name, exam_date) VALUES ('exam-a', 'A', '2030-01-01')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let memory = MemoryRepository::new(pool);

        let exam_scoped = memory
            .create(
                Some("exam-a"),
                MemoryType::LearningConstraint,
                "工作日只安排一科",
                MemorySource::UserStatement,
                1.0,
            )
            .await
            .unwrap();
        let global = memory
            .create(
                None,
                MemoryType::SchedulePreference,
                "周末上午学习",
                MemorySource::UserStatement,
                1.0,
            )
            .await
            .unwrap();
        let inactive = memory
            .create(
                None,
                MemoryType::SubjectPreference,
                "旧的偏好",
                MemorySource::BehaviorInferred,
                0.5,
            )
            .await
            .unwrap();
        memory.deactivate(&inactive.id).await.unwrap();

        let exam_rows = memory.list(Some("exam-a"), false).await.unwrap();
        let exam_ids: Vec<&str> = exam_rows.iter().map(|row| row.id.as_str()).collect();
        assert!(exam_ids.contains(&exam_scoped.id.as_str()));
        assert!(exam_ids.contains(&global.id.as_str()));
        assert!(!exam_ids.contains(&inactive.id.as_str()));

        let all_rows = memory.list(None, true).await.unwrap();
        assert_eq!(all_rows.len(), 3);
    }

    #[tokio::test]
    async fn relevant_returns_only_confirmed_and_orders_by_last_use() {
        let pool = memory_pool().await;
        let memory = MemoryRepository::new(pool);

        let candidate = memory
            .create(
                None,
                MemoryType::ConfirmedWeakness,
                "未确认的薄弱点",
                MemorySource::ModelCandidate,
                0.5,
            )
            .await
            .unwrap();
        let old = memory
            .create(
                None,
                MemoryType::DailyCapacity,
                "每天两小时",
                MemorySource::UserStatement,
                1.0,
            )
            .await
            .unwrap();
        let recent = memory
            .create(
                None,
                MemoryType::SchedulePreference,
                "早上背单词",
                MemorySource::UserStatement,
                1.0,
            )
            .await
            .unwrap();
        memory.touch(&recent.id).await.unwrap();

        let relevant = memory.relevant(None, 10).await.unwrap();
        let ids: Vec<&str> = relevant.iter().map(|row| row.id.as_str()).collect();
        // Candidate excluded; recently-used confirmed memory first.
        assert!(!ids.contains(&candidate.id.as_str()));
        assert_eq!(ids.first(), Some(&recent.id.as_str()));
        assert!(ids.contains(&old.id.as_str()));

        // touch on an unknown id is not found.
        assert_eq!(
            memory.touch("nope").await.unwrap_err(),
            AgentError::NotFound("nope".to_owned())
        );
    }
}
