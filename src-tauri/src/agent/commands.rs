use serde::Serialize;
use serde_json::json;
use sqlx::SqlitePool;
use tauri::{Emitter, State};

use super::context::{ContextAudit, ContextAuditRow};
use super::error::AgentError;
use super::executor::ToolUndoResponse;
use super::llm::LlmProvider;
use super::model::{
    AgentMessage, AgentRun, AgentSession, ApprovalRecord, RunEvent, ToolCallRequest,
    ToolCallResponse,
};
use super::planner::{Planner, PlannerTurn, TraceEntry};
use super::runtime::AgentRuntime;
use super::tools::ListedTool;
use crate::analytics::Analytics;
use crate::brief::{Brief, BriefBuilder};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

/// Status of the cloud LLM data-export consent for the *current* provider
/// configuration. `configured` is false when no cloud LLM is active; `consented`
/// is true only when the stored fingerprint matches provider/base/model/policy.
/// `fingerprint` is the expected fingerprint for the current config (identity
/// only — no key, URL, or body).
#[derive(Debug, Clone, Serialize)]
pub struct CloudConsentStatus {
    pub configured: bool,
    pub consented: bool,
    pub fingerprint: Option<String>,
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

/// Mandatory Task C: resolve an approval. Approving *executes* the approved
/// tool through the Rust executor (re-checking run scope, precondition hash,
/// and schema); rejecting only updates state. The confirm button must call
/// this, not the state-only `agent_decide_approval`.
#[tauri::command]
pub async fn agent_resolve_approval(
    runtime: State<'_, AgentRuntime>,
    approval_id: String,
    approve: bool,
) -> Result<ApprovalRecord, CommandError> {
    let approval_id = trimmed_required(approval_id, "approval_id")?;
    runtime
        .resolve_approval(&approval_id, approve)
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

/// Daily brief preview (Task 12): render today's brief on demand from local
/// Analytics only — no LLM call, no background job, no event push. Falls back
/// to the most recently active exam when no exam id is given.
#[tauri::command]
pub async fn agent_brief_preview(
    pool: State<'_, SqlitePool>,
    exam_id: Option<String>,
) -> Result<Brief, CommandError> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let exam_id = match exam_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(id) => Some(id.to_owned()),
        None => active_exam_fallback(pool.inner()).await?,
    };
    let builder = BriefBuilder::new(pool.inner().clone(), Analytics::new(pool.inner().clone()));
    builder
        .build(exam_id.as_deref(), &today)
        .await
        .map_err(Into::into)
}

/// Resolve the exam a brief targets: the persisted `agent_active_exam_id`, or
/// the most recently active exam as a fallback (mirrors the Scheduler helper).
async fn active_exam_fallback(pool: &SqlitePool) -> Result<Option<String>, CommandError> {
    let configured: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'agent_active_exam_id'")
            .fetch_optional(pool)
            .await
            .map_err(|_| CommandError {
                code: "persistence_error".to_owned(),
                message: "brief exam lookup failed".to_owned(),
            })?;
    if let Some(exam_id) = configured.filter(|value| !value.trim().is_empty()) {
        return Ok(Some(exam_id));
    }
    let latest: Option<String> =
        sqlx::query_scalar("SELECT id FROM exams ORDER BY updated_at DESC, rowid DESC LIMIT 1")
            .fetch_optional(pool)
            .await
            .map_err(|_| CommandError {
                code: "persistence_error".to_owned(),
                message: "brief exam lookup failed".to_owned(),
            })?;
    Ok(latest)
}

/// Cloud LLM data-export consent status for the current provider config.
/// Pure status read — never sends anything to the provider.
#[tauri::command]
pub async fn agent_cloud_consent_status(
    planner: State<'_, Planner>,
) -> Result<CloudConsentStatus, CommandError> {
    let fingerprint = planner
        .expected_consent_fingerprint()
        .await
        .map_err(CommandError::from)?;
    let consented = planner
        .cloud_consent_matches()
        .await
        .map_err(CommandError::from)?;
    Ok(CloudConsentStatus {
        configured: fingerprint.is_some(),
        consented,
        fingerprint,
    })
}

