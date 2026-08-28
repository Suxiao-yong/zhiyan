// flashcard.* tools (v0.3.0 Task 5): flashcard.create_batch bulk-imports
// cards under a subject (R3); flashcard.get_due lists due cards with the same
// due semantics as review.get_due (R0); flashcard.complete records one SM-2
// review outcome via the shared crate::review::schedule pure function and is
// transactionally undoable (R1).

use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

use crate::agent::error::AgentError;
use crate::review::{schedule, ReviewState};

use super::{Confirmation, Idempotency, RiskLevel, ToolDescriptor};

const MAX_BATCH: usize = 30;
const MAX_FIELD: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewFlashcard {
    pub front: String,
    pub back: String,
    pub knowledge_point_id: Option<String>,
    pub material_id: Option<String>,
    pub source_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlashcardCreateBatchInput {
    pub exam_id: Option<String>,
    pub subject_id: String,
    pub cards: Vec<NewFlashcard>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreatedFlashcard {
    pub id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlashcardCreateBatchOutput {
    pub created: Vec<CreatedFlashcard>,
    pub count: i64,
}

/// source_ref must be `§N` or `§N-§M` (e.g. `§1`, `§3-§5`).
fn valid_source_ref(value: &str) -> bool {
    static SOURCE_REF: OnceLock<Regex> = OnceLock::new();
    SOURCE_REF
        .get_or_init(|| Regex::new(r"^§\d+(-§\d+)?$").unwrap())
        .is_match(value)
}

fn validate_batch(cards: &[NewFlashcard]) -> Result<(), AgentError> {
    if cards.is_empty() || cards.len() > MAX_BATCH {
        return Err(AgentError::ToolSchemaInvalid);
    }
    for card in cards {
        for field in [&card.front, &card.back] {
            let len = field.chars().count();
            // 执行层兜底：schema 校验在 dispatcher 层，直调本函数（如测试）也必须拦住超长字段。
            if len == 0 || len > MAX_FIELD {
                return Err(AgentError::ToolSchemaInvalid);
            }
        }
        if card
            .source_ref
            .as_ref()
            .is_some_and(|value| !valid_source_ref(value))
        {
            return Err(AgentError::ToolSchemaInvalid);
        }
    }
    Ok(())
}

/// `flashcard.create_batch` (R3): persists a batch of flashcards. The subject
/// must exist; an explicit exam_id must match (`material.create` pattern).
/// Each card's knowledge point / material, when given, must exist and belong
/// to the same subject so provenance can never cross subjects.
pub async fn create_batch(
    tx: &mut Transaction<'_, Sqlite>,
    input: FlashcardCreateBatchInput,
) -> Result<FlashcardCreateBatchOutput, AgentError> {
    validate_batch(&input.cards)?;
    let subject_exam: Option<String> =
        sqlx::query_scalar("SELECT exam_id FROM subjects WHERE id = ?")
            .bind(&input.subject_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| {
                AgentError::Persistence("flashcard.create_batch subject check failed".to_owned())
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

    for card in &input.cards {
        if let Some(kp_id) = &card.knowledge_point_id {
            let kp_subject: Option<String> =
                sqlx::query_scalar("SELECT subject_id FROM knowledge_points WHERE id = ?")
                    .bind(kp_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(|_| {
                        AgentError::Persistence(
                            "flashcard.create_batch knowledge point check failed".to_owned(),
                        )
                    })?;
            match kp_subject {
                Some(subject_id) if subject_id == input.subject_id => {}
                Some(_) => {
                    return Err(AgentError::Persistence(
                        "knowledge point not in subject".to_owned(),
                    ))
                }
                None => {
                    return Err(AgentError::Persistence(
                        "knowledge point not found".to_owned(),
                    ))
                }
            }
        }
        if let Some(material_id) = &card.material_id {
            let material_subject: Option<String> =
                sqlx::query_scalar("SELECT subject_id FROM materials WHERE id = ?")
                    .bind(material_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(|_| {
                        AgentError::Persistence(
                            "flashcard.create_batch material check failed".to_owned(),
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
    }

    let mut created = Vec::with_capacity(input.cards.len());
    for card in &input.cards {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            r#"
            INSERT INTO flashcards (id, subject_id, knowledge_point_id, material_id, source_ref, front, back)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(&input.subject_id)
        .bind(&card.knowledge_point_id)
        .bind(&card.material_id)
        .bind(&card.source_ref)
        .bind(&card.front)
        .bind(&card.back)
        .execute(&mut **tx)
        .await
        .map_err(|_| AgentError::Persistence("flashcard.create_batch insert failed".to_owned()))?;
        created.push(CreatedFlashcard { id });
    }
    Ok(FlashcardCreateBatchOutput {
        count: created.len() as i64,
        created,
    })
}

pub fn create_batch_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "flashcard.create_batch",
        version: "1",
        input_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["subject_id","cards"],
            "properties":{
                "exam_id":{"type":"string","minLength":1},
                "subject_id":{"type":"string","minLength":1},
                "cards":{
                    "type":"array","minItems":1,"maxItems":30,
                    "items":{
                        "type":"object", "additionalProperties":false,
                        "required":["front","back"],
                        "properties":{
                            "front":{"type":"string","minLength":1,"maxLength":500},
                            "back":{"type":"string","minLength":1,"maxLength":500},
                            "knowledge_point_id":{"type":["string","null"],"minLength":1},
                            "material_id":{"type":["string","null"],"minLength":1},
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
                        "required":["id"],
                        "properties":{"id":{"type":"string"}}
                    }
                }
            }
        }),
        risk: RiskLevel::R3,
        confirmation: Confirmation::Required,
        supports_undo: false,
        timeout_ms: 3000,
        idempotency: Idempotency::RetrySafe,
        data_permissions: vec!["flashcards:write", "subjects:read"],
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashcardGetDueInput {
    pub exam_id: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct FlashcardDueItem {
    pub id: String,
    pub front: String,
    pub source_ref: Option<String>,
    pub knowledge_point_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlashcardGetDueOutput {
    pub count: i64,
    pub items: Vec<FlashcardDueItem>,
}

/// Due = not mastered AND never scheduled OR scheduled for today/earlier.
/// NULL `next_review_at` sorts first (never-scheduled beats overdue).
/// Same WHERE as `review.get_due`, scoped to flashcards.
pub async fn get_due(
    tx: &mut Transaction<'_, Sqlite>,
    input: FlashcardGetDueInput,
) -> Result<FlashcardGetDueOutput, AgentError> {
    let items = due_items(&mut **tx, &input.exam_id).await?;
    let count = items.len() as i64;
    Ok(FlashcardGetDueOutput { count, items })
}

/// Shared due-cards query: one SQL for the tool, the Tauri command, and the
/// scheduler reminder count.
async fn due_items<'e, E>(executor: E, exam_id: &str) -> Result<Vec<FlashcardDueItem>, AgentError>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    sqlx::query_as(
        r#"
        SELECT f.id, f.front, f.source_ref, kp.name AS knowledge_point_name
        FROM flashcards f
        JOIN subjects s ON s.id = f.subject_id
        LEFT JOIN knowledge_points kp ON kp.id = f.knowledge_point_id
        WHERE s.exam_id = ?
          AND f.mastered = 0
          AND (f.next_review_at IS NULL OR f.next_review_at <= date('now','localtime'))
        ORDER BY f.next_review_at ASC
        LIMIT 20
        "#,
    )
    .bind(exam_id)
    .fetch_all(executor)
    .await
    .map_err(|_| AgentError::Persistence("flashcard.get_due query failed".to_owned()))
}

/// Pool-level read for the future `flashcard_list_due` Tauri command.
pub async fn list_due(
    pool: &sqlx::SqlitePool,
    exam_id: &str,
) -> Result<FlashcardGetDueOutput, AgentError> {
    let items = due_items(pool, exam_id).await?;
    let count = items.len() as i64;
    Ok(FlashcardGetDueOutput { count, items })
}

/// Pool-level due count for task reminders. Same WHERE as `due_items` but
/// uncapped (the tool list caps at 20; the count does not).
pub async fn count_due(pool: &sqlx::SqlitePool, exam_id: &str) -> Result<i64, AgentError> {
    let count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM flashcards f
        JOIN subjects s ON s.id = f.subject_id
        WHERE s.exam_id = ?
          AND f.mastered = 0
          AND (f.next_review_at IS NULL OR f.next_review_at <= date('now','localtime'))
        "#,
    )
    .bind(exam_id)
    .fetch_one(pool)
    .await
    .map_err(|_| AgentError::Persistence("flashcard count query failed".to_owned()))?;
    Ok(count)
}

pub fn get_due_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "flashcard.get_due",
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
                    "required":["id","front","source_ref","knowledge_point_name"],
                    "properties":{
                        "id":{"type":"string"},
                        "front":{"type":"string"},
                        "source_ref":{"type":["string","null"]},
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
        data_permissions: vec!["flashcards:read"],
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashcardCompleteInput {
    pub flashcard_id: String,
    pub quality: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlashcardCompleteOutput {
    pub interval_days: f64,
    pub ease_factor: f64,
    pub next_review_at: String,
}

/// `flashcard.complete` (R1): reschedules one card through the shared SM-2
/// lite scheduler and masters it on quality=5. A missing or already-mastered
/// card must not be rescheduled. The date math matches `review.complete`.
pub async fn complete(
    tx: &mut Transaction<'_, Sqlite>,
    input: FlashcardCompleteInput,
) -> Result<FlashcardCompleteOutput, AgentError> {
    let current: Option<(f64, f64)> = sqlx::query_as(
        "SELECT review_interval_days, ease_factor FROM flashcards \
         WHERE id = ? AND mastered = 0",
    )
    .bind(&input.flashcard_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| AgentError::Persistence("flashcard.complete load failed".to_owned()))?;
    let Some((interval_days, ease_factor)) = current else {
        return Err(AgentError::Persistence(
            "flashcard not found or mastered".to_owned(),
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
        UPDATE flashcards
        SET review_count = review_count + 1,
            last_review_at = datetime('now','localtime'),
            review_interval_days = ?,
            ease_factor = ?,
            next_review_at = ?,
            mastered = CASE WHEN ? THEN 1 ELSE mastered END
        WHERE id = ? AND mastered = 0
        "#,
    )
    .bind(outcome.interval_days)
    .bind(outcome.ease_factor)
    .bind(&next_review_at)
    .bind(input.quality == 5)
    .bind(&input.flashcard_id)
    .execute(&mut **tx)
    .await
    .map_err(|_| AgentError::Persistence("flashcard.complete update failed".to_owned()))?;
    if updated.rows_affected() == 0 {
        return Err(AgentError::Persistence(
            "flashcard not found or mastered".to_owned(),
        ));
    }

    Ok(FlashcardCompleteOutput {
        interval_days: outcome.interval_days,
        ease_factor: outcome.ease_factor,
        next_review_at,
    })
}

pub fn complete_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "flashcard.complete",
        version: "1",
        input_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["flashcard_id","quality"],
            "properties":{
                "flashcard_id":{"type":"string","minLength":1},
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
        data_permissions: vec!["flashcards:write"],
    }
}
