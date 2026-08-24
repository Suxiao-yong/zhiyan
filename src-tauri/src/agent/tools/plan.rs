use chrono::{DateTime, Duration, FixedOffset, Timelike};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{Executor, FromRow, Sqlite, Transaction};
use uuid::Uuid;

use crate::agent::error::AgentError;
use crate::agent::plan_draft::{
    build_draft, DraftConflict, DraftInput, DraftKnowledgePoint, DraftSubject, PlanDraft,
    TaskEvidence,
};

use super::{Confirmation, Idempotency, RiskLevel, ToolDescriptor};

const WEEK_DAYS: usize = 7;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanGenerateInput {
    pub exam_id: String,
    /// Monday of the generation week (YYYY-MM-DD).
    pub week_start: String,
    #[serde(default)]
    pub daily_capacity_min: Option<i64>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PlanGenerateRow {
    pub date: String,
    pub subject_id: String,
    pub subject_name: String,
    pub planned_duration: i64,
    pub planned_tasks: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanGenerateOutput {
    pub week_start: String,
    pub capacity_min: i64,
    /// Rows already present for this week (replay returns them unchanged).
    pub rows: Vec<PlanGenerateRow>,
    pub newly_created: bool,
}

#[derive(Debug, Clone, Serialize, FromRow)]
struct SubjectWeightRow {
    id: String,
    name: String,
    weight: f64,
}

/// `plan.generate` (R2, approval-gated): a deterministic local rule-based
/// weekly draft. Subjects are slotted across the seven days weighted by
/// `subjects.weight`; each day gets the daily capacity. Rerunning the same
/// week is idempotent — existing local rows are returned unchanged.
pub async fn generate(
    tx: &mut Transaction<'_, Sqlite>,
    input: PlanGenerateInput,
) -> Result<PlanGenerateOutput, AgentError> {
    let capacity = input.daily_capacity_min.unwrap_or(120).clamp(15, 600);
    let week_start = input
        .week_start
        .parse::<chrono::NaiveDate>()
        .map_err(|_| AgentError::ToolSchemaInvalid)?;
    let exam_exists: Option<i64> = sqlx::query_scalar("SELECT COUNT(*) FROM exams WHERE id = ?")
        .bind(&input.exam_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| AgentError::Persistence("plan.generate exam check failed".to_owned()))?;
    if exam_exists.unwrap_or(0) == 0 {
        return Err(AgentError::Persistence("exam not found".to_owned()));
    }

    // Replay protection: an existing local generation for this week is the
    // result; nothing is written again.
    let existing: Vec<PlanGenerateRow> = sqlx::query_as(
        r#"
        SELECT p.date, p.subject_id, s.name AS subject_name,
               p.planned_duration, p.planned_tasks
        FROM study_plans p
        JOIN subjects s ON s.id = p.subject_id
        WHERE p.exam_id = ? AND p.generated_by = 'local'
          AND p.date BETWEEN ? AND ?
        ORDER BY p.date
        "#,
    )
    .bind(&input.exam_id)
    .bind(week_start.format("%Y-%m-%d").to_string())
    .bind(
        (week_start + chrono::Days::new(6))
            .format("%Y-%m-%d")
            .to_string(),
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| AgentError::Persistence("plan.generate read failed".to_owned()))?;
    if !existing.is_empty() {
        return Ok(PlanGenerateOutput {
            week_start: input.week_start,
            capacity_min: capacity,
            rows: existing,
            newly_created: false,
        });
    }

    let subjects: Vec<SubjectWeightRow> = sqlx::query_as(
        "SELECT id, name, COALESCE(weight, 1.0) AS weight FROM subjects \
         WHERE exam_id = ? ORDER BY weight DESC, name",
    )
    .bind(&input.exam_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| AgentError::Persistence("plan.generate subjects query failed".to_owned()))?;
    if subjects.is_empty() {
        return Err(AgentError::Persistence("exam has no subjects".to_owned()));
    }

    // Weighted slotting (deterministic, no loop hazards): floor each
    // subject's share of the seven days, then hand the remainder to the
    // subjects with the largest fractional parts, one day each.
    let total: f64 = subjects
        .iter()
        .map(|subject| subject.weight.max(0.01))
        .sum();
    let mut counts: Vec<i64> = subjects
        .iter()
        .map(|subject| (WEEK_DAYS as f64 * subject.weight.max(0.01) / total).floor() as i64)
        .collect();
    let mut fractions: Vec<(usize, f64)> = subjects
        .iter()
        .enumerate()
        .map(|(index, subject)| {
            let raw = WEEK_DAYS as f64 * subject.weight.max(0.01) / total;
            (index, raw - raw.floor())
        })
        .collect();
    fractions.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut remaining = WEEK_DAYS as i64 - counts.iter().sum::<i64>();
    for (index, _) in fractions {
        if remaining <= 0 {
            break;
        }
        counts[index] += 1;
        remaining -= 1;
    }

    // Flatten the counts into exactly seven daily slots ordered by the week
    // day, one subject per day (dates strictly inside the week).
    let mut slots: Vec<&SubjectWeightRow> = Vec::with_capacity(WEEK_DAYS);
    for (subject, count) in subjects.iter().zip(counts.iter()) {
        for _ in 0..*count {
            slots.push(subject);
        }
    }
    let mut rows = Vec::with_capacity(WEEK_DAYS);
    for (day, subject) in slots.iter().take(WEEK_DAYS).enumerate() {
        let date = week_start + chrono::Days::new(day as u64);
        rows.push(PlanGenerateRow {
            date: date.format("%Y-%m-%d").to_string(),
            subject_id: subject.id.clone(),
            subject_name: subject.name.clone(),
            planned_duration: capacity,
            planned_tasks: format!("按计划复习《{}》", subject.name),
        });
    }

    for row in &rows {
        sqlx::query(
            r#"
            INSERT INTO study_plans
                (id, exam_id, subject_id, date, planned_duration, planned_tasks,
                 status, generated_by)
            VALUES (?, ?, ?, ?, ?, ?, 'pending', 'local')
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&input.exam_id)
        .bind(&row.subject_id)
        .bind(&row.date)
        .bind(row.planned_duration)
        .bind(&row.planned_tasks)
        .execute(&mut **tx)
        .await
        .map_err(|_| AgentError::Persistence("plan.generate insert failed".to_owned()))?;
    }

    Ok(PlanGenerateOutput {
        week_start: input.week_start,
        capacity_min: capacity,
        rows,
        newly_created: true,
    })
}

pub fn generate_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "plan.generate",
        version: "1",
        input_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["exam_id","week_start"],
            "properties":{
                "exam_id":{"type":"string","minLength":1},
                "week_start":{"type":"string","pattern":"^\\d{4}-\\d{2}-\\d{2}$"},
                "daily_capacity_min":{"type":"integer","minimum":15,"maximum":600}
            }
        }),
        output_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["week_start","capacity_min","rows","newly_created"],
            "properties":{
                "week_start":{"type":"string"},
                "capacity_min":{"type":"integer"},
                "newly_created":{"type":"boolean"},
                "rows":{"type":"array","items":{"type":"object"}}
            }
        }),
        risk: RiskLevel::R2,
        confirmation: Confirmation::Required,
        supports_undo: false,
        timeout_ms: 3000,
        idempotency: Idempotency::RequiredExactlyOnce,
        data_permissions: vec!["study_plans:write", "subjects:read", "exams:read"],
    }
}

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
    if input.start_date.parse::<chrono::NaiveDate>().is_err()
        || input.end_date.parse::<chrono::NaiveDate>().is_err()
    {
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

// ---- plan.preview_generate / plan.apply_preview (Mandatory Task C) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanPreviewGenerateInput {
    pub exam_id: String,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub daily_hours: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanPreviewGenerateOutput {
    /// The agent_steps row that owns the persisted draft. `plan.apply_preview`
    /// accepts only this id (bound to the same run), never a free-form draft.
    pub preview_step_id: String,
    pub draft: PlanDraft,
}

/// `plan.preview_generate` (R0, read-only): builds a deterministic local
/// PlanDraft that mirrors the retired TypeScript local generator, analyzes
/// conflicts with existing plans, persists the draft on the preview step, and
/// returns it for a readable preview. No study_plans row is written here.
pub async fn preview_generate(
    tx: &mut Transaction<'_, Sqlite>,
    input: PlanPreviewGenerateInput,
    step_id: &str,
) -> Result<PlanPreviewGenerateOutput, AgentError> {
    let business_date = business_date_at(chrono::Local::now().fixed_offset());
    let start_date = input.start_date.as_deref().unwrap_or(&business_date);
    let daily_hours = match input.daily_hours {
        Some(hours) => hours,
        None => default_daily_hours(tx).await?,
    };

    #[derive(FromRow)]
    struct ExamRow {
        exam_date: String,
    }
    let exam = sqlx::query_as::<_, ExamRow>("SELECT exam_date FROM exams WHERE id = ?")
        .bind(&input.exam_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| {
            AgentError::Persistence("plan.preview_generate exam read failed".to_owned())
        })?;
    let Some(exam) = exam else {
        return Err(AgentError::Persistence("exam not found".to_owned()));
    };

    let subjects: Vec<DraftSubject> = sqlx::query_as(
        "SELECT id, name, COALESCE(weight, 0) AS weight, COALESCE(current_level, 3) AS current_level \
         FROM subjects WHERE exam_id = ? ORDER BY sort_order, created_at, id",
    )
    .bind(&input.exam_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| AgentError::Persistence("plan.preview_generate subjects read failed".to_owned()))?;

    let kps: Vec<DraftKnowledgePoint> = sqlx::query_as(
        "SELECT id, subject_id, name, COALESCE(current_mastery, 0) AS current_mastery, \
         (SELECT COUNT(*) FROM wrong_questions wq \
           WHERE wq.knowledge_point_id = knowledge_points.id AND wq.mastered = 0) AS wrong_count \
         FROM knowledge_points WHERE subject_id IN (SELECT id FROM subjects WHERE exam_id = ?) \
         ORDER BY sort_order, created_at, id",
    )
    .bind(&input.exam_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| {
        AgentError::Persistence("plan.preview_generate knowledge points read failed".to_owned())
    })?;

    let future_plan_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM study_plans WHERE exam_id = ? \
         AND (date > ? OR (date = ? AND (actual_duration IS NULL OR actual_duration = 0)))",
    )
    .bind(&input.exam_id)
    .bind(&business_date)
    .bind(&business_date)
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| {
        AgentError::Persistence("plan.preview_generate conflict count failed".to_owned())
    })?;

    let today_kept: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT subject_id FROM study_plans \
         WHERE exam_id = ? AND date = ? AND actual_duration IS NOT NULL AND actual_duration > 0",
    )
    .bind(&input.exam_id)
    .bind(&business_date)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| {
        AgentError::Persistence("plan.preview_generate kept-subject read failed".to_owned())
    })?;

    let mut draft = build_draft(DraftInput {
        exam_id: &input.exam_id,
        exam_date: &exam.exam_date,
        start_date,
        daily_hours,
        subjects,
        knowledge_points: kps,
        existing_future_plan_count: future_plan_count,
        today_kept_subject_ids: today_kept,
    })?;
    draft.precondition_hash = plan_state_hash(tx, &input.exam_id).await?;

    Ok(PlanPreviewGenerateOutput {
        preview_step_id: step_id.to_owned(),
        draft,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanApplyPreviewInput {
    /// The `plan.preview_generate` step id whose persisted draft is applied.
    pub preview_step_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanApplyPreviewOutput {
    pub preview_step_id: String,
    pub inserted: i64,
    pub replaced: i64,
    pub kept_subjects: i64,
    pub conflicts: Vec<DraftConflict>,
}

/// Undo receipt kind for `plan.apply_preview` (Task 5). Stored on the applied
/// step's `undo_json` so the executor can restore the pre-apply state.
pub const PLAN_APPLY_UNDO_KIND: &str = "plan.apply_preview.v1";

/// An existing plan row relevant to the apply projection. The projection is a
/// pure function of the draft + this list + the business date, shared by the
/// approval preview and the real apply so the UI numbers never drift.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ExistingPlanRow {
    pub id: String,
    pub date: String,
    pub subject_id: String,
    pub actual_duration: Option<i64>,
}

/// A row the apply would insert, with the fields the frontend shows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectedRow {
    pub date: String,
    pub subject_id: String,
    pub subject_name: String,
    pub knowledge_point_id: Option<String>,
    pub planned_tasks: String,
    pub planned_duration: i64,
    /// 审批卡依据行（知识点任务才有）；apply 落库时不写入 study_plans。
    #[serde(default)]
    pub evidence: Option<TaskEvidence>,
}

/// Result of projecting the draft against the existing plan state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApplyProjection {
    /// Rows the apply would actually write (kept-subject rows on the business
    /// date are excluded). This count is the preview's `affected_count`.
    pub writable_rows: Vec<ProjectedRow>,
    /// Subjects that already have actuals today; their draft rows are skipped.
    pub kept_subjects: Vec<String>,
    /// Existing replaceable plans (future, or today without actuals).
    pub replaced_count: i64,
}

