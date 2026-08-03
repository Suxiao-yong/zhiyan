// Daily brief (M4 Task 4): a local skeleton (today plans, overdue count, week
// completion, due wrong questions, weak areas, confirmed memory hints) always
// produced from Analytics + Memory, plus an optional LLM explanation paragraph
// when a provider is configured and healthy. Any provider error degrades to
// the local brief; the brief never fails the calling job.

use serde::Serialize;
use serde_json::json;
use sqlx::SqlitePool;

use crate::agent::error::AgentError;
use crate::agent::llm::{LlmProvider, ProviderMessage};
use crate::agent::memory::MemoryRepository;
use crate::analytics::{Analytics, DayStats, WeakArea};

#[derive(Debug, Clone, Serialize)]
pub struct Brief {
    pub date: String,
    /// "model" when an LLM explanation was appended, "local" otherwise.
    pub mode: String,
    pub summary: String,
    pub explanation: Option<String>,
    pub today_planned: i64,
    pub today_completed: i64,
    pub today_duration_min: i64,
    pub overdue_count: i64,
    pub week_completion_rate: f64,
    pub due_wrong_questions: i64,
    pub weak_areas: Vec<WeakArea>,
}

#[derive(Clone)]
pub(crate) struct BriefBuilder {
    pool: SqlitePool,
    analytics: Analytics,
    memory: MemoryRepository,
}

impl BriefBuilder {
    pub(crate) fn new(pool: SqlitePool, analytics: Analytics, memory: MemoryRepository) -> Self {
        Self {
            pool,
            analytics,
            memory,
        }
    }

    /// Build today's brief. `exam_id` selects the target exam; pass the
    /// resolved active exam (Scheduler resolves it, falling back to the most
    /// recently active exam). `provider` may be None for a pure local brief.
    pub(crate) async fn build(
        &self,
        exam_id: Option<&str>,
        today: &str,
        provider: Option<&LlmProvider>,
    ) -> Result<Brief, AgentError> {
        let Some(exam_id) = exam_id else {
            return Ok(Brief {
                date: today.to_owned(),
                mode: "local".to_owned(),
                summary: "尚未选择考试，暂无简报内容。".to_owned(),
                explanation: None,
                today_planned: 0,
                today_completed: 0,
                today_duration_min: 0,
                overdue_count: 0,
                week_completion_rate: 0.0,
                due_wrong_questions: 0,
                weak_areas: Vec::new(),
            });
        };

        let today_stats = self.analytics.day_stats(exam_id, today).await?;
        let overdue = self.analytics.overdue_plans(exam_id, today).await?;
        let monday = monday_of(today);
        let week = self.analytics.week_stats(exam_id, &monday).await?;
        let due_wrong_questions = self.due_wrong_questions(exam_id, today).await?;
        let weak_areas = self.analytics.weak_areas(exam_id, 0.6, 5).await?;
        let memories = self.memory.relevant(Some(exam_id), 3).await?;

        let summary = build_summary(
            today,
            &today_stats,
            overdue.len() as i64,
            &week,
            due_wrong_questions,
            &memories
                .iter()
                .map(|entry| entry.content.clone())
                .collect::<Vec<_>>(),
        );

        let explanation = match provider {
            Some(provider) => match self.explain_with_llm(provider, &summary).await {
                Ok(Some(text)) => Some(text),
                _ => None,
            },
            None => None,
        };

        Ok(Brief {
            date: today.to_owned(),
            mode: if explanation.is_some() {
                "model".to_owned()
            } else {
                "local".to_owned()
            },
            summary,
            explanation,
            today_planned: today_stats.planned,
            today_completed: today_stats.completed,
            today_duration_min: today_stats.actual_duration_min,
            overdue_count: overdue.len() as i64,
            week_completion_rate: week.completion_rate,
            due_wrong_questions,
            weak_areas,
        })
    }

    /// Unmastered wrong questions not reviewed within the last 7 days.
    async fn due_wrong_questions(&self, exam_id: &str, today: &str) -> Result<i64, AgentError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM wrong_questions wq \
             JOIN subjects s ON s.id = wq.subject_id \
             WHERE s.exam_id = ? AND wq.mastered = 0 \
               AND (wq.last_review_at IS NULL OR wq.last_review_at < date(?,'-7 days'))",
        )
        .bind(exam_id)
        .bind(today)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx)?;
        Ok(count)
    }

    /// One tool-free model turn that turns the local skeleton into an
    /// explanation paragraph. Never fails the brief: any provider error or a
    /// local-mode result returns None.
    async fn explain_with_llm(
        &self,
        provider: &LlmProvider,
        summary: &str,
    ) -> Result<Option<String>, AgentError> {
        let messages = vec![
            ProviderMessage {
                role: "system".into(),
                content: Some(
                    "你是智研的学习顾问。根据用户简报骨架，用 2-4 句中文给出今日学习建议，\
                     不要重复骨架数字，不编造骨架中没有的事实。"
                        .to_owned(),
                ),
                tool_calls: None,
                tool_call_id: None,
            },
            ProviderMessage {
                role: "user".into(),
                content: Some(format!("今日简报骨架：\n{summary}")),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        let mut chunks = Vec::new();
        match provider
            .chat_stream(&messages, &[], &mut |chunk| chunks.push(chunk.to_owned()))
            .await
        {
            Ok(response) => {
                let text = response.content.unwrap_or_default().trim().to_owned();
                Ok(if text.is_empty() { None } else { Some(text) })
            }
            Err(_) => Ok(None),
        }
    }
}

/// Monday of the week containing `date` (`YYYY-MM-DD`), shared by the brief
/// and the weekly report job.
pub(crate) fn monday_of(date: &str) -> String {
    use chrono::Datelike;
    match chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d") {
        Ok(parsed) => {
            let weekday = parsed.weekday().num_days_from_monday();
            (parsed - chrono::Days::new(weekday as u64))
                .format("%Y-%m-%d")
                .to_string()
        }
        Err(_) => date.to_owned(),
    }
}

