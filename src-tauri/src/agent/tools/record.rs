use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Sqlite, Transaction};
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

    if plan.status == "skipped" || plan.date.as_str() > business_date {
        return Err(AgentError::ToolSchemaInvalid);
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
