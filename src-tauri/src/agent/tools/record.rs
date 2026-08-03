use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Executor, FromRow, Sqlite, Transaction};
use uuid::Uuid;

use super::{Confirmation, Idempotency, RiskLevel, ToolDescriptor};
use crate::agent::error::AgentError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckinWrongQuestionInput {
    pub question_source: Option<String>,
    pub question_desc: Option<String>,
    pub correct_answer: Option<String>,
    pub my_answer: Option<String>,
    pub error_type: Option<String>,
    pub error_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordCheckinPlanInput {
    pub plan_id: String,
    pub duration_min: i64,
    pub content: Option<String>,
    #[serde(default)]
    pub questions_count: i64,
    #[serde(default)]
    pub correct_count: i64,
    pub mastery_rating: Option<i64>,
    pub difficulty_notes: Option<String>,
    pub mood: Option<i64>,
    pub session_time: Option<String>,
    #[serde(default)]
    pub finish: bool,
    #[serde(default)]
    pub wrong_questions: Vec<CheckinWrongQuestionInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecordCheckinPlanOutput {
    pub record_id: String,
    pub plan_id: String,
    pub date: String,
    pub subject_id: String,
    pub knowledge_point_id: Option<String>,
    pub actual_duration: i64,
    pub actual_tasks: Option<String>,
    pub status: String,
    pub wrong_question_ids: Vec<String>,
}

#[derive(sqlx::FromRow)]
struct CheckinPlanRow {
    id: String,
    date: String,
    subject_id: String,
    knowledge_point_id: Option<String>,
    status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordGetHistoryInput {
    pub exam_id: String,
    #[serde(default = "default_history_limit")]
    pub limit: i64,
}

fn default_history_limit() -> i64 {
    20
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct RecordHistoryRow {
    pub id: String,
    pub date: String,
    pub subject_id: String,
    pub subject_name: Option<String>,
    pub knowledge_point_id: Option<String>,
    pub knowledge_point_name: Option<String>,
    pub duration_min: i64,
    pub content: Option<String>,
    pub questions_count: i64,
    pub correct_count: i64,
    pub mastery_rating: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordGetHistoryOutput {
    pub records: Vec<RecordHistoryRow>,
}

/// `record.get_history` (R0): the most recent study records of an exam with
/// subject/knowledge-point names, newest first.
pub async fn get_history<'e, E>(
    executor: E,
    input: RecordGetHistoryInput,
) -> Result<RecordGetHistoryOutput, AgentError>
where
    E: Executor<'e, Database = Sqlite>,
{
    let records = sqlx::query_as::<_, RecordHistoryRow>(
        r#"
        SELECT
            r.id, r.date, r.subject_id, s.name AS subject_name,
            r.knowledge_point_id, k.name AS knowledge_point_name,
            COALESCE(r.duration_min, 0) AS duration_min, r.content,
            COALESCE(r.questions_count, 0) AS questions_count,
            COALESCE(r.correct_count, 0) AS correct_count,
            r.mastery_rating, r.created_at
        FROM study_records r
        JOIN subjects s ON s.id = r.subject_id
        LEFT JOIN knowledge_points k ON k.id = r.knowledge_point_id
        WHERE s.exam_id = ?
        ORDER BY r.date DESC, r.created_at DESC, r.rowid DESC
        LIMIT ?
        "#,
    )
    .bind(&input.exam_id)
    .bind(input.limit.clamp(1, 100))
    .fetch_all(executor)
    .await
    .map_err(|_| AgentError::Persistence("record.get_history query failed".to_owned()))?;
    Ok(RecordGetHistoryOutput { records })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordCreateFreeInput {
    pub exam_id: String,
    pub date: String,
    pub subject_id: String,
    pub knowledge_point_id: Option<String>,
    pub duration_min: i64,
    pub content: Option<String>,
    pub questions_count: Option<i64>,
    pub correct_count: Option<i64>,
    pub mastery_rating: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordCreateFreeOutput {
    pub id: String,
}

/// `record.create_free` (R1): a free-form study record for a subject of the
/// exam. The subject must belong to the exam; mastery must be 1-5 when set.
pub async fn create_free(
    tx: &mut Transaction<'_, Sqlite>,
    input: RecordCreateFreeInput,
) -> Result<RecordCreateFreeOutput, AgentError> {
    if input.duration_min < 0 {
        return Err(AgentError::ToolSchemaInvalid);
    }
    if let Some(rating) = input.mastery_rating {
        if !(1..=5).contains(&rating) {
            return Err(AgentError::ToolSchemaInvalid);
        }
    }
    let subject_exam: Option<String> =
        sqlx::query_scalar("SELECT exam_id FROM subjects WHERE id = ?")
            .bind(&input.subject_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| {
                AgentError::Persistence("record.create_free subject check failed".to_owned())
            })?;
    match subject_exam {
        None => return Err(AgentError::Persistence("subject not found".to_owned())),
        Some(exam_id) if exam_id != input.exam_id => {
            return Err(AgentError::Persistence("subject not in exam".to_owned()));
        }
        _ => {}
    }
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        r#"
        INSERT INTO study_records
            (id, date, subject_id, knowledge_point_id, duration_min, content,
             questions_count, correct_count, mastery_rating)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&input.date)
    .bind(&input.subject_id)
    .bind(&input.knowledge_point_id)
    .bind(input.duration_min)
    .bind(&input.content)
    .bind(input.questions_count.unwrap_or(0))
    .bind(input.correct_count.unwrap_or(0))
    .bind(input.mastery_rating)
    .execute(&mut **tx)
    .await
    .map_err(|_| AgentError::Persistence("record.create_free insert failed".to_owned()))?;
    Ok(RecordCreateFreeOutput { id })
}

pub fn create_free_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "record.create_free",
        version: "1",
        input_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["exam_id","date","subject_id","duration_min"],
            "properties":{
                "exam_id":{"type":"string","minLength":1},
                "date":{"type":"string","pattern":"^\\d{4}-\\d{2}-\\d{2}$"},
                "subject_id":{"type":"string","minLength":1},
                "knowledge_point_id":{"type":"string"},
                "duration_min":{"type":"integer","minimum":0},
                "content":{"type":"string"},
                "questions_count":{"type":"integer","minimum":0},
                "correct_count":{"type":"integer","minimum":0},
                "mastery_rating":{"type":"integer","minimum":1,"maximum":5}
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
        data_permissions: vec!["study_records:write", "subjects:read"],
    }
}

pub fn get_history_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "record.get_history",
        version: "1",
        input_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["exam_id"],
            "properties":{
                "exam_id":{"type":"string","minLength":1},
                "limit":{"type":"integer","minimum":1,"maximum":100}
            }
        }),
        output_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["records"],
            "properties":{"records":{"type":"array","items":{"type":"object"}}}
        }),
        risk: RiskLevel::R0,
        confirmation: Confirmation::Automatic,
        supports_undo: false,
        timeout_ms: 2000,
        idempotency: Idempotency::RetrySafe,
        data_permissions: vec![
            "study_records:read",
            "subjects:read",
            "knowledge_points:read",
        ],
    }
}

