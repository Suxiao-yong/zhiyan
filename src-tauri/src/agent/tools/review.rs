// review.* tools (learning-effectiveness-loop Task 4): review.get_due lists
// due wrong questions for spaced repetition; review.complete records one
// review outcome and reschedules via the SM-2 lite scheduler. get_due is R0;
// complete is R1 (auto-executed, transactionally undoable).

use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Sqlite, Transaction};

use crate::agent::error::AgentError;
use crate::review::{schedule, ReviewState};

use super::{Confirmation, Idempotency, RiskLevel, ToolDescriptor};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewGetDueInput {
    pub exam_id: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ReviewDueItem {
    pub id: String,
    pub question_desc: Option<String>,
    pub subject_name: String,
    pub knowledge_point_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewGetDueOutput {
    pub count: i64,
    pub items: Vec<ReviewDueItem>,
}

/// Due = not mastered AND never scheduled OR scheduled for today/earlier.
/// NULL `next_review_at` sorts first (never-scheduled beats overdue).
pub async fn get_due(
    tx: &mut Transaction<'_, Sqlite>,
    input: ReviewGetDueInput,
) -> Result<ReviewGetDueOutput, AgentError> {
    let items: Vec<ReviewDueItem> = sqlx::query_as(
        r#"
        SELECT wq.id, wq.question_desc, s.name AS subject_name, kp.name AS knowledge_point_name
        FROM wrong_questions wq
        JOIN subjects s ON s.id = wq.subject_id
        LEFT JOIN knowledge_points kp ON kp.id = wq.knowledge_point_id
        WHERE s.exam_id = ?
          AND wq.mastered = 0
          AND (wq.next_review_at IS NULL OR wq.next_review_at <= date('now','localtime'))
        ORDER BY wq.next_review_at ASC
        LIMIT 20
        "#,
    )
    .bind(&input.exam_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| AgentError::Persistence("review.get_due query failed".to_owned()))?;
    let count = items.len() as i64;
    Ok(ReviewGetDueOutput { count, items })
}

pub fn get_due_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "review.get_due",
        version: "1",
        input_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["exam_id"],
            "properties":{"exam_id":{"type":"string","minLength":1}}
        }),
        output_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["count","items"],
            "properties":{
                "count":{"type":"integer"},
                "items":{"type":"array","items":{
                    "type":"object", "additionalProperties":false,
                    "required":["id","question_desc","subject_name","knowledge_point_name"],
                    "properties":{
                        "id":{"type":"string"},
                        "question_desc":{"type":["string","null"]},
                        "subject_name":{"type":"string"},
                        "knowledge_point_name":{"type":["string","null"]}
                    }
                }}
            }
        }),
        risk: RiskLevel::R0,
        confirmation: Confirmation::Automatic,
        supports_undo: false,
        timeout_ms: 3000,
        idempotency: Idempotency::RetrySafe,
        data_permissions: vec![
            "wrong_questions:read",
            "subjects:read",
            "knowledge_points:read",
        ],
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewCompleteInput {
    pub wrong_question_id: String,
    pub quality: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewCompleteOutput {
    pub interval_days: f64,
    pub ease_factor: f64,
    pub next_review_at: String,
}

pub async fn complete(
    tx: &mut Transaction<'_, Sqlite>,
    input: ReviewCompleteInput,
) -> Result<ReviewCompleteOutput, AgentError> {
    let current: Option<(f64, f64)> = sqlx::query_as(
        "SELECT review_interval_days, ease_factor FROM wrong_questions \
         WHERE id = ? AND mastered = 0",
    )
    .bind(&input.wrong_question_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| AgentError::Persistence("review.complete load failed".to_owned()))?;
    // A missing or already-mastered question must not be rescheduled.
    let Some((interval_days, ease_factor)) = current else {
        return Err(AgentError::Persistence(
            "wrong question not found or mastered".to_owned(),
        ));
    };

    let outcome = schedule(
        ReviewState {
            interval_days,
            ease_factor,
        },
        input.quality,
    );
    let due_days = (outcome.due_in_days as i64).max(1);
    let next_review_at = (chrono::Local::now().date_naive() + chrono::Duration::days(due_days))
        .format("%Y-%m-%d")
        .to_string();

    let updated = sqlx::query(
        r#"
        UPDATE wrong_questions
        SET review_count = review_count + 1,
            last_review_at = datetime('now','localtime'),
            review_interval_days = ?,
            ease_factor = ?,
            next_review_at = ?
        WHERE id = ? AND mastered = 0
        "#,
    )
    .bind(outcome.interval_days)
    .bind(outcome.ease_factor)
    .bind(&next_review_at)
    .bind(&input.wrong_question_id)
    .execute(&mut **tx)
    .await
    .map_err(|_| AgentError::Persistence("review.complete update failed".to_owned()))?;
    if updated.rows_affected() == 0 {
        return Err(AgentError::Persistence(
            "wrong question not found or mastered".to_owned(),
        ));
    }

    Ok(ReviewCompleteOutput {
        interval_days: outcome.interval_days,
        ease_factor: outcome.ease_factor,
        next_review_at,
    })
}

pub fn complete_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "review.complete",
        version: "1",
        input_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["wrong_question_id","quality"],
            "properties":{
                "wrong_question_id":{"type":"string","minLength":1},
                "quality":{"type":"integer","minimum":0,"maximum":5}
            }
        }),
        output_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["interval_days","ease_factor","next_review_at"],
            "properties":{
                "interval_days":{"type":"number"},
                "ease_factor":{"type":"number"},
                "next_review_at":{"type":"string"}
            }
        }),
        risk: RiskLevel::R1,
        confirmation: Confirmation::Automatic,
        supports_undo: true,
        timeout_ms: 3000,
        idempotency: Idempotency::RetrySafe,
        data_permissions: vec!["wrong_questions:write"],
    }
}
