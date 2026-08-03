use chrono::{DateTime, Duration, FixedOffset, Timelike};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Executor, FromRow, Sqlite};

use crate::agent::error::AgentError;

use super::{Confirmation, Idempotency, RiskLevel, ToolDescriptor};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanGetTodayInput {
    pub exam_id: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PlanWithNames {
    pub id: String,
    pub exam_id: String,
    pub subject_id: String,
    pub knowledge_point_id: Option<String>,
    pub date: String,
    pub planned_tasks: Option<String>,
    pub planned_duration: Option<i64>,
    pub actual_duration: Option<i64>,
    pub actual_tasks: Option<String>,
    pub status: String,
    pub generated_by: String,
    pub ai_suggestion: Option<String>,
    pub user_modified: i64,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
    pub subject_name: Option<String>,
    pub knowledge_point_name: Option<String>,
    pub record_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanGetTodayOutput {
    pub business_date: String,
    pub plans: Vec<PlanWithNames>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanGetRangeInput {
    pub exam_id: String,
    pub start_date: String,
    pub end_date: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanGetRangeOutput {
    pub start_date: String,
    pub end_date: String,
    pub plans: Vec<PlanWithNames>,
}

/// `plan.get_range` lists plans in a date interval (inclusive).
pub async fn get_range<'e, E>(
    executor: E,
    input: PlanGetRangeInput,
) -> Result<PlanGetRangeOutput, AgentError>
where
    E: Executor<'e, Database = Sqlite>,
{
    if input.start_date > input.end_date {
        return Err(AgentError::ToolSchemaInvalid);
    }
    let plans = sqlx::query_as::<_, PlanWithNames>(
        r#"
        SELECT
            p.id, p.exam_id, p.subject_id, p.knowledge_point_id, p.date,
            p.planned_tasks, p.planned_duration, p.actual_duration, p.actual_tasks,
            p.status, p.generated_by, p.ai_suggestion, p.user_modified, p.sort_order,
            p.created_at, p.updated_at,
            s.name AS subject_name, k.name AS knowledge_point_name,
            COUNT(r.id) AS record_count
        FROM study_plans p
        LEFT JOIN subjects s ON s.id = p.subject_id
        LEFT JOIN knowledge_points k ON k.id = p.knowledge_point_id
        LEFT JOIN study_records r ON r.plan_id = p.id
        WHERE p.exam_id = ? AND p.date BETWEEN ? AND ?
        GROUP BY p.id
        ORDER BY p.date, p.sort_order, p.created_at
        "#,
    )
    .bind(&input.exam_id)
    .bind(&input.start_date)
    .bind(&input.end_date)
    .fetch_all(executor)
    .await
    .map_err(|_| AgentError::Persistence("plan.get_range query failed".to_owned()))?;
    Ok(PlanGetRangeOutput {
        start_date: input.start_date,
        end_date: input.end_date,
        plans,
    })
}

pub fn business_date_at(now: DateTime<FixedOffset>) -> String {
    let date = if now.hour() < 4 {
        now.date_naive() - Duration::days(1)
    } else {
        now.date_naive()
    };
    date.format("%Y-%m-%d").to_string()
}

pub async fn get_today<'e, E>(
    executor: E,
    input: PlanGetTodayInput,
    business_date: &str,
) -> Result<PlanGetTodayOutput, AgentError>
where
    E: Executor<'e, Database = Sqlite>,
{
    let plans = sqlx::query_as::<_, PlanWithNames>(
        r#"
        SELECT
            p.id,
            p.exam_id,
            p.subject_id,
            p.knowledge_point_id,
            p.date,
            p.planned_tasks,
            p.planned_duration,
            CASE
                WHEN COUNT(r.id) > 0 THEN COALESCE(SUM(r.duration_min), 0)
                ELSE p.actual_duration
            END AS actual_duration,
            CASE
                WHEN COUNT(r.id) > 0 THEN COALESCE(
                    (
                        SELECT latest.content
                        FROM study_records latest
                        WHERE latest.plan_id = p.id
                          AND latest.content IS NOT NULL
                          AND latest.content <> ''
                        ORDER BY latest.created_at DESC
                        LIMIT 1
                    ),
                    p.planned_tasks
                )
                ELSE p.actual_tasks
            END AS actual_tasks,
            CASE
                WHEN p.status = 'pending' AND COUNT(r.id) > 0 THEN 'in_progress'
                ELSE p.status
            END AS status,
            p.generated_by,
            p.ai_suggestion,
            p.user_modified,
            p.sort_order,
            p.created_at,
            p.updated_at,
            s.name AS subject_name,
            k.name AS knowledge_point_name,
            COUNT(r.id) AS record_count
        FROM study_plans p
        LEFT JOIN subjects s ON s.id = p.subject_id
        LEFT JOIN knowledge_points k ON k.id = p.knowledge_point_id
        LEFT JOIN study_records r ON r.plan_id = p.id
        WHERE p.exam_id = ? AND p.date = ?
        GROUP BY p.id
        ORDER BY p.date, p.sort_order, p.created_at
        "#,
    )
    .bind(input.exam_id)
    .bind(business_date)
    .fetch_all(executor)
    .await
    .map_err(|_| AgentError::Persistence("plan.get_today query failed".to_owned()))?;

    Ok(PlanGetTodayOutput {
        business_date: business_date.to_owned(),
        plans,
    })
}

pub fn descriptor() -> ToolDescriptor {
    let plan_properties = json!({
        "id": {"type":"string"}, "exam_id": {"type":"string"}, "subject_id": {"type":"string"},
        "knowledge_point_id": {"type":["string","null"]}, "date": {"type":"string"},
        "planned_tasks": {"type":["string","null"]}, "planned_duration": {"type":["integer","null"]},
        "actual_duration": {"type":["integer","null"]}, "actual_tasks": {"type":["string","null"]},
        "status": {"type":"string"}, "generated_by": {"type":"string"},
        "ai_suggestion": {"type":["string","null"]}, "user_modified": {"type":"integer"},
        "sort_order": {"type":"integer"}, "created_at": {"type":"string"}, "updated_at": {"type":"string"},
        "subject_name": {"type":["string","null"]}, "knowledge_point_name": {"type":["string","null"]},
        "record_count": {"type":"integer"}
    });
    let plan_required = json!([
        "id",
        "exam_id",
        "subject_id",
        "knowledge_point_id",
        "date",
        "planned_tasks",
        "planned_duration",
        "actual_duration",
        "actual_tasks",
        "status",
        "generated_by",
        "ai_suggestion",
        "user_modified",
        "sort_order",
        "created_at",
        "updated_at",
        "subject_name",
        "knowledge_point_name",
        "record_count"
    ]);
    let plan_schema = json!({
        "type":"object", "additionalProperties":false,
        "properties":plan_properties, "required":plan_required
    });
    ToolDescriptor {
        name: "plan.get_today",
        version: "1",
        input_schema: json!({"type":"object", "additionalProperties":false, "required":["exam_id"], "properties":{"exam_id":{"type":"string","minLength":1}}}),
        output_schema: json!({"type":"object", "additionalProperties":false, "required":["business_date","plans"], "properties":{"business_date":{"type":"string"},"plans":{"type":"array","items":plan_schema}}}),
        risk: RiskLevel::R0,
        confirmation: Confirmation::Automatic,
        supports_undo: false,
        timeout_ms: 2000,
        idempotency: Idempotency::RetrySafe,
        data_permissions: vec![
            "study_plans:read",
            "subjects:read",
            "knowledge_points:read",
            "study_records:aggregate",
        ],
    }
}

pub fn get_range_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "plan.get_range",
        version: "1",
        input_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["exam_id","start_date","end_date"],
            "properties":{
                "exam_id":{"type":"string","minLength":1},
                "start_date":{"type":"string","pattern":"^\\d{4}-\\d{2}-\\d{2}$"},
                "end_date":{"type":"string","pattern":"^\\d{4}-\\d{2}-\\d{2}$"}
            }
        }),
        output_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["start_date","end_date","plans"],
            "properties":{
                "start_date":{"type":"string"},
                "end_date":{"type":"string"},
                "plans":{"type":"array","items":{"type":"object"}}
            }
        }),
        risk: RiskLevel::R0,
        confirmation: Confirmation::Automatic,
        supports_undo: false,
        timeout_ms: 2000,
        idempotency: Idempotency::RetrySafe,
        data_permissions: vec![
            "study_plans:read",
            "subjects:read",
            "knowledge_points:read",
            "study_records:aggregate",
        ],
    }
}
