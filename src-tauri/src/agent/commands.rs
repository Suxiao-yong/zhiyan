use serde::Serialize;
use serde_json::json;
use tauri::{Emitter, State};

use super::context::{ContextAudit, ContextAuditRow};
use super::error::AgentError;
use super::executor::ToolUndoResponse;
use super::memory::{MemoryRecord, MemoryRepository, MemorySource, MemoryType};
use super::model::{
    AgentMessage, AgentRun, AgentSession, ApprovalRecord, RunEvent, ToolCallRequest,
    ToolCallResponse,
};
use super::planner::{Planner, PlannerTurn};
use super::runtime::AgentRuntime;
use super::tools::ListedTool;
use crate::brief::Brief;
use crate::scheduler::{JobRecord, JobType, Scheduler};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl From<AgentError> for CommandError {
    fn from(error: AgentError) -> Self {
        let code = error.code().to_owned();
        let message = match error {
            AgentError::Persistence(_) => "agent persistence failed".to_owned(),
            AgentError::NotFound(_) => "agent record not found".to_owned(),
            other => other.to_string(),
        };
        Self { code, message }
    }
}

fn trimmed_required(value: String, field: &str) -> Result<String, CommandError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(CommandError {
            code: "validation_error".to_owned(),
            message: format!("{field} must not be blank"),
        });
    }
    Ok(value.to_owned())
}

#[tauri::command]
pub async fn agent_health(runtime: State<'_, AgentRuntime>) -> Result<(), CommandError> {
    runtime.health().await.map_err(Into::into)
}

#[tauri::command]
pub async fn agent_prepare_database_restore(
    runtime: State<'_, AgentRuntime>,
    db_instances: State<'_, tauri_plugin_sql::DbInstances>,
) -> Result<(), CommandError> {
    runtime
        .prepare_database_restore()
        .await
        .map_err(CommandError::from)?;

    let plugin_pool = {
        let mut instances = db_instances.0.write().await;
        instances.remove("sqlite:zhiyan.db")
    };
    if let Some(tauri_plugin_sql::DbPool::Sqlite(pool)) = plugin_pool {
        pool.close().await;
    }
    Ok(())
}

#[tauri::command]
pub async fn agent_create_session(
    runtime: State<'_, AgentRuntime>,
    exam_id: Option<String>,
    title: String,
) -> Result<AgentSession, CommandError> {
    let title = trimmed_required(title, "title")?;
    runtime
        .create_session(exam_id.as_deref(), &title)
        .await
        .map_err(Into::into)
}

/// Agent OS sidebar (M5): recent sessions, newest activity first.
#[tauri::command]
pub async fn agent_session_list(
    runtime: State<'_, AgentRuntime>,
    limit: Option<i64>,
) -> Result<Vec<AgentSession>, CommandError> {
    let limit = limit.unwrap_or(50).clamp(1, 500);
    runtime
        .repository()
        .session_list(limit)
        .await
        .map_err(Into::into)
}

/// Agent OS conversation (M5): a session's messages, oldest first.
#[tauri::command]
pub async fn agent_session_messages(
    runtime: State<'_, AgentRuntime>,
    session_id: String,
) -> Result<Vec<AgentMessage>, CommandError> {
    let session_id = trimmed_required(session_id, "session_id")?;
    runtime
        .repository()
        .session_messages(&session_id)
        .await
        .map_err(Into::into)
}