/// Record explicit user consent for sending business context to the *current*
/// provider/base URL/model tuple. Persists the fingerprint; any later change to
/// the config invalidates it and requires a fresh confirmation.
#[tauri::command]
pub async fn agent_confirm_cloud_consent(
    planner: State<'_, Planner>,
) -> Result<CloudConsentStatus, CommandError> {
    let fingerprint = planner
        .confirm_cloud_consent()
        .await
        .map_err(CommandError::from)?;
    Ok(CloudConsentStatus {
        configured: fingerprint.is_some(),
        consented: fingerprint.is_some(),
        fingerprint,
    })
}

/// Cloud LLM provider capability test result (Task 6). `error_code` is set
/// with a stable code when the probe failed; on success it is `None`. No key,
/// URL, request body, or response body is ever included.
pub use super::planner::ProviderTestResult;

/// Probe the configured cloud provider with fixed diagnostic traffic. The
/// command itself never fails: connection problems surface as a stable
/// `error_code` in the result so the frontend can render a precise message.
#[tauri::command]
pub async fn agent_test_provider(
    planner: State<'_, Planner>,
) -> Result<ProviderTestResult, CommandError> {
    match planner.test_provider().await {
        Ok(result) => Ok(result),
        Err(error) => Ok(provider_test_result_from_error(error)),
    }
}

/// Map a provider probe failure to a stable `error_code` result (Task 6).
fn provider_test_result_from_error(error: AgentError) -> ProviderTestResult {
    ProviderTestResult {
        model: String::new(),
        latency_ms: 0,
        text_stream: false,
        tool_call: false,
        error_code: Some(error.code().to_owned()),
    }
}

/// Hidden planner entry point (M3 Part 1/2): build the provider from settings +
/// keyring, stream the model -> tool loop over the existing AgentRuntime, emit
/// one `agent-planner-chunk` event per content delta, and return the final
/// trace + usage. Degrades to a local-mode turn when no LLM is configured.
/// Reachable only via the hidden /agent-debug contract. Any consent/provider
/// failure before or inside the loop terminates the run with a stable error
/// code so nothing is left indefinitely `running`.
#[tauri::command]
pub async fn agent_run_planner(
    planner: State<'_, Planner>,
    runtime: State<'_, AgentRuntime>,
    pool: State<'_, SqlitePool>,
    app: tauri::AppHandle,
    run_id: String,
    goal: String,
) -> Result<PlannerTurn, CommandError> {
    let run_id = trimmed_required(run_id, "run_id")?;
    let goal = trimmed_required(goal, "goal")?;
    // Rule 11 (cloud consent) + provider construction: failures must fail the
    // run atomically so the UI never sees `running` while the command errors.
    let provider =
        prepare_provider(planner.inner(), runtime.inner(), pool.inner(), &run_id).await?;
    let mut on_chunk = |chunk: &str| {
        let _ = app.emit(
            "agent-planner-chunk",
            json!({ "run_id": run_id, "text": chunk }),
        );
    };
    let result = planner
        .run(provider.as_ref(), &run_id, &goal, &mut on_chunk)
        .await;
    match result {
        Ok(turn) => {
            // §2.4.5: a text answer or a finished tool loop completes the run.
            // A run that ended waiting for approval was already moved to
            // `waiting_approval` by the executor and must stay there.
            let waiting_approval = turn
                .trace
                .iter()
                .any(|entry| matches!(entry, TraceEntry::ToolWaitingApproval { .. }));
            if !waiting_approval {
                let _ = planner.transition_run(&run_id, RunEvent::Complete).await;
            }
            Ok(turn)
        }
        Err(error) => {
            // Provider / schema / persistence failures fail the run so nothing
            // is left indefinitely `running`. The original error is returned
            // only after the run was actually terminated; a failed termination
            // surfaces as `conflict` instead of a lying success/failure pair.
            runtime
                .fail_run(&run_id, error.code())
                .await
                .map_err(CommandError::from)?;
            Err(CommandError::from(error))
        }
    }
}

