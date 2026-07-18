use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::{Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

use super::{
    error::AgentError,
    tools::record::{self, RecordCheckinPlanInput, RecordCheckinPlanOutput},
};

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
}

impl AgentExecutor {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
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
        let result = execute_checkin_in_transaction(&mut tx, request, &key, &input_json).await;
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

    let baseline_completed: bool = sqlx::query_scalar(
        r#"
        SELECT p.status = 'completed' AND NOT EXISTS(
            SELECT 1
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
        )
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
    let policy_json = json!({"risk":"R1","confirmation":"automatic"}).to_string();
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