pub async fn checkin_plan(
    tx: &mut Transaction<'_, Sqlite>,
    input: RecordCheckinPlanInput,
    business_date: &str,
    record_id: &str,
) -> Result<RecordCheckinPlanOutput, AgentError> {
    validate_input(&input)?;
    let plan = sqlx::query_as::<_, CheckinPlanRow>(
        r#"
        SELECT id, date, subject_id, knowledge_point_id, status
        FROM study_plans WHERE id = ?
        "#,
    )
    .bind(&input.plan_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_sqlx)?
    .ok_or_else(|| AgentError::NotFound(input.plan_id.clone()))?;

    // ponytail: skipped/future plan is a business-state conflict, not malformed input.
    // Conflict (not persisted as a failed step) lets the run stay running so the
    // caller can recover; schema-level field errors below stay ToolSchemaInvalid.
    if plan.status == "skipped" || plan.date.as_str() > business_date {
        return Err(AgentError::Conflict);
    }
    let wrong_question_ids = input
        .wrong_questions
        .iter()
        .map(|_| Uuid::new_v4().to_string())
        .collect::<Vec<_>>();

    sqlx::query(
        r#"
        INSERT INTO study_records (
            id, plan_id, date, subject_id, knowledge_point_id, duration_min,
            content, questions_count, correct_count, mastery_rating,
            difficulty_notes, mood, session_time
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(record_id)
    .bind(&plan.id)
    .bind(&plan.date)
    .bind(&plan.subject_id)
    .bind(&plan.knowledge_point_id)
    .bind(input.duration_min)
    .bind(&input.content)
    .bind(input.questions_count)
    .bind(input.correct_count)
    .bind(input.mastery_rating)
    .bind(&input.difficulty_notes)
    .bind(input.mood)
    .bind(&input.session_time)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;

    for (wrong, wrong_id) in input.wrong_questions.into_iter().zip(&wrong_question_ids) {
        sqlx::query(
            r#"
            INSERT INTO wrong_questions (
                id, record_id, subject_id, knowledge_point_id, question_source,
                question_desc, correct_answer, my_answer, error_type, error_reason
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(wrong_id)
        .bind(record_id)
        .bind(&plan.subject_id)
        .bind(&plan.knowledge_point_id)
        .bind(wrong.question_source)
        .bind(wrong.question_desc)
        .bind(wrong.correct_answer)
        .bind(wrong.my_answer)
        .bind(wrong.error_type)
        .bind(wrong.error_reason)
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx)?;
    }

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
              WHEN status = 'skipped' THEN status
              WHEN ? = 1 OR status = 'completed' THEN 'completed'
              ELSE 'in_progress'
            END
        WHERE id = ?
        "#,
    )
    .bind(input.finish)
    .bind(&plan.id)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx)?;

    let (actual_duration, actual_tasks, status): (i64, Option<String>, String) = sqlx::query_as(
        "SELECT actual_duration, actual_tasks, status FROM study_plans WHERE id = ?",
    )
    .bind(&plan.id)
    .fetch_one(&mut **tx)
    .await
    .map_err(map_sqlx)?;

    Ok(RecordCheckinPlanOutput {
        record_id: record_id.to_owned(),
        plan_id: plan.id,
        date: plan.date,
        subject_id: plan.subject_id,
        knowledge_point_id: plan.knowledge_point_id,
        actual_duration,
        actual_tasks,
        status,
        wrong_question_ids,
    })
}

fn validate_input(input: &RecordCheckinPlanInput) -> Result<(), AgentError> {
    let valid_rating = |value: Option<i64>| value.is_none_or(|value| (1..=5).contains(&value));
    let valid_session = matches!(
        input.session_time.as_deref(),
        None | Some("morning" | "afternoon" | "evening")
    );
    if input.duration_min <= 0
        || input.questions_count < 0
        || input.correct_count < 0
        || input.correct_count > input.questions_count
        || !valid_rating(input.mastery_rating)
        || !valid_rating(input.mood)
        || !valid_session
    {
        return Err(AgentError::ToolSchemaInvalid);
    }
    Ok(())
}

fn map_sqlx(_error: sqlx::Error) -> AgentError {
    AgentError::Persistence("tool transaction failed".to_owned())
}

pub fn descriptor() -> ToolDescriptor {
    let wrong = json!({"type":"object", "additionalProperties":false, "properties": {
        "question_source":{"type":["string","null"]}, "question_desc":{"type":["string","null"]},
        "correct_answer":{"type":["string","null"]}, "my_answer":{"type":["string","null"]},
        "error_type":{"type":["string","null"]}, "error_reason":{"type":["string","null"]}
    }});
    ToolDescriptor {
        name: "record.checkin_plan",
        version: "1",
        input_schema: json!({"type":"object", "additionalProperties":false, "required":["plan_id","duration_min","finish"], "properties": {
            "plan_id":{"type":"string","minLength":1}, "duration_min":{"type":"integer","minimum":1},
            "content":{"type":["string","null"]}, "questions_count":{"type":"integer","minimum":0,"default":0},
            "correct_count":{"type":"integer","minimum":0,"default":0}, "mastery_rating":{"type":["integer","null"],"minimum":1,"maximum":5},
            "difficulty_notes":{"type":["string","null"]}, "mood":{"type":["integer","null"],"minimum":1,"maximum":5},
            "session_time":{"type":["string","null"],"enum":["morning","afternoon","evening",null]},
            "finish":{"type":"boolean","default":false}, "wrong_questions":{"type":"array","default":[],"items":wrong}
        }}),
        output_schema: json!({"type":"object", "additionalProperties":false,
            "required":["record_id","plan_id","date","subject_id","knowledge_point_id","actual_duration","actual_tasks","status","wrong_question_ids"],
            "properties": {
                "record_id":{"type":"string"}, "plan_id":{"type":"string"}, "date":{"type":"string"},
                "subject_id":{"type":"string"}, "knowledge_point_id":{"type":["string","null"]},
                "actual_duration":{"type":"integer"}, "actual_tasks":{"type":["string","null"]},
                "status":{"type":"string","enum":["in_progress","completed"]},
                "wrong_question_ids":{"type":"array","items":{"type":"string"}}
            }
        }),
        risk: RiskLevel::R1,
        confirmation: Confirmation::Automatic,
        supports_undo: true,
        timeout_ms: 5000,
        idempotency: Idempotency::RequiredExactlyOnce,
        data_permissions: vec![
            "study_plans:read_write",
            "study_records:write",
            "wrong_questions:write",
            "agent_audit:write",
        ],
    }
}