/// The apply-side projection shared by preview and apply. A row on the
/// business date whose subject already has actual progress is kept (skipped
/// from the draft); everything else is writable.
pub fn project_apply_rows(
    draft: &PlanDraft,
    existing_plans: &[ExistingPlanRow],
    business_date: &str,
) -> ApplyProjection {
    let mut kept_subjects: Vec<String> = existing_plans
        .iter()
        .filter(|plan| plan.date == business_date && plan.actual_duration.unwrap_or(0) > 0)
        .map(|plan| plan.subject_id.clone())
        .collect();
    kept_subjects.sort();
    kept_subjects.dedup();
    let replaced_count = existing_plans
        .iter()
        .filter(|plan| {
            plan.date.as_str() > business_date
                || (plan.date == business_date && plan.actual_duration.unwrap_or(0) == 0)
        })
        .count() as i64;
    let mut writable_rows = Vec::new();
    for day in &draft.daily_plans {
        for task in &day.tasks {
            if day.date == business_date && kept_subjects.contains(&task.subject_id) {
                continue;
            }
            writable_rows.push(ProjectedRow {
                date: day.date.clone(),
                subject_id: task.subject_id.clone(),
                subject_name: task.subject_name.clone(),
                knowledge_point_id: task.knowledge_point_id.clone(),
                planned_tasks: task.task.clone(),
                planned_duration: task.duration_min,
                evidence: task.evidence.clone(),
            });
        }
    }
    ApplyProjection {
        writable_rows,
        kept_subjects,
        replaced_count,
    }
}

