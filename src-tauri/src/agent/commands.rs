use serde::Serialize;
use serde_json::json;
use tauri::{Emitter, State};

use super::context::{ContextAudit, ContextAuditRow};
use super::error::AgentError;
use super::executor::ToolUndoResponse;
use super::model::{
    AgentRun, AgentSession, ApprovalRecord, RunEvent, ToolCallRequest, ToolCallResponse,
};
use super::planner::{Planner, PlannerTurn};
use super::runtime::AgentRuntime;
use super::tools::ListedTool;

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
        agent_context_audit_list, agent_decide_approval, agent_execute_tool, agent_list_tools,
        agent_run_planner, agent_undo_tool, trimmed_required, CommandError,
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
