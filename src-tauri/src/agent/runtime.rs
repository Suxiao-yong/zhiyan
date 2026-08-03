use super::error::AgentError;
use super::executor::{AgentExecutor, ToolUndoResponse};
use super::model::{
    AgentRun, AgentSession, ApprovalRecord, RunEvent, ToolCallRequest, ToolCallResponse,
};
use super::repository::AgentRepository;
use super::state;
use super::tools::ListedTool;

#[derive(Clone)]
pub struct AgentRuntime {
    repository: AgentRepository,
    executor: AgentExecutor,
}

impl AgentRuntime {
    pub fn new(repository: AgentRepository, executor: AgentExecutor) -> Self {
        Self {
            repository,
            executor,
        }
    }

    pub async fn list_tools(&self) -> Result<Vec<ListedTool>, AgentError> {
        self.executor.list_tools().await
    }

    pub async fn execute_tool(
        &self,
        request: ToolCallRequest,
    ) -> Result<ToolCallResponse, AgentError> {
        self.executor.execute(request).await
    }

    pub async fn decide_approval(
        &self,
        approval_id: &str,
        approve: bool,
    ) -> Result<ApprovalRecord, AgentError> {
        self.executor.decide_approval(approval_id, approve).await
    }

    pub async fn undo_tool(&self, step_id: &str) -> Result<ToolUndoResponse, AgentError> {
        self.executor.undo(step_id).await
    }

    pub async fn create_session(
        &self,
        exam_id: Option<&str>,
        title: &str,
    ) -> Result<AgentSession, AgentError> {
        self.repository.create_session(exam_id, title).await
    }

    /// Read access for the read-only UI commands (session list, messages,
    /// approvals).
    pub(crate) fn repository(&self) -> &AgentRepository {
        &self.repository
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
    use crate::agent::executor::AgentExecutor;
    use crate::agent::model::{RunEvent, RunStatus, ToolCallRequest, ToolCallResponse};
    use crate::agent::repository::AgentRepository;
    use crate::agent::tools::ToolOwnership;

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

        for migration in crate::db::migrations() {
            sqlx::raw_sql(migration.sql).execute(&pool).await.unwrap();
        }

        (
            AgentRuntime::new(
                AgentRepository::new(pool.clone()),
                AgentExecutor::new(pool.clone()),
            ),
            pool,
        )
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

    #[tokio::test]
    async fn runtime_is_the_public_tool_execution_boundary() {
        let (runtime, pool) = test_runtime().await;
        let listed = runtime.list_tools().await.unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(
            listed
                .iter()
                .find(|tool| tool.descriptor.name == "plan.get_today")
                .unwrap()
                .ownership,
            ToolOwnership::Shadow
        );

        sqlx::raw_sql(
            r#"
            INSERT INTO exams(id,name,exam_date) VALUES('exam-runtime','Runtime','2030-01-01');
            INSERT INTO subjects(id,exam_id,name) VALUES('subject-runtime','exam-runtime','Runtime');
            INSERT INTO agent_sessions(id,title) VALUES('session-tool','Tool');
            INSERT INTO agent_runs(id,session_id,goal,status)
            VALUES('run-tool','session-tool','Tool','running');
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-tool".to_owned(),
                step_index: 0,
                tool_name: "plan.get_today".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({"exam_id":"exam-runtime"}),
                idempotency_key: None,
                approval_id: None,
            })
            .await
            .unwrap();
        assert!(matches!(response, ToolCallResponse::Completed { .. }));
        assert_eq!(
            runtime
                .decide_approval("missing-approval", true)
                .await
                .unwrap_err()
                .code(),
            "not_found"
        );
        sqlx::query(
            "UPDATE settings SET value='rust-owned' WHERE key='agent_tool_owner.record.checkin_plan'",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            runtime.undo_tool("missing-step").await.unwrap_err().code(),
            "not_found"
        );
    }
}
