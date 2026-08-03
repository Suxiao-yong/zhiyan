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
        assert_eq!(listed.len(), 9);
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

    #[tokio::test]
    async fn query_tools_read_the_active_exam_range_and_history() {
        let (runtime, pool) = test_runtime().await;
        sqlx::raw_sql(
            r#"
            INSERT INTO exams(id,name,exam_date) VALUES('exam-q','Q','2030-01-01');
            INSERT INTO subjects(id,exam_id,name) VALUES('sub-q','exam-q','鏁板');
            INSERT INTO study_plans(id,exam_id,subject_id,date,planned_duration,status)
            VALUES('plan-q','exam-q','sub-q','2026-07-18',60,'pending');
            INSERT INTO study_records(id,date,subject_id,duration_min,questions_count,correct_count)
            VALUES('rec-q','2026-07-18','sub-q',30,5,4);
            INSERT INTO agent_sessions(id,title) VALUES('session-tool','Tool');
            INSERT INTO agent_runs(id,session_id,goal,status)
            VALUES('run-tool','session-tool','Tool','running');
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        // exam.get_active: no active-exam setting, so the latest exam wins.
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-tool".to_owned(),
                step_index: 0,
                tool_name: "exam.get_active".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({}),
                idempotency_key: None,
                approval_id: None,
            })
            .await
            .unwrap();
        let output = match response {
            ToolCallResponse::Completed { output, .. } => output,
            other => panic!("expected completed, got {other:?}"),
        };
        assert_eq!(output["exam_id"], "exam-q");
        assert_eq!(output["subjects"][0]["name"], "鏁板");

        // plan.get_range: plans within the interval.
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-tool".to_owned(),
                step_index: 1,
                tool_name: "plan.get_range".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({"exam_id":"exam-q","start_date":"2026-07-01","end_date":"2026-07-31"}),
                idempotency_key: None,
                approval_id: None,
            })
            .await
            .unwrap();
        let output = match response {
            ToolCallResponse::Completed { output, .. } => output,
            other => panic!("expected completed, got {other:?}"),
        };
        assert_eq!(output["plans"][0]["id"], "plan-q");

        // record.get_history: newest record with subject name.
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-tool".to_owned(),
                step_index: 2,
                tool_name: "record.get_history".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({"exam_id":"exam-q"}),
                idempotency_key: None,
                approval_id: None,
            })
            .await
            .unwrap();
        let output = match response {
            ToolCallResponse::Completed { output, .. } => output,
            other => panic!("expected completed, got {other:?}"),
        };
        assert_eq!(output["records"][0]["id"], "rec-q");
        assert_eq!(output["records"][0]["subject_name"], "鏁板");
        assert_eq!(output["records"][0]["duration_min"], 30);

        // Range validation: inverted dates are schema-invalid.
        let err = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-tool".to_owned(),
                step_index: 3,
                tool_name: "plan.get_range".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({"exam_id":"exam-q","start_date":"2026-07-31","end_date":"2026-07-01"}),
                idempotency_key: None,
                approval_id: None,
            })
            .await
            .unwrap_err();
        assert_eq!(err.code(), "tool_schema_invalid");
    }

    #[tokio::test]
    async fn write_tools_create_records_and_wrong_questions_idempotently() {
        let (runtime, pool) = test_runtime().await;
        sqlx::raw_sql(
            r#"
            INSERT INTO exams(id,name,exam_date) VALUES('exam-w','W','2030-01-01');
            INSERT INTO subjects(id,exam_id,name) VALUES('sub-w','exam-w','鏁板');
            INSERT INTO agent_sessions(id,title) VALUES('session-tool','Tool');
            INSERT INTO agent_runs(id,session_id,goal,status)
            VALUES('run-tool','session-tool','Tool','running');
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        // record.create_free inserts a record.
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-tool".to_owned(),
                step_index: 0,
                tool_name: "record.create_free".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({
                    "exam_id":"exam-w","date":"2026-07-18","subject_id":"sub-w",
                    "duration_min":45,"content":"鑷敱澶嶄範","questions_count":6,"correct_count":5
                }),
                idempotency_key: Some("free-1".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap();
        let output = match response {
            ToolCallResponse::Completed { output, .. } => output,
            other => panic!("expected completed, got {other:?}"),
        };
        let record_id = output["id"].as_str().unwrap().to_owned();
        let row_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row_count, 1);

        // Replaying the same idempotency key is rejected (only check-in
        // supports replayed delivery); the row is still written exactly once.
        let err = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-tool".to_owned(),
                step_index: 1,
                tool_name: "record.create_free".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({
                    "exam_id":"exam-w","date":"2026-07-18","subject_id":"sub-w","duration_min":45
                }),
                idempotency_key: Some("free-1".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap_err();
        assert_eq!(err.code(), "idempotency_conflict");
        let row_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row_count, 1);

        // wrong_question.create links to the record; replay adds nothing.
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-tool".to_owned(),
                step_index: 1,
                tool_name: "wrong_question.create".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({
                    "subject_id":"sub-w","record_id":record_id,
                    "question_desc":"閿欓鎻忚堪","my_answer":"x"
                }),
                idempotency_key: Some("wq-1".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap_or_else(|err| panic!("wq create failed: {err:?} (code {})", err.code()));
        let output = match response {
            ToolCallResponse::Completed { output, .. } => output,
            other => panic!("expected completed, got {other:?}"),
        };
        let wrong_id = output["id"].as_str().unwrap().to_owned();
        let err = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-tool".to_owned(),
                step_index: 2,
                tool_name: "wrong_question.create".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({
                    "subject_id":"sub-w","record_id":record_id,"question_desc":"閿欓鎻忚堪"
                }),
                idempotency_key: Some("wq-1".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap_err();
        assert_eq!(err.code(), "idempotency_conflict");
        let wq_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM wrong_questions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(wq_count, 1);

        // mark_mastered flips the flag; a missing id is a persistence error.
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-tool".to_owned(),
                step_index: 2,
                tool_name: "wrong_question.mark_mastered".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({"id": wrong_id}),
                idempotency_key: Some("mm-1".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap();
        let output = match response {
            ToolCallResponse::Completed { output, .. } => output,
            other => panic!("expected completed, got {other:?}"),
        };
        assert_eq!(output["mastered"], 1);
        let mastered: i64 = sqlx::query_scalar("SELECT mastered FROM wrong_questions WHERE id = ?")
            .bind(&wrong_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(mastered, 1);

        // A subject outside the exam is rejected (run then fails; reset it).
        let err = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-tool".to_owned(),
                step_index: 3,
                tool_name: "record.create_free".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({
                    "exam_id":"other","date":"2026-07-18","subject_id":"sub-w","duration_min":10
                }),
                idempotency_key: Some("free-2".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap_err();
        assert_eq!(err.code(), "persistence_error");
        sqlx::query("UPDATE agent_runs SET status='running', current_step=4 WHERE id='run-tool'")
            .execute(&pool)
            .await
            .unwrap();

        // Marking a missing wrong question is a persistence error.
        let err = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-tool".to_owned(),
                step_index: 4,
                tool_name: "wrong_question.mark_mastered".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({"id":"missing"}),
                idempotency_key: Some("mm-2".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap_err();
        assert_eq!(err.code(), "persistence_error");
    }

    #[tokio::test]
    async fn plan_generate_is_approval_gated_and_idempotent_per_week() {
        let (runtime, pool) = test_runtime().await;
        sqlx::raw_sql(
            r#"
            INSERT INTO exams(id,name,exam_date) VALUES('exam-g','G','2030-01-01');
            INSERT INTO subjects(id,exam_id,name,weight) VALUES
                ('sub-g1','exam-g','数学',2.0),
                ('sub-g2','exam-g','英语',1.0);
            INSERT INTO agent_sessions(id,title) VALUES('session-tool','Tool');
            INSERT INTO agent_runs(id,session_id,goal,status)
            VALUES('run-tool','session-tool','Tool','running');
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        // First call: R2 asks for a summary first, nothing is written yet.
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-tool".to_owned(),
                step_index: 0,
                tool_name: "plan.generate".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({
                    "exam_id":"exam-g","week_start":"2026-07-13","daily_capacity_min":90
                }),
                idempotency_key: Some("gen-1".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap();
        let preview = match response {
            ToolCallResponse::SummaryRequired { preview, .. } => preview,
            other => panic!("expected summary required, got {other:?}"),
        };
        assert_eq!(preview["exam_id"], "exam-g");
        let plan_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_plans")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(plan_count, 0);

        // Enabling auto-execution lets the same call dispatch: seven rows.
        sqlx::query("INSERT INTO settings(key,value) VALUES('agent_r2_auto_execute','true')")
            .execute(&pool)
            .await
            .unwrap();
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-tool".to_owned(),
                step_index: 0,
                tool_name: "plan.generate".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({
                    "exam_id":"exam-g","week_start":"2026-07-13","daily_capacity_min":90
                }),
                idempotency_key: Some("gen-1".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap();
        let output = match response {
            ToolCallResponse::Completed { output, .. } => output,
            other => panic!("expected completed, got {other:?}"),
        };
        assert_eq!(output["newly_created"], true);
        assert_eq!(output["rows"].as_array().unwrap().len(), 7);
        assert_eq!(output["capacity_min"], 90);
        let plan_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_plans")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(plan_count, 7);

        // Re-running the same week returns the existing rows unchanged.
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-tool".to_owned(),
                step_index: 1,
                tool_name: "plan.generate".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({
                    "exam_id":"exam-g","week_start":"2026-07-13","daily_capacity_min":90
                }),
                idempotency_key: Some("gen-2".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap();
        let output = match response {
            ToolCallResponse::Completed { output, .. } => output,
            other => panic!("expected completed, got {other:?}"),
        };
        assert_eq!(output["newly_created"], false);
        assert_eq!(output["rows"].as_array().unwrap().len(), 7);
        let plan_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_plans")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(plan_count, 7);

        // Weighted slotting: the heavier subject gets the most days.
        let math_days: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM study_plans WHERE subject_id='sub-g1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let english_days: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM study_plans WHERE subject_id='sub-g2'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(math_days > english_days);
        assert_eq!(math_days + english_days, 7);
    }

    #[tokio::test]
    async fn plan_generate_handles_many_subjects_without_stalling() {
        // Regression: with 8-14 equal-weight subjects the old slotting loop
        // never terminated (nothing to decrement). The floor+largest-fraction
        // algorithm must return exactly seven rows and finish quickly.
        let (runtime, pool) = test_runtime().await;
        let mut seed =
            String::from("INSERT INTO exams(id,name,exam_date) VALUES('exam-m','M','2030-01-01');");
        for index in 0..10 {
            seed.push_str(&format!(
                "INSERT INTO subjects(id,exam_id,name,weight) VALUES('sub-m{index}','exam-m','S{index}',1.0);"
            ));
        }
        seed.push_str(
            "INSERT INTO agent_sessions(id,title) VALUES('session-tool','Tool');\
             INSERT INTO agent_runs(id,session_id,goal,status)\
             VALUES('run-tool','session-tool','Tool','running');",
        );
        sqlx::raw_sql(&seed).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO settings(key,value) VALUES('agent_r2_auto_execute','true')")
            .execute(&pool)
            .await
            .unwrap();

        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-tool".to_owned(),
                step_index: 0,
                tool_name: "plan.generate".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({
                    "exam_id":"exam-m","week_start":"2026-07-13","daily_capacity_min":60
                }),
                idempotency_key: Some("gen-m".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap();
        let output = match response {
            ToolCallResponse::Completed { output, .. } => output,
            other => panic!("expected completed, got {other:?}"),
        };
        let rows = output["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 7);
        let plan_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_plans")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(plan_count, 7);
    }
}