fn build_summary(
    _today: &str,
    today_stats: &DayStats,
    overdue_count: i64,
    week: &DayStats,
    due_wrong_questions: i64,
    memory_hints: &[String],
) -> String {
    let mut parts = vec![format!(
        "今日计划 {} 项，已完成 {} 项（完成率 {:.0}%），已记录学习时长 {} 分钟。",
        today_stats.planned,
        today_stats.completed,
        today_stats.completion_rate * 100.0,
        today_stats.actual_duration_min,
    )];
    if overdue_count > 0 {
        parts.push(format!("有 {overdue_count} 项逾期计划尚未完成。"));
    }
    parts.push(format!("本周完成率 {:.0}%。", week.completion_rate * 100.0));
    if due_wrong_questions > 0 {
        parts.push(format!("有 {due_wrong_questions} 道错题待复习。"));
    }
    if let Some(hint) = memory_hints.first() {
        parts.push(format!("（记忆提示：{hint}）"));
    }
    parts.join("")
}

fn map_sqlx(_error: sqlx::Error) -> AgentError {
    AgentError::Persistence("brief query failed".to_owned())
}

/// The payload a daily_brief job should store in agent_jobs.last_result and
/// emit as the `agent-daily-brief` event.
pub fn brief_payload(brief: &Brief) -> serde_json::Value {
    json!({
        "date": brief.date,
        "mode": brief.mode,
        "summary": brief.summary,
        "explanation": brief.explanation,
        "today_planned": brief.today_planned,
        "today_completed": brief.today_completed,
        "today_duration_min": brief.today_duration_min,
        "overdue_count": brief.overdue_count,
        "week_completion_rate": brief.week_completion_rate,
        "due_wrong_questions": brief.due_wrong_questions,
        "weak_areas": brief.weak_areas,
    })
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    async fn brief_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        for migration in crate::db::migrations() {
            sqlx::raw_sql(migration.sql).execute(&pool).await.unwrap();
        }
        sqlx::raw_sql(
            r#"
            INSERT INTO exams (id, name, exam_date) VALUES ('exam-a', 'A', '2030-06-01');
            INSERT INTO subjects (id, exam_id, name) VALUES ('sub-math', 'exam-a', '数学');
            INSERT INTO knowledge_points (id, subject_id, name) VALUES ('kp-func', 'sub-math', '函数');
            INSERT INTO study_plans (id, exam_id, subject_id, date, planned_duration, status) VALUES
                ('p-1', 'exam-a', 'sub-math', '2026-07-18', 60, 'completed'),
                ('p-2', 'exam-a', 'sub-math', '2026-07-18', 30, 'pending'),
                ('p-old', 'exam-a', 'sub-math', '2026-07-15', 45, 'pending');
            INSERT INTO study_records (id, date, subject_id, knowledge_point_id, duration_min, questions_count, correct_count) VALUES
                ('r-1', '2026-07-18', 'sub-math', 'kp-func', 90, 10, 3);
            INSERT INTO wrong_questions (id, subject_id, knowledge_point_id, mastered, last_review_at) VALUES
                ('w-1', 'sub-math', 'kp-func', 0, NULL),
                ('w-2', 'sub-math', 'kp-func', 0, '2026-07-05'),
                ('w-3', 'sub-math', 'kp-func', 1, NULL);
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn local_brief_aggregates_today_overdue_week_and_due_wrong_questions() {
        let pool = brief_pool().await;
        let builder = BriefBuilder::new(
            pool.clone(),
            Analytics::new(pool.clone()),
            MemoryRepository::new(pool),
        );
        let brief = builder
            .build(Some("exam-a"), "2026-07-18", None)
            .await
            .unwrap();

        assert_eq!(brief.mode, "local");
        assert!(brief.explanation.is_none());
        assert_eq!(brief.today_planned, 2);
        assert_eq!(brief.today_completed, 1);
        assert_eq!(brief.today_duration_min, 90);
        assert_eq!(brief.overdue_count, 1);
        assert_eq!(brief.due_wrong_questions, 2);
        assert!(brief.summary.contains("今日计划 2 项"));
        assert!(brief.summary.contains("逾期"));
        assert!(brief.summary.contains("错题"));
    }

    #[tokio::test]
    async fn no_exam_returns_an_empty_local_brief() {
        let pool = brief_pool().await;
        let builder = BriefBuilder::new(
            pool.clone(),
            Analytics::new(pool.clone()),
            MemoryRepository::new(pool),
        );
        let brief = builder.build(None, "2026-07-18", None).await.unwrap();
        assert_eq!(brief.mode, "local");
        assert_eq!(brief.today_planned, 0);
        assert!(brief.summary.contains("尚未选择考试"));
    }

    #[tokio::test]
    async fn monday_of_rolls_back_to_the_week_start() {
        assert_eq!(monday_of("2026-07-18"), "2026-07-13");
        assert_eq!(monday_of("2026-07-13"), "2026-07-13");
        assert_eq!(monday_of("not-a-date"), "not-a-date");
    }
}
