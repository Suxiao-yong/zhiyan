use std::fmt::Write;

use chrono::Duration as ChronoDuration;
use chrono::{Local, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use sqlx::{Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

use super::tools::plan::PlanPreviewGenerateInput;
use super::{
    error::AgentError,
    model::{ApprovalRecord, ToolCallRequest, ToolCallResponse},
    policy::{self, PolicyContext, PolicyDecision},
    tools::{
        exam,
        plan::{
            self, PlanApplyPreviewInput, PlanGenerateInput, PlanGetRangeInput, PlanGetTodayInput,
        },
        record::{
            self, RecordCheckinPlanInput, RecordCheckinPlanOutput, RecordCreateFreeInput,
            RecordGetHistoryInput,
        },
        review::{self, ReviewCompleteInput, ReviewGetDueInput},
        wrong_question::{self, WrongQuestionCreateInput, WrongQuestionMarkMasteredInput},
        Idempotency, ListedTool, RiskLevel, ToolDescriptor, ToolOwnership, ToolRegistry,
    },
};

use super::policy::ApprovalGrant;

const RECORD_CHECKIN_TOOL: &str = "record.checkin_plan";
const RECORD_CHECKIN_VERSION: &str = "1";
const RECORD_CHECKIN_UNDO_KIND: &str = "record.checkin_plan.v1";
const REVIEW_COMPLETE_UNDO_KIND: &str = "review.complete.v1";

#[derive(Debug, Clone)]
pub struct RecordCheckinExecutionRequest {
    pub run_id: String,
    pub step_index: i64,
    pub input: RecordCheckinPlanInput,
    pub business_date: String,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecordCheckinExecutionResponse {
    pub step_id: String,
    pub output: RecordCheckinPlanOutput,
    pub replayed: bool,
    pub undo_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecordCheckinUndoOutput {
    pub record_id: String,
    pub plan_id: String,
    pub removed_wrong_question_ids: Vec<String>,
    pub actual_duration: i64,
    pub actual_tasks: Option<String>,
    pub status: String,
}

/// Undo result for `plan.apply_preview`: what was removed, restored, and
/// re-attached when the draft apply was rolled back.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanApplyUndoOutput {
    pub kind: String,
    pub exam_id: String,
    pub inserted_plan_ids: Vec<String>,
    pub restored_plan_ids: Vec<String>,
    pub restored_record_count: i64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolUndoResponse {
    pub step_id: String,
    /// Serialized undo output. `kind` discriminates the receipt type:
    /// `record.checkin_plan.v1` (RecordCheckinUndoOutput) or
    /// `plan.apply_preview.v1` (PlanApplyUndoOutput).
    pub output: Value,
}

#[derive(Debug, sqlx::FromRow)]
struct StoredStep {
    id: String,
    tool_name: String,
    tool_version: String,
    status: String,
    input_json: Option<String>,
    output_json: Option<String>,
    idempotency_key: Option<String>,
    undone_at: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct UndoStep {
    id: String,
    run_id: String,
    tool_name: String,
    tool_version: String,
    status: String,
    receipt_json: Option<String>,
    undo_json: Option<String>,
    undone_at: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct StoredApproval {
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

#[derive(Debug, sqlx::FromRow)]
struct StoredRun {
    status: String,
    current_step: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct RecordCheckinUndoReceipt {
    kind: String,
    record_id: String,
    plan_id: String,
    wrong_question_ids: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct DispatchResult {
    output: Value,
    receipt: Option<Value>,
    undo: Option<Value>,
    undo_available: bool,
}

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct TestDispatcherConfig {
    dispatch_count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    invalid_output: bool,
    rollback_before_dispatch_once: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    start_idempotency_race_once: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

#[derive(Clone)]
pub(crate) enum ToolDispatcher {
    BuiltIn,
    #[cfg(test)]
    Synthetic(TestDispatcherConfig),
}

enum CoreOutcome {
    Response(ToolCallResponse),
    CommittedError(AgentError),
}

struct ReservedStep {
    id: String,
    existing_status: Option<String>,
}

#[derive(Clone)]
pub struct AgentExecutor {
    pool: SqlitePool,
    registry: ToolRegistry,
    dispatcher: ToolDispatcher,
}

impl AgentExecutor {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            registry: ToolRegistry::built_in(),
            dispatcher: ToolDispatcher::BuiltIn,
        }
    }

    #[cfg(test)]
    fn for_test(
        pool: SqlitePool,
        registry: ToolRegistry,
        dispatcher: TestDispatcherConfig,
    ) -> Self {
        Self {
            pool,
            registry,
            dispatcher: ToolDispatcher::Synthetic(dispatcher),
        }
    }

    pub async fn list_tools(&self) -> Result<Vec<ListedTool>, AgentError> {
        // Task 13: every registered tool is Rust-owned; the legacy
        // `agent_tool_owner.*` settings are no longer read.
        Ok(self
            .registry
            .descriptors()
            .into_iter()
            .map(|descriptor| ListedTool {
                descriptor: descriptor.clone(),
                ownership: ToolOwnership::RustOwned,
            })
            .collect())
    }

    pub async fn execute(&self, request: ToolCallRequest) -> Result<ToolCallResponse, AgentError> {
        let descriptor = self
            .registry
            .get(&request.tool_name, &request.tool_version)?
            .clone();
        self.registry
            .validate_input(&request.tool_name, &request.tool_version, &request.input)?;
        let normalized_input = normalize_input(&descriptor, request.input.clone())?;
        let input_json = canonical_json(normalized_input.clone()).to_string();
        let first = self
            .execute_once(&request, &descriptor, normalized_input.clone(), &input_json)
            .await;
        if first != Err(AgentError::IdempotencyConflict) || request.idempotency_key.is_none() {
            return first;
        }
        self.resolve_idempotency_race(&request, &descriptor, normalized_input, &input_json)
            .await
    }

    async fn execute_once(
        &self,
        request: &ToolCallRequest,
        descriptor: &ToolDescriptor,
        normalized_input: Value,
        input_json: &str,
    ) -> Result<ToolCallResponse, AgentError> {
        if self.dispatcher.start_idempotency_race() {
            return Err(AgentError::IdempotencyConflict);
        }
        let mut tx = self.pool.begin().await.map_err(map_sqlx)?;
        let result = self
            .execute_in_transaction(&mut tx, request, descriptor, normalized_input, input_json)
            .await;
        match result {
            Ok(CoreOutcome::Response(response)) => {
                tx.commit().await.map_err(map_sqlx)?;
                Ok(response)
            }
            Ok(CoreOutcome::CommittedError(error)) => {
                tx.commit().await.map_err(map_sqlx)?;
                Err(error)
            }
            Err(error) => {
                tx.rollback().await.map_err(map_sqlx)?;
                if should_persist_failure(&error) {
                    persist_failed_attempt(
                        &self.pool,
                        request,
                        descriptor,
                        input_json,
                        error.code(),
                    )
                    .await?;
                }
                Err(error)
            }
        }
    }

    async fn resolve_idempotency_race(
        &self,
        request: &ToolCallRequest,
        descriptor: &ToolDescriptor,
        normalized_input: Value,
        input_json: &str,
    ) -> Result<ToolCallResponse, AgentError> {
        let key = request
            .idempotency_key
            .as_deref()
            .ok_or(AgentError::IdempotencyConflict)?;
        for _ in 0..3 {
            let mut tx = match self.pool.begin().await {
                Ok(tx) => tx,
                Err(error) if is_sqlite_busy(&error) => continue,
                Err(error) => return Err(map_sqlx(error)),
            };
            let stored = match find_step_by_idempotency_key(&mut tx, key).await {
                Ok(stored) => stored,
                Err(AgentError::IdempotencyConflict) => {
                    tx.rollback().await.map_err(map_sqlx)?;
                    continue;
                }
                Err(error) => {
                    tx.rollback().await.map_err(map_sqlx)?;
                    return Err(error);
                }
            };
            tx.rollback().await.map_err(map_sqlx)?;
            match stored {
                Some(stored) if stored.status == "completed" => {
                    validate_stored_step(&stored, request, input_json)?;
                    return replay_response(&stored, descriptor.supports_undo);
                }
                Some(_) => {
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
                None => {
                    match self
                        .execute_once(request, descriptor, normalized_input.clone(), input_json)
                        .await
                    {
                        Err(AgentError::IdempotencyConflict) => {
                            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                        }
                        result => return result,
                    }
                }
            }
        }
        Err(AgentError::IdempotencyConflict)
    }

    pub async fn decide_approval(
        &self,
        approval_id: &str,
        approve: bool,
    ) -> Result<ApprovalRecord, AgentError> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx)?;
        let approval = load_approval(&mut tx, approval_id).await?;
        let expires_at = chrono::DateTime::parse_from_rfc3339(&approval.expires_at)
            .map_err(|_| AgentError::ApprovalInvalid)?
            .with_timezone(&Utc);
        if matches!(approval.status.as_str(), "pending" | "approved") && expires_at <= Utc::now() {
            let (tool_name, tool_version): (String, String) = sqlx::query_as(
                "SELECT tool_name,tool_version FROM agent_steps WHERE id=? AND run_id=?",
            )
            .bind(&approval.step_id)
            .bind(&approval.run_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_sqlx)?;
            let descriptor = self.registry.get(&tool_name, &tool_version)?.clone();
            terminalize_expired_approval(&mut tx, &approval, &descriptor).await?;
            tx.commit().await.map_err(map_sqlx)?;
            return Err(AgentError::ApprovalInvalid);
        }
        let result = decide_approval_in_transaction(&mut tx, approval_id, approve).await;
        finish_transaction(tx, result).await
    }

    /// Mandatory Task C: resolve an approval by *executing* the approved tool.
    ///
    /// - approve: reloads the step's original input, re-runs the run-scope
    ///   guard (Rule 12), the R3 precondition-hash check, and schema
    ///   validation, then dispatches the tool in the same transaction and
    ///   finalizes approval/step/run states. A stale/expired approval or a
    ///   changed precondition fails safely without any business write.
    /// - reject: only updates approval/step/run state — no business write.
    ///
    /// The frontend "confirm" button must call this command, never the
    /// state-only `agent_decide_approval`, so approving an R3 write really
    /// performs the write through the Rust executor.
    pub async fn resolve_approval(
        &self,
        approval_id: &str,
        approve: bool,
    ) -> Result<ApprovalRecord, AgentError> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx)?;
        let approval = load_approval(&mut tx, approval_id).await?;
        let expires_at = chrono::DateTime::parse_from_rfc3339(&approval.expires_at)
            .map_err(|_| AgentError::ApprovalInvalid)?
            .with_timezone(&Utc);
        if matches!(approval.status.as_str(), "pending" | "approved") && expires_at <= Utc::now() {
            let (tool_name, tool_version): (String, String) = sqlx::query_as(
                "SELECT tool_name,tool_version FROM agent_steps WHERE id=? AND run_id=?",
            )
            .bind(&approval.step_id)
            .bind(&approval.run_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_sqlx)?;
            let descriptor = self.registry.get(&tool_name, &tool_version)?.clone();
            terminalize_expired_approval(&mut tx, &approval, &descriptor).await?;
            tx.commit().await.map_err(map_sqlx)?;
            return Err(AgentError::ApprovalInvalid);
        }
        let result = if approve {
            // A dispatch-time failure (precondition/schema/persistence) must
            // roll back the attempted business write (plans/records/wrong
            // questions) while still landing the run and step in observable
            // terminal states. The approval execution runs inside a savepoint
            // so `finalize_approval_failure` below starts from the
            // pre-dispatch state (step waiting_approval, approval pending)
            // instead of committing the failed tool's writes.
            sqlx::query("SAVEPOINT approve_dispatch")
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx)?;
            let attempt = self.resolve_approval_approved(&mut tx, &approval).await;
            if attempt.is_err() {
                sqlx::query("ROLLBACK TO SAVEPOINT approve_dispatch")
                    .execute(&mut *tx)
                    .await
                    .map_err(map_sqlx)?;
            }
            sqlx::query("RELEASE SAVEPOINT approve_dispatch")
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx)?;
            attempt
        } else {
            decide_approval_in_transaction(&mut tx, approval_id, false).await
        };
        match result {
            Ok(record) => {
                tx.commit().await.map_err(map_sqlx)?;
                Ok(record)
            }
            // The failed tool never performed a committed business write: the
            // savepoint rolled its writes back, and only the terminal state
            // updates (step/run failed, approval rejected) are committed.
            Err(error) => {
                let _ = finalize_approval_failure(&mut tx, &approval, error.code()).await;
                tx.commit().await.map_err(map_sqlx)?;
                Err(error)
            }
        }
    }

    async fn resolve_approval_approved(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        approval: &StoredApproval,
    ) -> Result<ApprovalRecord, AgentError> {
        if approval.status != "pending" {
            return Err(AgentError::ApprovalInvalid);
        }
        let (tool_name, tool_version, input_json, idempotency_key): (
            String,
            String,
            Option<String>,
            Option<String>,
        ) = sqlx::query_as(
            "SELECT tool_name, tool_version, input_json, idempotency_key \
             FROM agent_steps WHERE id=? AND run_id=? AND status='waiting_approval'",
        )
        .bind(&approval.step_id)
        .bind(&approval.run_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(map_sqlx)?
        .ok_or(AgentError::ApprovalInvalid)?;
        let input: Value =
            serde_json::from_str(input_json.as_deref().ok_or(AgentError::ApprovalInvalid)?)
                .map_err(|_| AgentError::ApprovalInvalid)?;
        let descriptor = self.registry.get(&tool_name, &tool_version)?.clone();
        self.registry
            .validate_input(&tool_name, &tool_version, &input)?;

        // Rule 12: re-check the input against the run's bound exam.
        enforce_run_scope(tx, &approval.run_id, descriptor.name, &input).await?;

        // R3 precondition re-check: the state the approval was based on must
        // still match. Tools with their own precondition (plan.apply_preview's
        // plan-state hash) additionally re-check inside dispatch.
        let expected = approval_precondition_hash(approval)?;
        let current_hash = self.dispatcher.precondition_hash(tx, &input).await?;
        if current_hash != expected {
            let _ =
                terminalize_failed_approval_step(tx, approval, &descriptor, "precondition_changed")
                    .await;
            return Err(AgentError::PreconditionChanged);
        }

        // Atomic claim: only a still-waiting step may run.
        let lock = sqlx::query(
            "UPDATE agent_steps SET status='running', policy_json=? \
             WHERE id=? AND status='waiting_approval'",
        )
        .bind(policy_receipt(&descriptor, "approved_executing").to_string())
        .bind(&approval.step_id)
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx)?;
        if lock.rows_affected() != 1 {
            return Err(AgentError::Conflict);
        }

        // Execute the tool and finalize the step.
        let dispatched = self
            .dispatcher
            .dispatch(tx, &descriptor, input, &approval.step_id)
            .await?;
        self.registry
            .validate_output(&descriptor, &dispatched.output)?;
        complete_dispatched_step(
            tx,
            &approval.run_id,
            &approval.step_id,
            &descriptor,
            &dispatched,
            policy_receipt(&descriptor, "approved_executed"),
        )
        .await?;

        // Approval decided; run completes (the approved write is the last step).
        sqlx::query(
            "UPDATE agent_approvals SET status='approved', decided_at=? \
             WHERE id=? AND status='pending'",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(&approval.id)
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx)?;
        sqlx::query(
            "UPDATE agent_runs SET status='completed', completed_at=datetime('now','localtime') \
             WHERE id=? AND status='waiting_approval'",
        )
        .bind(&approval.run_id)
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx)?;

        insert_tool_event(
            tx,
            &approval.run_id,
            &approval.step_id,
            "tool.completed",
            &descriptor,
            "approved_executed",
            None,
        )
        .await?;
        let _ = idempotency_key;
        approval_record(load_approval(tx, &approval.id).await?)
    }

    async fn execute_in_transaction(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        request: &ToolCallRequest,
        descriptor: &ToolDescriptor,
        input: Value,
        input_json: &str,
    ) -> Result<CoreOutcome, AgentError> {
        // Task 13: every registered tool is Rust-owned; the legacy
        // `agent_tool_owner.*` settings are no longer read at execution time.

        let run = load_run(tx, &request.run_id).await?;
        // Rule 12: every tool call is confined to the exam bound to the run's
        // session. Inputs carrying exam/subject/knowledge-point/plan/record/
        // wrong-question references must all resolve to the bound exam; a run
        // without a bound exam may not touch business data at all. Violations
        // return the stable `tool_scope_violation` before any SQL mutation.
        enforce_run_scope(tx, &request.run_id, descriptor.name, &input).await?;
        let stored = find_existing_step(tx, request).await?;
        if let Some(stored) = stored.as_ref() {
            validate_stored_step(stored, request, input_json)?;
            if stored.status == "completed" {
                return Ok(CoreOutcome::Response(replay_response(
                    stored,
                    descriptor.supports_undo,
                )?));
            }
            if descriptor.risk == RiskLevel::R3
                && matches!(stored.status.as_str(), "cancelled" | "failed")
            {
                return Err(AgentError::ApprovalInvalid);
            }
        }
        validate_run_gate(&run, request, stored.as_ref())?;

        let reserved = if let Some(stored) = stored {
            ReservedStep {
                id: stored.id,
                existing_status: Some(stored.status),
            }
        } else {
            let step_id = reserve_step(tx, request, descriptor, input_json).await?;
            insert_tool_event(
                tx,
                &request.run_id,
                &step_id,
                "tool.requested",
                descriptor,
                "requested",
                None,
            )
            .await?;
            ReservedStep {
                id: step_id,
                existing_status: None,
            }
        };

        let user_allows_r2 = if descriptor.risk == RiskLevel::R2 {
            read_bool_setting(tx, "agent_r2_auto_execute").await?
        } else {
            false
        };
        let decision = match descriptor.risk {
            RiskLevel::R3 => {
                return self
                    .handle_r3(tx, request, descriptor, &reserved, input)
                    .await;
            }
            risk => policy::decide(PolicyContext {
                risk,
                user_allows_r2,
                approval: None,
            })?,
        };

        if decision == PolicyDecision::PresentSummary {
            sqlx::query("UPDATE agent_steps SET status='pending', policy_json=? WHERE id=?")
                .bind(policy_receipt(descriptor, "summary").to_string())
                .bind(&reserved.id)
                .execute(&mut **tx)
                .await
                .map_err(map_sqlx)?;
            return Ok(CoreOutcome::Response(ToolCallResponse::SummaryRequired {
                step_id: reserved.id,
                preview: input,
            }));
        }
        if decision == PolicyDecision::NavigateOnly {
            set_step_running(tx, &reserved.id).await?;
            complete_dispatched_step(
                tx,
                &request.run_id,
                &reserved.id,
                descriptor,
                &DispatchResult {
                    output: json!({"ok":false}),
                    receipt: None,
                    undo: None,
                    undo_available: false,
                },
                policy_receipt(descriptor, "navigation"),
            )
            .await?;
            advance_run(tx, request).await?;
            return Ok(CoreOutcome::Response(
                ToolCallResponse::NavigationRequired {
                    route: "/settings".to_owned(),
                    reason: "tool_requires_navigation".to_owned(),
                },
            ));
        }

        set_step_running(tx, &reserved.id).await?;
        self.dispatcher.rollback_before_dispatch()?;
        // Built-in dispatch is transaction-local SQLite work. Dropping the timeout future
        // cancels only statements on this transaction, which is rolled back by the caller.
        let dispatched = tokio::time::timeout(
            std::time::Duration::from_millis(descriptor.timeout_ms),
            self.dispatcher
                .dispatch(tx, descriptor, input, &reserved.id),
        )
        .await
        .map_err(|_| AgentError::ToolTimeout)??;
        self.registry
            .validate_output(descriptor, &dispatched.output)?;
        let decision_name = if decision == PolicyDecision::ExecuteWithUndo {
            "execute_with_undo"
        } else {
            "execute"
        };
        complete_dispatched_step(
            tx,
            &request.run_id,
            &reserved.id,
            descriptor,
            &dispatched,
            policy_receipt(descriptor, decision_name),
        )
        .await?;
        advance_run(tx, request).await?;
        Ok(CoreOutcome::Response(ToolCallResponse::Completed {
            step_id: reserved.id,
            output: dispatched.output,
            replayed: false,
            undo_available: dispatched.undo_available,
        }))
    }

    async fn handle_r3(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        request: &ToolCallRequest,
        descriptor: &ToolDescriptor,
        reserved: &ReservedStep,
        input: Value,
    ) -> Result<CoreOutcome, AgentError> {
        if reserved.existing_status.as_deref() != Some("waiting_approval") {
            if request.approval_id.is_some() {
                return Err(AgentError::ApprovalInvalid);
            }
            let current_hash = self.dispatcher.precondition_hash(tx, &input).await?;
            let preview = build_approval_preview(tx, descriptor.name, &input).await?;
            let approval = create_pending_approval(
                tx,
                request,
                descriptor,
                &reserved.id,
                &current_hash,
                preview,
            )
            .await?;
            return Ok(CoreOutcome::Response(waiting_response(&approval)?));
        }

        let approval = load_approval_for_step(tx, &reserved.id).await?;
        let expires_at = chrono::DateTime::parse_from_rfc3339(&approval.expires_at)
            .map_err(|_| AgentError::ApprovalInvalid)?
            .with_timezone(&Utc);
        if expires_at <= Utc::now() {
            terminalize_expired_approval(tx, &approval, descriptor).await?;
            return Ok(CoreOutcome::CommittedError(AgentError::ApprovalInvalid));
        }
        let Some(requested_approval_id) = request.approval_id.as_deref() else {
            if approval.status == "pending" {
                return Ok(CoreOutcome::Response(waiting_response(&approval)?));
            }
            return Err(AgentError::ApprovalInvalid);
        };
        if requested_approval_id != approval.id || approval.run_id != request.run_id {
            return Err(AgentError::ApprovalInvalid);
        }

        let current_hash = self.dispatcher.precondition_hash(tx, &input).await?;
        let stored_hash = approval_precondition_hash(&approval)?;
        let decision = policy::decide(PolicyContext {
            risk: descriptor.risk,
            user_allows_r2: false,
            approval: Some(ApprovalGrant {
                approval_id: &approval.id,
                step_id: &approval.step_id,
                expected_step_id: &reserved.id,
                status: &approval.status,
                expires_at,
                now: Utc::now(),
                precondition_hash: &stored_hash,
                current_precondition_hash: &current_hash,
            }),
        });
        if !matches!(decision, Ok(PolicyDecision::Execute)) {
            if approval.status == "approved" && stored_hash != current_hash {
                terminalize_failed_approval_step(tx, &approval, descriptor, "stale_precondition")
                    .await?;
                return Ok(CoreOutcome::CommittedError(AgentError::ApprovalInvalid));
            }
            return Err(AgentError::ApprovalInvalid);
        }

        set_step_running(tx, &reserved.id).await?;
        let dispatched = tokio::time::timeout(
            std::time::Duration::from_millis(descriptor.timeout_ms),
            self.dispatcher
                .dispatch(tx, descriptor, input, &reserved.id),
        )
        .await
        .map_err(|_| AgentError::ToolTimeout)??;
        self.registry
            .validate_output(descriptor, &dispatched.output)?;
        complete_dispatched_step(
            tx,
            &request.run_id,
            &reserved.id,
            descriptor,
            &dispatched,
            policy_receipt(descriptor, "execute"),
        )
        .await?;
        advance_run(tx, request).await?;
        Ok(CoreOutcome::Response(ToolCallResponse::Completed {
            step_id: reserved.id.clone(),
            output: dispatched.output,
            replayed: false,
            undo_available: dispatched.undo_available,
        }))
    }

    pub async fn execute_record_checkin_plan(
        &self,
        request: RecordCheckinExecutionRequest,
    ) -> Result<RecordCheckinExecutionResponse, AgentError> {
        let key = request
            .idempotency_key
            .as_deref()
            .filter(|key| !key.trim().is_empty())
            .ok_or(AgentError::IdempotencyRequired)?
            .to_owned();
        let normalized =
            serde_json::to_value(&request.input).map_err(|_| AgentError::ToolSchemaInvalid)?;
        let input_json = canonical_json(normalized.clone()).to_string();
        // Same-key race on WAL: the loser of the step-reservation INSERT sees a
        // unique violation (busy or reserved-by-other) and must retry a bounded
        // number of times to replay the winner's completed step instead of
        // surfacing a spurious conflict (mirrors the generic execute() path).
        let mut attempt = 0_u32;
        loop {
            let mut tx = self.pool.begin().await.map_err(map_sqlx)?;
            let result = async {
                execute_checkin_in_transaction(&mut tx, &request, &key, &input_json, false).await
            }
            .await;
            match finish_transaction(tx, result).await {
                Err(AgentError::IdempotencyConflict) if attempt < 2 => {
                    attempt += 1;
                    continue;
                }
                other => return other,
            }
        }
    }

    pub async fn undo(&self, step_id: &str) -> Result<ToolUndoResponse, AgentError> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx)?;
        let result = async { undo_in_transaction(&mut tx, step_id).await }.await;
        finish_transaction(tx, result).await
    }
}

