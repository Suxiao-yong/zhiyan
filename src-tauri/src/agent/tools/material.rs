// material.create tool (v0.3.0 Task 3, R3 write): stores pasted text as a
// study material for a subject. No undo support; retry-safe idempotent.

use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

use crate::agent::error::AgentError;

use super::{Confirmation, Idempotency, RiskLevel, ToolDescriptor};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialCreateInput {
    pub exam_id: Option<String>,
    pub subject_id: String,
    pub title: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaterialCreateOutput {
    pub material_id: String,
    pub title: String,
    pub char_count: i64,
}

/// `material.create` (R3): persists a titled text body under a subject. The
/// subject must exist; when an explicit exam_id is supplied it must match the
/// subject's owning exam (`record.create_free` pattern). The content itself is
/// never echoed back — the output carries only id/title/char_count.
pub async fn create(
    tx: &mut Transaction<'_, Sqlite>,
    input: MaterialCreateInput,
) -> Result<MaterialCreateOutput, AgentError> {
    let subject_exam: Option<String> =
        sqlx::query_scalar("SELECT exam_id FROM subjects WHERE id = ?")
            .bind(&input.subject_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| {
                AgentError::Persistence("material.create subject check failed".to_owned())
            })?;
    let Some(exam_id) = subject_exam else {
        return Err(AgentError::Persistence("subject not found".to_owned()));
    };
    if input
        .exam_id
        .as_ref()
        .is_some_and(|expected| *expected != exam_id)
    {
        return Err(AgentError::Persistence("subject not in exam".to_owned()));
    }
    let char_count = input.content.chars().count() as i64;
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        r#"
        INSERT INTO materials (id, exam_id, subject_id, title, content)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&exam_id)
    .bind(&input.subject_id)
    .bind(&input.title)
    .bind(&input.content)
    .execute(&mut **tx)
    .await
    .map_err(|_| AgentError::Persistence("material.create insert failed".to_owned()))?;
    Ok(MaterialCreateOutput {
        material_id: id,
        title: input.title,
        char_count,
    })
}

pub fn create_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "material.create",
        version: "1",
        input_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["subject_id","title","content"],
            "properties":{
                "exam_id":{"type":"string","minLength":1},
                "subject_id":{"type":"string","minLength":1},
                "title":{"type":"string","minLength":1,"maxLength":200},
                "content":{"type":"string","minLength":1,"maxLength":50000}
            }
        }),
        output_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["material_id","title","char_count"],
            "properties":{
                "material_id":{"type":"string"},
                "title":{"type":"string"},
                "char_count":{"type":"integer","minimum":0}
            }
        }),
        risk: RiskLevel::R3,
        confirmation: Confirmation::Required,
        supports_undo: false,
        timeout_ms: 3000,
        idempotency: Idempotency::RetrySafe,
        data_permissions: vec!["materials:write", "subjects:read"],
    }
}
