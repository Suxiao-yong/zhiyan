use super::error::AgentError;
use super::model::{AgentMessage, AgentRun, AgentSession, ApprovalRecord, RunStatus};
use serde_json::Value;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Clone)]
pub struct AgentRepository {
    pool: SqlitePool,
}

impl AgentRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn health(&self) -> Result<(), AgentError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(map_sqlx)?;
        Ok(())
    }

    pub async fn create_session(
        &self,
        exam_id: Option<&str>,
        title: &str,
    ) -> Result<AgentSession, AgentError> {
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO agent_sessions (id, exam_id, title) VALUES (?, ?, ?)")
            .bind(&id)
            .bind(exam_id)
            .bind(title)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx)?;
        sqlx::query_as::<_, AgentSession>("SELECT * FROM agent_sessions WHERE id = ?")
            .bind(&id)
            .fetch_one(&self.pool)
            .await
            .map_err(map_sqlx)
    }

    /// Recent sessions, newest activity first (M5 Agent OS sidebar).
    pub async fn session_list(&self, limit: i64) -> Result<Vec<AgentSession>, AgentError> {
        sqlx::query_as::<_, AgentSession>(
            "SELECT id, exam_id, title, status, created_at, updated_at \
             FROM agent_sessions ORDER BY updated_at DESC, rowid DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx)
    }

    /// A session's conversation, oldest first.
    pub async fn session_messages(
        &self,
        session_id: &str,
    ) -> Result<Vec<AgentMessage>, AgentError> {
        sqlx::query_as::<_, AgentMessage>(
            "SELECT id, session_id, run_id, role, text, content_json, prompt_tokens, \
             completion_tokens, model, created_at \
             FROM agent_messages WHERE session_id = ? ORDER BY created_at, rowid",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx)
    }

    /// Approvals, pending first then decided, newest first (M5 approval card).
    pub async fn approval_list(&self, limit: i64) -> Result<Vec<ApprovalRecord>, AgentError> {
        sqlx::query_as::<_, ApprovalRow>(
            "SELECT id, run_id, step_id, risk, preview_json, precondition_json, status, \
             expires_at, decided_at, created_at FROM agent_approvals \
             ORDER BY CASE WHEN status = 'pending' THEN 0 ELSE 1 END, created_at DESC \
             LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx)?
        .into_iter()
        .map(TryInto::try_into)
        .collect()
    }

    pub async fn create_run(
        &self,
        session_id: &str,
        goal: &str,
        trigger_source: &str,
    ) -> Result<AgentRun, AgentError> {
        let id = Uuid::new_v4().to_string();
        let mut tx = self.pool.begin().await.map_err(map_sqlx)?;
        sqlx::query(
            "INSERT INTO agent_runs (id, session_id, goal, status, trigger_source) \
             VALUES (?, ?, ?, 'queued', ?)",
        )
        .bind(&id)
        .bind(session_id)
        .bind(goal)
        .bind(trigger_source)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;

        let payload = serde_json::json!({ "goal": goal });
        if let Err(error) = sqlx::query(
            "INSERT INTO agent_events (run_id, event_type, payload_json) \
             VALUES (?, 'run.created', ?)",
        )
        .bind(&id)
        .bind(payload.to_string())
        .execute(&mut *tx)
        .await
        {
            tx.rollback().await.map_err(map_sqlx)?;
            return Err(map_sqlx(error));
        }

        tx.commit().await.map_err(map_sqlx)?;
        self.get_run(&id).await
    }

    pub async fn get_run(&self, id: &str) -> Result<AgentRun, AgentError> {
        sqlx::query_as::<_, AgentRun>("SELECT * FROM agent_runs WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(map_sqlx)
    }

    /// Direct status setter for migration, recovery, and test setup.
    ///
    /// This deliberately does not append a transition audit event. Normal runtime
    /// state changes must use `transition_run_status` instead.
    pub async fn update_run_status(
        &self,
        id: &str,
        status: RunStatus,
        error_code: Option<&str>,
    ) -> Result<AgentRun, AgentError> {
        let status = status.to_string();
        let result = sqlx::query(
            "UPDATE agent_runs SET status = ?, error_code = ?, \
             started_at = CASE WHEN ? = 'running' \
                 THEN COALESCE(started_at, datetime('now','localtime')) ELSE started_at END, \
             completed_at = CASE WHEN ? IN ('completed','cancelled','failed') \
                 THEN datetime('now','localtime') ELSE NULL END \
             WHERE id = ?",
        )
        .bind(&status)
        .bind(error_code)
        .bind(&status)
        .bind(&status)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx)?;
        if result.rows_affected() == 0 {
            return Err(AgentError::NotFound(id.to_owned()));
        }
        self.get_run(id).await
    }

    pub async fn transition_run_status(
        &self,
        id: &str,
        expected: RunStatus,
        next: RunStatus,
        event: &str,
    ) -> Result<AgentRun, AgentError> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx)?;
        let next_value = next.to_string();
        let result = sqlx::query(
            "UPDATE agent_runs SET status = ?, \
             started_at = CASE WHEN ? = 'running' \
                 THEN COALESCE(started_at, datetime('now','localtime')) ELSE started_at END, \
             completed_at = CASE WHEN ? IN ('completed','cancelled','failed') \
                 THEN datetime('now','localtime') ELSE NULL END \
             WHERE id = ? AND status = ?",
        )
        .bind(&next_value)
        .bind(&next_value)
        .bind(&next_value)
        .bind(id)
        .bind(expected.to_string())
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        if result.rows_affected() == 0 {
            tx.rollback().await.map_err(map_sqlx)?;
            return Err(AgentError::Conflict);
        }

        let payload = serde_json::json!({
            "from": expected.to_string(),
            "to": next_value,
            "event": event,
        });
        if let Err(error) = sqlx::query(
            "INSERT INTO agent_events (run_id, event_type, payload_json) \
             VALUES (?, 'run.status_changed', ?)",
        )
        .bind(id)
        .bind(payload.to_string())
        .execute(&mut *tx)
        .await
        {
            tx.rollback().await.map_err(map_sqlx)?;
            return Err(map_sqlx(error));
        }

        tx.commit().await.map_err(map_sqlx)?;
        self.get_run(id).await
    }

    pub async fn append_event(
        &self,
        run_id: &str,
        event_type: &str,
        payload: &Value,
    ) -> Result<(), AgentError> {
        sqlx::query("INSERT INTO agent_events (run_id, event_type, payload_json) VALUES (?, ?, ?)")
            .bind(run_id)
            .bind(event_type)
            .bind(payload.to_string())
            .execute(&self.pool)
            .await
            .map_err(map_sqlx)?;
        Ok(())
    }

    pub async fn prepare_database_restore(&self) -> Result<(), AgentError> {
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await
            .map_err(map_sqlx)?;
        self.pool.close().await;
        Ok(())
    }

    pub async fn interrupt_active_runs(&self) -> Result<u64, AgentError> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx)?;
        let ids: Vec<String> = sqlx::query_scalar(
            "UPDATE agent_runs SET status = 'interrupted', completed_at = NULL \
             WHERE status = 'running' RETURNING id",
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(map_sqlx)?;

        for id in &ids {
            sqlx::query(
                "INSERT INTO agent_events (run_id, event_type, payload_json) \
                 VALUES (?, 'run.interrupted', ?)",
            )
            .bind(id)
            .bind(serde_json::json!({ "reason": "application_restart" }).to_string())
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx)?;
        }

        tx.commit().await.map_err(map_sqlx)?;
        Ok(ids.len() as u64)
    }
}