impl ToolDispatcher {
    fn start_idempotency_race(&self) -> bool {
        #[cfg(test)]
        if let Self::Synthetic(config) = self {
            use std::sync::atomic::Ordering;
            return config
                .start_idempotency_race_once
                .as_ref()
                .is_some_and(|flag| flag.swap(false, Ordering::SeqCst));
        }
        false
    }

    fn rollback_before_dispatch(&self) -> Result<(), AgentError> {
        #[cfg(test)]
        if let Self::Synthetic(config) = self {
            use std::sync::atomic::Ordering;
            if config
                .rollback_before_dispatch_once
                .as_ref()
                .is_some_and(|flag| flag.swap(false, Ordering::SeqCst))
            {
                return Err(AgentError::IdempotencyConflict);
            }
        }
        Ok(())
    }

    async fn precondition_hash(
        &self,
        _tx: &mut Transaction<'_, Sqlite>,
        input: &Value,
    ) -> Result<String, AgentError> {
        #[cfg(test)]
        if let Self::Synthetic(_) = self {
            let source_id = input["source_id"]
                .as_str()
                .ok_or(AgentError::ToolSchemaInvalid)?;
            let source_value: String =
                sqlx::query_scalar("SELECT value FROM synthetic_sources WHERE id=?")
                    .bind(source_id)
                    .fetch_optional(&mut **_tx)
                    .await
                    .map_err(map_sqlx)?
                    .ok_or_else(|| AgentError::NotFound(source_id.to_owned()))?;
            return Ok(hash_value(&json!({"id":source_id,"value":source_value})));
        }
        Ok(hash_value(input))
    }

