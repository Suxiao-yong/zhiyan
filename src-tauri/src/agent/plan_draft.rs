// PlanDraft (Mandatory Task C): a pure, deterministic local plan draft that
// mirrors the behavior of the retired TypeScript `plan-generator.ts` local
// algorithm — non-week date ranges, subject/knowledge-point rotation, phases
// by exam date, daily capacity, and existing-plan conflict analysis.
//
// The builder is a pure function over snapshot inputs (no database access), so
// fixture-based equivalence tests can compare it row-by-row against the old
// TypeScript behavior. Writes (apply) live in `tools/plan.rs` and are gated by
// the R3 approval + precondition-hash checks of the executor.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::agent::error::AgentError;

/// Version of the draft format. Bumped on any incompatible change so stale
/// previews fail precondition checks instead of being applied blindly.
pub const PLAN_DRAFT_VERSION: &str = "1";

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct DraftSubject {
    pub id: String,
    pub name: String,
    pub weight: f64,
    pub current_level: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct DraftKnowledgePoint {
    pub id: String,
    pub subject_id: String,
    pub name: String,
    pub current_mastery: i64,
    /// 该知识点下未掌握错题数（查询时以标量子查询统计）。
    pub wrong_count: i64,
}

/// 计划任务的确定性依据（纯本地计算，不走 LLM）：审批卡展示“为什么安排这个任务”。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskEvidence {
    pub mastery: i64,
    pub wrong_question_count: i64,
    pub days_to_exam: Option<i64>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftInput<'a> {
    pub exam_id: &'a str,
    pub exam_date: &'a str,
    pub start_date: &'a str,
    pub daily_hours: f64,
    pub subjects: Vec<DraftSubject>,
    pub knowledge_points: Vec<DraftKnowledgePoint>,
    /// Number of existing plans dated after today that the apply step would
    /// replace (conflict analysis).
    pub existing_future_plan_count: i64,
    /// Subject ids that already have actual progress today; the apply step
    /// keeps them and skips duplicate rows for those subjects.
    pub today_kept_subject_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DraftPhase {
    pub name: String,
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DraftTask {
    pub subject_id: String,
    pub subject_name: String,
    pub knowledge_point_id: Option<String>,
    pub task: String,
    pub duration_min: i64,
    /// 知识点任务携带的确定性依据；无知识点的综合复习任务为 None。
    #[serde(default)]
    pub evidence: Option<TaskEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DraftDay {
    pub date: String,
    pub tasks: Vec<DraftTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DraftConflictKind {
    /// Future plans (after today) will be replaced by this draft.
    ReplaceFuturePlans,
    /// Today's plans without actual progress will be replaced.
    ReplaceTodayWithoutActual,
    /// Today's subjects with actual progress are kept; duplicate rows skipped.
    KeepTodayActualSubjects,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DraftConflict {
    pub kind: DraftConflictKind,
    pub affected_count: i64,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DraftSummary {
    pub total_tasks: i64,
    pub total_duration_min: i64,
    pub avg_daily_min: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanDraft {
    pub version: String,
    pub exam_id: String,
    pub generated_at: String,
    pub start_date: String,
    pub total_days: i64,
    pub phases: Vec<DraftPhase>,
    pub daily_plans: Vec<DraftDay>,
    pub conflicts: Vec<DraftConflict>,
    pub summary: DraftSummary,
    /// SHA-256 of the exam's current study_plans state; apply re-computes it
    /// and refuses to write when the data changed underneath the draft.
    pub precondition_hash: String,
}

fn fmt_date(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// 依据文案的确定性规则：错题优先于掌握度自评。
fn reason_text(mastery: i64, wrong_count: i64, has_wrong: bool) -> String {
    if has_wrong && mastery <= 2 {
        format!("掌握度偏低({mastery}/5)且有{wrong_count}道未掌握错题，优先攻克")
    } else if has_wrong {
        format!("有{wrong_count}道未掌握错题需巩固")
    } else if mastery <= 2 {
        format!("自评掌握度较低({mastery}/5)")
    } else {
        format!("巩固提升({mastery}/5)")
    }
}

/// Phase split mirroring the retired TypeScript algorithm: fewer than 7 days
/// collapse into a single sprint phase; otherwise base/stren/sprint follow the
/// 0.5 / 0.3 / remainder split (each at least one day).
fn phase_split(total_days: i64) -> (i64, i64, i64) {
    if total_days < 7 {
        return (0, 0, total_days);
    }
    let base = (total_days as f64 * 0.5).floor() as i64;
    let stren = (total_days as f64 * 0.3).floor() as i64;
    let base = base.max(1);
    let stren = stren.max(1);
    let sprint = (total_days - base - stren).max(1);
    (base, stren, sprint)
}

/// Build the deterministic draft. Pure: no I/O, all inputs are passed in.
pub fn build_draft(input: DraftInput<'_>) -> Result<PlanDraft, AgentError> {
    let exam_date = NaiveDate::parse_from_str(input.exam_date, "%Y-%m-%d")
        .map_err(|_| AgentError::ToolSchemaInvalid)?;
    let start_date = NaiveDate::parse_from_str(input.start_date, "%Y-%m-%d")
        .map_err(|_| AgentError::ToolSchemaInvalid)?;
    let total_days = exam_date.signed_duration_since(start_date).num_days();
    if total_days <= 0 {
        return Err(AgentError::ToolSchemaInvalid);
    }
    if input.subjects.is_empty() {
        return Err(AgentError::ToolSchemaInvalid);
    }
    let daily_hours = input.daily_hours.clamp(0.5, 16.0);

    let (base_days, stren_days, sprint_days) = phase_split(total_days);

    // Subject weights: lower current level and higher weight get more time.
    let gaps: Vec<f64> = input
        .subjects
        .iter()
        .map(|subject| ((6 - subject.current_level) as f64).max(0.0) * subject.weight.max(0.01))
        .collect();
    let total_gap: f64 = gaps.iter().sum::<f64>().max(1.0);

    // Knowledge points per subject, low mastery first, rotated day by day.
    let mut kps_by_subject: Vec<Vec<&DraftKnowledgePoint>> = input
        .subjects
        .iter()
        .map(|subject| {
            let mut kps: Vec<&DraftKnowledgePoint> = input
                .knowledge_points
                .iter()
                .filter(|kp| kp.subject_id == subject.id)
                .collect();
            kps.sort_by_key(|kp| kp.current_mastery);
            kps
        })
        .collect();
    let mut kp_idx: Vec<usize> = vec![0; input.subjects.len()];

    let mut daily_plans = Vec::with_capacity(total_days as usize);
    for day_offset in 0..total_days {
        let date = start_date + chrono::Days::new(day_offset as u64);
        // 1970-01-01 was a Thursday (weekday 4 in 0=Sun..6=Sat numbering);
        // weekday = (epoch_days + 4) % 7, Sunday when it wraps to 0.
        let epoch_days = date
            .signed_duration_since(
                chrono::NaiveDate::from_ymd_opt(1970, 1, 1).expect("static date"),
            )
            .num_days();
        let is_sunday = (epoch_days + 4).rem_euclid(7) == 0;
        let days_to_exam = exam_date.signed_duration_since(date).num_days();
        let hours = if is_sunday {
            daily_hours / 2.0
        } else {
            daily_hours
        };
        let total_min = (hours * 60.0).round() as i64;

        let mut tasks = Vec::new();
        for (index, subject) in input.subjects.iter().enumerate() {
            // Round toward the largest remainder so the sums stay stable and
            // the durations look natural (the old algorithm rounded each).
            let raw = total_min as f64 * (gaps[index] / total_gap);
            let min = raw.round() as i64;
            if min < 10 {
                continue;
            }
            let kps = &mut kps_by_subject[index];
            let mut kp_id = None;
            let mut evidence = None;
            let mut task = format!("{} 综合复习", subject.name);
            if !kps.is_empty() {
                let idx = kp_idx[index] % kps.len();
                let kp = kps[idx];
                kp_id = Some(kp.id.clone());
                task = if is_sunday {
                    format!("复习：{}", kp.name)
                } else {
                    format!("学习：{}", kp.name)
                };
                evidence = Some(TaskEvidence {
                    mastery: kp.current_mastery,
                    wrong_question_count: kp.wrong_count,
                    days_to_exam: Some(days_to_exam),
                    reason: reason_text(kp.current_mastery, kp.wrong_count, kp.wrong_count > 0),
                });
                kp_idx[index] = idx + 1;
            }
            tasks.push(DraftTask {
                subject_id: subject.id.clone(),
                subject_name: subject.name.clone(),
                knowledge_point_id: kp_id,
                task,
                duration_min: min,
                evidence,
            });
        }
        daily_plans.push(DraftDay {
            date: fmt_date(date),
            tasks,
        });
    }

    // Phases.
    let mut phases = Vec::new();
    let days = |offset: i64| fmt_date(start_date + chrono::Days::new(offset as u64));
    if base_days > 0 {
        phases.push(DraftPhase {
            name: "基础期".into(),
            start: days(0),
            end: days(base_days - 1),
        });
    }
    if stren_days > 0 {
        phases.push(DraftPhase {
            name: "强化期".into(),
            start: days(base_days),
            end: days(base_days + stren_days - 1),
        });
    }
    if sprint_days > 0 {
        phases.push(DraftPhase {
            name: "冲刺期".into(),
            start: days(base_days + stren_days),
            end: days(total_days - 1),
        });
    }

    // Conflicts with the existing plan state (mirrors applyGeneratedPlan).
    let mut conflicts = Vec::new();
    if input.existing_future_plan_count > 0 {
        conflicts.push(DraftConflict {
            kind: DraftConflictKind::ReplaceFuturePlans,
            affected_count: input.existing_future_plan_count,
            detail: format!(
                "将替换考试日期前的 {} 条未来计划",
                input.existing_future_plan_count
            ),
        });
    }
    conflicts.push(DraftConflict {
        kind: DraftConflictKind::ReplaceTodayWithoutActual,
        affected_count: 0,
        detail: "今天尚未有实际进度的计划将被替换".to_owned(),
    });
    if !input.today_kept_subject_ids.is_empty() {
        conflicts.push(DraftConflict {
            kind: DraftConflictKind::KeepTodayActualSubjects,
            affected_count: input.today_kept_subject_ids.len() as i64,
            detail: format!(
                "今天已有实际进度的 {} 个科目将保留",
                input.today_kept_subject_ids.len()
            ),
        });
    }

    let total_tasks: i64 = daily_plans.iter().map(|day| day.tasks.len() as i64).sum();
    let total_duration_min: i64 = daily_plans
        .iter()
        .flat_map(|day| day.tasks.iter())
        .map(|task| task.duration_min)
        .sum();
    let avg_daily_min = if total_days > 0 {
        total_duration_min / total_days
    } else {
        0
    };

    Ok(PlanDraft {
        version: PLAN_DRAFT_VERSION.to_owned(),
        exam_id: input.exam_id.to_owned(),
        generated_at: input.start_date.to_owned(),
        start_date: input.start_date.to_owned(),
        total_days,
        phases,
        daily_plans,
        conflicts,
        summary: DraftSummary {
            total_tasks,
            total_duration_min,
            avg_daily_min,
        },
        precondition_hash: String::new(), // filled by the caller with the DB state hash
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_subjects() -> Vec<DraftSubject> {
        vec![
            DraftSubject {
                id: "sub-math".into(),
                name: "数学".into(),
                weight: 2.0,
                current_level: 3,
            },
            DraftSubject {
                id: "sub-eng".into(),
                name: "英语".into(),
                weight: 1.0,
                current_level: 4,
            },
        ]
    }

    fn sample_kps() -> Vec<DraftKnowledgePoint> {
        vec![
            DraftKnowledgePoint {
                id: "kp-func".into(),
                subject_id: "sub-math".into(),
                name: "函数".into(),
                current_mastery: 2,
                wrong_count: 0,
            },
            DraftKnowledgePoint {
                id: "kp-geo".into(),
                subject_id: "sub-math".into(),
                name: "几何".into(),
                current_mastery: 4,
                wrong_count: 0,
            },
            DraftKnowledgePoint {
                id: "kp-word".into(),
                subject_id: "sub-eng".into(),
                name: "词汇".into(),
                current_mastery: 3,
                wrong_count: 0,
            },
        ]
    }

    fn input<'a>(exam_date: &'a str, start_date: &'a str, hours: f64) -> DraftInput<'a> {
        DraftInput {
            exam_id: "exam-a",
            exam_date,
            start_date,
            daily_hours: hours,
            subjects: sample_subjects(),
            knowledge_points: sample_kps(),
            existing_future_plan_count: 3,
            today_kept_subject_ids: Vec::new(),
        }
    }

    #[test]
    fn draft_covers_the_full_non_week_range_and_phases() {
        // 30 days from today to exam date.
        let draft = build_draft(input("2030-02-01", "2030-01-03", 6.0)).unwrap();
        assert_eq!(draft.total_days, 29);
        assert_eq!(draft.daily_plans.len(), 29);
        assert_eq!(draft.daily_plans[0].date, "2030-01-03");
        assert_eq!(draft.daily_plans.last().unwrap().date, "2030-01-31");
        // 29 days: base=floor(14.5)=14, stren=floor(8.7)=8, sprint=7.
        assert_eq!(draft.phases.len(), 3);
        assert_eq!(draft.phases[0].name, "基础期");
        assert_eq!(draft.phases[0].start, "2030-01-03");
        assert_eq!(draft.phases[0].end, "2030-01-16");
        assert_eq!(draft.phases[1].name, "强化期");
        assert_eq!(draft.phases[2].name, "冲刺期");
        assert_eq!(draft.phases[2].end, "2030-01-31");
        assert_eq!(
            draft.summary.total_tasks as usize,
            draft
                .daily_plans
                .iter()
                .map(|d| d.tasks.len())
                .sum::<usize>()
        );
        assert!(draft.summary.avg_daily_min > 0);
    }

    #[test]
    fn short_ranges_collapse_into_a_single_sprint_phase() {
        let draft = build_draft(input("2030-01-08", "2030-01-03", 6.0)).unwrap();
        assert_eq!(draft.total_days, 5);
        assert_eq!(draft.phases.len(), 1);
        assert_eq!(draft.phases[0].name, "冲刺期");
        assert_eq!(draft.phases[0].start, "2030-01-03");
        assert_eq!(draft.phases[0].end, "2030-01-07");
    }

    #[test]
    fn subject_rotation_uses_gap_weights_and_skips_minor_days() {
        // 数学 gap=(6-3)*2=6; 英语 gap=(6-4)*1=2; share 6/8 vs 2/8.
        let draft = build_draft(input("2030-01-10", "2030-01-04", 4.0)).unwrap();
        // 240 min total: math 180, english 60 (both >= 10, both present).
        let day = &draft.daily_plans[0];
        assert_eq!(day.tasks.len(), 2);
        assert_eq!(day.tasks[0].subject_id, "sub-math");
        assert_eq!(day.tasks[0].duration_min, 180);
        assert_eq!(day.tasks[1].subject_id, "sub-eng");
        assert_eq!(day.tasks[1].duration_min, 60);
        // Low-mastery knowledge point first, then rotation.
        assert_eq!(day.tasks[0].knowledge_point_id.as_deref(), Some("kp-func"));
        assert_eq!(day.tasks[0].task, "学习：函数");
        let second = &draft.daily_plans[1];
        assert_eq!(
            second.tasks[0].knowledge_point_id.as_deref(),
            Some("kp-geo")
        );

        // A tiny share must be skipped (< 10 min): hours=0.5 → 30 min total,
        // english share 30*2/8=7.5 → rounded 8 < 10 → skipped.
        let draft = build_draft(input("2030-01-10", "2030-01-04", 0.5)).unwrap();
        let day = &draft.daily_plans[0];
        assert_eq!(day.tasks.len(), 1);
        assert_eq!(day.tasks[0].subject_id, "sub-math");
    }

    #[test]
    fn sunday_gets_half_the_capacity_and_review_tasks() {
        // 2030-01-06 is a Sunday.
        let draft = build_draft(input("2030-01-10", "2030-01-06", 6.0)).unwrap();
        let day = &draft.daily_plans[0];
        let total: i64 = day.tasks.iter().map(|t| t.duration_min).sum();
        // 180 min instead of 360; every task is a 复习 task.
        assert_eq!(total, 180);
        assert!(day.tasks.iter().all(|t| t.task.starts_with("复习：")));
    }

    #[test]
    fn conflicts_reflect_future_plans_and_kept_subjects() {
        let mut draft_input = input("2030-01-10", "2030-01-04", 6.0);
        draft_input.existing_future_plan_count = 7;
        draft_input.today_kept_subject_ids = vec!["sub-math".to_owned()];
        let draft = build_draft(draft_input).unwrap();
        assert!(draft
            .conflicts
            .iter()
            .any(|c| c.kind == DraftConflictKind::ReplaceFuturePlans && c.affected_count == 7));
        assert!(draft
            .conflicts
            .iter()
            .any(|c| c.kind == DraftConflictKind::KeepTodayActualSubjects));
    }

    #[test]
    fn reason_text_covers_all_four_branches() {
        assert_eq!(
            reason_text(2, 3, true),
            "掌握度偏低(2/5)且有3道未掌握错题，优先攻克"
        );
        assert_eq!(reason_text(4, 2, true), "有2道未掌握错题需巩固");
        assert_eq!(reason_text(1, 0, false), "自评掌握度较低(1/5)");
        assert_eq!(reason_text(4, 0, false), "巩固提升(4/5)");
    }

    #[test]
    fn draft_tasks_attach_evidence_with_mastery_wrong_count_and_days_to_exam() {
        let mut draft_input = input("2030-01-10", "2030-01-04", 4.0);
        // kp-func：掌握度 2、未掌握错题 3（低掌握优先排期，占据第 0 天数学任务）。
        draft_input.knowledge_points[0].wrong_count = 3;
        let draft = build_draft(draft_input).unwrap();

        let task = &draft.daily_plans[0].tasks[0];
        assert_eq!(task.knowledge_point_id.as_deref(), Some("kp-func"));
        let evidence = task.evidence.as_ref().expect("kp 任务必须携带 evidence");
        assert_eq!(evidence.mastery, 2);
        assert_eq!(evidence.wrong_question_count, 3);
        assert_eq!(evidence.days_to_exam, Some(6)); // 2030-01-10 − 2030-01-04
        assert_eq!(
            evidence.reason,
            "掌握度偏低(2/5)且有3道未掌握错题，优先攻克"
        );

        // 无知识点的“综合复习”任务不携带 evidence。
        let mut draft_input = input("2030-01-10", "2030-01-04", 4.0);
        draft_input.knowledge_points.clear();
        let draft = build_draft(draft_input).unwrap();
        let task = &draft.daily_plans[0].tasks[0];
        assert_eq!(task.knowledge_point_id, None);
        assert!(task.evidence.is_none());
    }

    #[test]
    fn past_or_today_exam_date_is_rejected() {
        assert!(build_draft(input("2030-01-04", "2030-01-04", 6.0)).is_err());
        assert!(build_draft(input("2030-01-03", "2030-01-04", 6.0)).is_err());
    }

    #[test]
    fn no_subjects_is_rejected() {
        let mut draft_input = input("2030-01-10", "2030-01-04", 6.0);
        draft_input.subjects = Vec::new();
        assert!(build_draft(draft_input).is_err());
    }

    /// Fixture equivalence (Mandatory Task C): the draft must reproduce the
    /// retired TypeScript local generator's output for a fixed 30-day, 2-subject,
    /// 3-knowledge-point case (generated independently by
    /// scripts/gen-plan-draft-fixture.py). Any drift in phase split, subject
    /// rotation, Sunday half-capacity, or task naming breaks this test.
    #[test]
    fn fixture_equivalence_reproduces_the_retired_local_generator() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/plan-draft-rotation-30d.json"
        ))
        .expect("fixture must parse");
        let draft = build_draft(DraftInput {
            exam_id: "exam-a",
            exam_date: fixture["input"]["exam_date"].as_str().unwrap(),
            start_date: fixture["input"]["start_date"].as_str().unwrap(),
            daily_hours: fixture["input"]["daily_hours"].as_f64().unwrap(),
            subjects: sample_subjects(),
            knowledge_points: sample_kps(),
            existing_future_plan_count: 0,
            today_kept_subject_ids: Vec::new(),
        })
        .unwrap();

        assert_eq!(draft.total_days, fixture["total_days"].as_i64().unwrap());
        assert_eq!(draft.phases.len(), 3);
        for (index, phase) in draft.phases.iter().enumerate() {
            assert_eq!(
                phase.name,
                fixture["phases"][index]["name"].as_str().unwrap()
            );
            assert_eq!(
                phase.start,
                fixture["phases"][index]["start"].as_str().unwrap()
            );
            assert_eq!(phase.end, fixture["phases"][index]["end"].as_str().unwrap());
        }
        assert_eq!(
            draft.daily_plans.len(),
            fixture["daily_plans"].as_array().unwrap().len()
        );
        for (day, expected) in draft
            .daily_plans
            .iter()
            .zip(fixture["daily_plans"].as_array().unwrap())
        {
            assert_eq!(day.date, expected["date"].as_str().unwrap());
            assert_eq!(day.tasks.len(), expected["tasks"].as_array().unwrap().len());
            for (task, expected_task) in day.tasks.iter().zip(expected["tasks"].as_array().unwrap())
            {
                assert_eq!(
                    task.subject_id,
                    expected_task["subject_id"].as_str().unwrap()
                );
                assert_eq!(
                    task.duration_min,
                    expected_task["duration_min"].as_i64().unwrap()
                );
                assert_eq!(task.task, expected_task["task"].as_str().unwrap());
                assert_eq!(
                    task.knowledge_point_id.as_deref(),
                    expected_task["knowledge_point_id"].as_str()
                );
            }
        }
        assert_eq!(
            draft.summary.total_tasks,
            fixture["summary"]["total_tasks"].as_i64().unwrap()
        );
        assert_eq!(
            draft.summary.total_duration_min,
            fixture["summary"]["total_duration_min"].as_i64().unwrap()
        );
        assert_eq!(
            draft.summary.avg_daily_min,
            fixture["summary"]["avg_daily_min"].as_i64().unwrap()
        );
    }
}
