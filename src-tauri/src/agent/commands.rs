use serde::Serialize;
use tauri::State;

use super::error::AgentError;
use super::model::{AgentRun, AgentSession, RunEvent};
use super::runtime::AgentRuntime;

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

#[cfg(test)]
mod tests {
    use super::{trimmed_required, CommandError};
    use crate::agent::error::AgentError;

    #[test]
    fn required_values_are_trimmed_and_blank_values_are_rejected() {
        assert_eq!(
            trimmed_required("  value  ".to_owned(), "title").unwrap(),
            "value"
        );

        for field in ["title", "goal", "session_id", "run_id"] {
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
}