    pub(crate) async fn dispatch(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        descriptor: &ToolDescriptor,
        input: Value,
        step_id: &str,
    ) -> Result<DispatchResult, AgentError> {
        match self {
            Self::BuiltIn => match descriptor.name {
                "plan.get_today" => {
                    let input: PlanGetTodayInput =
                        serde_json::from_value(input).map_err(|_| AgentError::ToolSchemaInvalid)?;
                    let business_date = plan::business_date_at(Local::now().fixed_offset());
                    let output = plan::get_today(&mut **tx, input, &business_date).await?;
                    Ok(DispatchResult {
                        output: serde_json::to_value(output)
                            .map_err(|_| AgentError::ToolSchemaInvalid)?,
                        receipt: Some(json!({"delivery":"rust"})),
                        undo: None,
                        undo_available: false,
                    })
                }
                "plan.get_range" => {
                    let input: PlanGetRangeInput =
                        serde_json::from_value(input).map_err(|_| AgentError::ToolSchemaInvalid)?;
                    let output = plan::get_range(&mut **tx, input).await?;
                    Ok(DispatchResult {
                        output: serde_json::to_value(output)
                            .map_err(|_| AgentError::ToolSchemaInvalid)?,
                        receipt: Some(json!({"delivery":"rust"})),
                        undo: None,
                        undo_available: false,
                    })
                }
                "plan.generate" => {
                    let input: PlanGenerateInput =
                        serde_json::from_value(input).map_err(|_| AgentError::ToolSchemaInvalid)?;
                    let output = plan::generate(tx, input).await?;
                    Ok(DispatchResult {
                        output: serde_json::to_value(output)
                            .map_err(|_| AgentError::ToolSchemaInvalid)?,
                        receipt: Some(json!({"delivery":"rust"})),
                        undo: None,
                        undo_available: false,
                    })
                }
                "plan.preview_generate" => {
                    let input: PlanPreviewGenerateInput =
                        serde_json::from_value(input).map_err(|_| AgentError::ToolSchemaInvalid)?;
                    let output = plan::preview_generate(tx, input, step_id).await?;
                    Ok(DispatchResult {
                        output: serde_json::to_value(output)
                            .map_err(|_| AgentError::ToolSchemaInvalid)?,
                        receipt: Some(json!({"delivery":"rust"})),
                        undo: None,
                        undo_available: false,
                    })
                }
                "plan.apply_preview" => {
                    let input: PlanApplyPreviewInput =
                        serde_json::from_value(input).map_err(|_| AgentError::ToolSchemaInvalid)?;
                    let (output, undo) = plan::apply_preview(tx, input).await?;
                    Ok(DispatchResult {
                        output: serde_json::to_value(output)
                            .map_err(|_| AgentError::ToolSchemaInvalid)?,
                        receipt: Some(json!({"delivery":"rust"})),
                        undo: Some(
                            serde_json::to_value(&undo)
                                .map_err(|_| AgentError::ToolSchemaInvalid)?,
                        ),
                        undo_available: true,
                    })
                }
                "record.get_history" => {
                    let input: RecordGetHistoryInput =
                        serde_json::from_value(input).map_err(|_| AgentError::ToolSchemaInvalid)?;
                    let output = record::get_history(&mut **tx, input).await?;
                    Ok(DispatchResult {
                        output: serde_json::to_value(output)
                            .map_err(|_| AgentError::ToolSchemaInvalid)?,
                        receipt: Some(json!({"delivery":"rust"})),
                        undo: None,
                        undo_available: false,
                    })
                }
                "record.create_free" => {
                    let input: RecordCreateFreeInput =
                        serde_json::from_value(input).map_err(|_| AgentError::ToolSchemaInvalid)?;
                    let output = record::create_free(tx, input).await?;
                    Ok(DispatchResult {
                        output: serde_json::to_value(output)
                            .map_err(|_| AgentError::ToolSchemaInvalid)?,
                        receipt: Some(json!({"delivery":"rust"})),
                        undo: None,
                        undo_available: false,
                    })
                }
                "wrong_question.create" => {
                    let input: WrongQuestionCreateInput =
                        serde_json::from_value(input).map_err(|_| AgentError::ToolSchemaInvalid)?;
                    let output = wrong_question::create(tx, input).await?;
                    Ok(DispatchResult {
                        output: serde_json::to_value(output)
                            .map_err(|_| AgentError::ToolSchemaInvalid)?,
                        receipt: Some(json!({"delivery":"rust"})),
                        undo: None,
                        undo_available: false,
                    })
                }
                "wrong_question.mark_mastered" => {
                    let input: WrongQuestionMarkMasteredInput =
                        serde_json::from_value(input).map_err(|_| AgentError::ToolSchemaInvalid)?;
                    let output = wrong_question::mark_mastered(tx, input).await?;
                    Ok(DispatchResult {
                        output: serde_json::to_value(output)
                            .map_err(|_| AgentError::ToolSchemaInvalid)?,
                        receipt: Some(json!({"delivery":"rust"})),
                        undo: None,
                        undo_available: false,
                    })
                }
                "exam.get_active" => {
                    let _: Value =
                        serde_json::from_value(input).map_err(|_| AgentError::ToolSchemaInvalid)?;
                    let output = exam::get_active(tx).await?;
                    Ok(DispatchResult {
                        output: serde_json::to_value(output)
                            .map_err(|_| AgentError::ToolSchemaInvalid)?,
                        receipt: Some(json!({"delivery":"rust"})),
                        undo: None,
                        undo_available: false,
                    })
                }
                "review.get_due" => {
                    let input: ReviewGetDueInput =
                        serde_json::from_value(input).map_err(|_| AgentError::ToolSchemaInvalid)?;
                    let output = review::get_due(tx, input).await?;
                    Ok(DispatchResult {
                        output: serde_json::to_value(output)
                            .map_err(|_| AgentError::ToolSchemaInvalid)?,
                        receipt: Some(json!({"delivery":"rust"})),
                        undo: None,
                        undo_available: false,
                    })
                }
                "review.complete" => {
                    let input: ReviewCompleteInput =
                        serde_json::from_value(input).map_err(|_| AgentError::ToolSchemaInvalid)?;
                    let prior: Option<(i64, f64, f64, Option<String>)> = sqlx::query_as(
                        "SELECT review_count, ease_factor, review_interval_days, next_review_at \
                         FROM wrong_questions WHERE id = ?",
                    )
                    .bind(&input.wrong_question_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(map_sqlx)?;
                    let output = review::complete(tx, input.clone()).await?;
                    // The tool already rejected missing/mastered rows; the
                    // snapshot below is only taken for undo bookkeeping.
                    let (review_count, ease_factor, review_interval_days, next_review_at) =
                        prior.ok_or_else(|| {
                            AgentError::Persistence(
                                "wrong question not found or mastered".to_owned(),
                            )
                        })?;
                    Ok(DispatchResult {
                        output: serde_json::to_value(output)
                            .map_err(|_| AgentError::ToolSchemaInvalid)?,
                        receipt: Some(json!({"delivery":"rust"})),
                        undo: Some(json!({
                            "kind": REVIEW_COMPLETE_UNDO_KIND,
                            "wrong_question_id": input.wrong_question_id,
                            "review_count": review_count,
                            "ease_factor": ease_factor,
                            "review_interval_days": review_interval_days,
                            "next_review_at": next_review_at,
                        })),
                        undo_available: true,
                    })
                }
                RECORD_CHECKIN_TOOL => {
                    let input: RecordCheckinPlanInput =
                        serde_json::from_value(input).map_err(|_| AgentError::ToolSchemaInvalid)?;
                    dispatch_record_checkin_business(
                        tx,
                        step_id,
                        input,
                        &plan::business_date_at(Local::now().fixed_offset()),
                    )
                    .await
                }
                _ => Err(AgentError::ToolNotFound),
            },
            #[cfg(test)]
            Self::Synthetic(config) => {
                use std::sync::atomic::Ordering;
                config.dispatch_count.fetch_add(1, Ordering::SeqCst);
                let source_id = input["source_id"]
                    .as_str()
                    .ok_or(AgentError::ToolSchemaInvalid)?;
                sqlx::query("INSERT INTO synthetic_business(source_id) VALUES(?)")
                    .bind(source_id)
                    .execute(&mut **tx)
                    .await
                    .map_err(map_sqlx)?;
                Ok(DispatchResult {
                    output: if config.invalid_output {
                        json!({"invalid":true})
                    } else {
                        json!({"ok":true})
                    },
                    receipt: None,
                    undo: None,
                    undo_available: false,
                })
            }
        }
    }
}

pub(crate) async fn dispatch_record_checkin_business(
    tx: &mut Transaction<'_, Sqlite>,
    step_id: &str,
    input: RecordCheckinPlanInput,
    business_date: &str,
) -> Result<DispatchResult, AgentError> {
    let baseline_completed: bool = sqlx::query_scalar(
        r#"
        SELECT p.status = 'completed' AND COALESCE((
            SELECT MAX(CASE
                WHEN json_extract(prior.receipt_json,'$.compensation.baseline_completed') = 1
                    THEN 1 ELSE 0 END)
            FROM agent_steps AS prior
            JOIN study_records AS prior_record
              ON prior_record.id = json_extract(prior.undo_json,'$.record_id')
             AND prior_record.plan_id = p.id
            WHERE prior.id <> ?
              AND prior.tool_name = ? AND prior.tool_version = ?
              AND prior.status = 'completed' AND prior.undone_at IS NULL
              AND json_extract(prior.undo_json,'$.kind') = ?
              AND json_extract(prior.undo_json,'$.plan_id') = p.id
              AND json_extract(prior.receipt_json,'$.compensation.finish') = 1
        ),1)
        FROM study_plans AS p WHERE p.id=?
        "#,
    )
    .bind(step_id)
    .bind(RECORD_CHECKIN_TOOL)
    .bind(RECORD_CHECKIN_VERSION)
    .bind(RECORD_CHECKIN_UNDO_KIND)
    .bind(&input.plan_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_sqlx)?
    .unwrap_or(false);
    let finish = input.finish;
    let record_id = Uuid::new_v4().to_string();
    let output = record::checkin_plan(tx, input, business_date, &record_id).await?;
    let undo = RecordCheckinUndoReceipt {
        kind: RECORD_CHECKIN_UNDO_KIND.to_owned(),
        record_id: output.record_id.clone(),
        plan_id: output.plan_id.clone(),
        wrong_question_ids: output.wrong_question_ids.clone(),
    };
    Ok(DispatchResult {
        output: serde_json::to_value(output).map_err(|_| AgentError::ToolSchemaInvalid)?,
        receipt: Some(json!({
            "compensation": {
                "finish": finish,
                "baseline_completed": baseline_completed
            },
            "undo_result": null
        })),
        undo: Some(serde_json::to_value(undo).map_err(|_| AgentError::ToolSchemaInvalid)?),
        undo_available: true,
    })
}

pub(crate) async fn execute_checkin_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    request: &RecordCheckinExecutionRequest,
    idempotency_key: &str,
    input_json: &str,
    emit_requested: bool,
) -> Result<RecordCheckinExecutionResponse, AgentError> {
    let stored = sqlx::query_as::<_, StoredStep>(
        r#"
        SELECT id, tool_name, tool_version, status, input_json, output_json, idempotency_key, undone_at
        FROM agent_steps WHERE idempotency_key = ?
        "#,
    )
    .bind(idempotency_key)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    if let Some(stored) = stored {
        if stored.tool_name != RECORD_CHECKIN_TOOL
            || stored.tool_version != RECORD_CHECKIN_VERSION
            || stored.input_json.as_deref() != Some(input_json)
            || stored.status != "completed"
        {
            return Err(AgentError::IdempotencyConflict);
        }
        let output = serde_json::from_str(
            stored
                .output_json
                .as_deref()
                .ok_or(AgentError::IdempotencyConflict)?,
        )
        .map_err(|_| AgentError::IdempotencyConflict)?;
        return Ok(RecordCheckinExecutionResponse {
            step_id: stored.id,
            output,
            replayed: true,
            undo_available: stored.undone_at.is_none(),
        });
    }

    let step_id = Uuid::new_v4().to_string();
    sqlx::query(
        r#"
        INSERT INTO agent_steps (
            id, run_id, step_index, tool_name, tool_version, risk, status,
            input_json, idempotency_key, started_at
        ) VALUES (?, ?, ?, ?, ?, 1, 'running', ?, ?, datetime('now','localtime'))
        "#,
    )
    .bind(&step_id)
    .bind(&request.run_id)
    .bind(request.step_index)
    .bind(RECORD_CHECKIN_TOOL)
    .bind(RECORD_CHECKIN_VERSION)
    .bind(input_json)
    .bind(idempotency_key)
    .execute(&mut **tx)
    .await
    .map_err(map_reservation_error)?;

    if emit_requested {
        insert_tool_event(
            tx,
            &request.run_id,
            &step_id,
            "tool.requested",
            &record::descriptor(),
            "requested",
            None,
        )
        .await?;
    }

    let decision = policy::decide(PolicyContext {
        risk: RiskLevel::R1,
        user_allows_r2: false,
        approval: None,
    })?;
    if decision != PolicyDecision::ExecuteWithUndo {
        return Err(AgentError::Conflict);
    }

    let dispatched = dispatch_record_checkin_business(
        tx,
        &step_id,
        request.input.clone(),
        &request.business_date,
    )
    .await?;
    let descriptor = record::descriptor();
    ToolRegistry::built_in().validate_output(&descriptor, &dispatched.output)?;
    complete_dispatched_step(
        tx,
        &request.run_id,
        &step_id,
        &descriptor,
        &dispatched,
        policy_receipt(&descriptor, "execute_with_undo"),
    )
    .await?;
    let output: RecordCheckinPlanOutput =
        serde_json::from_value(dispatched.output).map_err(|_| AgentError::ToolSchemaInvalid)?;

    Ok(RecordCheckinExecutionResponse {
        step_id,
        output,
        replayed: false,
        undo_available: true,
    })
}

