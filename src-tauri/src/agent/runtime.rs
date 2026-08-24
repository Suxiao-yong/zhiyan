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

    /// Mandatory Task C: approve executes the approved tool through the Rust
    /// executor (re-checking scope/precondition/schema); reject only updates
    /// state. The frontend confirm button must use this path.
    pub async fn resolve_approval(
        &self,
        approval_id: &str,
        approve: bool,
    ) -> Result<ApprovalRecord, AgentError> {
        self.executor.resolve_approval(approval_id, approve).await
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

    /// Atomically fail a run that is still `running` or `waiting_approval`,
    /// persisting the stable error code. Returns `Conflict` when the run is
    /// already terminal so callers never leave the database claiming `running`
    /// while reporting a failure to the caller.
    pub async fn fail_run(&self, run_id: &str, error_code: &str) -> Result<AgentRun, AgentError> {
        self.repository.fail_run(run_id, error_code).await
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
        let runtime = AgentRuntime::new(
            AgentRepository::new(pool.clone()),
            AgentExecutor::new(pool.clone()),
        );
        (runtime, pool)
    }

    /// Exam P (数学 weight2/level3 + 英语 weight1/level4 + 3 KPs) with one old
    /// future plan and a run bound to it, for the preview/apply approval tests.
    async fn draft_runtime() -> (AgentRuntime, sqlx::SqlitePool) {
        let (runtime, pool) = test_runtime().await;
        sqlx::raw_sql(
            r#"
            INSERT INTO exams(id,name,exam_date) VALUES('exam-p','P','2030-01-31');
            INSERT INTO subjects(id,exam_id,name,weight,current_level) VALUES
                ('sub-m','exam-p','数学',2.0,3),
                ('sub-e','exam-p','英语',1.0,4);
            INSERT INTO knowledge_points(id,subject_id,name,current_mastery) VALUES
                ('kp-f','sub-m','函数',2),
                ('kp-g','sub-m','几何',4),
                ('kp-w','sub-e','词汇',3);
            INSERT INTO agent_sessions(id,title,exam_id) VALUES('session-p','P','exam-p');
            INSERT INTO agent_runs(id,session_id,goal,status) VALUES('run-p','session-p','P','running');
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        // The old future plan must stay ahead of the business date so the
        // apply projection replaces it instead of counting it as a separate
        // today plan (the app treats dates before 04:00 as the previous day).
        let business_date =
            crate::agent::tools::plan::business_date_at(chrono::Local::now().fixed_offset());
        let old_plan_date = chrono::NaiveDate::parse_from_str(&business_date, "%Y-%m-%d")
            .unwrap()
            .checked_add_days(chrono::Days::new(30))
            .unwrap()
            .format("%Y-%m-%d")
            .to_string();

        sqlx::query(
            "INSERT INTO study_plans(id,exam_id,subject_id,date,planned_tasks,planned_duration,status)
             VALUES(?, ?, ?, ?, ?, ?, ?)",
        )
        .bind("plan-old")
        .bind("exam-p")
        .bind("sub-m")
        .bind(&old_plan_date)
        .bind("旧计划")
        .bind(60_i64)
        .bind("pending")
        .execute(&pool)
        .await
        .unwrap();
        (runtime, pool)
    }

    async fn preview_step_id(runtime: &AgentRuntime, run_id: &str, step_index: i64) -> String {
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: run_id.to_owned(),
                step_index,
                tool_name: "plan.preview_generate".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({
                    "exam_id":"exam-p","start_date":"2030-01-01","daily_hours":6.0
                }),
                idempotency_key: None,
                approval_id: None,
            })
            .await
            .unwrap();
        let ToolCallResponse::Completed { output, .. } = response else {
            panic!("preview_generate must complete without approval");
        };
        output["preview_step_id"]
            .as_str()
            .expect("preview_step_id in output")
            .to_owned()
    }

    #[tokio::test]
    async fn preview_generate_returns_a_draft_without_writing_plans() {
        let (runtime, pool) = draft_runtime().await;
        let step_id = preview_step_id(&runtime, "run-p", 0).await;
        assert!(!step_id.is_empty());
        let preview_row: (String, String) =
            sqlx::query_as("SELECT tool_name, status FROM agent_steps WHERE id=?")
                .bind(&step_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(preview_row.0, "plan.preview_generate");
        assert_eq!(preview_row.1, "completed");
        // No study_plans row was written by the preview.
        let plans: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_plans")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(plans, 1);
    }

    #[tokio::test]
    async fn approving_apply_preview_writes_exactly_once_and_reject_writes_nothing() {
        // Approve path.
        let (runtime, pool) = draft_runtime().await;
        let step_id = preview_step_id(&runtime, "run-p", 0).await;
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-p".to_owned(),
                step_index: 1,
                tool_name: "plan.apply_preview".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({ "preview_step_id": step_id }),
                idempotency_key: Some("apply-1".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap();
        let ToolCallResponse::WaitingApproval { approval_id, .. } = response else {
            panic!("apply_preview must request approval");
        };
        let run_status: String =
            sqlx::query_scalar("SELECT status FROM agent_runs WHERE id='run-p'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(run_status, "waiting_approval");

        let approved = runtime.resolve_approval(&approval_id, true).await.unwrap();
        assert_eq!(approved.status, "approved");
        // The 1 old future plan was replaced by the 60 draft rows.
        let plans: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_plans")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(plans, 60);
        let run_status: String =
            sqlx::query_scalar("SELECT status FROM agent_runs WHERE id='run-p'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(run_status, "completed");

        // Reject path: a second run on a fresh database writes nothing.
        let (runtime, pool) = draft_runtime().await;
        let step_id = preview_step_id(&runtime, "run-p", 0).await;
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-p".to_owned(),
                step_index: 1,
                tool_name: "plan.apply_preview".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({ "preview_step_id": step_id }),
                idempotency_key: Some("apply-2".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap();
        let ToolCallResponse::WaitingApproval { approval_id, .. } = response else {
            panic!("apply_preview must request approval");
        };
        runtime.resolve_approval(&approval_id, false).await.unwrap();
        let plans: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_plans")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(plans, 1);
        let run_status: String =
            sqlx::query_scalar("SELECT status FROM agent_runs WHERE id='run-p'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(run_status, "cancelled");
    }

    #[tokio::test]
    async fn repeating_confirmation_never_double_writes() {
        let (runtime, pool) = draft_runtime().await;
        let step_id = preview_step_id(&runtime, "run-p", 0).await;
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-p".to_owned(),
                step_index: 1,
                tool_name: "plan.apply_preview".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({ "preview_step_id": step_id }),
                idempotency_key: Some("apply-3".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap();
        let ToolCallResponse::WaitingApproval { approval_id, .. } = response else {
            panic!("apply_preview must request approval");
        };
        runtime.resolve_approval(&approval_id, true).await.unwrap();
        // Re-approving the already-approved approval is rejected; no second write.
        let error = runtime
            .resolve_approval(&approval_id, true)
            .await
            .unwrap_err();
        assert_eq!(error.code(), "approval_invalid");
        let plans: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_plans")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(plans, 60);
    }

    #[tokio::test]
    async fn manual_plan_change_after_preview_fails_the_precondition_safely() {
        let (runtime, pool) = draft_runtime().await;
        let step_id = preview_step_id(&runtime, "run-p", 0).await;
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-p".to_owned(),
                step_index: 1,
                tool_name: "plan.apply_preview".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({ "preview_step_id": step_id }),
                idempotency_key: Some("apply-4".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap();
        let ToolCallResponse::WaitingApproval { approval_id, .. } = response else {
            panic!("apply_preview must request approval");
        };
        // The user manually edits the plan between preview and confirmation.
        sqlx::query("UPDATE study_plans SET status='completed' WHERE id='plan-old'")
            .execute(&pool)
            .await
            .unwrap();

        let error = runtime
            .resolve_approval(&approval_id, true)
            .await
            .unwrap_err();
        assert_eq!(error.code(), "precondition_changed");
        // No draft row was inserted and the run/step/approval are terminal.
        let plans: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_plans")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(plans, 1);
        let run_status: String =
            sqlx::query_scalar("SELECT status FROM agent_runs WHERE id='run-p'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(run_status, "failed");
        let approval_status: String =
            sqlx::query_scalar("SELECT status FROM agent_approvals WHERE id=?")
                .bind(&approval_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(approval_status, "rejected");
    }

    #[tokio::test]
    async fn apply_preview_of_another_exam_draft_is_a_scope_violation() {
        let (runtime, pool) = draft_runtime().await;
        // A preview generated in a run bound to exam B.
        sqlx::raw_sql(
            r#"
            INSERT INTO exams(id,name,exam_date) VALUES('exam-b','B','2030-06-30');
            INSERT INTO subjects(id,exam_id,name,weight,current_level) VALUES
                ('sub-b','exam-b','行测',1.0,3);
            INSERT INTO agent_sessions(id,title,exam_id) VALUES('session-b','B','exam-b');
            INSERT INTO agent_runs(id,session_id,goal,status)
            VALUES('run-b','session-b','B','running');
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        let preview_b = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-b".to_owned(),
                step_index: 0,
                tool_name: "plan.preview_generate".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({
                    "exam_id":"exam-b","start_date":"2030-01-01","daily_hours":6.0
                }),
                idempotency_key: None,
                approval_id: None,
            })
            .await
            .unwrap();
        let ToolCallResponse::Completed { output, .. } = preview_b else {
            panic!("preview must complete");
        };
        let preview_b_step = output["preview_step_id"].as_str().unwrap().to_owned();

        // Run A tries to apply exam B's draft -> scope violation, nothing written.
        let error = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-p".to_owned(),
                step_index: 1,
                tool_name: "plan.apply_preview".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({ "preview_step_id": preview_b_step }),
                idempotency_key: Some("apply-b".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap_err();
        assert_eq!(error.code(), "tool_scope_violation");
        let plans: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_plans")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(plans, 1);
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
        assert_eq!(listed.len(), 13);
        assert_eq!(
            listed
                .iter()
                .find(|tool| tool.descriptor.name == "plan.get_today")
                .unwrap()
                .ownership,
            ToolOwnership::RustOwned
        );

        sqlx::raw_sql(
            r#"
            INSERT INTO exams(id,name,exam_date) VALUES('exam-runtime','Runtime','2030-01-01');
            INSERT INTO subjects(id,exam_id,name) VALUES('subject-runtime','exam-runtime','Runtime');
            INSERT INTO agent_sessions(id,title,exam_id) VALUES('session-tool','Tool','exam-runtime');
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
    async fn legacy_ownership_settings_are_inert_and_never_rewritten() {
        let (runtime, pool) = test_runtime().await;
        sqlx::raw_sql(
            r#"
            INSERT INTO exams(id,name,exam_date) VALUES('exam-legacy','Legacy','2030-01-01');
            INSERT INTO subjects(id,exam_id,name) VALUES('sub-legacy','exam-legacy','数学');
            INSERT INTO study_plans(id,exam_id,subject_id,date,planned_duration,status)
            VALUES('plan-legacy','exam-legacy','sub-legacy','2020-01-01',30,'pending');
            INSERT INTO agent_sessions(id,title,exam_id) VALUES('session-legacy','Legacy','exam-legacy');
            INSERT INTO agent_runs(id,session_id,goal,status)
            VALUES('run-legacy','session-legacy','Legacy','running');
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        // Migration v5/v10 seeded legacy ownership values; since Task 13 they
        // must be inert: descriptors and execution paths come from Rust only.
        let legacy = [
            ("plan.get_today", "shadow"),
            ("record.checkin_plan", "typescript"),
            ("plan.generate", "rust-owned"),
            ("exam.get_active", "rust-owned"),
            ("wrong_question.create", "unavailable"),
        ];
        for (tool, value) in legacy {
            // Migration v5/v10 already seeded these keys; replace the value
            // to simulate the legacy states the plan warned about.
            sqlx::query("INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)")
                .bind(format!("agent_tool_owner.{tool}"))
                .bind(value)
                .execute(&pool)
                .await
                .unwrap();
        }

        // Every listed tool is Rust-owned regardless of the legacy setting.
        let listed = runtime.list_tools().await.unwrap();
        assert_eq!(listed.len(), 13);
        for tool in &listed {
            assert_eq!(
                tool.ownership,
                ToolOwnership::RustOwned,
                "{} must be Rust-owned",
                tool.descriptor.name
            );
        }

        // A read tool still executes.
        let read = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-legacy".to_owned(),
                step_index: 0,
                tool_name: "plan.get_today".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({"exam_id":"exam-legacy"}),
                idempotency_key: None,
                approval_id: None,
            })
            .await
            .unwrap();
        assert!(matches!(read, ToolCallResponse::Completed { .. }));

        // A write tool still runs the R3 approval path as Rust-owned and keeps
        // its receipt/undo behavior despite `record.checkin_plan=typescript`.
        let checkin_input = serde_json::json!({
            "plan_id":"plan-legacy","duration_min":30,"finish":true,
            "questions_count":5,"correct_count":4,"wrong_questions":[]
        });
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-legacy".to_owned(),
                step_index: 1,
                tool_name: "record.checkin_plan".to_owned(),
                tool_version: "1".to_owned(),
                input: checkin_input.clone(),
                idempotency_key: Some("legacy-checkin-1".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap();
        let ToolCallResponse::WaitingApproval {
            approval_id,
            step_id,
            ..
        } = response
        else {
            panic!("checkin must wait for approval");
        };
        runtime.resolve_approval(&approval_id, true).await.unwrap();
        sqlx::query("UPDATE agent_runs SET status='running', current_step=2 WHERE id='run-legacy'")
            .execute(&pool)
            .await
            .unwrap();
        let completed = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-legacy".to_owned(),
                step_index: 1,
                tool_name: "record.checkin_plan".to_owned(),
                tool_version: "1".to_owned(),
                input: checkin_input,
                idempotency_key: Some("legacy-checkin-1".to_owned()),
                approval_id: Some(approval_id),
            })
            .await
            .unwrap();
        assert!(matches!(completed, ToolCallResponse::Completed { .. }));

        let (policy_json, undo_json): (String, String) = sqlx::query_as(
            "SELECT policy_json, COALESCE(undo_json,'') FROM agent_steps WHERE id=?",
        )
        .bind(&step_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            policy_json.contains("\"delivery\":\"rust\""),
            "policy: {policy_json}"
        );
        assert!(
            undo_json.contains("record.checkin_plan.v1"),
            "undo_json: {undo_json}"
        );
        let records: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(records, 1);

        // Undo still works.
        let undone = runtime.undo_tool(&step_id).await.unwrap();
        assert_eq!(undone.step_id, step_id);
        let records: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(records, 0);

        // The legacy settings are never rewritten by the runtime.
        for (tool, value) in legacy {
            let stored: String = sqlx::query_scalar("SELECT value FROM settings WHERE key=?")
                .bind(format!("agent_tool_owner.{tool}"))
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(stored.as_str(), value, "{tool} must not be rewritten");
        }
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
            INSERT INTO agent_sessions(id,title,exam_id) VALUES('session-tool','Tool','exam-q');
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

    /// Execute a model write, approve it, then fetch the completed output.
    /// Task 10: every model-triggered write is R3 — the approval card shows a
    /// preview, and only resolve_approval performs the business write.
    /// After approve the run completes; the test resets it to `running` so
    /// further steps of the same scenario can proceed.
    async fn approve_and_output(
        runtime: &AgentRuntime,
        pool: &sqlx::SqlitePool,
        run_id: &str,
        step_index: i64,
        tool_name: &str,
        input: serde_json::Value,
        idempotency_key: Option<&str>,
    ) -> serde_json::Value {
        let request = ToolCallRequest {
            run_id: run_id.to_owned(),
            step_index,
            tool_name: tool_name.to_owned(),
            tool_version: "1".to_owned(),
            input: input.clone(),
            idempotency_key: idempotency_key.map(str::to_owned),
            approval_id: None,
        };
        let response = runtime.execute_tool(request.clone()).await.unwrap();
        let ToolCallResponse::WaitingApproval { approval_id, .. } = response else {
            panic!("R3 write must request approval");
        };
        runtime.resolve_approval(&approval_id, true).await.unwrap();
        sqlx::query("UPDATE agent_runs SET status='running', current_step=? WHERE id=?")
            .bind(step_index + 1)
            .bind(run_id)
            .execute(pool)
            .await
            .unwrap();
        let response = runtime.execute_tool(request).await.unwrap();
        let ToolCallResponse::Completed { output, .. } = response else {
            panic!("approved write must complete");
        };
        output
    }

    /// Execute a model write and reject it; nothing must be written.
    async fn reject_write(
        runtime: &AgentRuntime,
        run_id: &str,
        step_index: i64,
        tool_name: &str,
        input: serde_json::Value,
        idempotency_key: Option<&str>,
    ) {
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: run_id.to_owned(),
                step_index,
                tool_name: tool_name.to_owned(),
                tool_version: "1".to_owned(),
                input,
                idempotency_key: idempotency_key.map(str::to_owned),
                approval_id: None,
            })
            .await
            .unwrap();
        let ToolCallResponse::WaitingApproval { approval_id, .. } = response else {
            panic!("R3 write must request approval");
        };
        runtime.resolve_approval(&approval_id, false).await.unwrap();
    }

    #[tokio::test]
    async fn write_tools_are_approval_gated_and_write_once_after_resolve() {
        let (runtime, pool) = test_runtime().await;
        sqlx::raw_sql(
            r#"
            INSERT INTO exams(id,name,exam_date) VALUES('exam-w','W','2030-01-01');
            INSERT INTO subjects(id,exam_id,name) VALUES('sub-w','exam-w','数学');
            INSERT INTO agent_sessions(id,title,exam_id) VALUES('session-tool','Tool','exam-w');
            INSERT INTO agent_runs(id,session_id,goal,status)
            VALUES('run-tool','session-tool','Tool','running');
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        // record.create_free: waiting approval -> approve writes exactly once.
        let create_input = serde_json::json!({
            "exam_id":"exam-w","date":"2026-07-18","subject_id":"sub-w",
            "duration_min":45,"content":"自由复习","questions_count":6,"correct_count":5
        });
        let output = approve_and_output(
            &runtime,
            &pool,
            "run-tool",
            0,
            "record.create_free",
            create_input.clone(),
            Some("free-1"),
        )
        .await;
        let record_id = output["id"].as_str().unwrap().to_owned();
        let row_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row_count, 1);

        // Replaying the same idempotency key and identical input after
        // completion never double-writes.
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-tool".to_owned(),
                step_index: 0,
                tool_name: "record.create_free".to_owned(),
                tool_version: "1".to_owned(),
                input: create_input,
                idempotency_key: Some("free-1".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap();
        assert!(matches!(response, ToolCallResponse::Completed { .. }));
        let row_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row_count, 1);

        // wrong_question.create links to the record.
        let wq_output = approve_and_output(
            &runtime,
            &pool,
            "run-tool",
            1,
            "wrong_question.create",
            serde_json::json!({
                "subject_id":"sub-w","record_id":record_id,
                "question_desc":"错题描述","my_answer":"x"
            }),
            Some("wq-1"),
        )
        .await;
        let wrong_id = wq_output["id"].as_str().unwrap().to_owned();
        let wq_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM wrong_questions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(wq_count, 1);

        // mark_mastered flips the flag.
        approve_and_output(
            &runtime,
            &pool,
            "run-tool",
            2,
            "wrong_question.mark_mastered",
            serde_json::json!({"id": wrong_id}),
            Some("mm-1"),
        )
        .await;
        let mastered: i64 = sqlx::query_scalar("SELECT mastered FROM wrong_questions WHERE id = ?")
            .bind(&wrong_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(mastered, 1);

        // A different exam id on an exam-bound run is a scope violation before
        // any approval is created; nothing is written.
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
        assert_eq!(err.code(), "tool_scope_violation");
        let record_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(record_count, 1);
        sqlx::query("UPDATE agent_runs SET status='running', current_step=4 WHERE id='run-tool'")
            .execute(&pool)
            .await
            .unwrap();

        // Approving a write whose target is missing fails safely at dispatch:
        // the run lands in `failed`, no row is created.
        let response = runtime
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
            .unwrap();
        let ToolCallResponse::WaitingApproval { approval_id, .. } = response else {
            panic!("R3 write must request approval");
        };
        let error = runtime
            .resolve_approval(&approval_id, true)
            .await
            .unwrap_err();
        assert_eq!(error.code(), "persistence_error");
        let run_status: String =
            sqlx::query_scalar("SELECT status FROM agent_runs WHERE id='run-tool'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(run_status, "failed");
        sqlx::query("UPDATE agent_runs SET status='running', current_step=5 WHERE id='run-tool'")
            .execute(&pool)
            .await
            .unwrap();

        // Rejecting a write performs no business write at all.
        reject_write(
            &runtime,
            "run-tool",
            5,
            "record.create_free",
            serde_json::json!({
                "exam_id":"exam-w","date":"2026-07-18","subject_id":"sub-w","duration_min":10
            }),
            Some("free-3"),
        )
        .await;
        let record_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(record_count, 1);
        let run_status: String =
            sqlx::query_scalar("SELECT status FROM agent_runs WHERE id='run-tool'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(run_status, "cancelled");
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
            INSERT INTO agent_sessions(id,title,exam_id) VALUES('session-tool','Tool','exam-g');
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

        // The seven rows land on seven distinct days inside the week, with the
        // heavier subject spanning the first five days.
        let distinct_dates: Vec<String> =
            sqlx::query_scalar("SELECT DISTINCT date FROM study_plans ORDER BY date")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(distinct_dates.len(), 7);
        assert_eq!(distinct_dates.first().unwrap(), "2026-07-13");
        assert_eq!(distinct_dates.last().unwrap(), "2026-07-19");
        let math_dates: Vec<String> = sqlx::query_scalar(
            "SELECT date FROM study_plans WHERE subject_id='sub-g1' ORDER BY date",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            math_dates,
            vec![
                "2026-07-13".to_owned(),
                "2026-07-14".to_owned(),
                "2026-07-15".to_owned(),
                "2026-07-16".to_owned(),
                "2026-07-17".to_owned(),
            ]
        );
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
            "INSERT INTO agent_sessions(id,title,exam_id) VALUES('session-tool','Tool','exam-m');\
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

    #[tokio::test]
    async fn create_free_validates_knowledge_point_ownership() {
        let (runtime, pool) = test_runtime().await;
        sqlx::raw_sql(
            r#"
            INSERT INTO exams(id,name,exam_date) VALUES('exam-k','K','2030-01-01');
            INSERT INTO subjects(id,exam_id,name) VALUES('sub-k1','exam-k','数学');
            INSERT INTO subjects(id,exam_id,name) VALUES('sub-k2','exam-k','英语');
            INSERT INTO knowledge_points(id,subject_id,name) VALUES('kp-k1','sub-k1','函数');
            INSERT INTO agent_sessions(id,title,exam_id) VALUES('session-tool','Tool','exam-k');
            INSERT INTO agent_runs(id,session_id,goal,status)
            VALUES('run-tool','session-tool','Tool','running');
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        // A knowledge point of the same subject is accepted after approval.
        let output = approve_and_output(
            &runtime,
            &pool,
            "run-tool",
            0,
            "record.create_free",
            serde_json::json!({
                "exam_id":"exam-k","date":"2026-07-18","subject_id":"sub-k1",
                "knowledge_point_id":"kp-k1","duration_min":30
            }),
            Some("kf-1"),
        )
        .await;
        assert!(output["id"].as_str().is_some());
        let row_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row_count, 1);

        // A knowledge point of another subject is rejected at dispatch time
        // (approval requested, then fails on approve; run lands in failed).
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-tool".to_owned(),
                step_index: 1,
                tool_name: "record.create_free".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({
                    "exam_id":"exam-k","date":"2026-07-18","subject_id":"sub-k2",
                    "knowledge_point_id":"kp-k1","duration_min":30
                }),
                idempotency_key: Some("kf-2".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap();
        let ToolCallResponse::WaitingApproval { approval_id, .. } = response else {
            panic!("R3 write must request approval");
        };
        let err = runtime
            .resolve_approval(&approval_id, true)
            .await
            .unwrap_err();
        assert_eq!(err.code(), "persistence_error");
        sqlx::query("UPDATE agent_runs SET status='running', current_step=2 WHERE id='run-tool'")
            .execute(&pool)
            .await
            .unwrap();

        // An unknown knowledge point is also rejected at dispatch.
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-tool".to_owned(),
                step_index: 2,
                tool_name: "record.create_free".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({
                    "exam_id":"exam-k","date":"2026-07-18","subject_id":"sub-k1",
                    "knowledge_point_id":"missing-kp","duration_min":30
                }),
                idempotency_key: Some("kf-3".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap();
        let ToolCallResponse::WaitingApproval { approval_id, .. } = response else {
            panic!("R3 write must request approval");
        };
        let err = runtime
            .resolve_approval(&approval_id, true)
            .await
            .unwrap_err();
        assert_eq!(err.code(), "persistence_error");
        sqlx::query("UPDATE agent_runs SET status='running', current_step=3 WHERE id='run-tool'")
            .execute(&pool)
            .await
            .unwrap();

        // wrong_question.create also rejects a cross-subject knowledge point.
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-tool".to_owned(),
                step_index: 3,
                tool_name: "wrong_question.create".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({
                    "subject_id":"sub-k2","knowledge_point_id":"kp-k1",
                    "question_desc":"跨科目错题"
                }),
                idempotency_key: Some("kw-1".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap();
        let ToolCallResponse::WaitingApproval { approval_id, .. } = response else {
            panic!("R3 write must request approval");
        };
        let err = runtime
            .resolve_approval(&approval_id, true)
            .await
            .unwrap_err();
        assert_eq!(err.code(), "persistence_error");
    }

    #[tokio::test]
    async fn cross_exam_reads_and_writes_are_rejected_with_tool_scope_violation() {
        let (runtime, pool) = test_runtime().await;
        // Two exams with their own data; the session is bound to exam A.
        sqlx::raw_sql(
            r#"
            INSERT INTO exams(id,name,exam_date) VALUES('exam-a','A','2030-01-01');
            INSERT INTO exams(id,name,exam_date) VALUES('exam-b','B','2030-06-01');
            INSERT INTO subjects(id,exam_id,name) VALUES('sub-a','exam-a','数学');
            INSERT INTO subjects(id,exam_id,name) VALUES('sub-b','exam-b','行测');
            INSERT INTO study_plans(id,exam_id,subject_id,date,planned_tasks,status)
            VALUES('plan-b','exam-b','sub-b','2026-07-18','行测任务','pending');
            INSERT INTO study_records(id,date,subject_id,duration_min)
            VALUES('rec-b','2026-07-18','sub-b',30);
            INSERT INTO knowledge_points(id,subject_id,name) VALUES('kp-b','sub-b','行测考点');
            INSERT INTO wrong_questions(id,subject_id,question_desc,mastered,review_count)
            VALUES('wq-b','sub-b','行测错题',0,0);
            INSERT INTO agent_sessions(id,title,exam_id) VALUES('session-a','A','exam-a');
            INSERT INTO agent_runs(id,session_id,goal,status)
            VALUES('run-a','session-a','A','running');
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        let step = |index: i64| ToolCallRequest {
            run_id: "run-a".to_owned(),
            step_index: index,
            tool_name: String::new(),
            tool_version: "1".to_owned(),
            input: serde_json::json!({}),
            idempotency_key: None,
            approval_id: None,
        };

        // Read across exams: plan.get_today with exam B's id.
        let read = step(0);
        let read = ToolCallRequest {
            tool_name: "plan.get_today".to_owned(),
            input: serde_json::json!({"exam_id":"exam-b"}),
            ..read
        };
        assert_eq!(
            runtime.execute_tool(read).await.unwrap_err().code(),
            "tool_scope_violation"
        );

        // Range read across exams.
        let range = step(1);
        let range = ToolCallRequest {
            tool_name: "plan.get_range".to_owned(),
            input: serde_json::json!({"exam_id":"exam-b","start_date":"2026-07-01","end_date":"2026-07-31"}),
            ..range
        };
        assert_eq!(
            runtime.execute_tool(range).await.unwrap_err().code(),
            "tool_scope_violation"
        );

        // Write across exams: record.create_free with exam B's id and subject.
        let write = step(2);
        let write = ToolCallRequest {
            tool_name: "record.create_free".to_owned(),
            input: serde_json::json!({
                "exam_id":"exam-b","date":"2026-07-18","subject_id":"sub-b","duration_min":10
            }),
            idempotency_key: Some("free-b".to_owned()),
            ..write
        };
        assert_eq!(
            runtime.execute_tool(write).await.unwrap_err().code(),
            "tool_scope_violation"
        );

        // Write across exams: mark exam B's wrong question mastered.
        let mastered = step(3);
        let mastered = ToolCallRequest {
            tool_name: "wrong_question.mark_mastered".to_owned(),
            input: serde_json::json!({"id":"wq-b"}),
            idempotency_key: Some("mm-b".to_owned()),
            ..mastered
        };
        assert_eq!(
            runtime.execute_tool(mastered).await.unwrap_err().code(),
            "tool_scope_violation"
        );

        // No SQL mutation happened: exam B's plan/record/wrong question are
        // untouched and no exam-A rows were created.
        let plan_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_plans")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(plan_count, 1);
        let record_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(record_count, 1);
        let mastered_flag: i64 =
            sqlx::query_scalar("SELECT mastered FROM wrong_questions WHERE id='wq-b'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(mastered_flag, 0);
    }

    /// Task 5: the approval preview for `plan.apply_preview` reports the real
    /// writable draft rows via the same projection the apply uses; it never
    /// echoes the raw request, the preview step id, or any key.
    #[tokio::test]
    async fn preview_contract_reports_real_draft_rows_and_sanitized_fields() {
        let (runtime, pool) = draft_runtime().await;
        let step_id = preview_step_id(&runtime, "run-p", 0).await;
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-p".to_owned(),
                step_index: 1,
                tool_name: "plan.apply_preview".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({ "preview_step_id": step_id }),
                idempotency_key: Some("contract-1".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap();
        let ToolCallResponse::WaitingApproval { preview, .. } = response else {
            panic!("apply_preview must request approval");
        };

        // Rebuild the exact inputs the preview used and run the shared
        // projection so the preview numbers can never drift from the apply.
        let draft: crate::agent::plan_draft::PlanDraft = {
            let output_json: String = sqlx::query_scalar(
                "SELECT output_json FROM agent_steps \
                 WHERE id=? AND tool_name='plan.preview_generate'",
            )
            .bind(&step_id)
            .fetch_one(&pool)
            .await
            .unwrap();
            let output: serde_json::Value = serde_json::from_str(&output_json).unwrap();
            serde_json::from_value(output["draft"].clone()).unwrap()
        };
        let existing: Vec<crate::agent::tools::plan::ExistingPlanRow> = sqlx::query_as(
            "SELECT id, date, subject_id, actual_duration FROM study_plans WHERE exam_id=?",
        )
        .bind(&draft.exam_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        let business_date =
            crate::agent::tools::plan::business_date_at(chrono::Local::now().fixed_offset());
        let projected =
            crate::agent::tools::plan::project_apply_rows(&draft, &existing, &business_date);
        let flattened = draft
            .daily_plans
            .iter()
            .flat_map(|day| {
                day.tasks.iter().map(|task| {
                    (
                        day.date.clone(),
                        task.subject_name.clone(),
                        task.task.clone(),
                        task.duration_min,
                    )
                })
            })
            .collect::<Vec<_>>();

        assert_eq!(
            preview["affected_count"],
            serde_json::json!(projected.writable_rows.len())
        );
        assert_eq!(
            preview["fields"]["draft_row_count"],
            serde_json::json!(flattened.len())
        );
        let rows = preview["fields"]["rows"].as_array().unwrap();
        assert!(rows.iter().all(|row| {
            row.get("date").is_some()
                && row.get("subject_name").is_some()
                && row.get("planned_tasks").is_some()
                && row.get("planned_duration").is_some()
        }));
        // Task 7：知识点任务携带确定性依据。draft_runtime 种子 kp-f（函数）掌握度
        // 2、无错题；注意 fixture 科目按 id 排序（sub-e 在前），不能假设首行是数学。
        let func_row = rows
            .iter()
            .find(|row| {
                row["planned_tasks"]
                    .as_str()
                    .is_some_and(|task| task.contains("函数"))
            })
            .expect("draft 必须包含知识点“函数”的任务行");
        assert_eq!(func_row["evidence"]["mastery"], serde_json::json!(2));
        assert_eq!(
            func_row["evidence"]["reason"],
            serde_json::json!("自评掌握度较低(2/5)")
        );
        assert!(preview["fields"].get("precondition_hash").is_some());
        // The preview never exposes the preview step id, the raw request, or
        // a key.
        assert!(preview["fields"].get("preview_step_id").is_none());
        assert!(!preview.to_string().contains("preview_step_id"));
        assert!(!preview.to_string().contains("sk-"));
    }

    async fn apply_and_approve(
        runtime: &AgentRuntime,
        step_id: &str,
    ) -> crate::agent::model::ApprovalRecord {
        let response = runtime
            .execute_tool(ToolCallRequest {
                run_id: "run-p".to_owned(),
                step_index: 1,
                tool_name: "plan.apply_preview".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({ "preview_step_id": step_id }),
                idempotency_key: Some(format!("apply-{}", uuid::Uuid::new_v4())),
                approval_id: None,
            })
            .await
            .unwrap();
        let ToolCallResponse::WaitingApproval { approval_id, .. } = response else {
            panic!("apply_preview must request approval");
        };
        runtime.resolve_approval(&approval_id, true).await.unwrap()
    }

    #[tokio::test]
    async fn apply_preview_undo_restores_replaced_plans_and_marks_the_step_undone() {
        let (runtime, pool) = draft_runtime().await;
        let step_id = preview_step_id(&runtime, "run-p", 0).await;
        let approved = apply_and_approve(&runtime, &step_id).await;
        assert_eq!(approved.status, "approved");
        let plans: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_plans")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(plans, 60);

        // Undo removes the 60 draft rows and restores the replaced plan.
        let undo = runtime.undo_tool(&approved.step_id).await.unwrap();
        assert_eq!(undo.output["kind"], "plan.apply_preview.v1");
        assert_eq!(undo.output["status"], "undone");
        assert_eq!(
            undo.output["inserted_plan_ids"].as_array().unwrap().len(),
            60
        );
        assert_eq!(
            undo.output["restored_plan_ids"].as_array().unwrap().len(),
            1
        );
        let plans: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_plans")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(plans, 1);
        let row: (String, String, i64, String) = sqlx::query_as(
            "SELECT subject_id, planned_tasks, planned_duration, status \
             FROM study_plans WHERE id='plan-old'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, "sub-m");
        assert_eq!(row.1, "旧计划");
        assert_eq!(row.2, 60);
        assert_eq!(row.3, "pending");

        // A repeated undo of the same step is a conflict, not a silent no-op.
        assert_eq!(
            runtime
                .undo_tool(&approved.step_id)
                .await
                .unwrap_err()
                .code(),
            "conflict"
        );
    }

    #[tokio::test]
    async fn apply_preview_undo_reattaches_detached_records() {
        let (runtime, pool) = draft_runtime().await;
        // The replaced future plan owns a study record; the apply detaches it
        // and the undo must re-attach it.
        sqlx::query(
            "INSERT INTO study_records(id,date,subject_id,plan_id,duration_min,content) \
             VALUES('rec-1','2030-01-10','sub-m','plan-old',30,'复习')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let step_id = preview_step_id(&runtime, "run-p", 0).await;
        let approved = apply_and_approve(&runtime, &step_id).await;
        let plan_id: Option<String> =
            sqlx::query_scalar("SELECT plan_id FROM study_records WHERE id='rec-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(plan_id.is_none());

        let undo = runtime.undo_tool(&approved.step_id).await.unwrap();
        assert_eq!(undo.output["restored_record_count"], serde_json::json!(1));
        let plan_id: Option<String> =
            sqlx::query_scalar("SELECT plan_id FROM study_records WHERE id='rec-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(plan_id.as_deref(), Some("plan-old"));
    }

    #[tokio::test]
    async fn apply_preview_undo_rejects_after_external_modification() {
        let (runtime, pool) = draft_runtime().await;
        let step_id = preview_step_id(&runtime, "run-p", 0).await;
        let approved = apply_and_approve(&runtime, &step_id).await;

        // A manual edit between apply and undo changes the plan state hash.
        sqlx::query("UPDATE study_plans SET status='completed' WHERE id <> 'plan-old'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            runtime
                .undo_tool(&approved.step_id)
                .await
                .unwrap_err()
                .code(),
            "conflict"
        );
        // Nothing was rolled back; the external edit stays.
        let plans: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_plans")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(plans, 60);
        let completed: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM study_plans WHERE status='completed'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(completed, 60);
    }
}
