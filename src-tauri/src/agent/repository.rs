use super::error::AgentError;
use super::model::{AgentRun, AgentSession, RunStatus};
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