/// Agent OS approval card (M5): approvals, pending first then decided.
#[tauri::command]
pub async fn agent_approval_list(
    runtime: State<'_, AgentRuntime>,
    limit: Option<i64>,
) -> Result<Vec<ApprovalRecord>, CommandError> {
    let limit = limit.unwrap_or(20).clamp(1, 200);
    runtime
        .repository()
        .approval_list(limit)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn agent_create_run(
    runtime: State<'_, AgentRuntime>,
    session_id: String,
    goal: String,
) -> Result<AgentRun, CommandError> {
    let session_id = trimmed_required(session_id, "session_id")?;
    let goal = trimmed_required(goal, "goal")?;
    runtime
        .create_run(&session_id, &goal)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn agent_start_run(
    runtime: State<'_, AgentRuntime>,
    run_id: String,
) -> Result<AgentRun, CommandError> {
    let run_id = trimmed_required(run_id, "run_id")?;
    runtime
        .transition_run(&run_id, RunEvent::Start)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn agent_cancel_run(
    runtime: State<'_, AgentRuntime>,
    run_id: String,
) -> Result<AgentRun, CommandError> {
    let run_id = trimmed_required(run_id, "run_id")?;
    runtime
        .transition_run(&run_id, RunEvent::Cancel)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn agent_list_tools(
    runtime: State<'_, AgentRuntime>,
) -> Result<Vec<ListedTool>, CommandError> {
    runtime.list_tools().await.map_err(Into::into)
}

#[tauri::command]
pub async fn agent_execute_tool(
    runtime: State<'_, AgentRuntime>,
    request: ToolCallRequest,
) -> Result<ToolCallResponse, CommandError> {
    runtime.execute_tool(request).await.map_err(Into::into)
}

#[tauri::command]
pub async fn agent_decide_approval(
    runtime: State<'_, AgentRuntime>,
    approval_id: String,
    approve: bool,
) -> Result<ApprovalRecord, CommandError> {
    let approval_id = trimmed_required(approval_id, "approval_id")?;
    runtime
        .decide_approval(&approval_id, approve)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn agent_undo_tool(
    runtime: State<'_, AgentRuntime>,
    step_id: String,
) -> Result<ToolUndoResponse, CommandError> {
    let step_id = trimmed_required(step_id, "step_id")?;
    runtime.undo_tool(&step_id).await.map_err(Into::into)
}

/// Context Inspector read (M3 Part 3): every model-call audit row of a run —
/// tools offered, in-scope data categories, record IDs, field sets, token
/// usage, and the local-mode flag. Never contains raw business content.
#[tauri::command]
pub async fn agent_context_audit_list(
    audit: State<'_, ContextAudit>,
    run_id: String,
) -> Result<Vec<ContextAuditRow>, CommandError> {
    let run_id = trimmed_required(run_id, "run_id")?;
    audit.list(&run_id).await.map_err(Into::into)
}

/// Structured long-term memory management (M3 Part 3). Lists memories of an
/// exam (or all exams), newest first; `include_inactive` surfaces deactivated
/// memories. exam_id may be null; memories with exam_id IS NULL apply globally.
#[tauri::command]
pub async fn agent_memory_list(
    memory: State<'_, MemoryRepository>,
    exam_id: Option<String>,
    include_inactive: bool,
) -> Result<Vec<MemoryRecord>, CommandError> {
    let exam_id = exam_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    memory
        .list(exam_id, include_inactive)
        .await
        .map_err(Into::into)
}

/// Create a memory. Explicit user statements (source=user_statement) are
/// confirmed automatically; behavior_inferred and model_candidate memories
/// start as candidates awaiting user confirmation.
#[tauri::command]
pub async fn agent_memory_create(
    memory: State<'_, MemoryRepository>,
    exam_id: Option<String>,
    memory_type: String,
    content: String,
    source: String,
    confidence: f64,
) -> Result<MemoryRecord, CommandError> {
    let content = trimmed_required(content, "content")?;
    let memory_type = MemoryType::parse(&memory_type).ok_or_else(|| CommandError {
        code: "validation_error".to_owned(),
        message: "memory_type must be one of schedule_preference, daily_capacity, subject_preference, learning_constraint, reminder_preference, strategy_preference, confirmed_weakness".to_owned(),
    })?;
    let source = MemorySource::parse(&source).ok_or_else(|| CommandError {
        code: "validation_error".to_owned(),
        message: "source must be one of user_statement, behavior_inferred, model_candidate"
            .to_owned(),
    })?;
    if !(0.0..=1.0).contains(&confidence) {
        return Err(CommandError {
            code: "validation_error".to_owned(),
            message: "confidence must be between 0 and 1".to_owned(),
        });
    }
    let exam_id = exam_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    memory
        .create(exam_id, memory_type, &content, source, confidence)
        .await
        .map_err(Into::into)
}

/// Confirm a candidate memory.
#[tauri::command]
pub async fn agent_memory_confirm(
    memory: State<'_, MemoryRepository>,
    id: String,
) -> Result<MemoryRecord, CommandError> {
    let id = trimmed_required(id, "id")?;
    memory.confirm(&id).await.map_err(Into::into)
}

/// Edit a memory's content.
#[tauri::command]
pub async fn agent_memory_update(
    memory: State<'_, MemoryRepository>,
    id: String,
    content: String,
) -> Result<MemoryRecord, CommandError> {
    let id = trimmed_required(id, "id")?;
    let content = trimmed_required(content, "content")?;
    memory
        .update_content(&id, &content)
        .await
        .map_err(Into::into)
}

/// Deactivate a memory without deleting it.
#[tauri::command]
pub async fn agent_memory_deactivate(
    memory: State<'_, MemoryRepository>,
    id: String,
) -> Result<MemoryRecord, CommandError> {
    let id = trimmed_required(id, "id")?;
    memory.deactivate(&id).await.map_err(Into::into)
}

/// Permanently delete a memory.
#[tauri::command]
pub async fn agent_memory_delete(
    memory: State<'_, MemoryRepository>,
    id: String,
) -> Result<(), CommandError> {
    let id = trimmed_required(id, "id")?;
    memory.delete(&id).await.map_err(Into::into)
}

/// Hidden job debug reads (M4): list recent background jobs.
#[tauri::command]
pub async fn agent_job_list(
    scheduler: State<'_, Scheduler>,
    limit: Option<i64>,
) -> Result<Vec<JobRecord>, CommandError> {
    let limit = limit.unwrap_or(50).clamp(1, 500);
    scheduler.list(limit).await.map_err(Into::into)
}

/// Hidden job debug write (M4): schedule a job instance with a dedup key.
#[tauri::command]
pub async fn agent_job_schedule(
    scheduler: State<'_, Scheduler>,
    job_type: String,
    dedup_key: String,
    scheduled_at: String,
) -> Result<Option<String>, CommandError> {
    let job_type = JobType::parse(&job_type).ok_or_else(|| CommandError {
        code: "validation_error".to_owned(),
        message: "job_type must be one of daily_brief, task_reminder, overdue_check, weekly_report, retry_failed, cleanup_failed".to_owned(),
    })?;
    let dedup_key = trimmed_required(dedup_key, "dedup_key")?;
    let scheduled_at = trimmed_required(scheduled_at, "scheduled_at")?;
    scheduler
        .schedule(job_type, &dedup_key, &scheduled_at)
        .await
        .map_err(Into::into)
}

/// Hidden daily brief preview (M4): render today's brief on demand. Falls back
/// to the most recently active exam when no exam id is given.
#[tauri::command]
pub async fn agent_brief_preview(
    scheduler: State<'_, Scheduler>,
    exam_id: Option<String>,
) -> Result<Brief, CommandError> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    scheduler
        .brief_preview(exam_id.as_deref(), &today)
        .await
        .map_err(Into::into)
}

/// Hidden planner entry point (M3 Part 1/2): build the provider from settings +
/// keyring, stream the model -> tool loop over the existing AgentRuntime, emit
/// one `agent-planner-chunk` event per content delta, and return the final
/// trace + usage. Degrades to a local-mode turn when no LLM is configured.
/// Reachable only via the hidden /agent-debug contract.
#[tauri::command]
pub async fn agent_run_planner(
    planner: State<'_, Planner>,
    app: tauri::AppHandle,
    run_id: String,
    goal: String,
) -> Result<PlannerTurn, CommandError> {
    let run_id = trimmed_required(run_id, "run_id")?;
    let goal = trimmed_required(goal, "goal")?;
    let provider = planner.build_provider().await.map_err(CommandError::from)?;
    let mut on_chunk = |chunk: &str| {
        let _ = app.emit(
            "agent-planner-chunk",
            json!({ "run_id": run_id, "text": chunk }),
        );
    };
    planner
        .run(provider.as_ref(), &run_id, &goal, &mut on_chunk)
        .await
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::{
        agent_approval_list, agent_brief_preview, agent_context_audit_list, agent_decide_approval,
        agent_execute_tool, agent_job_list, agent_job_schedule, agent_list_tools,
        agent_memory_confirm, agent_memory_create, agent_memory_deactivate, agent_memory_delete,
        agent_memory_list, agent_memory_update, agent_run_planner, agent_session_list,
        agent_session_messages, agent_undo_tool, trimmed_required, CommandError,
    };
    use crate::agent::error::AgentError;

    #[test]
    fn required_values_are_trimmed_and_blank_values_are_rejected() {
        assert_eq!(
            trimmed_required("  value  ".to_owned(), "title").unwrap(),
            "value"
        );

        for field in [
            "title",
            "goal",
            "session_id",
            "run_id",
            "approval_id",
            "step_id",
        ] {
            let error = trimmed_required(" \t\n ".to_owned(), field).unwrap_err();
            assert_eq!(
                error,
                CommandError {
                    code: "validation_error".to_owned(),
                    message: format!("{field} must not be blank"),
                }
            );
        }
    }

    #[test]
    fn typed_tool_command_functions_compile() {
        let _ = agent_list_tools;
        let _ = agent_execute_tool;
        let _ = agent_decide_approval;
        let _ = agent_undo_tool;
        let _ = agent_run_planner;
        let _ = agent_context_audit_list;
        let _ = agent_memory_list;
        let _ = agent_memory_create;
        let _ = agent_memory_confirm;
        let _ = agent_memory_update;
        let _ = agent_memory_deactivate;
        let _ = agent_memory_delete;
        let _ = agent_job_list;
        let _ = agent_job_schedule;
        let _ = agent_brief_preview;
        let _ = agent_session_list;
        let _ = agent_session_messages;
        let _ = agent_approval_list;
    }

    #[test]
    fn persistence_errors_are_redacted_at_the_command_boundary() {
        let secret = "database disk image is malformed at C:\\private\\zhiyan.db";

        let error = CommandError::from(AgentError::Persistence(secret.to_owned()));

        assert_eq!(error.code, "persistence_error");
        assert_eq!(error.message, "agent persistence failed");
        assert!(!error.message.contains(secret));
    }

    #[test]
    fn defined_domain_errors_keep_safe_actionable_messages() {
        let error = CommandError::from(AgentError::InvalidTransition {
            from: "completed".to_owned(),
            event: "start".to_owned(),
        });

        assert_eq!(error.code, "invalid_transition");
        assert_eq!(
            error.message,
            "invalid transition from completed using start"
        );
    }

    #[test]
    fn idempotency_conflict_has_stable_safe_command_error() {
        let error = CommandError::from(AgentError::IdempotencyConflict);
        assert_eq!(error.code, "idempotency_conflict");
        assert_eq!(
            error.message,
            "idempotency key is already being resolved; retry"
        );
        assert!(!error.message.contains("constraint"));
    }

    #[test]
    fn provider_errors_are_redacted_and_safe() {
        let cases = [
            (
                AgentError::ProviderUnavailable,
                "provider_unavailable",
                "llm provider is unavailable",
            ),
            (
                AgentError::ProviderRequestFailed,
                "provider_request_failed",
                "llm provider request failed",
            ),
            (
                AgentError::BudgetExhausted,
                "budget_exhausted",
                "llm token budget exhausted",
            ),
            (
                AgentError::MaxIterations,
                "max_iterations",
                "planner reached the maximum tool iterations",
            ),
        ];
        for (error, code, message) in cases {
            let cmd = CommandError::from(error);
            assert_eq!(cmd.code, code);
            assert_eq!(cmd.message, message);
            // No provider secret, URL, key, or response body leaks.
            assert!(!cmd.message.contains("http"));
            assert!(!cmd.message.contains("sk-"));
        }
    }
}