pub(crate) async fn undo_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    step_id: &str,
) -> Result<ToolUndoResponse, AgentError> {
    let step = sqlx::query_as::<_, UndoStep>(
        r#"
        SELECT id, run_id, tool_name, tool_version, status, receipt_json, undo_json, undone_at
        FROM agent_steps WHERE id = ?
        "#,
    )
    .bind(step_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_sqlx)?
    .ok_or_else(|| AgentError::NotFound(step_id.to_owned()))?;

    if step.status != "completed" {
        return Err(AgentError::ToolSchemaInvalid);
    }

    // Undo is per-tool: each write tool owns its receipt shape and restore
    // logic. Adding a tool means adding a branch here plus its helper.
    match (step.tool_name.as_str(), step.tool_version.as_str()) {
        (RECORD_CHECKIN_TOOL, RECORD_CHECKIN_VERSION) => {
            undo_checkin_in_transaction(tx, &step).await
        }
        ("plan.apply_preview", "1") => undo_apply_preview_in_transaction(tx, &step).await,
        ("review.complete", "1") => undo_review_complete_in_transaction(tx, &step).await,
        _ => Err(AgentError::ToolSchemaInvalid),
    }
}

/// Undo for `review.complete`: restore the four scheduling columns captured
/// before the review. A repeated undo is a conflict; a question that was
/// re-mastered or deleted after the review refuses to roll back silently.
async fn undo_review_complete_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    step: &UndoStep,
) -> Result<ToolUndoResponse, AgentError> {
    if step.undone_at.is_some() {
        return Err(AgentError::Conflict);
    }
    let undo: Value = serde_json::from_str(
        step.undo_json
            .as_deref()
            .ok_or(AgentError::ToolSchemaInvalid)?,
    )
    .map_err(|_| AgentError::ToolSchemaInvalid)?;
    if undo["kind"].as_str() != Some(REVIEW_COMPLETE_UNDO_KIND) {
        return Err(AgentError::ToolSchemaInvalid);
    }
    let wrong_question_id = undo["wrong_question_id"]
        .as_str()
        .ok_or(AgentError::ToolSchemaInvalid)?;
    // 收据字段缺失时 fail-closed：拒绝撤销而不是把调度列静默清零。
    let review_count = undo["review_count"]
        .as_i64()
        .ok_or(AgentError::ToolSchemaInvalid)?;
    let ease_factor = undo["ease_factor"]
        .as_f64()
        .ok_or(AgentError::ToolSchemaInvalid)?;
    let review_interval_days = undo["review_interval_days"]
        .as_f64()
        .ok_or(AgentError::ToolSchemaInvalid)?;
    let updated = sqlx::query(
        r#"
        UPDATE wrong_questions
        SET review_count = ?,
            ease_factor = ?,
            review_interval_days = ?,
            next_review_at = ?
        WHERE id = ? AND mastered = 0
        "#,
    )
    .bind(review_count)
    .bind(ease_factor)
    .bind(review_interval_days)
    .bind(undo["next_review_at"].as_str())
    .bind(wrong_question_id)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    if updated.rows_affected() != 1 {
        return Err(AgentError::Conflict);
    }

    let updated_step = sqlx::query(
        "UPDATE agent_steps SET undone_at=datetime('now','localtime') \
         WHERE id=? AND undone_at IS NULL",
    )
    .bind(&step.id)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    if updated_step.rows_affected() != 1 {
        return Err(AgentError::IdempotencyConflict);
    }
    sqlx::query(
        r#"
        INSERT INTO agent_events(run_id, step_id, event_type, payload_json)
        VALUES(?, ?, 'tool.undone', ?)
        "#,
    )
    .bind(&step.run_id)
    .bind(&step.id)
    .bind(
        json!({
            "step_id": step.id,
            "tool_name": "review.complete",
            "tool_version": "1",
            "result": "undone"
        })
        .to_string(),
    )
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    Ok(ToolUndoResponse {
        step_id: step.id.clone(),
        output: undo,
    })
}

/// Undo for `record.checkin_plan`: remove the written record (and its wrong
/// questions), then recompute the plan's actuals. Refuses when the record was
/// re-attached to another plan or the plan changed underneath (conflict).
async fn undo_checkin_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    step: &UndoStep,
) -> Result<ToolUndoResponse, AgentError> {
    if !record::descriptor().supports_undo {
        return Err(AgentError::ToolSchemaInvalid);
    }
    if step.undone_at.is_some() {
        return stored_undo_response(step);
    }

    let undo: RecordCheckinUndoReceipt = serde_json::from_str(
        step.undo_json
            .as_deref()
            .ok_or(AgentError::ToolSchemaInvalid)?,
    )
    .map_err(|_| AgentError::ToolSchemaInvalid)?;
    if undo.kind != RECORD_CHECKIN_UNDO_KIND {
        return Err(AgentError::ToolSchemaInvalid);
    }
    let mut receipt: Value = step
        .receipt_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|_| AgentError::ToolSchemaInvalid)?
        .unwrap_or_else(|| json!({}));
    let baseline_completed = receipt
        .pointer("/compensation/baseline_completed")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let target_plan_id: Option<Option<String>> =
        sqlx::query_scalar("SELECT plan_id FROM study_records WHERE id = ?")
            .bind(&undo.record_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(map_sqlx)?;
    let target_exists = match target_plan_id {
        Some(Some(plan_id)) if plan_id == undo.plan_id => true,
        Some(_) => return Err(AgentError::Conflict),
        None => false,
    };

    for wrong_id in &undo.wrong_question_ids {
        let wrong_record_id: Option<Option<String>> =
            sqlx::query_scalar("SELECT record_id FROM wrong_questions WHERE id = ?")
                .bind(wrong_id)
                .fetch_optional(&mut **tx)
                .await
                .map_err(map_sqlx)?;
        match wrong_record_id {
            Some(Some(record_id)) if record_id == undo.record_id => {}
            Some(None) => {}
            _ => return Err(AgentError::Conflict),
        }
    }

    for wrong_id in &undo.wrong_question_ids {
        let deleted = sqlx::query("DELETE FROM wrong_questions WHERE id = ?")
            .bind(wrong_id)
            .execute(&mut **tx)
            .await
            .map_err(map_sqlx)?;
        if deleted.rows_affected() != 1 {
            return Err(AgentError::Conflict);
        }
    }
    let deleted_record = sqlx::query("DELETE FROM study_records WHERE id = ? AND plan_id = ?")
        .bind(&undo.record_id)
        .bind(&undo.plan_id)
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx)?;
    if deleted_record.rows_affected() != u64::from(target_exists) {
        return Err(AgentError::Conflict);
    }

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records WHERE plan_id = ?")
        .bind(&undo.plan_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(map_sqlx)?;
    let other_finish: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM agent_steps AS finish_step
            JOIN study_records AS finish_record
              ON finish_record.id = json_extract(finish_step.undo_json, '$.record_id')
             AND finish_record.plan_id = json_extract(finish_step.undo_json, '$.plan_id')
            WHERE finish_step.id <> ?
              AND finish_step.tool_name = ?
              AND finish_step.tool_version = ?
              AND finish_step.status = 'completed'
              AND finish_step.undone_at IS NULL
              AND json_extract(finish_step.undo_json, '$.kind') = ?
              AND json_extract(finish_step.undo_json, '$.plan_id') = ?
              AND json_extract(finish_step.receipt_json, '$.compensation.finish') = 1
        )
        "#,
    )
    .bind(&step.id)
    .bind(RECORD_CHECKIN_TOOL)
    .bind(RECORD_CHECKIN_VERSION)
    .bind(RECORD_CHECKIN_UNDO_KIND)
    .bind(&undo.plan_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    let updated_plan = if remaining == 0 {
        sqlx::query(
            r#"
            UPDATE study_plans
            SET actual_duration=0,
                actual_tasks=planned_tasks,
                status=CASE WHEN status='skipped' THEN 'skipped' ELSE 'pending' END
            WHERE id=?
            "#,
        )
        .bind(&undo.plan_id)
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx)?
    } else {
        sqlx::query(
            r#"
            UPDATE study_plans
            SET actual_duration = COALESCE((
                  SELECT SUM(duration_min) FROM study_records WHERE plan_id = study_plans.id
                ), 0),
                actual_tasks = COALESCE((
                  SELECT content FROM study_records
                  WHERE plan_id = study_plans.id AND content IS NOT NULL AND content <> ''
                  ORDER BY created_at DESC, id DESC LIMIT 1
                ), planned_tasks),
                status = CASE
                    WHEN status='skipped' THEN 'skipped'
                    WHEN ? = 1 OR ? = 1 THEN 'completed'
                    ELSE 'in_progress'
                END
            WHERE id=?
            "#,
        )
        .bind(other_finish)
        .bind(baseline_completed)
        .bind(&undo.plan_id)
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx)?
    };
    if updated_plan.rows_affected() != 1 {
        return Err(AgentError::Conflict);
    }

    let (actual_duration, actual_tasks, status): (i64, Option<String>, String) = sqlx::query_as(
        "SELECT actual_duration, actual_tasks, status FROM study_plans WHERE id = ?",
    )
    .bind(&undo.plan_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    let output = RecordCheckinUndoOutput {
        record_id: undo.record_id,
        plan_id: undo.plan_id,
        removed_wrong_question_ids: undo.wrong_question_ids,
        actual_duration,
        actual_tasks,
        status,
    };
    let response = ToolUndoResponse {
        step_id: step.id.clone(),
        output: serde_json::to_value(&output).map_err(|_| AgentError::ToolSchemaInvalid)?,
    };
    receipt["undo_result"] =
        serde_json::to_value(&response).map_err(|_| AgentError::ToolSchemaInvalid)?;

    let updated = sqlx::query(
        r#"
        UPDATE agent_steps
        SET undone_at=datetime('now','localtime'), receipt_json=?
        WHERE id=? AND undone_at IS NULL
        "#,
    )
    .bind(receipt.to_string())
    .bind(&step.id)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    if updated.rows_affected() != 1 {
        return Err(AgentError::IdempotencyConflict);
    }
    sqlx::query(
        r#"
        INSERT INTO agent_events(run_id, step_id, event_type, payload_json)
        VALUES(?, ?, 'tool.undone', ?)
        "#,
    )
    .bind(&step.run_id)
    .bind(&step.id)
    .bind(
        json!({
            "step_id": step.id,
            "tool_name": RECORD_CHECKIN_TOOL,
            "tool_version": RECORD_CHECKIN_VERSION,
            "result": "undone"
        })
        .to_string(),
    )
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    Ok(response)
}

/// Undo for `plan.apply_preview`: delete the inserted draft rows, restore the
/// replaced plan rows and the detached record->plan relationships, then verify
/// the restored state hash matches the pre-apply snapshot. Any missing
/// snapshot, repeated undo, or external modification since apply returns
/// `conflict` — user changes are never silently overwritten.
async fn undo_apply_preview_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    step: &UndoStep,
) -> Result<ToolUndoResponse, AgentError> {
    if step.undone_at.is_some() {
        // A repeated undo of an already-undone apply is a conflict, never a
        // silent no-op: the UI must not pretend a second rollback happened.
        return Err(AgentError::Conflict);
    }
    let undo: crate::agent::tools::plan::PlanApplyUndoReceipt = serde_json::from_str(
        step.undo_json
            .as_deref()
            .ok_or(AgentError::ToolSchemaInvalid)?,
    )
    .map_err(|_| AgentError::ToolSchemaInvalid)?;
    if undo.kind != crate::agent::tools::plan::PLAN_APPLY_UNDO_KIND {
        return Err(AgentError::ToolSchemaInvalid);
    }

    // Precondition: nothing may have changed since the apply. A manual edit, a
    // second apply, or a deleted inserted row all change the plan state hash.
    let current_hash = plan::plan_state_hash(tx, &undo.exam_id).await?;
    if current_hash != undo.hash_after {
        return Err(AgentError::Conflict);
    }
    for id in &undo.inserted_plan_ids {
        let exists: Option<String> = sqlx::query_scalar("SELECT id FROM study_plans WHERE id=?")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(map_sqlx)?;
        if exists.is_none() {
            return Err(AgentError::Conflict);
        }
    }
    // The detached records must still be detached (not re-attached or deleted).
    for record in &undo.detached_records {
        let current_plan: Option<Option<String>> =
            sqlx::query_scalar("SELECT plan_id FROM study_records WHERE id=?")
                .bind(&record.id)
                .fetch_optional(&mut **tx)
                .await
                .map_err(map_sqlx)?;
        if !matches!(current_plan, Some(None)) {
            return Err(AgentError::Conflict);
        }
    }

    for id in &undo.inserted_plan_ids {
        let deleted = sqlx::query("DELETE FROM study_plans WHERE id=?")
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(map_sqlx)?;
        if deleted.rows_affected() != 1 {
            return Err(AgentError::Conflict);
        }
    }

    let mut restored_plan_ids = Vec::with_capacity(undo.replaced_plans.len());
    for restored in &undo.replaced_plans {
        sqlx::query(
            "INSERT INTO study_plans \
             (id, exam_id, subject_id, knowledge_point_id, date, planned_tasks, \
              planned_duration, actual_duration, actual_tasks, status, generated_by, \
              ai_suggestion, user_modified, created_at, updated_at, sort_order) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&restored.id)
        .bind(&restored.exam_id)
        .bind(&restored.subject_id)
        .bind(&restored.knowledge_point_id)
        .bind(&restored.date)
        .bind(&restored.planned_tasks)
        .bind(restored.planned_duration)
        .bind(restored.actual_duration)
        .bind(&restored.actual_tasks)
        .bind(&restored.status)
        .bind(&restored.generated_by)
        .bind(&restored.ai_suggestion)
        .bind(restored.user_modified)
        .bind(&restored.created_at)
        .bind(&restored.updated_at)
        .bind(restored.sort_order)
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx)?;
        restored_plan_ids.push(restored.id.clone());
    }
    for record in &undo.detached_records {
        let updated =
            sqlx::query("UPDATE study_records SET plan_id=? WHERE id=? AND plan_id IS NULL")
                .bind(&record.plan_id)
                .bind(&record.id)
                .execute(&mut **tx)
                .await
                .map_err(map_sqlx)?;
        if updated.rows_affected() != 1 {
            return Err(AgentError::Conflict);
        }
    }

    // The restored state must match the pre-apply snapshot exactly.
    let restored_hash = plan::plan_state_hash(tx, &undo.exam_id).await?;
    if restored_hash != undo.hash_before {
        return Err(AgentError::Conflict);
    }

    let output = PlanApplyUndoOutput {
        kind: undo.kind.clone(),
        exam_id: undo.exam_id.clone(),
        inserted_plan_ids: undo.inserted_plan_ids.clone(),
        restored_plan_ids,
        restored_record_count: undo.detached_records.len() as i64,
        status: "undone".to_owned(),
    };
    let response = ToolUndoResponse {
        step_id: step.id.clone(),
        output: serde_json::to_value(&output).map_err(|_| AgentError::ToolSchemaInvalid)?,
    };
    let mut receipt: Value = step
        .receipt_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|_| AgentError::ToolSchemaInvalid)?
        .unwrap_or_else(|| json!({}));
    receipt["undo_result"] =
        serde_json::to_value(&response).map_err(|_| AgentError::ToolSchemaInvalid)?;

    let updated = sqlx::query(
        r#"
        UPDATE agent_steps
        SET undone_at=datetime('now','localtime'), receipt_json=?
        WHERE id=? AND undone_at IS NULL
        "#,
    )
    .bind(receipt.to_string())
    .bind(&step.id)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    if updated.rows_affected() != 1 {
        return Err(AgentError::IdempotencyConflict);
    }
    sqlx::query(
        r#"
        INSERT INTO agent_events(run_id, step_id, event_type, payload_json)
        VALUES(?, ?, 'tool.undone', ?)
        "#,
    )
    .bind(&step.run_id)
    .bind(&step.id)
    .bind(
        json!({
            "step_id": step.id,
            "tool_name": "plan.apply_preview",
            "tool_version": "1",
            "result": "undone"
        })
        .to_string(),
    )
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    Ok(response)
}