/// Consent + provider construction shared by `agent_run_planner`. Every error
/// path terminates the run with its stable code via `fail_run` (atomic, only
/// from `running`/`waiting_approval`); only a successful termination lets the
/// original error reach the caller. `Ok(None)` is a legitimate local-mode turn
/// only when no cloud config exists; a configured-but-unusable cloud provider
/// is `provider_unavailable`.
async fn prepare_provider(
    planner: &Planner,
    runtime: &AgentRuntime,
    pool: &SqlitePool,
    run_id: &str,
) -> Result<Option<LlmProvider>, CommandError> {
    if let Err(error) = planner.ensure_cloud_consent().await {
        runtime
            .fail_run(run_id, error.code())
            .await
            .map_err(CommandError::from)?;
        return Err(error.into());
    }
    let has_cloud_config = match sqlx::query_scalar::<_, Option<String>>(
        "SELECT value FROM settings WHERE key='llm_provider'",
    )
    .fetch_optional(pool)
    .await
    {
        Ok(value) => value.flatten().is_some_and(|value| {
            let provider = value.trim();
            !provider.is_empty() && provider != "ollama"
        }),
        Err(error) => {
            let error = AgentError::Persistence(format!("provider settings read failed: {error}"));
            runtime
                .fail_run(run_id, error.code())
                .await
                .map_err(CommandError::from)?;
            return Err(error.into());
        }
    };
    match planner.build_provider().await {
        Ok(Some(provider)) => Ok(Some(provider)),
        Ok(None) if !has_cloud_config => Ok(None),
        Ok(None) => {
            runtime
                .fail_run(run_id, "provider_unavailable")
                .await
                .map_err(CommandError::from)?;
            Err(CommandError::from(AgentError::ProviderUnavailable))
        }
        Err(error) => {
            runtime
                .fail_run(run_id, error.code())
                .await
                .map_err(CommandError::from)?;
            Err(error.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        agent_approval_list, agent_brief_preview, agent_context_audit_list, agent_decide_approval,
        agent_execute_tool, agent_list_tools, agent_run_planner, agent_session_list,
        agent_session_messages, agent_undo_tool, provider_test_result_from_error, trimmed_required,
        CommandError,
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

    #[test]
    fn provider_test_result_maps_every_failure_to_a_stable_error_code() {
        for error in [
            AgentError::ProviderUnavailable,
            AgentError::ProviderAuthFailed,
            AgentError::ProviderRateLimited,
            AgentError::ProviderTimeout,
            AgentError::ProviderProtocolError,
            AgentError::ProviderRequestFailed,
            AgentError::ConsentRequired,
        ] {
            let code = error.code().to_owned();
            let result = provider_test_result_from_error(error);
            assert_eq!(result.error_code.as_deref(), Some(code.as_str()));
            assert!(!result.model.contains("http"));
            assert!(!result.model.contains("sk-"));
            assert!(!result.error_code.as_deref().unwrap().contains("http"));
        }
    }
}

/// Command-boundary preflight tests (Task 3): consent/provider construction
/// failures must terminate the run atomically with a stable error code, never
/// leaving the database claiming `running` while the caller gets an error.
#[cfg(test)]
mod preflight_tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::agent::executor::AgentExecutor;
    use crate::agent::repository::AgentRepository;
    use crate::agent::runtime::AgentRuntime;

    async fn preflight_runtime() -> (AgentRuntime, Planner, SqlitePool) {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        for migration in crate::db::migrations() {
            sqlx::raw_sql(migration.sql).execute(&pool).await.unwrap();
        }
        let runtime = AgentRuntime::new(
            AgentRepository::new(pool.clone()),
            AgentExecutor::new(pool.clone()),
        );
        let planner = Planner::new(pool.clone(), runtime.clone());
        (runtime, planner, pool)
    }

    /// A run already in `running`, with the session it belongs to.
    async fn started_run(pool: &SqlitePool) -> String {
        sqlx::query("INSERT INTO agent_sessions(id,title) VALUES('pre-s','pre')")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO agent_runs(id,session_id,goal,status) \
             VALUES('pre-run','pre-s','pre','running')",
        )
        .execute(pool)
        .await
        .unwrap();
        "pre-run".to_owned()
    }

    async fn run_status(pool: &SqlitePool, run_id: &str) -> String {
        sqlx::query_scalar("SELECT status FROM agent_runs WHERE id=?")
            .bind(run_id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn run_error_code(pool: &SqlitePool, run_id: &str) -> String {
        sqlx::query_scalar("SELECT error_code FROM agent_runs WHERE id=?")
            .bind(run_id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// The `CommandError` produced by a failing `prepare_provider`, panicking
    /// when the preflight unexpectedly succeeds.
    async fn prepare_error(
        planner: &Planner,
        runtime: &AgentRuntime,
        pool: &SqlitePool,
        run_id: &str,
    ) -> CommandError {
        match prepare_provider(planner, runtime, pool, run_id).await {
            Ok(_) => panic!("expected prepare_provider to fail"),
            Err(error) => error,
        }
    }

    async fn configure_cloud(pool: &SqlitePool, provider: &str) {
        sqlx::query("INSERT INTO settings(key,value) VALUES('llm_provider',?)")
            .bind(provider)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO settings(key,value) VALUES('llm_base_url',?)")
            .bind("https://example.invalid")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO settings(key,value) VALUES('llm_model',?)")
            .bind("test-model")
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn consent_failure_terminates_the_run_with_consent_required() {
        let (runtime, planner, pool) = preflight_runtime().await;
        let run_id = started_run(&pool).await;
        // A cloud config exists but the user never granted consent: the run
        // must be failed with the consent code, not left running.
        configure_cloud(&pool, "deepseek").await;
        let error = prepare_error(&planner, &runtime, &pool, &run_id).await;
        assert_eq!(error.code, "consent_required");
        assert_eq!(run_status(&pool, &run_id).await, "failed");
        assert_eq!(run_error_code(&pool, &run_id).await, "consent_required");
    }

    #[tokio::test]
    async fn no_cloud_config_degrades_to_local_mode_without_failing_the_run() {
        let (runtime, planner, pool) = preflight_runtime().await;
        let run_id = started_run(&pool).await;
        // No provider setting at all: `Ok(None)` is the legitimate local turn
        // and the run keeps running until the loop completes it.
        let provider = prepare_provider(&planner, &runtime, &pool, &run_id)
            .await
            .unwrap();
        assert!(provider.is_none());
        assert_eq!(run_status(&pool, &run_id).await, "running");
    }

    #[tokio::test]
    async fn configured_cloud_without_key_fails_as_provider_unavailable() {
        let (runtime, planner, pool) = preflight_runtime().await;
        let run_id = started_run(&pool).await;
        // A provider name unique to this process never has a keyring entry, so
        // consent passes and provider construction deterministically degrades
        // to `Ok(None)` while a cloud config is present.
        let provider_name = format!("test-nokey-{}", uuid::Uuid::new_v4());
        configure_cloud(&pool, &provider_name).await;
        let fingerprint = crate::agent::planner::cloud_consent_fingerprint(
            &provider_name,
            "https://example.invalid",
            "test-model",
        );
        sqlx::query(
            "INSERT INTO settings(key,value,description) \
             VALUES('cloud_llm_consent_fingerprint',?,'test')",
        )
        .bind(fingerprint)
        .execute(&pool)
        .await
        .unwrap();
        let error = prepare_error(&planner, &runtime, &pool, &run_id).await;
        assert_eq!(error.code, "provider_unavailable");
        assert_eq!(run_status(&pool, &run_id).await, "failed");
        assert_eq!(run_error_code(&pool, &run_id).await, "provider_unavailable");
    }

    #[tokio::test]
    async fn settings_read_failure_fails_the_run_as_persistence_error() {
        let (runtime, planner, pool) = preflight_runtime().await;
        let run_id = started_run(&pool).await;
        // The settings read (consent or provider config) fails: the run is
        // failed with the stable persistence code and the original code is
        // preserved in the database.
        sqlx::raw_sql("DROP TABLE settings")
            .execute(&pool)
            .await
            .unwrap();
        let error = prepare_error(&planner, &runtime, &pool, &run_id).await;
        assert_eq!(error.code, "persistence_error");
        assert_eq!(run_status(&pool, &run_id).await, "failed");
        assert_eq!(run_error_code(&pool, &run_id).await, "persistence_error");
    }

    #[tokio::test]
    async fn failed_termination_surfaces_conflict_and_never_keeps_running() {
        let (runtime, planner, pool) = preflight_runtime().await;
        let run_id = started_run(&pool).await;
        // The run is already terminal, so the consent failure cannot be
        // persisted: the caller must see `conflict`, not a fake success, and
        // the database must not claim the run is `running`.
        sqlx::query("UPDATE agent_runs SET status='completed' WHERE id=?")
            .bind(&run_id)
            .execute(&pool)
            .await
            .unwrap();
        configure_cloud(&pool, "deepseek").await;
        let error = prepare_error(&planner, &runtime, &pool, &run_id).await;
        assert_eq!(error.code, "conflict");
        assert_eq!(run_status(&pool, &run_id).await, "completed");
    }
}
