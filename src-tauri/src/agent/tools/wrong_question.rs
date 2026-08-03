// wrong_question.* tools (M6 Task 2, R1 writes): wrong_question.create logs a
// wrong question (optionally linked to a study record); mark_mastered flips
// the mastered flag and bumps review_count. Both are retry-safe idempotent.

use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

use crate::agent::error::AgentError;

use super::{Confirmation, Idempotency, RiskLevel, ToolDescriptor};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrongQuestionCreateInput {
    pub subject_id: String,
    pub record_id: Option<String>,
    pub knowledge_point_id: Option<String>,
    pub question_source: Option<String>,
    pub question_desc: String,
    pub correct_answer: Option<String>,
    pub my_answer: Option<String>,
    pub error_type: Option<String>,
    pub error_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WrongQuestionCreateOutput {
    pub id: String,
}

pub async fn create(
    tx: &mut Transaction<'_, Sqlite>,
    input: WrongQuestionCreateInput,
) -> Result<WrongQuestionCreateOutput, AgentError> {
    if let Some(record_id) = &input.record_id {
        let record_exists: Option<i64> =
            sqlx::query_scalar("SELECT COUNT(*) FROM study_records WHERE id = ?")
                .bind(record_id)
                .fetch_one(&mut **tx)
                .await
                .map_err(|_| {
                    AgentError::Persistence("wrong_question.create record check failed".to_owned())
                })?;
        if record_exists.unwrap_or(0) == 0 {
            return Err(AgentError::Persistence("record not found".to_owned()));
        }
    }
    let subject_exists: Option<i64> =
        sqlx::query_scalar("SELECT COUNT(*) FROM subjects WHERE id = ?")
            .bind(&input.subject_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|_| {
                AgentError::Persistence("wrong_question.create subject check failed".to_owned())
            })?;
    if subject_exists.unwrap_or(0) == 0 {
        return Err(AgentError::Persistence("subject not found".to_owned()));
    }
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        r#"
        INSERT INTO wrong_questions
            (id, record_id, subject_id, knowledge_point_id, question_source,
             question_desc, correct_answer, my_answer, error_type, error_reason)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&input.record_id)
    .bind(&input.subject_id)
    .bind(&input.knowledge_point_id)
    .bind(&input.question_source)
    .bind(&input.question_desc)
    .bind(&input.correct_answer)
    .bind(&input.my_answer)
    .bind(&input.error_type)
    .bind(&input.error_reason)
    .execute(&mut **tx)
    .await
    .map_err(|_| AgentError::Persistence("wrong_question.create insert failed".to_owned()))?;
    Ok(WrongQuestionCreateOutput { id })
}

pub fn create_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "wrong_question.create",
        version: "1",
        input_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["subject_id","question_desc"],
            "properties":{
                "subject_id":{"type":"string","minLength":1},
                "record_id":{"type":"string"},
                "knowledge_point_id":{"type":"string"},
                "question_source":{"type":"string"},
                "question_desc":{"type":"string","minLength":1},
                "correct_answer":{"type":"string"},
                "my_answer":{"type":"string"},
                "error_type":{"type":"string"},
                "error_reason":{"type":"string"}
            }
        }),
        output_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["id"],
            "properties":{"id":{"type":"string"}}
        }),
        risk: RiskLevel::R1,
        confirmation: Confirmation::Automatic,
        supports_undo: false,
        timeout_ms: 3000,
        idempotency: Idempotency::RetrySafe,
        data_permissions: vec![
            "wrong_questions:write",
            "study_records:read",
            "subjects:read",
        ],
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrongQuestionMarkMasteredInput {
    pub id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WrongQuestionMarkMasteredOutput {
    pub id: String,
    pub mastered: i64,
}

pub async fn mark_mastered(
    tx: &mut Transaction<'_, Sqlite>,
    input: WrongQuestionMarkMasteredInput,
) -> Result<WrongQuestionMarkMasteredOutput, AgentError> {
    let result = sqlx::query(
        "UPDATE wrong_questions \
         SET mastered = 1, review_count = review_count + 1, last_review_at = datetime('now','localtime') \
         WHERE id = ?",
    )
    .bind(&input.id)
    .execute(&mut **tx)
    .await
    .map_err(|_| AgentError::Persistence("wrong_question.mark_mastered update failed".to_owned()))?;
    if result.rows_affected() == 0 {
        return Err(AgentError::Persistence(
            "wrong question not found".to_owned(),
        ));
    }
    Ok(WrongQuestionMarkMasteredOutput {
        id: input.id,
        mastered: 1,
    })
}

pub fn mark_mastered_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "wrong_question.mark_mastered",
        version: "1",
        input_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["id"],
            "properties":{"id":{"type":"string","minLength":1}}
        }),
        output_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["id","mastered"],
            "properties":{"id":{"type":"string"},"mastered":{"type":"integer"}}
        }),
        risk: RiskLevel::R1,
        confirmation: Confirmation::Automatic,
        supports_undo: false,
        timeout_ms: 3000,
        idempotency: Idempotency::RetrySafe,
        data_permissions: vec!["wrong_questions:write"],
    }
}