/// A full `study_plans` row snapshot used to restore replaced plans on undo.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PlanSnapshotRow {
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
    pub generated_by: Option<String>,
    pub ai_suggestion: Option<String>,
    pub user_modified: i64,
    pub created_at: String,
    pub updated_at: String,
    pub sort_order: i64,
}

/// A `study_records` row that the apply detached from a replaced plan.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DetachedRecordRow {
    pub id: String,
    pub plan_id: String,
}

/// Undo receipt persisted on the applied step's `undo_json`. It carries every
/// snapshot needed to restore the pre-apply state: the replaced plan rows, the
/// detached record->plan relationships, the inserted plan ids, and the plan
/// state hash before/after apply (any external change since apply breaks the
/// undo precondition and yields `conflict`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanApplyUndoReceipt {
    pub kind: String,
    pub exam_id: String,
    pub inserted_plan_ids: Vec<String>,
    pub replaced_plans: Vec<PlanSnapshotRow>,
    pub detached_records: Vec<DetachedRecordRow>,
    pub hash_before: String,
    pub hash_after: String,
}

/// `plan.apply_preview` (R3, approval-gated): loads the persisted draft owned
/// by the preview step, re-checks the plan-state precondition hash, then
/// applies the draft inside the same transaction — replacing future plans and
/// today's plans without actual progress, keeping today's subjects that
/// already have actuals, and inserting the draft rows. Replaying an already
/// applied draft fails the precondition check instead of double-writing.
/// Returns the output plus the undo receipt for the step's `undo_json`.
pub async fn apply_preview(
    tx: &mut Transaction<'_, Sqlite>,
    input: PlanApplyPreviewInput,
) -> Result<(PlanApplyPreviewOutput, PlanApplyUndoReceipt), AgentError> {
    let draft_json: Option<String> = sqlx::query_scalar(
        "SELECT output_json FROM agent_steps \
         WHERE id = ? AND tool_name = 'plan.preview_generate' AND status = 'completed'",
    )
    .bind(&input.preview_step_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| AgentError::Persistence("plan.apply_preview draft read failed".to_owned()))?;
    let Some(draft_json) = draft_json else {
        return Err(AgentError::ApprovalInvalid);
    };
    // output_json holds PlanPreviewGenerateOutput; the draft lives at $.draft.
    let output: Value =
        serde_json::from_str(&draft_json).map_err(|_| AgentError::ApprovalInvalid)?;
    let draft: PlanDraft = serde_json::from_value(
        output
            .get("draft")
            .cloned()
            .ok_or(AgentError::ApprovalInvalid)?,
    )
    .map_err(|_| AgentError::ApprovalInvalid)?;
    if draft.version != crate::agent::plan_draft::PLAN_DRAFT_VERSION {
        return Err(AgentError::ApprovalInvalid);
    }

    // Precondition: the exam's plan state must still match what the preview
    // was generated against. Any manual edit or applied change fails safely.
    let hash_before = plan_state_hash(tx, &draft.exam_id).await?;
    if hash_before != draft.precondition_hash {
        return Err(AgentError::PreconditionChanged);
    }

    let business_date = business_date_at(chrono::Local::now().fixed_offset());
    let applied = apply_draft_rows(tx, &draft, &business_date).await?;
    let hash_after = plan_state_hash(tx, &draft.exam_id).await?;

    let undo = PlanApplyUndoReceipt {
        kind: PLAN_APPLY_UNDO_KIND.to_owned(),
        exam_id: draft.exam_id.clone(),
        inserted_plan_ids: applied.inserted_ids,
        replaced_plans: applied.replaced_snapshot,
        detached_records: applied.detached_records,
        hash_before,
        hash_after,
    };

    Ok((
        PlanApplyPreviewOutput {
            preview_step_id: input.preview_step_id,
            inserted: applied.counts.inserted,
            replaced: applied.counts.replaced,
            kept_subjects: applied.counts.kept_subjects,
            conflicts: draft.conflicts.clone(),
        },
        undo,
    ))
}

