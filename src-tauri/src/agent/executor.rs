#[cfg(test)]
use std::hash::{Hash, Hasher};

#[cfg(test)]
use chrono::Duration as ChronoDuration;
use chrono::{Local, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::{Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

use super::{
    error::AgentError,
    model::{ApprovalRecord, ToolCallRequest, ToolCallResponse},
    policy::{self, PolicyContext, PolicyDecision},
    tools::{
        plan::{self, PlanGetTodayInput},
        record::{self, RecordCheckinPlanInput, RecordCheckinPlanOutput},
        Idempotency, ListedTool, RiskLevel, ToolDescriptor, ToolOwnership, ToolRegistry,
    },
};

#[cfg(test)]
use super::policy::ApprovalGrant;

const RECORD_CHECKIN_TOOL: &str = "record.checkin_plan";
const RECORD_CHECKIN_VERSION: &str = "1";
const RECORD_CHECKIN_UNDO_KIND: &str = "record.checkin_plan.v1";

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolUndoResponse {
    pub step_id: String,
    pub output: RecordCheckinUndoOutput,
}

#[derive(Debug, sqlx::FromRow)]
struct StoredStep {
    id: String,
    tool_name: String,
    tool_version: String,
    status: String,
    input_json: Option<String>,
    output_json: Option<String>,
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

#[derive(Debug, Serialize, Deserialize)]
struct RecordCheckinUndoReceipt {
    kind: String,
    record_id: String,
    plan_id: String,
    wrong_question_ids: Vec<String>,
}

#[derive(Clone)]
pub struct AgentExecutor {
    pool: SqlitePool,
    registry: ToolRegistry,
    #[cfg(test)]
    test_dispatch_count: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
}

impl AgentExecutor {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            registry: ToolRegistry::built_in(),
            #[cfg(test)]
            test_dispatch_count: None,
        }
    }

    #[cfg(test)]
    fn for_test(
        pool: SqlitePool,
        registry: ToolRegistry,
        dispatch_count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) -> Self {
        Self {
            pool,
            registry,
            test_dispatch_count: Some(dispatch_count),
        }
    }

    pub async fn list_tools(&self) -> Result<Vec<ListedTool>, AgentError> {
        let mut listed = Vec::new();
        for descriptor in self.registry.descriptors() {
            let key = format!("agent_tool_owner.{}", descriptor.name);
            let value: Option<String> =
                sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
                    .bind(key)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(map_sqlx)?;
            let ownership = match value.as_deref() {
                Some("typescript") => ToolOwnership::Typescript,
                Some("shadow") => ToolOwnership::Shadow,
                Some("rust-owned") => ToolOwnership::RustOwned,
                _ => ToolOwnership::Unavailable,
            };
            listed.push(ListedTool {
                descriptor: descriptor.clone(),
                ownership,
            });
        }
        Ok(listed)
    }

    pub async fn execute(&self, request: ToolCallRequest) -> Result<ToolCallResponse, AgentError> {
        let descriptor = self
            .registry
            .get(&request.tool_name, &request.tool_version)?
            .clone();
        self.registry
            .validate_input(&request.tool_name, &request.tool_version, &request.input)?;
        let ownership = self.ownership_for(descriptor.name).await?;
        ensure_executable_ownership(&descriptor, ownership)?;

        match descriptor.name {
            "plan.get_today" => {
                self.execute_plan_get_today(request, &descriptor, ownership)
                    .await
            }
            RECORD_CHECKIN_TOOL => self.execute_generic_checkin(request, &descriptor).await,
            _ => {
                #[cfg(test)]
                if self.test_dispatch_count.is_some() {
                    return self.execute_synthetic(request, &descriptor).await;
                }
                Err(AgentError::ToolNotFound)
            }
        }
    }

    pub async fn decide_approval(
        &self,
        approval_id: &str,
        approve: bool,
    ) -> Result<ApprovalRecord, AgentError> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx)?;
        let result = decide_approval_in_transaction(&mut tx, approval_id, approve).await;
        finish_transaction(tx, result).await
    }

    async fn ownership_for(&self, tool_name: &str) -> Result<ToolOwnership, AgentError> {
        let key = format!("agent_tool_owner.{tool_name}");
        let value: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_sqlx)?;
        Ok(parse_ownership(value.as_deref()))
    }

    async fn execute_plan_get_today(
        &self,
        request: ToolCallRequest,
        descriptor: &ToolDescriptor,
        ownership: ToolOwnership,
    ) -> Result<ToolCallResponse, AgentError> {
        let input: PlanGetTodayInput = serde_json::from_value(request.input.clone())
            .map_err(|_| AgentError::ToolSchemaInvalid)?;
        let input_json = canonical_json(request.input.clone()).to_string();
        let audit_request = request.clone();
        let mut tx = self.pool.begin().await.map_err(map_sqlx)?;
        let result = async {
            if let Some(replay) =
                replay_step(&mut tx, &request, &input_json, descriptor.supports_undo).await?
            {
                return Ok(replay);
            }
            let step_id = reserve_step(&mut tx, &request, descriptor, &input_json).await?;
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
            let decision = policy::decide(PolicyContext {
                risk: descriptor.risk,
                user_allows_r2: false,
                approval: None,
            })?;
            if decision != PolicyDecision::Execute {
                return Err(AgentError::Conflict);
            }
            let business_date = plan::business_date_at(Local::now().fixed_offset());
            let output = tokio::time::timeout(
                std::time::Duration::from_millis(descriptor.timeout_ms),
                plan::get_today(&mut *tx, input, &business_date),
            )
            .await
            .map_err(|_| AgentError::ToolTimeout)??;
            let output = serde_json::to_value(output).map_err(|_| AgentError::ToolSchemaInvalid)?;
            self.registry.validate_output(descriptor, &output)?;
            complete_step(
                &mut tx,
                &request.run_id,
                &step_id,
                descriptor,
                &output,
                json!({
                    "risk": descriptor.risk,
                    "confirmation": descriptor.confirmation,
                    "decision": "execute",
                    "delivery": if ownership == ToolOwnership::Shadow { "shadow" } else { "rust" }
                }),
                Some(json!({
                    "delivery": if ownership == ToolOwnership::Shadow { "shadow" } else { "rust" }
                })),
            )
            .await?;
            Ok(ToolCallResponse::Completed {
                step_id,
                output,
                replayed: false,
                undo_available: false,
            })
        }
        .await;
        finish_tool_transaction(
            &self.pool,
            tx,
            result,
            &audit_request,
            descriptor,
            &input_json,
        )
        .await
    }

    async fn execute_generic_checkin(
        &self,
        request: ToolCallRequest,
        descriptor: &ToolDescriptor,
    ) -> Result<ToolCallResponse, AgentError> {
        let input: RecordCheckinPlanInput =
            serde_json::from_value(request.input).map_err(|_| AgentError::ToolSchemaInvalid)?;
        let key = request
            .idempotency_key
            .as_deref()
            .filter(|key| !key.trim().is_empty())
            .ok_or(AgentError::IdempotencyRequired)?
            .to_owned();
        let input_json = canonical_json(
            serde_json::to_value(&input).map_err(|_| AgentError::ToolSchemaInvalid)?,
        )
        .to_string();
        let execution_request = RecordCheckinExecutionRequest {
            run_id: request.run_id,
            step_index: request.step_index,
            input,
            business_date: plan::business_date_at(Local::now().fixed_offset()),
            idempotency_key: Some(key.clone()),
        };
        let audit_request = ToolCallRequest {
            run_id: execution_request.run_id.clone(),
            step_index: execution_request.step_index,
            tool_name: RECORD_CHECKIN_TOOL.to_owned(),
            tool_version: RECORD_CHECKIN_VERSION.to_owned(),
            input: serde_json::to_value(&execution_request.input)
                .map_err(|_| AgentError::ToolSchemaInvalid)?,
            idempotency_key: execution_request.idempotency_key.clone(),
            approval_id: None,
        };
        let mut tx = self.pool.begin().await.map_err(map_sqlx)?;
        let result = async {
            let response = tokio::time::timeout(
                std::time::Duration::from_millis(descriptor.timeout_ms),
                execute_checkin_in_transaction(&mut tx, execution_request, &key, &input_json, true),
            )
            .await
            .map_err(|_| AgentError::ToolTimeout)??;
            let output = serde_json::to_value(&response.output)
                .map_err(|_| AgentError::ToolSchemaInvalid)?;
            self.registry.validate_output(descriptor, &output)?;
            Ok(ToolCallResponse::Completed {
                step_id: response.step_id,
                output,
                replayed: response.replayed,
                undo_available: response.undo_available,
            })
        }
        .await;
        finish_tool_transaction(
            &self.pool,
            tx,
            result,
            &audit_request,
            descriptor,
            &input_json,
        )
        .await
    }

    #[cfg(test)]
    async fn execute_synthetic(
        &self,
        request: ToolCallRequest,
        descriptor: &ToolDescriptor,
    ) -> Result<ToolCallResponse, AgentError> {
        let input_json = canonical_json(request.input.clone()).to_string();
        let mut tx = self.pool.begin().await.map_err(map_sqlx)?;
        let result = async {
            let stored = sqlx::query_as::<_, StoredStep>(
                r#"
                SELECT id, tool_name, tool_version, status, input_json, output_json, undone_at
                FROM agent_steps WHERE run_id=? AND step_index=?
                "#,
            )
            .bind(&request.run_id)
            .bind(request.step_index)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_sqlx)?;

            let (step_id, existing_status) = if let Some(stored) = stored {
                if stored.tool_name != request.tool_name
                    || stored.tool_version != request.tool_version
                    || stored.input_json.as_deref() != Some(input_json.as_str())
                {
                    return Err(AgentError::IdempotencyConflict);
                }
                if stored.status == "completed" {
                    let output = serde_json::from_str(
                        stored
                            .output_json
                            .as_deref()
                            .ok_or(AgentError::IdempotencyConflict)?,
                    )
                    .map_err(|_| AgentError::IdempotencyConflict)?;
                    return Ok(ToolCallResponse::Completed {
                        step_id: stored.id,
                        output,
                        replayed: true,
                        undo_available: false,
                    });
                }
                (stored.id, Some(stored.status))
            } else {
                let step_id = reserve_step(&mut tx, &request, descriptor, &input_json).await?;
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
                (step_id, None)
            };

            match descriptor.risk {
                RiskLevel::R2 => {
                    let allowed = read_bool_setting(&mut tx, "agent_r2_auto_execute").await?;
                    if policy::decide(PolicyContext {
                        risk: descriptor.risk,
                        user_allows_r2: allowed,
                        approval: None,
                    })? == PolicyDecision::PresentSummary
                    {
                        sqlx::query(
                            "UPDATE agent_steps SET status='pending', policy_json=? WHERE id=?",
                        )
                        .bind(json!({"risk":descriptor.risk,"decision":"summary"}).to_string())
                        .bind(&step_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(map_sqlx)?;
                        return Ok(ToolCallResponse::SummaryRequired {
                            step_id,
                            preview: request.input,
                        });
                    }
                }
                RiskLevel::R3 => {
                    let current_hash = synthetic_precondition_hash(&mut tx, &request.input).await?;
                    if let Some(approval_id) = request.approval_id.as_deref() {
                        let approval = load_approval(&mut tx, approval_id).await?;
                        let expires_at = chrono::DateTime::parse_from_rfc3339(&approval.expires_at)
                            .map_err(|_| AgentError::ApprovalInvalid)?
                            .with_timezone(&Utc);
                        let stored_hash = approval_precondition_hash(&approval)?;
                        let decision = policy::decide(PolicyContext {
                            risk: descriptor.risk,
                            user_allows_r2: false,
                            approval: Some(ApprovalGrant {
                                approval_id: &approval.id,
                                step_id: &approval.step_id,
                                expected_step_id: &step_id,
                                status: &approval.status,
                                expires_at,
                                now: Utc::now(),
                                precondition_hash: &stored_hash,
                                current_precondition_hash: &current_hash,
                            }),
                        })?;
                        if decision != PolicyDecision::Execute {
                            return Err(AgentError::ApprovalInvalid);
                        }
                    } else if existing_status.as_deref() == Some("waiting_approval") {
                        let approval = load_approval_for_step(&mut tx, &step_id).await?;
                        return waiting_response(&approval);
                    } else {
                        let approval = create_pending_approval(
                            &mut tx,
                            &request,
                            descriptor,
                            &step_id,
                            &current_hash,
                        )
                        .await?;
                        return waiting_response(&approval);
                    }
                }
                RiskLevel::R4 => {
                    let decision = policy::decide(PolicyContext {
                        risk: descriptor.risk,
                        user_allows_r2: false,
                        approval: None,
                    })?;
                    if decision == PolicyDecision::NavigateOnly {
                        set_step_running(&mut tx, &step_id).await?;
                        complete_step(
                            &mut tx,
                            &request.run_id,
                            &step_id,
                            descriptor,
                            &json!({"ok":false}),
                            json!({"risk":descriptor.risk,"decision":"navigation"}),
                            None,
                        )
                        .await?;
                        return Ok(ToolCallResponse::NavigationRequired {
                            route: "/settings".to_owned(),
                            reason: "tool_requires_navigation".to_owned(),
                        });
                    }
                }
                _ => return Err(AgentError::ToolNotFound),
            }

            set_step_running(&mut tx, &step_id).await?;
            let counter = self
                .test_dispatch_count
                .as_ref()
                .expect("synthetic dispatcher must exist")
                .clone();
            let source_id = request.input["source_id"]
                .as_str()
                .ok_or(AgentError::ToolSchemaInvalid)?
                .to_owned();
            let output = tokio::time::timeout(
                std::time::Duration::from_millis(descriptor.timeout_ms),
                async {
                    counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    sqlx::query("INSERT INTO synthetic_business(source_id) VALUES(?)")
                        .bind(source_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(map_sqlx)?;
                    Ok::<_, AgentError>(json!({"ok":true}))
                },
            )
            .await
            .map_err(|_| AgentError::ToolTimeout)??;
            self.registry.validate_output(descriptor, &output)?;
            complete_step(
                &mut tx,
                &request.run_id,
                &step_id,
                descriptor,
                &output,
                json!({"risk":descriptor.risk,"decision":"execute"}),
                None,
            )
            .await?;
            Ok(ToolCallResponse::Completed {
                step_id,
                output,
                replayed: false,
                undo_available: false,
            })
        }
        .await;
        finish_transaction(tx, result).await
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
        let input_json = canonical_json(
            serde_json::to_value(&request.input).map_err(|_| AgentError::ToolSchemaInvalid)?,
        )
        .to_string();
        let mut tx = self.pool.begin().await.map_err(map_sqlx)?;
        let result =
            execute_checkin_in_transaction(&mut tx, request, &key, &input_json, false).await;
        finish_transaction(tx, result).await
    }

    pub async fn undo(&self, step_id: &str) -> Result<ToolUndoResponse, AgentError> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx)?;
        let result = undo_in_transaction(&mut tx, step_id).await;
        finish_transaction(tx, result).await
    }
}

