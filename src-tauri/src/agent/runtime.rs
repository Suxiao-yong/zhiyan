use super::error::AgentError;
use super::model::{AgentRun, AgentSession, RunEvent};
use super::repository::AgentRepository;
use super::state;

#[derive(Clone)]
pub struct AgentRuntime {
    repository: AgentRepository,
}

impl AgentRuntime {
    pub fn new(repository: AgentRepository) -> Self {
        Self { repository }
    }

    pub async fn create_session(
        &self,
        exam_id: Option<&str>,
        title: &str,
    ) -> Result<AgentSession, AgentError> {
        self.repository.create_session(exam_id, title).await
    }

    pub async fn create_run(&self, session_id: &str, goal: &str) -> Result<AgentRun, AgentError> {
        self.repository.create_run(session_id, goal, "user").await
    }

    pub async fn transition_run(
        &self,
        run_id: &str,
        event: RunEvent,
    ) -> Result<AgentRun, AgentError> {
        let current = self.repository.get_run(run_id).await?;
        let next = state::transition(current.status, event)?;
        self.repository
            .transition_run_status(run_id, current.status, next, &event.to_string())
            .await
    }

    pub async fn recover_interrupted(&self) -> Result<u64, AgentError> {
        self.repository.interrupt_active_runs().await
    }

    pub async fn health(&self) -> Result<(), AgentError> {
        self.repository.health().await
    }

    pub async fn prepare_database_restore(&self) -> Result<(), AgentError> {
        self.repository.prepare_database_restore().await
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    use super::AgentRuntime;
    use crate::agent::error::AgentError;
    use crate::agent::model::{RunEvent, RunStatus};
    use crate::agent::repository::AgentRepository;
    use crate::db::AGENT_SCHEMA_SQL;

    async fn test_runtime() -> (AgentRuntime, sqlx::SqlitePool) {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();

        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(foreign_keys, 1);

        sqlx::query("CREATE TABLE exams (id TEXT PRIMARY KEY)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::raw_sql(AGENT_SCHEMA_SQL)
            .execute(&pool)
            .await
            .unwrap();

        (AgentRuntime::new(AgentRepository::new(pool.clone())), pool)
    }

    async fn status_event_count(pool: &sqlx::SqlitePool, run_id: &str) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_events WHERE run_id = ? AND event_type = 'run.status_changed'",
        )
        .bind(run_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn creates_queued_user_run_and_transitions_through_state_machine() {
        let (runtime, pool) = test_runtime().await;

        let session = runtime.create_session(None, "Session").await.unwrap();
        let run = runtime.create_run(&session.id, "Goal").await.unwrap();
        assert_eq!(run.status, RunStatus::Queued);
        assert_eq!(run.trigger_source, "user");

        let running = runtime
            .transition_run(&run.id, RunEvent::Start)
            .await
            .unwrap();
        assert_eq!(running.status, RunStatus::Running);

        let cancelled = runtime
            .transition_run(&run.id, RunEvent::Cancel)
            .await
            .unwrap();
        assert_eq!(cancelled.status, RunStatus::Cancelled);
        assert_eq!(status_event_count(&pool, &run.id).await, 2);
    }

    #[tokio::test]
    async fn terminal_transition_is_rejected_without_an_audit_event() {
        let (runtime, pool) = test_runtime().await;
        let session = runtime.create_session(None, "Session").await.unwrap();
        let run = runtime.create_run(&session.id, "Goal").await.unwrap();
        runtime
            .transition_run(&run.id, RunEvent::Cancel)
            .await
            .unwrap();
        let before = status_event_count(&pool, &run.id).await;

        let error = runtime
            .transition_run(&run.id, RunEvent::Start)
            .await
            .unwrap_err();

        assert_eq!(error.code(), "invalid_transition");
        assert!(matches!(error, AgentError::InvalidTransition { .. }));
        assert_eq!(status_event_count(&pool, &run.id).await, before);
    }

    #[tokio::test]
    async fn recovery_interrupts_running_but_preserves_waiting_approval() {
        let (runtime, pool) = test_runtime().await;
        let session = runtime.create_session(None, "Session").await.unwrap();
        let running = runtime.create_run(&session.id, "Running").await.unwrap();
        runtime
            .transition_run(&running.id, RunEvent::Start)
            .await
            .unwrap();
        let waiting = runtime.create_run(&session.id, "Waiting").await.unwrap();
        runtime
            .transition_run(&waiting.id, RunEvent::Start)
            .await
            .unwrap();
        runtime
            .transition_run(&waiting.id, RunEvent::RequestApproval)
            .await
            .unwrap();

        assert_eq!(runtime.recover_interrupted().await.unwrap(), 1);

        let running_status: String =
            sqlx::query_scalar("SELECT status FROM agent_runs WHERE id = ?")
                .bind(&running.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let waiting_status: String =
            sqlx::query_scalar("SELECT status FROM agent_runs WHERE id = ?")
                .bind(&waiting.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(running_status, "interrupted");
        assert_eq!(waiting_status, "waiting_approval");
    }

    #[tokio::test]
    async fn health_succeeds_for_connected_repository() {
        let (runtime, _pool) = test_runtime().await;

        runtime.health().await.unwrap();
    }
}