struct ApplyCounts {
    inserted: i64,
    replaced: i64,
    kept_subjects: i64,
}

struct ApplyOutcome {
    counts: ApplyCounts,
    inserted_ids: Vec<String>,
    replaced_snapshot: Vec<PlanSnapshotRow>,
    detached_records: Vec<DetachedRecordRow>,
}

/// Mirror of the retired `applyGeneratedPlan`: detach records and delete
/// replaceable plans, keep today's subjects with actuals, insert draft rows.
/// The projection (`project_apply_rows`) decides exactly which draft rows are
/// writable; the apply and the approval preview share it so the UI numbers
/// and the actual write count cannot drift.
async fn apply_draft_rows(
    tx: &mut Transaction<'_, Sqlite>,
    draft: &PlanDraft,
    business_date: &str,
) -> Result<ApplyOutcome, AgentError> {
    let existing: Vec<ExistingPlanRow> = sqlx::query_as(
        "SELECT id, date, subject_id, actual_duration FROM study_plans WHERE exam_id = ?",
    )
    .bind(&draft.exam_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| AgentError::Persistence("plan.apply_preview existing read failed".to_owned()))?;
    let projection = project_apply_rows(draft, &existing, business_date);

    // Snapshot the replaceable plans (full rows) and their records before
    // deleting/detaching, so undo can restore them exactly.
    let replaceable_sql = "SELECT id, exam_id, subject_id, knowledge_point_id, date, \
         planned_tasks, planned_duration, actual_duration, actual_tasks, status, \
         generated_by, ai_suggestion, user_modified, created_at, updated_at, \
         COALESCE(sort_order, 0) AS sort_order FROM study_plans \
         WHERE exam_id = ? AND (date > ? OR (date = ? AND (actual_duration IS NULL OR actual_duration = 0)))";
    let replaced_snapshot: Vec<PlanSnapshotRow> = sqlx::query_as(replaceable_sql)
        .bind(&draft.exam_id)
        .bind(business_date)
        .bind(business_date)
        .fetch_all(&mut **tx)
        .await
        .map_err(|_| AgentError::Persistence("plan.apply_preview snapshot failed".to_owned()))?;
    let detached_records: Vec<DetachedRecordRow> = sqlx::query_as(
        "SELECT id, plan_id FROM study_records \
         WHERE plan_id IN (SELECT id FROM study_plans \
           WHERE exam_id = ? AND (date > ? OR (date = ? AND (actual_duration IS NULL OR actual_duration = 0))))",
    )
    .bind(&draft.exam_id)
    .bind(business_date)
    .bind(business_date)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| AgentError::Persistence("plan.apply_preview detach read failed".to_owned()))?;

    // Detach study records from the plans about to be replaced (records stay).
    sqlx::query(
        "UPDATE study_records SET plan_id = NULL \
         WHERE plan_id IN (SELECT id FROM study_plans \
           WHERE exam_id = ? AND (date > ? OR (date = ? AND (actual_duration IS NULL OR actual_duration = 0))))",
    )
    .bind(&draft.exam_id)
    .bind(business_date)
    .bind(business_date)
    .execute(&mut **tx)
    .await
    .map_err(|_| AgentError::Persistence("plan.apply_preview detach failed".to_owned()))?;

    let replaced = sqlx::query(
        "DELETE FROM study_plans WHERE exam_id = ? \
         AND (date > ? OR (date = ? AND (actual_duration IS NULL OR actual_duration = 0)))",
    )
    .bind(&draft.exam_id)
    .bind(business_date)
    .bind(business_date)
    .execute(&mut **tx)
    .await
    .map_err(|_| AgentError::Persistence("plan.apply_preview delete failed".to_owned()))?
    .rows_affected() as i64;

    let mut inserted_ids = Vec::with_capacity(projection.writable_rows.len());
    let mut order_by_date: std::collections::HashMap<String, i64> =
        std::collections::HashMap::new();
    for row in &projection.writable_rows {
        let id = Uuid::new_v4().to_string();
        let order = order_by_date.entry(row.date.clone()).or_insert(0);
        sqlx::query(
            r#"
            INSERT INTO study_plans
                (id, exam_id, subject_id, knowledge_point_id, date, planned_tasks,
                 planned_duration, actual_duration, actual_tasks, status, generated_by,
                 ai_suggestion, user_modified, sort_order)
            VALUES (?, ?, ?, ?, ?, ?, ?, NULL, NULL, 'pending', 'local', NULL, 0, ?)
            "#,
        )
        .bind(&id)
        .bind(&draft.exam_id)
        .bind(&row.subject_id)
        .bind(&row.knowledge_point_id)
        .bind(&row.date)
        .bind(&row.planned_tasks)
        .bind(row.planned_duration)
        .bind(*order)
        .execute(&mut **tx)
        .await
        .map_err(|_| AgentError::Persistence("plan.apply_preview insert failed".to_owned()))?;
        inserted_ids.push(id);
        *order += 1;
    }

    Ok(ApplyOutcome {
        counts: ApplyCounts {
            inserted: inserted_ids.len() as i64,
            replaced,
            kept_subjects: projection.kept_subjects.len() as i64,
        },
        inserted_ids,
        replaced_snapshot,
        detached_records,
    })
}