async fn execute_checkin_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    request: RecordCheckinExecutionRequest,
    idempotency_key: &str,
    input_json: &str,
    emit_requested: bool,
) -> Result<RecordCheckinExecutionResponse, AgentError> {
    let stored = sqlx::query_as::<_, StoredStep>(
        r#"
        SELECT id, tool_name, tool_version, status, input_json, output_json, undone_at
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

    let baseline_completed: bool = sqlx::query_scalar(
        r#"
        SELECT p.status = 'completed' AND COALESCE((
            SELECT MAX(CASE
                WHEN json_extract(
                    prior.receipt_json,
                    '$.compensation.baseline_completed'
                ) = 1 THEN 1
                ELSE 0
            END)
            FROM agent_steps AS prior
            JOIN study_records AS prior_record
              ON prior_record.id = json_extract(prior.undo_json, '$.record_id')
             AND prior_record.plan_id = p.id
            WHERE prior.id <> ?
              AND prior.tool_name = ?
              AND prior.tool_version = ?
              AND prior.status = 'completed'
              AND prior.undone_at IS NULL
              AND json_extract(prior.undo_json, '$.kind') = ?
              AND json_extract(prior.undo_json, '$.plan_id') = p.id
              AND json_extract(prior.receipt_json, '$.compensation.finish') = 1
        ), 1)
        FROM study_plans AS p
        WHERE p.id = ?
        "#,
    )
    .bind(&step_id)
    .bind(RECORD_CHECKIN_TOOL)
    .bind(RECORD_CHECKIN_VERSION)
    .bind(RECORD_CHECKIN_UNDO_KIND)
    .bind(&request.input.plan_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_sqlx)?
    .unwrap_or(false);
    let record_id = Uuid::new_v4().to_string();
    let finish = request.input.finish;
    let output =
        record::checkin_plan(tx, request.input, &request.business_date, &record_id).await?;
    let output_json = serde_json::to_string(&output).map_err(|_| AgentError::ToolSchemaInvalid)?;
    let undo = RecordCheckinUndoReceipt {
        kind: RECORD_CHECKIN_UNDO_KIND.to_owned(),
        record_id: output.record_id.clone(),
        plan_id: output.plan_id.clone(),
        wrong_question_ids: output.wrong_question_ids.clone(),
    };
    let undo_json = serde_json::to_string(&undo).map_err(|_| AgentError::ToolSchemaInvalid)?;
    let policy_json = json!({
        "risk":"R1",
        "confirmation":"automatic",
        "decision":"execute_with_undo"
    })
    .to_string();
    let receipt_json = json!({
        "compensation": {
            "finish": finish,
            "baseline_completed": baseline_completed
        },
        "undo_result": null
    })
    .to_string();
    let updated = sqlx::query(
        r#"
        UPDATE agent_steps
        SET status='completed', output_json=?, policy_json=?, receipt_json=?, undo_json=?,
            completed_at=datetime('now','localtime')
        WHERE id=? AND status='running'
        "#,
    )
    .bind(&output_json)
    .bind(policy_json)
    .bind(receipt_json)
    .bind(undo_json)
    .bind(&step_id)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    if updated.rows_affected() != 1 {
        return Err(AgentError::Conflict);
    }

    sqlx::query(
        r#"
        INSERT INTO agent_events(run_id, step_id, event_type, payload_json)
        VALUES(?, ?, 'tool.completed', ?)
        "#,
    )
    .bind(&request.run_id)
    .bind(&step_id)
    .bind(
        json!({
            "step_id": step_id,
            "tool_name": RECORD_CHECKIN_TOOL,
            "tool_version": RECORD_CHECKIN_VERSION,
            "risk": "R1",
            "result": "completed"
        })
        .to_string(),
    )
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;

    Ok(RecordCheckinExecutionResponse {
        step_id,
        output,
        replayed: false,
        undo_available: true,
    })
}

async fn undo_in_transaction(
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

    if step.status != "completed"
        || step.tool_name != RECORD_CHECKIN_TOOL
        || step.tool_version != RECORD_CHECKIN_VERSION
        || !record::descriptor().supports_undo
    {
        return Err(AgentError::ToolSchemaInvalid);
    }
    if step.undone_at.is_some() {
        return stored_undo_response(&step);
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
        output,
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

async fn finish_tool_transaction<T>(
    pool: &SqlitePool,
    tx: Transaction<'_, Sqlite>,
    result: Result<T, AgentError>,
    request: &ToolCallRequest,
    descriptor: &ToolDescriptor,
    input_json: &str,
) -> Result<T, AgentError> {
    match result {
        Ok(value) => {
            tx.commit().await.map_err(map_sqlx)?;
            Ok(value)
        }
        Err(error) => {
            tx.rollback().await.map_err(map_sqlx)?;
            if !matches!(
                error,
                AgentError::IdempotencyConflict
                    | AgentError::IdempotencyRequired
                    | AgentError::ApprovalInvalid
            ) {
                persist_failed_attempt(pool, request, descriptor, input_json, error.code()).await?;
            }
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
                error,idempotency_key,started_at,completed_at
            ) VALUES(?,?,?,?,?,?,'failed',?,?,?,datetime('now','localtime'),datetime('now','localtime'))
            "#,
        )
        .bind(&step_id)
        .bind(&request.run_id)
        .bind(request.step_index)
        .bind(descriptor.name)
        .bind(descriptor.version)
        .bind(risk_number(descriptor.risk))
        .bind(input_json)
        .bind(error_code)
        .bind(&request.idempotency_key)
        .execute(&mut *tx)
        .await
        .map_err(map_reservation_error)?;
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
    match &error {
        sqlx::Error::Database(database_error) if database_error.is_unique_violation() => {
            AgentError::IdempotencyConflict
        }
        _ => map_sqlx(error),
    }
}

fn map_sqlx(_error: sqlx::Error) -> AgentError {
    AgentError::Persistence("tool transaction failed".to_owned())
}

fn parse_ownership(value: Option<&str>) -> ToolOwnership {
    match value {
        Some("typescript") => ToolOwnership::Typescript,
        Some("shadow") => ToolOwnership::Shadow,
        Some("rust-owned") => ToolOwnership::RustOwned,
        _ => ToolOwnership::Unavailable,
    }
}

fn ensure_executable_ownership(
    descriptor: &ToolDescriptor,
    ownership: ToolOwnership,
) -> Result<(), AgentError> {
    if ownership == ToolOwnership::Unavailable {
        return Err(AgentError::OwnershipUnavailable);
    }
    if descriptor.name == "plan.get_today"
        && matches!(ownership, ToolOwnership::Shadow | ToolOwnership::RustOwned)
    {
        return Ok(());
    }
    if ownership == ToolOwnership::RustOwned {
        Ok(())
    } else {
        Err(AgentError::OwnershipNotRust)
    }
}

async fn replay_step(
    tx: &mut Transaction<'_, Sqlite>,
    request: &ToolCallRequest,
    input_json: &str,
    supports_undo: bool,
) -> Result<Option<ToolCallResponse>, AgentError> {
    let stored = sqlx::query_as::<_, StoredStep>(
        r#"
        SELECT id, tool_name, tool_version, status, input_json, output_json, undone_at
        FROM agent_steps
        WHERE run_id = ? AND step_index = ?
        "#,
    )
    .bind(&request.run_id)
    .bind(request.step_index)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    let Some(stored) = stored else {
        return Ok(None);
    };
    if stored.tool_name != request.tool_name
        || stored.tool_version != request.tool_version
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
    Ok(Some(ToolCallResponse::Completed {
        step_id: stored.id,
        output,
        replayed: true,
        undo_available: supports_undo && stored.undone_at.is_none(),
    }))
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

async fn complete_step(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    step_id: &str,
    descriptor: &ToolDescriptor,
    output: &Value,
    policy: Value,
    receipt: Option<Value>,
) -> Result<(), AgentError> {
    let updated = sqlx::query(
        "UPDATE agent_steps SET status='completed', output_json=?, policy_json=?, receipt_json=?, completed_at=datetime('now','localtime') WHERE id=? AND status='running'",
    )
    .bind(output.to_string())
    .bind(policy.to_string())
    .bind(receipt.map(|value| value.to_string()))
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

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
async fn synthetic_precondition_hash(
    tx: &mut Transaction<'_, Sqlite>,
    input: &Value,
) -> Result<String, AgentError> {
    let source_id = input["source_id"]
        .as_str()
        .ok_or(AgentError::ToolSchemaInvalid)?;
    let source_value: String = sqlx::query_scalar("SELECT value FROM synthetic_sources WHERE id=?")
        .bind(source_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(map_sqlx)?
        .ok_or_else(|| AgentError::NotFound(source_id.to_owned()))?;
    let canonical = canonical_json(json!({"id":source_id,"value":source_value})).to_string();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    canonical.hash(&mut hasher);
    Ok(format!("{:016x}", hasher.finish()))
}

#[cfg(test)]
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

#[cfg(test)]
async fn create_pending_approval(
    tx: &mut Transaction<'_, Sqlite>,
    request: &ToolCallRequest,
    descriptor: &ToolDescriptor,
    step_id: &str,
    precondition_hash: &str,
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
    .bind(request.input.to_string())
    .bind(json!({"hash":precondition_hash}).to_string())
    .bind(&expires_at)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;
    sqlx::query("UPDATE agent_steps SET status='waiting_approval', policy_json=? WHERE id=? AND status='running'")
        .bind(json!({"risk":descriptor.risk,"decision":"waiting_approval"}).to_string())
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

#[cfg(test)]
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
        atomic::{AtomicUsize, Ordering},
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
            idempotency: Idempotency::NoAutomaticRetry,
            data_permissions: vec!["synthetic:write"],
        }
    }

    async fn setup(risk: RiskLevel) -> (AgentExecutor, sqlx::SqlitePool, Arc<AtomicUsize>, String) {
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
            counter.clone(),
        );
        (executor, pool, counter, name.to_owned())
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
        for case in ["expired", "stale", "rejected"] {
            let (executor, pool, counter, name) = setup(RiskLevel::R3).await;
            let ToolCallResponse::WaitingApproval { approval_id, .. } =
                executor.execute(request(&name, None)).await.unwrap()
            else {
                panic!("R3 must wait")
            };

            match case {
                "expired" => {
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
                .execute(request(&name, Some(approval_id)))
                .await
                .unwrap_err();
            assert_eq!(error.code(), "approval_invalid", "case={case}");
            assert_eq!(counter.load(Ordering::SeqCst), 0, "case={case}");
        }
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
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_steps")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
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
}
