// exam.* tools (M6 Task 1, query only / R0): exam.get_active returns the
// active exam (settings agent_active_exam_id, falling back to the most recent)
// with its subjects, so the agent can scope later queries.

use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{FromRow, Sqlite, Transaction};

use crate::agent::error::AgentError;

use super::{Confirmation, Idempotency, RiskLevel, ToolDescriptor};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExamGetActiveInput {}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ExamSubjectRow {
    pub id: String,
    pub name: String,
    pub weight: f64,
    pub target_score: Option<f64>,
    pub current_level: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExamGetActiveOutput {
    pub exam_id: Option<String>,
    pub exam_name: Option<String>,
    pub exam_date: Option<String>,
    pub subjects: Vec<ExamSubjectRow>,
}

pub async fn get_active(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<ExamGetActiveOutput, AgentError> {
    #[derive(FromRow)]
    struct ExamRow {
        id: String,
        name: String,
        exam_date: Option<String>,
    }
    let configured: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'agent_active_exam_id'")
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| {
                AgentError::Persistence("exam.get_active settings query failed".to_owned())
            })?;
    let exam: Option<ExamRow> = match configured.filter(|value| !value.trim().is_empty()) {
        Some(exam_id) => {
            sqlx::query_as::<_, ExamRow>("SELECT id, name, exam_date FROM exams WHERE id = ?")
                .bind(exam_id)
                .fetch_optional(&mut **tx)
                .await
                .map_err(|_| AgentError::Persistence("exam.get_active query failed".to_owned()))?
        }
        None => sqlx::query_as::<_, ExamRow>(
            "SELECT id, name, exam_date FROM exams ORDER BY updated_at DESC, rowid DESC LIMIT 1",
        )
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| AgentError::Persistence("exam.get_active query failed".to_owned()))?,
    };
    let Some(exam) = exam else {
        return Ok(ExamGetActiveOutput {
            exam_id: None,
            exam_name: None,
            exam_date: None,
            subjects: Vec::new(),
        });
    };
    let subjects = sqlx::query_as::<_, ExamSubjectRow>(
        "SELECT id, name, COALESCE(weight, 0) AS weight, target_score, current_level \
         FROM subjects WHERE exam_id = ? ORDER BY weight DESC, name",
    )
    .bind(&exam.id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| AgentError::Persistence("exam.get_active subjects query failed".to_owned()))?;
    Ok(ExamGetActiveOutput {
        exam_id: Some(exam.id),
        exam_name: Some(exam.name),
        exam_date: exam.exam_date,
        subjects,
    })
}

pub fn descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "exam.get_active",
        version: "1",
        input_schema: json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "exam_id": {"type": ["string","null"]},
                "exam_name": {"type": ["string","null"]},
                "exam_date": {"type": ["string","null"]},
                "subjects": {"type": "array", "items": {"type": "object"}}
            },
            "required": ["exam_id", "exam_name", "exam_date", "subjects"]
        }),
        risk: RiskLevel::R0,
        confirmation: Confirmation::Automatic,
        supports_undo: false,
        timeout_ms: 2000,
        idempotency: Idempotency::RetrySafe,
        data_permissions: vec!["exam:read", "subject:read"],
    }
}