/// Stable SHA-256 fingerprint of the exam's study_plans state. Used both when
/// a preview is generated and when it is applied (and by the apply undo); any
/// change in between fails the precondition safely.
pub(crate) async fn plan_state_hash(
    tx: &mut Transaction<'_, Sqlite>,
    exam_id: &str,
) -> Result<String, AgentError> {
    #[derive(Debug, Clone, Serialize, sqlx::FromRow)]
    struct HashRow {
        id: String,
        date: String,
        subject_id: String,
        status: String,
        actual_duration: Option<i64>,
        actual_tasks: Option<String>,
    }
    let rows: Vec<HashRow> = sqlx::query_as(
        "SELECT id, date, subject_id, status, actual_duration, actual_tasks \
         FROM study_plans WHERE exam_id = ? ORDER BY id",
    )
    .bind(exam_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| AgentError::Persistence("plan.state hash failed".to_owned()))?;
    let canonical = serde_json::to_string(&rows).unwrap_or_default();
    let digest = Sha256::digest(canonical.as_bytes());
    let mut fingerprint = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        write!(&mut fingerprint, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(fingerprint)
}

/// The shared `daily_study_hours` setting (default 6) used when the model does
/// not pass an explicit daily capacity.
async fn default_daily_hours(tx: &mut Transaction<'_, Sqlite>) -> Result<f64, AgentError> {
    let value: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'daily_study_hours'")
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| AgentError::Persistence("daily_study_hours read failed".to_owned()))?;
    Ok(value
        .and_then(|raw| raw.parse::<f64>().ok())
        .filter(|hours| *hours > 0.0)
        .unwrap_or(6.0))
}