fn stored_undo_response(step: &UndoStep) -> Result<ToolUndoResponse, AgentError> {
    let receipt: Value = serde_json::from_str(
        step.receipt_json
            .as_deref()
            .ok_or(AgentError::ToolSchemaInvalid)?,
    )
    .map_err(|_| AgentError::ToolSchemaInvalid)?;
    serde_json::from_value(
        receipt
            .get("undo_result")
            .cloned()
            .ok_or(AgentError::ToolSchemaInvalid)?,
    )
    .map_err(|_| AgentError::ToolSchemaInvalid)
}

async fn finish_transaction<T>(
    tx: Transaction<'_, Sqlite>,
    result: Result<T, AgentError>,
) -> Result<T, AgentError> {
    match result {
        Ok(value) => {
            tx.commit().await.map_err(map_sqlx)?;
            Ok(value)
        }
        Err(error) => {
            tx.rollback().await.map_err(map_sqlx)?;
            Err(error)
        }
    }
}

async fn persist_failed_attempt(
    pool: &SqlitePool,
    request: &ToolCallRequest,
    descriptor: &ToolDescriptor,
    input_json: &str,
    error_code: &str,
) -> Result<(), AgentError> {
    let mut tx = pool.begin().await.map_err(map_sqlx)?;
    let result = async {
        let step_id = Uuid::new_v4().to_string();
        sqlx::query(
            r#"
            INSERT INTO agent_steps(
                id,run_id,step_index,tool_name,tool_version,risk,status,input_json,
                policy_json,error,idempotency_key,started_at,completed_at
            ) VALUES(?,?,?,?,?,?,'failed',?,?,?,?,datetime('now','localtime'),datetime('now','localtime'))
            "#,
        )
        .bind(&step_id)
        .bind(&request.run_id)
        .bind(request.step_index)
        .bind(descriptor.name)
        .bind(descriptor.version)
        .bind(risk_number(descriptor.risk))
        .bind(input_json)
        .bind(policy_receipt(descriptor, "failed").to_string())
        .bind(error_code)
        .bind(&request.idempotency_key)
        .execute(&mut *tx)
        .await
        .map_err(map_reservation_error)?;
        let run_updated = sqlx::query(
            "UPDATE agent_runs SET status='failed',error_code=?,completed_at=datetime('now','localtime') WHERE id=? AND status='running' AND current_step=?",
        )
        .bind(error_code)
        .bind(&request.run_id)
        .bind(request.step_index)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        if run_updated.rows_affected() != 1 {
            return Err(AgentError::Conflict);
        }
        insert_tool_event(
            &mut tx,
            &request.run_id,
            &step_id,
            "tool.requested",
            descriptor,
            "requested",
            None,
        )
        .await?;
        insert_tool_event(
            &mut tx,
            &request.run_id,
            &step_id,
            "tool.failed",
            descriptor,
            "failed",
            Some(error_code),
        )
        .await
    }
    .await;
    finish_transaction(tx, result).await
}

fn canonical_json(value: Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut entries = object.into_iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonical_json(value)))
                    .collect::<Map<_, _>>(),
            )
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonical_json).collect()),
        other => other,
    }
}

fn map_reservation_error(error: sqlx::Error) -> AgentError {
    if is_sqlite_busy(&error) {
        return AgentError::IdempotencyConflict;
    }
    match &error {
        sqlx::Error::Database(database_error)
            if database_error.is_unique_violation()
                && database_error
                    .message()
                    .contains("agent_steps.idempotency_key") =>
        {
            AgentError::IdempotencyConflict
        }
        sqlx::Error::Database(database_error) if database_error.is_unique_violation() => {
            AgentError::Conflict
        }
        _ => map_sqlx(error),
    }
}

fn map_sqlx(_error: sqlx::Error) -> AgentError {
    AgentError::Persistence("tool transaction failed".to_owned())
}

fn is_sqlite_busy(error: &sqlx::Error) -> bool {
    let sqlx::Error::Database(database_error) = error else {
        return false;
    };
    matches!(
        database_error.code().as_deref(),
        Some("5" | "6" | "261" | "517")
    )
}

fn normalize_input(descriptor: &ToolDescriptor, input: Value) -> Result<Value, AgentError> {
    if descriptor.name == RECORD_CHECKIN_TOOL {
        let typed: RecordCheckinPlanInput =
            serde_json::from_value(input).map_err(|_| AgentError::ToolSchemaInvalid)?;
        serde_json::to_value(typed).map_err(|_| AgentError::ToolSchemaInvalid)
    } else {
        Ok(input)
    }
}

/// Stable JSON representation used for idempotency comparison and storage.
/// Since Task 13 the check-in input is stored in this full canonical form too,
/// so the R3 approval restore path (resolve_approval_approved) can re-dispatch
/// it; a hash-only snapshot could not be turned back into an executable input.
fn hash_value(value: &Value) -> String {
    let canonical = canonical_json(value.clone()).to_string();
    let digest = Sha256::digest(canonical.as_bytes());
    let mut fingerprint = String::with_capacity(7 + digest.len() * 2);
    fingerprint.push_str("sha256:");
    for byte in digest {
        write!(&mut fingerprint, "{byte:02x}").expect("writing to a String cannot fail");
    }
    fingerprint
}

fn should_persist_failure(error: &AgentError) -> bool {
    matches!(
        error,
        AgentError::ToolSchemaInvalid
            | AgentError::ToolTimeout
            | AgentError::Persistence(_)
            | AgentError::NotFound(_)
    )
}