fn map_sqlx(error: sqlx::Error) -> AgentError {
    match error {
        sqlx::Error::RowNotFound => AgentError::NotFound("agent record".to_owned()),
        other => AgentError::Persistence(other.to_string()),
    }
}

#[derive(sqlx::FromRow)]
struct ApprovalRow {
    id: String,
    run_id: String,
    step_id: String,
    risk: i64,
    preview_json: Option<String>,
    precondition_json: Option<String>,
    status: String,
    expires_at: String,
    decided_at: Option<String>,
    created_at: String,
}

impl TryFrom<ApprovalRow> for ApprovalRecord {
    type Error = AgentError;

    fn try_from(row: ApprovalRow) -> Result<Self, Self::Error> {
        let preview = row
            .preview_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| AgentError::ApprovalInvalid)?
            .unwrap_or(Value::Null);
        let precondition_hash = row
            .precondition_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .and_then(|value| value["hash"].as_str().map(str::to_owned))
            .unwrap_or_default();
        Ok(ApprovalRecord {
            id: row.id,
            run_id: row.run_id,
            step_id: row.step_id,
            risk: row.risk,
            preview,
            precondition_hash,
            status: row.status,
            expires_at: row.expires_at,
            decided_at: row.decided_at,
            created_at: row.created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    async fn repo_pool() -> SqlitePool {
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
    async fn session_list_returns_newest_activity_first() {
        let pool = repo_pool().await;
        let repo = AgentRepository::new(pool);
        repo.create_session(None, "第一会话").await.unwrap();
        repo.create_session(None, "第二会话").await.unwrap();

        let sessions = repo.session_list(10).await.unwrap();
        assert_eq!(sessions.len(), 2);
        // Most recently created first (same-second timestamps break ties by rowid).
        assert_eq!(sessions[0].title, "第二会话");
        assert_eq!(sessions[1].title, "第一会话");
        assert_eq!(sessions[0].status, "active");
    }

    #[tokio::test]
    async fn session_messages_returns_oldest_first() {
        let pool = repo_pool().await;
        let repo = AgentRepository::new(pool.clone());
        let session = repo.create_session(None, "S").await.unwrap();
        for (role, text) in [("user", "你好"), ("assistant", "你好！")] {
            sqlx::query(
                "INSERT INTO agent_messages (id, session_id, role, text) VALUES (?, ?, ?, ?)",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&session.id)
            .bind(role)
            .bind(text)
            .execute(&pool)
            .await
            .unwrap();
        }

        let messages = repo.session_messages(&session.id).await.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].text, "你好");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].prompt_tokens, 0);
    }

    #[tokio::test]
    async fn approval_list_orders_pending_first() {
        let pool = repo_pool().await;
        let repo = AgentRepository::new(pool.clone());
        let session = repo.create_session(None, "S").await.unwrap();
        let run = repo.create_run(&session.id, "目标", "user").await.unwrap();
        sqlx::query(
            "INSERT INTO agent_steps (id, run_id, step_index, tool_name, tool_version, risk, status) \
             VALUES ('step-1', ?, 0, 'plan.get_today', 'v1', 2, 'waiting_approval')",
        )
        .bind(&run.id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO agent_steps (id, run_id, step_index, tool_name, tool_version, risk, status) \
             VALUES ('step-2', ?, 1, 'record.checkin_plan', 'v1', 2, 'waiting_approval')",
        )
        .bind(&run.id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO agent_approvals \
             (id, run_id, step_id, risk, preview_json, precondition_json, status, expires_at) \
             VALUES ('ap-1', ?, 'step-1', 2, '{\"plan_ids\":[\"p-1\"]}', '{\"hash\":\"h1\"}', \
                     'pending', '2099-01-01 00:00:00')",
        )
        .bind(&run.id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO agent_approvals \
             (id, run_id, step_id, risk, preview_json, precondition_json, status, expires_at) \
             VALUES ('ap-2', ?, 'step-2', 2, '{\"plan_ids\":[]}', '{\"hash\":\"h2\"}', \
                     'approved', '2099-01-01 00:00:00')",
        )
        .bind(&run.id)
        .execute(&pool)
        .await
        .unwrap();

        let approvals = repo.approval_list(10).await.unwrap();
        assert_eq!(approvals.len(), 2);
        assert_eq!(approvals[0].id, "ap-1");
        assert_eq!(approvals[0].status, "pending");
        assert_eq!(approvals[0].precondition_hash, "h1");
        assert_eq!(approvals[0].preview["plan_ids"][0], "p-1");
        assert_eq!(approvals[1].status, "approved");
    }
}