pub fn preview_generate_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "plan.preview_generate",
        version: "1",
        input_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["exam_id"],
            "properties":{
                "exam_id":{"type":"string","minLength":1},
                "start_date":{"type":"string","pattern":"^\\d{4}-\\d{2}-\\d{2}$"},
                "daily_hours":{"type":"number","minimum":1,"maximum":16}
            }
        }),
        output_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["preview_step_id","draft"],
            "properties":{
                "preview_step_id":{"type":"string"},
                "draft":{"type":"object"}
            }
        }),
        risk: RiskLevel::R0,
        confirmation: Confirmation::Automatic,
        supports_undo: false,
        timeout_ms: 3000,
        idempotency: Idempotency::RetrySafe,
        data_permissions: vec![
            "study_plans:read",
            "subjects:read",
            "knowledge_points:read",
            "study_records:aggregate",
        ],
    }
}

pub fn apply_preview_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "plan.apply_preview",
        version: "1",
        input_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["preview_step_id"],
            "properties":{
                "preview_step_id":{"type":"string","minLength":1}
            }
        }),
        output_schema: json!({
            "type":"object", "additionalProperties":false,
            "required":["preview_step_id","inserted","replaced","kept_subjects"],
            "properties":{
                "preview_step_id":{"type":"string"},
                "inserted":{"type":"integer"},
                "replaced":{"type":"integer"},
                "kept_subjects":{"type":"integer"},
                "conflicts":{"type":"array"}
            }
        }),
        risk: RiskLevel::R3,
        confirmation: Confirmation::Required,
        supports_undo: true,
        timeout_ms: 5000,
        idempotency: Idempotency::RetrySafe,
        data_permissions: vec!["study_plans:read_write", "study_records:read_write"],
    }
}