/// Rule 12: confine every tool call to the exam bound to the run's session.
///
/// - `exam.get_active` has no input references and is the only tool a run
///   without a bound exam may call (it resolves the active exam itself).
/// - A run without a bound exam may not touch business data at all: any input
///   carrying exam/plan/record/subject/knowledge-point/wrong-question
///   reference is rejected with the stable `tool_scope_violation` code.
/// - A run bound to exam A must have every input reference resolve to A; the
///   model cannot read or write another exam's data by supplying another id.
///
/// This runs before any SQL mutation; all checks are read-only.
async fn enforce_run_scope(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    tool_name: &str,
    input: &Value,
) -> Result<(), AgentError> {
    if tool_name == "exam.get_active" {
        return Ok(());
    }
    let bound_exam: Option<String> =
        sqlx::query_scalar("SELECT s.exam_id FROM agent_runs r JOIN agent_sessions s ON s.id = r.session_id WHERE r.id = ?")
            .bind(run_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(map_sqlx)?
            .flatten();

    let scope_error = || AgentError::ToolScopeViolation;

    // A run without a bound exam has no scope: any business reference is a
    // violation (except exam.get_active handled above).
    let Some(exam) = bound_exam else {
        if input_references_business_data(tool_name, input) {
            return Err(scope_error());
        }
        return Ok(());
    };

    if let Some(exam_id) = input.get("exam_id").and_then(Value::as_str) {
        if exam_id != exam {
            return Err(scope_error());
        }
    }
    if let Some(plan_id) = input.get("plan_id").and_then(Value::as_str) {
        let plan_exam: Option<String> =
            sqlx::query_scalar("SELECT exam_id FROM study_plans WHERE id = ?")
                .bind(plan_id)
                .fetch_optional(&mut **tx)
                .await
                .map_err(map_sqlx)?;
        // A missing resource is the tool layer's not-found error; only a
        // resource that exists outside the bound exam is a scope violation.
        if let Some(plan_exam) = plan_exam {
            if plan_exam != exam {
                return Err(scope_error());
            }
        }
    }
    if let Some(record_id) = input.get("record_id").and_then(Value::as_str) {
        let record_exam: Option<String> = sqlx::query_scalar(
            "SELECT s.exam_id FROM study_records r JOIN subjects s ON s.id = r.subject_id \
             WHERE r.id = ?",
        )
        .bind(record_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(map_sqlx)?;
        if let Some(record_exam) = record_exam {
            if record_exam != exam {
                return Err(scope_error());
            }
        }
    }
    if let Some(subject_id) = input.get("subject_id").and_then(Value::as_str) {
        let subject_exam: Option<String> =
            sqlx::query_scalar("SELECT exam_id FROM subjects WHERE id = ?")
                .bind(subject_id)
                .fetch_optional(&mut **tx)
                .await
                .map_err(map_sqlx)?;
        if let Some(subject_exam) = subject_exam {
            if subject_exam != exam {
                return Err(scope_error());
            }
        }
    }
    if let Some(kp_id) = input.get("knowledge_point_id").and_then(Value::as_str) {
        let kp_exam: Option<String> = sqlx::query_scalar(
            "SELECT s.exam_id FROM knowledge_points k JOIN subjects s ON s.id = k.subject_id \
             WHERE k.id = ?",
        )
        .bind(kp_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(map_sqlx)?;
        if let Some(kp_exam) = kp_exam {
            if kp_exam != exam {
                return Err(scope_error());
            }
        }
    }
    if tool_name == "wrong_question.mark_mastered" {
        if let Some(wq_id) = input.get("id").and_then(Value::as_str) {
            let wq_exam: Option<String> = sqlx::query_scalar(
                "SELECT s.exam_id FROM wrong_questions w JOIN subjects s ON s.id = w.subject_id \
                 WHERE w.id = ?",
            )
            .bind(wq_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(map_sqlx)?;
            if let Some(wq_exam) = wq_exam {
                if wq_exam != exam {
                    return Err(scope_error());
                }
            }
        }
    }
    // `review.complete` references a wrong question through a dedicated field.
    if tool_name == "review.complete" {
        if let Some(wq_id) = input.get("wrong_question_id").and_then(Value::as_str) {
            let wq_exam: Option<String> = sqlx::query_scalar(
                "SELECT s.exam_id FROM wrong_questions w JOIN subjects s ON s.id = w.subject_id \
                 WHERE w.id = ?",
            )
            .bind(wq_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(map_sqlx)?;
            if let Some(wq_exam) = wq_exam {
                if wq_exam != exam {
                    return Err(scope_error());
                }
            }
        }
    }
    // `plan.apply_preview` carries no business id, but its persisted draft is
    // exam-scoped: the draft's exam must equal the bound exam.
    if tool_name == "plan.apply_preview" {
        if let Some(step_id) = input.get("preview_step_id").and_then(Value::as_str) {
            let draft_exam: Option<String> = sqlx::query_scalar(
                "SELECT json_extract(output_json, '$.draft.exam_id') FROM agent_steps \
                 WHERE id = ? AND tool_name = 'plan.preview_generate'",
            )
            .bind(step_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(map_sqlx)?;
            if let Some(draft_exam) = draft_exam {
                if draft_exam != exam {
                    return Err(scope_error());
                }
            }
        }
    }
    Ok(())
}

/// Whether an input carries any business-data reference field at all.
/// `wrong_question.mark_mastered` reuses `id` as its business reference;
/// `plan.apply_preview` applies an exam-scoped draft, so it is also a
/// business-data touch.
fn input_references_business_data(tool_name: &str, input: &Value) -> bool {
    for field in [
        "exam_id",
        "plan_id",
        "record_id",
        "subject_id",
        "knowledge_point_id",
    ] {
        if input.get(field).is_some() {
            return true;
        }
    }
    if tool_name == "wrong_question.mark_mastered" && input.get("id").is_some() {
        return true;
    }
    if tool_name == "review.complete" && input.get("wrong_question_id").is_some() {
        return true;
    }
    if tool_name == "plan.apply_preview" {
        return true;
    }
    false
}

async fn load_run(tx: &mut Transaction<'_, Sqlite>, run_id: &str) -> Result<StoredRun, AgentError> {
    sqlx::query_as::<_, StoredRun>("SELECT status,current_step FROM agent_runs WHERE id=?")
        .bind(run_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(map_sqlx)?
        .ok_or_else(|| AgentError::NotFound(run_id.to_owned()))
}

async fn find_existing_step(
    tx: &mut Transaction<'_, Sqlite>,
    request: &ToolCallRequest,
) -> Result<Option<StoredStep>, AgentError> {
    if let Some(key) = request.idempotency_key.as_deref() {
        if let Some(stored) = find_step_by_idempotency_key(tx, key).await? {
            return Ok(Some(stored));
        }
    }
    let stored = sqlx::query_as::<_, StoredStep>(
        r#"
        SELECT id,tool_name,tool_version,status,input_json,output_json,idempotency_key,undone_at
        FROM agent_steps WHERE run_id=? AND step_index=?
        "#,
    )
    .bind(&request.run_id)
    .bind(request.step_index)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    if stored.as_ref().is_some_and(|stored| {
        stored.idempotency_key.as_deref() != request.idempotency_key.as_deref()
    }) {
        return Err(AgentError::Conflict);
    }
    Ok(stored)
}

async fn find_step_by_idempotency_key(
    tx: &mut Transaction<'_, Sqlite>,
    key: &str,
) -> Result<Option<StoredStep>, AgentError> {
    sqlx::query_as::<_, StoredStep>(
        r#"
        SELECT id,tool_name,tool_version,status,input_json,output_json,idempotency_key,undone_at
        FROM agent_steps WHERE idempotency_key=?
        "#,
    )
    .bind(key)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| {
        if is_sqlite_busy(&error) {
            AgentError::IdempotencyConflict
        } else {
            map_sqlx(error)
        }
    })
}

fn validate_stored_step(
    stored: &StoredStep,
    request: &ToolCallRequest,
    input_json: &str,
) -> Result<(), AgentError> {
    if stored.tool_name == request.tool_name
        && stored.tool_version == request.tool_version
        && stored.input_json.as_deref() == Some(input_json)
    {
        Ok(())
    } else {
        Err(AgentError::IdempotencyConflict)
    }
}

fn replay_response(
    stored: &StoredStep,
    supports_undo: bool,
) -> Result<ToolCallResponse, AgentError> {
    let output = serde_json::from_str(
        stored
            .output_json
            .as_deref()
            .ok_or(AgentError::IdempotencyConflict)?,
    )
    .map_err(|_| AgentError::IdempotencyConflict)?;
    Ok(ToolCallResponse::Completed {
        step_id: stored.id.clone(),
        output,
        replayed: true,
        undo_available: supports_undo && stored.undone_at.is_none(),
    })
}

fn validate_run_gate(
    run: &StoredRun,
    request: &ToolCallRequest,
    stored: Option<&StoredStep>,
) -> Result<(), AgentError> {
    let waiting_replay = stored.is_some_and(|step| step.status == "waiting_approval")
        && matches!(run.status.as_str(), "waiting_approval" | "running");
    if request.step_index == run.current_step && (run.status == "running" || waiting_replay) {
        Ok(())
    } else {
        Err(AgentError::Conflict)
    }
}

fn policy_receipt(descriptor: &ToolDescriptor, decision: &str) -> Value {
    json!({
        "risk": descriptor.risk,
        "confirmation": descriptor.confirmation,
        "decision": decision,
        "delivery": "rust",
    })
}

async fn complete_dispatched_step(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    step_id: &str,
    descriptor: &ToolDescriptor,
    dispatched: &DispatchResult,
    policy: Value,
) -> Result<(), AgentError> {
    let receipt = receipt_with_permissions(dispatched.receipt.clone(), descriptor);
    let updated = sqlx::query(
        r#"
        UPDATE agent_steps
        SET status='completed',output_json=?,policy_json=?,receipt_json=?,undo_json=?,
            error=NULL,completed_at=datetime('now','localtime')
        WHERE id=? AND status='running'
        "#,
    )
    .bind(dispatched.output.to_string())
    .bind(policy.to_string())
    .bind(receipt.to_string())
    .bind(dispatched.undo.as_ref().map(Value::to_string))
    .bind(step_id)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    if updated.rows_affected() != 1 {
        return Err(AgentError::Conflict);
    }
    insert_tool_event(
        tx,
        run_id,
        step_id,
        "tool.completed",
        descriptor,
        "completed",
        None,
    )
    .await
}

fn receipt_with_permissions(receipt: Option<Value>, descriptor: &ToolDescriptor) -> Value {
    let mut receipt = match receipt {
        Some(Value::Object(receipt)) => receipt,
        _ => Map::new(),
    };
    receipt.insert("permissions".to_owned(), json!(descriptor.data_permissions));
    Value::Object(receipt)
}

async fn advance_run(
    tx: &mut Transaction<'_, Sqlite>,
    request: &ToolCallRequest,
) -> Result<(), AgentError> {
    let updated = sqlx::query(
        "UPDATE agent_runs SET current_step=current_step+1 WHERE id=? AND status='running' AND current_step=?",
    )
    .bind(&request.run_id)
    .bind(request.step_index)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    if updated.rows_affected() == 1 {
        Ok(())
    } else {
        Err(AgentError::Conflict)
    }
}

async fn reserve_step(
    tx: &mut Transaction<'_, Sqlite>,
    request: &ToolCallRequest,
    descriptor: &ToolDescriptor,
    input_json: &str,
) -> Result<String, AgentError> {
    if descriptor.idempotency == Idempotency::RequiredExactlyOnce
        && request
            .idempotency_key
            .as_deref()
            .is_none_or(|key| key.trim().is_empty())
    {
        return Err(AgentError::IdempotencyRequired);
    }
    let step_id = Uuid::new_v4().to_string();
    sqlx::query(
        r#"
        INSERT INTO agent_steps (
            id, run_id, step_index, tool_name, tool_version, risk, status,
            input_json, idempotency_key, started_at
        ) VALUES (?, ?, ?, ?, ?, ?, 'running', ?, ?, datetime('now','localtime'))
        "#,
    )
    .bind(&step_id)
    .bind(&request.run_id)
    .bind(request.step_index)
    .bind(descriptor.name)
    .bind(descriptor.version)
    .bind(risk_number(descriptor.risk))
    .bind(input_json)
    .bind(&request.idempotency_key)
    .execute(&mut **tx)
    .await
    .map_err(map_reservation_error)?;
    Ok(step_id)
}

async fn insert_tool_event(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    step_id: &str,
    event_type: &str,
    descriptor: &ToolDescriptor,
    result: &str,
    error_code: Option<&str>,
) -> Result<(), AgentError> {
    sqlx::query(
        "INSERT INTO agent_events(run_id, step_id, event_type, payload_json) VALUES(?, ?, ?, ?)",
    )
    .bind(run_id)
    .bind(step_id)
    .bind(event_type)
    .bind(
        json!({
            "step_id": step_id,
            "tool_name": descriptor.name,
            "tool_version": descriptor.version,
            "risk": descriptor.risk,
            "result": result,
            "error_code": error_code,
        })
        .to_string(),
    )
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    Ok(())
}

fn risk_number(risk: RiskLevel) -> i64 {
    match risk {
        RiskLevel::R0 => 0,
        RiskLevel::R1 => 1,
        RiskLevel::R2 => 2,
        RiskLevel::R3 => 3,
        RiskLevel::R4 => 4,
    }
}

async fn load_approval(
    tx: &mut Transaction<'_, Sqlite>,
    approval_id: &str,
) -> Result<StoredApproval, AgentError> {
    sqlx::query_as::<_, StoredApproval>(
        r#"
        SELECT id, run_id, step_id, risk, preview_json, precondition_json,
               status, expires_at, decided_at, created_at
        FROM agent_approvals WHERE id=?
        "#,
    )
    .bind(approval_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_sqlx)?
    .ok_or_else(|| AgentError::NotFound(approval_id.to_owned()))
}

fn approval_precondition_hash(approval: &StoredApproval) -> Result<String, AgentError> {
    let precondition: Value = serde_json::from_str(
        approval
            .precondition_json
            .as_deref()
            .ok_or(AgentError::ApprovalInvalid)?,
    )
    .map_err(|_| AgentError::ApprovalInvalid)?;
    precondition["hash"]
        .as_str()
        .map(str::to_owned)
        .ok_or(AgentError::ApprovalInvalid)
}

fn approval_record(approval: StoredApproval) -> Result<ApprovalRecord, AgentError> {
    let preview = approval
        .preview_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|_| AgentError::ApprovalInvalid)?
        .unwrap_or(Value::Null);
    let precondition_hash = approval_precondition_hash(&approval)?;
    Ok(ApprovalRecord {
        id: approval.id,
        run_id: approval.run_id,
        step_id: approval.step_id,
        risk: approval.risk,
        preview,
        precondition_hash,
        status: approval.status,
        expires_at: approval.expires_at,
        decided_at: approval.decided_at,
        created_at: approval.created_at,
    })
}

async fn decide_approval_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    approval_id: &str,
    approve: bool,
) -> Result<ApprovalRecord, AgentError> {
    let approval = load_approval(tx, approval_id).await?;
    let expires_at = chrono::DateTime::parse_from_rfc3339(&approval.expires_at)
        .map_err(|_| AgentError::ApprovalInvalid)?
        .with_timezone(&Utc);
    let now = Utc::now();
    if approval.status != "pending" || expires_at <= now {
        return Err(AgentError::ApprovalInvalid);
    }
    let status = if approve { "approved" } else { "rejected" };
    let decided_at = now.to_rfc3339();
    let updated = sqlx::query(
        "UPDATE agent_approvals SET status=?, decided_at=? WHERE id=? AND status='pending' AND expires_at>?",
    )
    .bind(status)
    .bind(&decided_at)
    .bind(approval_id)
    .bind(now.to_rfc3339())
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    if updated.rows_affected() != 1 {
        return Err(AgentError::ApprovalInvalid);
    }

    let run_status = if approve { "running" } else { "cancelled" };
    let run_updated = sqlx::query(
        r#"
        UPDATE agent_runs
        SET status=?,
            completed_at=CASE WHEN ?='cancelled' THEN datetime('now','localtime') ELSE NULL END
        WHERE id=? AND status='waiting_approval'
        "#,
    )
    .bind(run_status)
    .bind(run_status)
    .bind(&approval.run_id)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    if run_updated.rows_affected() != 1 {
        return Err(AgentError::Conflict);
    }
    if !approve {
        let step_updated = sqlx::query(
            "UPDATE agent_steps SET status='cancelled',policy_json=?,error='approval_rejected',completed_at=datetime('now','localtime') WHERE id=? AND status='waiting_approval'",
        )
        .bind(
            json!({
                "risk": format!("R{}", approval.risk),
                "decision": "rejected",
                "delivery": "rust",
            })
            .to_string(),
        )
        .bind(&approval.step_id)
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx)?;
        if step_updated.rows_affected() != 1 {
            return Err(AgentError::Conflict);
        }
    }

    let (tool_name, tool_version, risk): (String, String, i64) = sqlx::query_as(
        "SELECT tool_name, tool_version, risk FROM agent_steps WHERE id=? AND run_id=?",
    )
    .bind(&approval.step_id)
    .bind(&approval.run_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    let event_type = if approve {
        "approval.approved"
    } else {
        "approval.rejected"
    };
    sqlx::query("INSERT INTO agent_events(run_id,step_id,event_type,payload_json) VALUES(?,?,?,?)")
        .bind(&approval.run_id)
        .bind(&approval.step_id)
        .bind(event_type)
        .bind(
            json!({
                "approval_id": approval.id,
                "step_id": approval.step_id,
                "tool_name": tool_name,
                "tool_version": tool_version,
                "risk": format!("R{risk}"),
                "result": status,
            })
            .to_string(),
        )
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx)?;

    approval_record(load_approval(tx, approval_id).await?)
}

async fn terminalize_expired_approval(
    tx: &mut Transaction<'_, Sqlite>,
    approval: &StoredApproval,
    descriptor: &ToolDescriptor,
) -> Result<(), AgentError> {
    sqlx::query(
        "UPDATE agent_approvals SET status='expired',decided_at=? WHERE id=? AND status IN ('pending','approved')",
    )
    .bind(Utc::now().to_rfc3339())
    .bind(&approval.id)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    terminalize_failed_approval_step(tx, approval, descriptor, "approval_expired").await
}

async fn terminalize_failed_approval_step(
    tx: &mut Transaction<'_, Sqlite>,
    approval: &StoredApproval,
    descriptor: &ToolDescriptor,
    result: &str,
) -> Result<(), AgentError> {
    let step_updated = sqlx::query(
        "UPDATE agent_steps SET status='failed',policy_json=?,error='approval_invalid',completed_at=datetime('now','localtime') WHERE id=? AND status='waiting_approval'",
    )
    .bind(policy_receipt(descriptor, "failed").to_string())
    .bind(&approval.step_id)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    let run_updated = sqlx::query(
        "UPDATE agent_runs SET status='failed',error_code='approval_invalid',completed_at=datetime('now','localtime') WHERE id=? AND status IN ('waiting_approval','running')",
    )
    .bind(&approval.run_id)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    if step_updated.rows_affected() != 1 || run_updated.rows_affected() != 1 {
        return Err(AgentError::Conflict);
    }
    insert_tool_event(
        tx,
        &approval.run_id,
        &approval.step_id,
        "tool.failed",
        descriptor,
        result,
        Some("approval_invalid"),
    )
    .await
}

/// Finalize a failed approval resolution: the step (running or waiting) and
/// the run land in `failed`, the approval in `failed`, with the stable error
/// code surfaced on the run. Used when an approved write fails at dispatch
/// time (precondition/schema/persistence) so the run is never left
/// `waiting_approval` or `running`.
async fn finalize_approval_failure(
    tx: &mut Transaction<'_, Sqlite>,
    approval: &StoredApproval,
    error_code: &str,
) -> Result<(), AgentError> {
    sqlx::query(
        "UPDATE agent_steps SET status='failed',error=?,completed_at=datetime('now','localtime') \
         WHERE id=? AND status IN ('running','waiting_approval')",
    )
    .bind(error_code)
    .bind(&approval.step_id)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    sqlx::query(
        "UPDATE agent_runs SET status='failed',error_code=?,completed_at=datetime('now','localtime') \
         WHERE id=? AND status IN ('waiting_approval','running')",
    )
    .bind(error_code)
    .bind(&approval.run_id)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    sqlx::query(
        "UPDATE agent_approvals SET status='rejected',decided_at=? WHERE id=? AND status='pending'",
    )
    .bind(Utc::now().to_rfc3339())
    .bind(&approval.id)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    Ok(())
}

async fn read_bool_setting(
    tx: &mut Transaction<'_, Sqlite>,
    key: &str,
) -> Result<bool, AgentError> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key=?")
        .bind(key)
        .fetch_optional(&mut **tx)
        .await
        .map_err(map_sqlx)?;
    Ok(matches!(value.as_deref(), Some("true" | "1")))
}

async fn set_step_running(
    tx: &mut Transaction<'_, Sqlite>,
    step_id: &str,
) -> Result<(), AgentError> {
    let updated = sqlx::query(
        "UPDATE agent_steps SET status='running' WHERE id=? AND status IN ('pending','running','waiting_approval')",
    )
    .bind(step_id)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    if updated.rows_affected() != 1 {
        return Err(AgentError::Conflict);
    }
    Ok(())
}

async fn load_approval_for_step(
    tx: &mut Transaction<'_, Sqlite>,
    step_id: &str,
) -> Result<StoredApproval, AgentError> {
    let approval_id: String = sqlx::query_scalar("SELECT id FROM agent_approvals WHERE step_id=?")
        .bind(step_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(map_sqlx)?;
    load_approval(tx, &approval_id).await
}

/// Build the standardized, sanitized approval preview (Task 10 Step 2). It
/// carries the action name, affected-object count, date range, before/after
/// summary, structured fields, conflicts, risk, and undo availability — never
/// the API key, never raw model output, never full request bodies. For
/// `plan.apply_preview` it includes the actual draft rows summary and the
/// precondition hash instead of "call plan.generate".
async fn build_approval_preview(
    tx: &mut Transaction<'_, Sqlite>,
    tool_name: &str,
    input: &Value,
) -> Result<Value, AgentError> {
    let mut preview = serde_json::Map::new();
    preview.insert("tool".to_owned(), Value::String(tool_name.to_owned()));
    preview.insert("risk".to_owned(), json!(3));
    preview.insert(
        "undo_available".to_owned(),
        Value::Bool(tool_name == "record.checkin_plan" || tool_name == "plan.apply_preview"),
    );

    let (action, affected_count, summary, conflicts, date_range, fields) = match tool_name {
        "plan.apply_preview" => {
            let step_id = input
                .get("preview_step_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            let draft_json: Option<String> = sqlx::query_scalar(
                "SELECT output_json FROM agent_steps \
                 WHERE id = ? AND tool_name = 'plan.preview_generate'",
            )
            .bind(step_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(map_sqlx)?;
            match draft_json {
                Some(json) => {
                    // output_json holds PlanPreviewGenerateOutput; the draft
                    // lives at $.draft and deserializes to the same PlanDraft
                    // the apply will use, so preview and apply share the exact
                    // projection (writable rows can never drift).
                    let output: Value = serde_json::from_str(&json).unwrap_or(Value::Null);
                    let draft: Option<crate::agent::plan_draft::PlanDraft> = output
                        .get("draft")
                        .cloned()
                        .and_then(|draft| serde_json::from_value(draft).ok());
                    if let Some(draft) = draft {
                        let business_date = plan::business_date_at(Local::now().fixed_offset());
                        let existing: Vec<plan::ExistingPlanRow> = sqlx::query_as(
                            "SELECT id, date, subject_id, actual_duration \
                             FROM study_plans WHERE exam_id = ?",
                        )
                        .bind(&draft.exam_id)
                        .fetch_all(&mut **tx)
                        .await
                        .map_err(map_sqlx)?;
                        let projection =
                            plan::project_apply_rows(&draft, &existing, &business_date);
                        let rows: Vec<Value> = projection
                            .writable_rows
                            .iter()
                            .map(|row| {
                                json!({
                                    "date": row.date,
                                    "subject_name": row.subject_name,
                                    "planned_tasks": row.planned_tasks,
                                    "planned_duration": row.planned_duration,
                                    "evidence": row.evidence,
                                })
                            })
                            .collect();
                        let flattened = draft
                            .daily_plans
                            .iter()
                            .flat_map(|day| {
                                day.tasks.iter().map(|task| {
                                    json!({
                                        "date": day.date,
                                        "subject_name": task.subject_name,
                                        "planned_tasks": task.task,
                                        "planned_duration": task.duration_min,
                                    })
                                })
                            })
                            .collect::<Vec<_>>();
                        let sanitized_fields = json!({
                            "precondition_hash": draft.precondition_hash,
                            "draft_row_count": flattened.len(),
                            "rows": rows,
                            "kept_subjects": projection.kept_subjects,
                            "conflicts": draft.conflicts,
                        });
                        (
                            "应用计划草案".to_owned(),
                            projection.writable_rows.len() as i64,
                            format!(
                                "将应用 {}~{} 共 {} 天的本地生成计划：{} 项任务；可写入 {} 行",
                                draft.start_date,
                                draft
                                    .daily_plans
                                    .last()
                                    .map(|day| day.date.as_str())
                                    .unwrap_or(""),
                                draft.total_days,
                                flattened.len(),
                                projection.writable_rows.len(),
                            ),
                            serde_json::to_value(&draft.conflicts).unwrap_or_else(|_| json!([])),
                            format!(
                                "{}~{}",
                                draft.start_date,
                                draft
                                    .daily_plans
                                    .last()
                                    .map(|day| day.date.as_str())
                                    .unwrap_or("")
                            ),
                            sanitized_fields,
                        )
                    } else {
                        (
                            "应用计划草案".to_owned(),
                            0_i64,
                            "计划草案已失效，请重新生成".to_owned(),
                            json!([]),
                            String::new(),
                            Value::Null,
                        )
                    }
                }
                None => (
                    "应用计划草案".to_owned(),
                    0_i64,
                    "计划草案已失效，请重新生成".to_owned(),
                    json!([]),
                    String::new(),
                    Value::Null,
                ),
            }
        }
        "record.checkin_plan" => {
            let plan_id = input.get("plan_id").and_then(Value::as_str).unwrap_or("");
            let duration = input
                .get("duration_min")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let finish = input
                .get("finish")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let date = input.get("date").and_then(Value::as_str).unwrap_or("");
            (
                if finish {
                    "完成计划打卡".to_owned()
                } else {
                    "记录计划学习进度".to_owned()
                },
                1_i64,
                format!(
                    "计划 {plan_id} 学习 {duration} 分钟{}",
                    if finish { "（完成）" } else { "" }
                ),
                json!([]),
                date.to_owned(),
                json!({"plan_id": plan_id, "duration_min": duration, "finish": finish}),
            )
        }
        "record.create_free" => {
            let subject_id = input
                .get("subject_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            let duration = input
                .get("duration_min")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let date = input.get("date").and_then(Value::as_str).unwrap_or("");
            (
                "记录自由学习".to_owned(),
                1_i64,
                format!("科目 {subject_id} 学习 {duration} 分钟"),
                json!([]),
                date.to_owned(),
                json!({"subject_id": subject_id, "duration_min": duration, "date": date}),
            )
        }
        "wrong_question.create" => {
            let subject_id = input
                .get("subject_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            (
                "记录错题".to_owned(),
                1_i64,
                format!("新增一条 {subject_id} 的错题"),
                json!([]),
                String::new(),
                json!({"subject_id": subject_id}),
            )
        }
        "wrong_question.mark_mastered" => {
            let id = input.get("id").and_then(Value::as_str).unwrap_or("");
            (
                "标记错题已掌握".to_owned(),
                1_i64,
                format!("将错题 {id} 标记为已掌握"),
                json!([]),
                String::new(),
                json!({"id": id}),
            )
        }
        _ => (
            "执行操作".to_owned(),
            0_i64,
            String::new(),
            json!([]),
            String::new(),
            Value::Null,
        ),
    };

    preview.insert("action".to_owned(), Value::String(action));
    preview.insert("affected_count".to_owned(), json!(affected_count));
    preview.insert("summary".to_owned(), Value::String(summary));
    preview.insert("conflicts".to_owned(), conflicts);
    preview.insert("date_range".to_owned(), Value::String(date_range));
    // Sanitized structured fields: only whitelisted scalars (or the projected
    // draft rows). The raw request body, API key, and model output never appear.
    preview.insert("fields".to_owned(), fields);
    Ok(Value::Object(preview))
}

async fn create_pending_approval(
    tx: &mut Transaction<'_, Sqlite>,
    request: &ToolCallRequest,
    descriptor: &ToolDescriptor,
    step_id: &str,
    precondition_hash: &str,
    preview: Value,
) -> Result<StoredApproval, AgentError> {
    let approval_id = Uuid::new_v4().to_string();
    let expires_at = (Utc::now() + ChronoDuration::minutes(10)).to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO agent_approvals(
            id,run_id,step_id,risk,preview_json,precondition_json,status,expires_at
        ) VALUES(?,?,?,?,?,?,'pending',?)
        "#,
    )
    .bind(&approval_id)
    .bind(&request.run_id)
    .bind(step_id)
    .bind(3_i64)
    .bind(preview.to_string())
    .bind(json!({"hash":precondition_hash}).to_string())
    .bind(&expires_at)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    sqlx::query("UPDATE agent_steps SET status='waiting_approval', policy_json=? WHERE id=? AND status='running'")
        .bind(policy_receipt(descriptor, "waiting_approval").to_string())
        .bind(step_id)
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx)?;
    let run_updated = sqlx::query(
        "UPDATE agent_runs SET status='waiting_approval' WHERE id=? AND status='running'",
    )
    .bind(&request.run_id)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    if run_updated.rows_affected() != 1 {
        return Err(AgentError::Conflict);
    }
    insert_tool_event(
        tx,
        &request.run_id,
        step_id,
        "tool.waiting_approval",
        descriptor,
        "waiting_approval",
        None,
    )
    .await?;
    load_approval(tx, &approval_id).await
}

fn waiting_response(approval: &StoredApproval) -> Result<ToolCallResponse, AgentError> {
    let preview = approval
        .preview_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|_| AgentError::ApprovalInvalid)?
        .unwrap_or(Value::Null);
    Ok(ToolCallResponse::WaitingApproval {
        step_id: approval.step_id.clone(),
        approval_id: approval.id.clone(),
        preview,
        expires_at: approval.expires_at.clone(),
    })
}

#[cfg(test)]
mod policy_executor_tests {
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };

    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::agent::model::RunStatus;
    use crate::agent::tools::{Confirmation, Idempotency};

    fn descriptor(name: &'static str, risk: RiskLevel) -> ToolDescriptor {
        ToolDescriptor {
            name,
            version: "1",
            input_schema: json!({
                "type":"object",
                "additionalProperties":false,
                "required":["source_id"],
                "properties":{"source_id":{"type":"string"}}
            }),
            output_schema: json!({
                "type":"object",
                "additionalProperties":false,
                "required":["ok"],
                "properties":{"ok":{"type":"boolean"}}
            }),
            risk,
            confirmation: match risk {
                RiskLevel::R2 => Confirmation::SummaryOrSetting,
                RiskLevel::R3 => Confirmation::Required,
                RiskLevel::R4 => Confirmation::NavigationOnly,
                _ => Confirmation::Automatic,
            },
            supports_undo: false,
            timeout_ms: 1_000,
            idempotency: if risk == RiskLevel::R1 {
                Idempotency::RequiredExactlyOnce
            } else {
                Idempotency::NoAutomaticRetry
            },
            data_permissions: vec!["synthetic:write"],
        }
    }

    async fn setup_with(
        risk: RiskLevel,
        invalid_output: bool,
    ) -> (AgentExecutor, sqlx::SqlitePool, Arc<AtomicUsize>, String) {
        setup_with_hooks(risk, invalid_output, None, None).await
    }

    async fn setup_with_hooks(
        risk: RiskLevel,
        invalid_output: bool,
        rollback_before_dispatch_once: Option<Arc<std::sync::atomic::AtomicBool>>,
        start_idempotency_race_once: Option<Arc<std::sync::atomic::AtomicBool>>,
    ) -> (AgentExecutor, sqlx::SqlitePool, Arc<AtomicUsize>, String) {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        for migration in crate::db::migrations() {
            sqlx::raw_sql(migration.sql).execute(&pool).await.unwrap();
        }
        sqlx::raw_sql(
            r#"
            INSERT INTO agent_sessions(id,title) VALUES('session-policy','Policy');
            INSERT INTO agent_runs(id,session_id,goal,status)
            VALUES('run-policy','session-policy','Policy run','running');
            CREATE TABLE synthetic_sources(id TEXT PRIMARY KEY, value TEXT NOT NULL);
            INSERT INTO synthetic_sources(id,value) VALUES('source-1','before');
            CREATE TABLE synthetic_business(id INTEGER PRIMARY KEY AUTOINCREMENT, source_id TEXT);
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        let name = match risk {
            RiskLevel::R1 => "synthetic.r1",
            RiskLevel::R2 => "synthetic.r2",
            RiskLevel::R3 => "synthetic.r3",
            RiskLevel::R4 => "synthetic.r4",
            _ => unreachable!(),
        };
        sqlx::query("INSERT INTO settings(key,value) VALUES(?, 'rust-owned')")
            .bind(format!("agent_tool_owner.{name}"))
            .execute(&pool)
            .await
            .unwrap();
        let counter = Arc::new(AtomicUsize::new(0));
        let executor = AgentExecutor::for_test(
            pool.clone(),
            ToolRegistry::for_test(vec![descriptor(name, risk)]),
            TestDispatcherConfig {
                dispatch_count: counter.clone(),
                invalid_output,
                rollback_before_dispatch_once,
                start_idempotency_race_once,
            },
        );
        (executor, pool, counter, name.to_owned())
    }

    async fn setup(risk: RiskLevel) -> (AgentExecutor, sqlx::SqlitePool, Arc<AtomicUsize>, String) {
        setup_with(risk, false).await
    }

    fn request(name: &str, approval_id: Option<String>) -> ToolCallRequest {
        ToolCallRequest {
            run_id: "run-policy".to_owned(),
            step_index: 0,
            tool_name: name.to_owned(),
            tool_version: "1".to_owned(),
            input: json!({"source_id":"source-1"}),
            idempotency_key: None,
            approval_id,
        }
    }

    fn r1_request(name: &str, key: &str) -> ToolCallRequest {
        ToolCallRequest {
            idempotency_key: Some(key.to_owned()),
            ..request(name, None)
        }
    }

    #[test]
    fn canonical_fingerprint_uses_stable_prefixed_sha256() {
        assert_eq!(
            hash_value(&json!({"b":2,"a":1})),
            "sha256:43258cff783fe7036d8a43033f830adfc60ec037382473548ac742b888292777"
        );
    }

    #[tokio::test]
    async fn r1_first_transaction_rollback_is_retried_to_one_completion() {
        let rollback_once = Arc::new(AtomicBool::new(true));
        let (executor, pool, counter, name) =
            setup_with_hooks(RiskLevel::R1, false, Some(rollback_once), None).await;

        let response = executor
            .execute(r1_request(&name, "synthetic/r1/rollback"))
            .await
            .unwrap();

        assert!(matches!(
            response,
            ToolCallResponse::Completed {
                replayed: false,
                ..
            }
        ));
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM synthetic_business")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_steps")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_events")
                .fetch_one(&pool)
                .await
                .unwrap(),
            2
        );
    }

    #[tokio::test]
    async fn r1_unresolved_key_stops_after_three_reads_without_dispatch_or_audit() {
        let race_once = Arc::new(AtomicBool::new(true));
        let (executor, pool, counter, name) =
            setup_with_hooks(RiskLevel::R1, false, None, Some(race_once)).await;
        sqlx::query(
            r#"
            INSERT INTO agent_steps(
                id,run_id,step_index,tool_name,tool_version,risk,status,input_json,idempotency_key
            ) VALUES('unresolved-step','run-policy',0,?, '1',1,'running',?,?)
            "#,
        )
        .bind(&name)
        .bind(canonical_json(json!({"source_id":"source-1"})).to_string())
        .bind("synthetic/r1/unresolved")
        .execute(&pool)
        .await
        .unwrap();

        let error = executor
            .execute(r1_request(&name, "synthetic/r1/unresolved"))
            .await
            .unwrap_err();

        assert_eq!(error.code(), "idempotency_conflict");
        assert_eq!(
            error.to_string(),
            "idempotency key is already being resolved; retry"
        );
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM synthetic_business")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_events")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn legacy_ownership_settings_do_not_block_or_rewrite_execution() {
        for owner in ["typescript", "shadow"] {
            let (executor, pool, counter, name) = setup(RiskLevel::R1).await;
            sqlx::query("UPDATE settings SET value=? WHERE key=?")
                .bind(owner)
                .bind(format!("agent_tool_owner.{name}"))
                .execute(&pool)
                .await
                .unwrap();
            // Task 13: legacy ownership values are inert — the tool still
            // executes as Rust-owned and the setting is never rewritten.
            executor
                .execute(r1_request(&name, &format!("synthetic/r1/{owner}")))
                .await
                .unwrap();
            assert_eq!(counter.load(Ordering::SeqCst), 1);
            let stored: String = sqlx::query_scalar("SELECT value FROM settings WHERE key=?")
                .bind(format!("agent_tool_owner.{name}"))
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(
                stored.as_str(),
                owner,
                "legacy setting must not be rewritten"
            );
        }
    }

    #[tokio::test]
    async fn r2_summary_blocks_dispatch_until_setting_is_enabled() {
        let (executor, pool, counter, name) = setup(RiskLevel::R2).await;
        sqlx::query("INSERT INTO settings(key,value) VALUES('agent_r2_auto_execute','false')")
            .execute(&pool)
            .await
            .unwrap();

        let summary = executor.execute(request(&name, None)).await.unwrap();
        assert!(matches!(summary, ToolCallResponse::SummaryRequired { .. }));
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM synthetic_business")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );

        sqlx::query("UPDATE settings SET value='true' WHERE key='agent_r2_auto_execute'")
            .execute(&pool)
            .await
            .unwrap();
        let completed = executor.execute(request(&name, None)).await.unwrap();
        assert!(matches!(
            completed,
            ToolCallResponse::Completed {
                replayed: false,
                ..
            }
        ));
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn r3_persists_one_approval_and_only_live_exact_approval_dispatches() {
        let (executor, pool, counter, name) = setup(RiskLevel::R3).await;

        let waiting = executor.execute(request(&name, None)).await.unwrap();
        let ToolCallResponse::WaitingApproval {
            step_id,
            approval_id,
            expires_at,
            ..
        } = waiting
        else {
            panic!("R3 must wait")
        };
        assert!(!expires_at.is_empty());
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM agent_approvals WHERE status='pending'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT status FROM agent_runs WHERE id='run-policy'")
                .fetch_one(&pool)
                .await
                .unwrap(),
            "waiting_approval"
        );
        let policy_json: String =
            sqlx::query_scalar("SELECT policy_json FROM agent_steps WHERE id=?")
                .bind(&step_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let policy: Value = serde_json::from_str(&policy_json).unwrap();
        assert_eq!(policy["decision"], "waiting_approval");
        assert_eq!(policy["delivery"], "rust");

        let approved = executor.decide_approval(&approval_id, true).await.unwrap();
        assert_eq!(approved.status, "approved");
        assert_eq!(approved.step_id, step_id);
        let completed = executor
            .execute(request(&name, Some(approval_id)))
            .await
            .unwrap();
        assert!(matches!(completed, ToolCallResponse::Completed { .. }));
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn expired_stale_and_rejected_r3_approvals_never_dispatch() {
        for case in ["expired", "expired_no_id", "stale", "rejected"] {
            let (executor, pool, counter, name) = setup(RiskLevel::R3).await;
            let ToolCallResponse::WaitingApproval { approval_id, .. } =
                executor.execute(request(&name, None)).await.unwrap()
            else {
                panic!("R3 must wait")
            };

            match case {
                "expired" | "expired_no_id" => {
                    sqlx::query(
                        "UPDATE agent_approvals SET expires_at='2000-01-01T00:00:00Z' WHERE id=?",
                    )
                    .bind(&approval_id)
                    .execute(&pool)
                    .await
                    .unwrap();
                }
                "stale" => {
                    executor.decide_approval(&approval_id, true).await.unwrap();
                    sqlx::query("UPDATE synthetic_sources SET value='after' WHERE id='source-1'")
                        .execute(&pool)
                        .await
                        .unwrap();
                }
                "rejected" => {
                    executor.decide_approval(&approval_id, false).await.unwrap();
                }
                _ => unreachable!(),
            }
            let error = executor
                .execute(request(
                    &name,
                    (case != "expired_no_id").then_some(approval_id),
                ))
                .await
                .unwrap_err();
            assert_eq!(error.code(), "approval_invalid", "case={case}");
            assert_eq!(counter.load(Ordering::SeqCst), 0, "case={case}");
            let expected = if case.starts_with("expired") {
                ("expired", "failed", "failed")
            } else if case == "rejected" {
                ("rejected", "cancelled", "cancelled")
            } else {
                ("approved", "failed", "failed")
            };
            let actual: (String, String, String) = sqlx::query_as(
                r#"
                SELECT a.status, s.status, r.status
                FROM agent_approvals a
                JOIN agent_steps s ON s.id=a.step_id
                JOIN agent_runs r ON r.id=a.run_id
                "#,
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(
                (actual.0.as_str(), actual.1.as_str(), actual.2.as_str()),
                expected,
                "case={case}"
            );
            let policy_json: String = sqlx::query_scalar("SELECT policy_json FROM agent_steps")
                .fetch_one(&pool)
                .await
                .unwrap();
            let policy: Value = serde_json::from_str(&policy_json).unwrap();
            assert_eq!(
                policy["decision"],
                if case == "rejected" {
                    "rejected"
                } else {
                    "failed"
                },
                "case={case}"
            );
            assert_eq!(policy["delivery"], "rust", "case={case}");
        }
    }

    #[tokio::test]
    async fn legacy_ownership_never_blocks_reservation_in_the_same_core() {
        let (executor, pool, counter, name) = setup(RiskLevel::R2).await;
        sqlx::query("UPDATE settings SET value='typescript' WHERE key=?")
            .bind(format!("agent_tool_owner.{name}"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO settings(key,value) VALUES('agent_r2_auto_execute','true')")
            .execute(&pool)
            .await
            .unwrap();

        // Task 13: the legacy `typescript` value no longer makes the tool
        // unavailable; the write executes through the Rust core.
        let response = executor.execute(request(&name, None)).await.unwrap();
        assert!(matches!(response, ToolCallResponse::Completed { .. }));
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn output_schema_failure_rolls_back_business_and_never_completes_step() {
        let (executor, pool, counter, name) = setup_with(RiskLevel::R2, true).await;
        sqlx::query("INSERT INTO settings(key,value) VALUES('agent_r2_auto_execute','true')")
            .execute(&pool)
            .await
            .unwrap();

        let error = executor.execute(request(&name, None)).await.unwrap_err();

        assert_eq!(error.code(), "tool_schema_invalid");
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM synthetic_business")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM agent_events WHERE event_type='tool.completed'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn r4_returns_settings_navigation_without_dispatch() {
        let (executor, _pool, counter, name) = setup(RiskLevel::R4).await;

        let response = executor.execute(request(&name, None)).await.unwrap();

        assert_eq!(
            response,
            ToolCallResponse::NavigationRequired {
                route: "/settings".to_owned(),
                reason: "tool_requires_navigation".to_owned(),
            }
        );
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn synthetic_business_and_step_roll_back_when_completion_event_fails() {
        let (executor, pool, counter, name) = setup(RiskLevel::R2).await;
        sqlx::query("INSERT INTO settings(key,value) VALUES('agent_r2_auto_execute','true')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::raw_sql(
            r#"
            CREATE TRIGGER reject_synthetic_completed BEFORE INSERT ON agent_events
            WHEN NEW.event_type='tool.completed'
            BEGIN SELECT RAISE(ABORT,'synthetic audit failure'); END;
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let error = executor.execute(request(&name, None)).await.unwrap_err();

        assert_eq!(error.code(), "persistence_error");
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM synthetic_business")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        let (status, failed_events): (String, i64) = sqlx::query_as(
            r#"
            SELECT s.status,
                   (SELECT COUNT(*) FROM agent_events WHERE event_type='tool.failed')
            FROM agent_steps s
            "#,
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((status.as_str(), failed_events), ("failed", 1));
    }

    #[tokio::test]
    async fn approval_decision_events_and_run_status_are_atomic() {
        let (executor, pool, _counter, name) = setup(RiskLevel::R3).await;
        let ToolCallResponse::WaitingApproval { approval_id, .. } =
            executor.execute(request(&name, None)).await.unwrap()
        else {
            panic!("R3 must wait")
        };

        executor.decide_approval(&approval_id, true).await.unwrap();

        let status: RunStatus =
            sqlx::query_scalar("SELECT status FROM agent_runs WHERE id='run-policy'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, RunStatus::Running);
        let events: Vec<String> = sqlx::query_scalar(
            "SELECT event_type FROM agent_events WHERE run_id='run-policy' ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            events,
            [
                "tool.requested",
                "tool.waiting_approval",
                "approval.approved"
            ]
        );
    }

    #[tokio::test]
    async fn deciding_an_expired_approval_terminalizes_step_and_run() {
        let (executor, pool, counter, name) = setup(RiskLevel::R3).await;
        let ToolCallResponse::WaitingApproval { approval_id, .. } =
            executor.execute(request(&name, None)).await.unwrap()
        else {
            panic!("R3 must wait")
        };
        sqlx::query("UPDATE agent_approvals SET expires_at='2000-01-01T00:00:00Z' WHERE id=?")
            .bind(&approval_id)
            .execute(&pool)
            .await
            .unwrap();

        let error = executor
            .decide_approval(&approval_id, true)
            .await
            .unwrap_err();

        assert_eq!(error.code(), "approval_invalid");
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        let states: (String, String, String) = sqlx::query_as(
            r#"
            SELECT a.status,s.status,r.status
            FROM agent_approvals a
            JOIN agent_steps s ON s.id=a.step_id
            JOIN agent_runs r ON r.id=a.run_id
            WHERE a.id=?
            "#,
        )
        .bind(&approval_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            (states.0.as_str(), states.1.as_str(), states.2.as_str()),
            ("expired", "failed", "failed")
        );
    }
}
