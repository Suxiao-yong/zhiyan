// knowledge_point.create_batch tool (v0.3.0 Task 4, R3 write): bulk-creates
// knowledge points for a subject, resolving intra-batch parent references by
// declaration order and optionally tagging each point with a source material
// + section ref. No undo support; retry-safe idempotent.

use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

use crate::agent::error::AgentError;

use super::{Confirmation, Idempotency, RiskLevel, ToolDescriptor};

const MAX_BATCH: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchConcept {
    pub name: String,
    pub parent_name: Option<String>,
    pub source_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgePointCreateBatchInput {
    pub exam_id: Option<String>,
    pub subject_id: String,
    #[serde(default)]
    pub material_id: Option<String>,
    pub concepts: Vec<BatchConcept>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreatedKnowledgePoint {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KnowledgePointCreateBatchOutput {
    pub created: Vec<CreatedKnowledgePoint>,
    pub count: i64,
}

/// source_ref must be `§N` or `§N-§M` (e.g. `§1`, `§3-§5`).
fn valid_source_ref(value: &str) -> bool {
    static SOURCE_REF: OnceLock<Regex> = OnceLock::new();
    SOURCE_REF
        .get_or_init(|| Regex::new(r"^§\d+(-§\d+)?$").unwrap())
        .is_match(value)
}

fn validate_batch(concepts: &[BatchConcept]) -> Result<(), AgentError> {
    if concepts.is_empty() || concepts.len() > MAX_BATCH {
        return Err(AgentError::ToolSchemaInvalid);
    }
    for concept in concepts {
        let len = concept.name.chars().count();
        if len == 0 || len > 100 {
            return Err(AgentError::ToolSchemaInvalid);
        }
        // 执行层兜底：schema 校验在 dispatcher 层，直调本函数（如测试）也必须拦住非法 source_ref。
        if concept
            .source_ref
            .as_ref()
            .is_some_and(|value| !valid_source_ref(value))
        {
            return Err(AgentError::ToolSchemaInvalid);
        }
    }
    Ok(())
}

/// `knowledge_point.create_batch` (R3): persists a batch of knowledge points.
/// The subject must exist; when an explicit exam_id is supplied it must match
/// the subject's owning exam (`material.create` pattern). When material_id is
/// supplied it must exist and belong to the same subject. A `parent_name`
/// resolves to an earlier entry in this batch (first same-named entry wins),
/// falling back to an existing knowledge point of this subject; anything else
/// is rejected so a typo can never silently orphan a subtree.
pub async fn create_batch(
    tx: &mut Transaction<'_, Sqlite>,
    input: KnowledgePointCreateBatchInput,
) -> Result<KnowledgePointCreateBatchOutput, AgentError> {
    validate_batch(&input.concepts)?;
    let subject_exam: Option<String> =
        sqlx::query_scalar("SELECT exam_id FROM subjects WHERE id = ?")
            .bind(&input.subject_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| {
                AgentError::Persistence(
                    "knowledge_point.create_batch subject check failed".to_owned(),
                )
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
    // The material, when given, must exist and belong to the same subject.
    if let Some(material_id) = &input.material_id {
        let material_subject: Option<String> =
            sqlx::query_scalar("SELECT subject_id FROM materials WHERE id = ?")
                .bind(material_id)
                .fetch_optional(&mut **tx)
                .await
                .map_err(|_| {
                    AgentError::Persistence(
                        "knowledge_point.create_batch material check failed".to_owned(),
                    )
                })?;
        match material_subject {
            Some(subject_id) if subject_id == input.subject_id => {}
            Some(_) => {
                return Err(AgentError::Persistence(
                    "material not in subject".to_owned(),
                ))
            }
            None => return Err(AgentError::Persistence("material not found".to_owned())),
        }
    }

    // Intra-batch parent map: first same-named earlier entry wins; later
    // duplicates are inserted but never referenced as parents.
    let mut batch_ids: Vec<(String, String)> = Vec::with_capacity(input.concepts.len());
    let mut created = Vec::with_capacity(input.concepts.len());
    for concept in &input.concepts {
        let parent_id = match &concept.parent_name {
            None => None,
            Some(parent_name) => {
                if let Some((_, id)) = batch_ids.iter().find(|(name, _)| name == parent_name) {
                    Some(id.clone())
                } else {
                    sqlx::query_scalar(
                        "SELECT id FROM knowledge_points WHERE subject_id = ? AND name = ? ORDER BY rowid LIMIT 1",
                    )
                    .bind(&input.subject_id)
                    .bind(parent_name)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(|_| {
                        AgentError::Persistence(
                            "knowledge_point.create_batch parent lookup failed".to_owned(),
                        )
                    })?
                    .ok_or_else(|| {
                        AgentError::Persistence("parent knowledge point not found".to_owned())
                    })?
                }
            }
        };
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            r#"
            INSERT INTO knowledge_points (id, subject_id, name, parent_id, sort_order, material_id, source_ref)
            VALUES (?, ?, ?, ?, 0, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(&input.subject_id)
        .bind(&concept.name)
        .bind(&parent_id)
        .bind(&input.material_id)
        .bind(&concept.source_ref)
        .execute(&mut **tx)
        .await
        .map_err(|_| AgentError::Persistence("knowledge_point.create_batch insert failed".to_owned()))?;
        created.push(CreatedKnowledgePoint {
            id: id.clone(),
            name: concept.name.clone(),
            parent_id: parent_id.clone(),
        });
        batch_ids.push((concept.name.clone(), id));
    }
    Ok(KnowledgePointCreateBatchOutput {
        count: created.len() as i64,
        created,
    })
}

pub fn create_batch_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "knowledge_point.create_batch",
        version: "1",
        input_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["subject_id","concepts"],
            "properties":{
                "exam_id":{"type":"string","minLength":1},
                "subject_id":{"type":"string","minLength":1},
                "material_id":{"type":"string","minLength":1},
                "concepts":{
                    "type":"array","minItems":1,"maxItems":50,
                    "items":{
                        "type":"object", "additionalProperties":false,
                        "required":["name"],
                        "properties":{
                            "name":{"type":"string","minLength":1,"maxLength":100},
                            "parent_name":{"type":["string","null"],"minLength":1},
                            "source_ref":{"type":"string","pattern":"^§\\d+(-§\\d+)?$"}
                        }
                    }
                }
            }
        }),
        output_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["created","count"],
            "properties":{
                "count":{"type":"integer","minimum":1},
                "created":{
                    "type":"array",
                    "items":{
                        "type":"object", "additionalProperties":false,
                        "required":["id","name"],
                        "properties":{
                            "id":{"type":"string"},
                            "name":{"type":"string"},
                            "parent_id":{"type":["string","null"]}
                        }
                    }
                }
            }
        }),
        risk: RiskLevel::R3,
        confirmation: Confirmation::Required,
        supports_undo: false,
        timeout_ms: 3000,
        idempotency: Idempotency::RetrySafe,
        data_permissions: vec!["knowledge_points:write", "subjects:read"],
    }
}
