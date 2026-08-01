// Local aggregation (M4 Task 3): the Fallback Engine core — pure SQL over the
// business tables, no LLM. Feeds the daily brief (Task 4) and the reminder /
// overdue jobs (Task 5) with overdue plans, day/week statistics, and rule-based
// weak-area detection. Dates are local `YYYY-MM-DD` strings aligned with the
// app's business-day rules.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::agent::error::AgentError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverduePlan {
    pub id: String,
    pub subject_id: String,
    pub date: String,
    pub planned_duration: Option<i64>,
    pub status: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DayStats {
    pub planned: i64,
    pub completed: i64,
    pub skipped: i64,
    /// completed / planned (0.0 when nothing is planned).
    pub completion_rate: f64,
    /// Total minutes recorded for the exam+period (from study_records).
    pub actual_duration_min: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeakArea {
    pub subject_id: String,
    pub subject_name: String,
    pub knowledge_point_id: Option<String>,
    pub knowledge_point_name: Option<String>,
    pub total_questions: i64,
    pub correct_questions: i64,
    pub correctness: f64,
}

#[derive(Clone)]
pub struct Analytics {
    pool: SqlitePool,
}

impl Analytics {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Plans before `today` that still need attention (pending or in_progress).
    pub async fn overdue_plans(
        &self,
        exam_id: &str,
        today: &str,
    ) -> Result<Vec<OverduePlan>, AgentError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: String,
            subject_id: String,
            date: String,
            planned_duration: Option<i64>,
            status: String,
        }
        let rows = sqlx::query_as::<_, Row>(
            "SELECT id, subject_id, date, planned_duration, status \
             FROM study_plans \
             WHERE exam_id = ? AND date < ? AND status IN ('pending','in_progress') \
             ORDER BY date, id",
        )
        .bind(exam_id)
        .bind(today)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx)?;
        Ok(rows
            .into_iter()
            .map(|row| OverduePlan {
                id: row.id,
                subject_id: row.subject_id,
                date: row.date,
                planned_duration: row.planned_duration,
                status: row.status,
            })
            .collect())
    }

    /// One day's plan/record summary for an exam.
    pub async fn day_stats(&self, exam_id: &str, date: &str) -> Result<DayStats, AgentError> {
        self.period_stats(exam_id, date, date).await
    }

    /// A 7-day window starting at `monday` (inclusive) for an exam.
    pub async fn week_stats(&self, exam_id: &str, monday: &str) -> Result<DayStats, AgentError> {
        let monday_date = chrono::NaiveDate::parse_from_str(monday, "%Y-%m-%d")
            .map_err(|_| AgentError::Persistence("invalid week start date".to_owned()))?;
        let sunday = monday_date + chrono::Days::new(6);
        self.period_stats(exam_id, monday, &sunday.format("%Y-%m-%d").to_string())
            .await
    }

    /// Plan/record summary over a date range (inclusive). The plan side counts
    /// rows whose `date` falls in the range; the duration side joins records
    /// through subjects to stay exam-scoped.
    async fn period_stats(
        &self,
        exam_id: &str,
        start: &str,
        end: &str,
    ) -> Result<DayStats, AgentError> {
        let (planned, completed, skipped): (i64, i64, i64) = sqlx::query_as(
            "SELECT \
               COUNT(*) FILTER (WHERE status != 'skipped'), \
               COUNT(*) FILTER (WHERE status = 'completed'), \
               COUNT(*) FILTER (WHERE status = 'skipped') \
             FROM study_plans WHERE exam_id = ? AND date BETWEEN ? AND ?",
        )
        .bind(exam_id)
        .bind(start)
        .bind(end)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx)?;
        let actual_duration_min: Option<i64> = sqlx::query_scalar(
            "SELECT COALESCE(SUM(r.duration_min), 0) \
             FROM study_records r JOIN subjects s ON s.id = r.subject_id \
             WHERE s.exam_id = ? AND r.date BETWEEN ? AND ?",
        )
        .bind(exam_id)
        .bind(start)
        .bind(end)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx)?;
        let completion_rate = if planned > 0 {
            completed as f64 / planned as f64
        } else {
            0.0
        };
        Ok(DayStats {
            planned,
            completed,
            skipped,
            completion_rate,
            actual_duration_min: actual_duration_min.unwrap_or(0),
        })
    }

    /// Rule-based weak areas: knowledge-point groups whose question correctness
    /// is below `threshold` (default 0.6), worst first, capped at `limit`.
    pub async fn weak_areas(
        &self,
        exam_id: &str,
        threshold: f64,
        limit: i64,
    ) -> Result<Vec<WeakArea>, AgentError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            subject_id: String,
            subject_name: String,
            knowledge_point_id: Option<String>,
            knowledge_point_name: Option<String>,
            total_questions: i64,
            correct_questions: i64,
        }
        let rows = sqlx::query_as::<_, Row>(
            "SELECT r.subject_id, s.name AS subject_name, r.knowledge_point_id, \
                    COALESCE(kp.name, '') AS knowledge_point_name, \
                    SUM(r.questions_count) AS total_questions, \
                    SUM(r.correct_count) AS correct_questions \
             FROM study_records r \
             JOIN subjects s ON s.id = r.subject_id \
             LEFT JOIN knowledge_points kp ON kp.id = r.knowledge_point_id \
             WHERE s.exam_id = ? AND r.questions_count > 0 \
             GROUP BY r.subject_id, r.knowledge_point_id \
             HAVING SUM(r.questions_count) > 0 \
                AND (1.0 * SUM(r.correct_count) / SUM(r.questions_count)) < ? \
             ORDER BY (1.0 * SUM(r.correct_count) / SUM(r.questions_count)) ASC \
             LIMIT ?",
        )
        .bind(exam_id)
        .bind(threshold)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx)?;
        Ok(rows
            .into_iter()
            .map(|row| WeakArea {
                correctness: if row.total_questions > 0 {
                    row.correct_questions as f64 / row.total_questions as f64
                } else {
                    0.0
                },
                subject_id: row.subject_id,
                subject_name: row.subject_name,
                knowledge_point_id: row.knowledge_point_id,
                knowledge_point_name: row.knowledge_point_name,
                total_questions: row.total_questions,
                correct_questions: row.correct_questions,
            })
            .collect())
    }
}

fn map_sqlx(_error: sqlx::Error) -> AgentError {
    AgentError::Persistence("analytics query failed".to_owned())
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    async fn analytics_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        for migration in crate::db::migrations() {
            sqlx::raw_sql(migration.sql).execute(&pool).await.unwrap();
        }
        seed(&pool).await;
        pool
    }

    async fn seed(pool: &SqlitePool) {
        sqlx::raw_sql(
            r#"
            INSERT INTO exams (id, name, exam_date) VALUES ('exam-a', 'A', '2030-06-01');
            INSERT INTO subjects (id, exam_id, name) VALUES
                ('sub-math', 'exam-a', '数学'),
                ('sub-eng', 'exam-a', '英语');
            INSERT INTO knowledge_points (id, subject_id, name) VALUES
                ('kp-func', 'sub-math', '函数'),
                ('kp-geo', 'sub-math', '几何');

            -- Overdue: two pending plans before today; one completed before today is not overdue.
            INSERT INTO study_plans (id, exam_id, subject_id, date, planned_duration, status) VALUES
                ('plan-old-1', 'exam-a', 'sub-math', '2026-07-15', 60, 'pending'),
                ('plan-old-2', 'exam-a', 'sub-eng', '2026-07-16', 30, 'in_progress'),
                ('plan-done', 'exam-a', 'sub-math', '2026-07-15', 60, 'completed'),
                ('plan-today', 'exam-a', 'sub-math', '2026-07-18', 45, 'completed'),
                ('plan-today-2', 'exam-a', 'sub-eng', '2026-07-18', 20, 'pending'),
                ('plan-skip', 'exam-a', 'sub-math', '2026-07-18', 10, 'skipped'),
                ('plan-next', 'exam-a', 'sub-math', '2026-07-19', 40, 'pending');

            -- Records: 120 min on 07-18, 60 min on 07-17 (week window Monday 07-13).
            INSERT INTO study_records (id, date, subject_id, knowledge_point_id, duration_min, questions_count, correct_count) VALUES
                ('rec-1', '2026-07-18', 'sub-math', 'kp-func', 90, 10, 3),
                ('rec-2', '2026-07-18', 'sub-eng', NULL, 30, 5, 5),
                ('rec-3', '2026-07-17', 'sub-math', 'kp-geo', 60, 8, 4),
                ('rec-4', '2026-07-13', 'sub-math', 'kp-geo', 45, 6, 2);
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn overdue_plans_only_returns_unfinished_past_plans() {
        let pool = analytics_pool().await;
        let analytics = Analytics::new(pool);
        let overdue = analytics
            .overdue_plans("exam-a", "2026-07-18")
            .await
            .unwrap();
        let ids: Vec<&str> = overdue.iter().map(|plan| plan.id.as_str()).collect();
        assert_eq!(ids, vec!["plan-old-1", "plan-old-2"]);
        assert_eq!(overdue[0].planned_duration, Some(60));
        assert_eq!(overdue[1].status, "in_progress");
    }

    #[tokio::test]
    async fn day_stats_counts_plans_and_joined_duration() {
        let pool = analytics_pool().await;
        let analytics = Analytics::new(pool);
        let stats = analytics.day_stats("exam-a", "2026-07-18").await.unwrap();
        assert_eq!(stats.planned, 2); // plan-today + plan-today-2 (skipped excluded)
        assert_eq!(stats.completed, 1);
        assert_eq!(stats.skipped, 1);
        assert!((stats.completion_rate - 0.5).abs() < 1e-9);
        assert_eq!(stats.actual_duration_min, 120);
    }

    #[tokio::test]
    async fn week_stats_covers_a_seven_day_window() {
        let pool = analytics_pool().await;
        let analytics = Analytics::new(pool);
        // Monday 2026-07-13 through Sunday 2026-07-19.
        let stats = analytics.week_stats("exam-a", "2026-07-13").await.unwrap();
        // Non-skipped plans dated 07-13..07-19: old-1, old-2, done, today, today-2, next.
        assert_eq!(stats.planned, 6);
        assert_eq!(stats.completed, 2);
        assert_eq!(stats.actual_duration_min, 225); // 90 + 30 + 60 + 45
    }

    #[tokio::test]
    async fn weak_areas_rank_below_threshold_worst_first() {
        let pool = analytics_pool().await;
        let analytics = Analytics::new(pool);
        // 函数 10/3 = 0.3; 几何 8/4 = 0.5; 英语 5/5 = 1.0.
        let weak = analytics.weak_areas("exam-a", 0.6, 10).await.unwrap();
        assert_eq!(weak.len(), 2);
        assert_eq!(weak[0].knowledge_point_name.as_deref(), Some("函数"));
        assert!((weak[0].correctness - 0.3).abs() < 1e-9);
        assert_eq!(weak[1].knowledge_point_name.as_deref(), Some("几何"));
        assert_eq!(weak[1].subject_name, "数学");

        let none = analytics.weak_areas("exam-a", 0.1, 10).await.unwrap();
        assert!(none.is_empty());
    }

    #[tokio::test]
    async fn empty_exam_returns_empty_aggregations() {
        let pool = analytics_pool().await;
        let analytics = Analytics::new(pool);
        assert!(analytics
            .overdue_plans("exam-nope", "2026-07-18")
            .await
            .unwrap()
            .is_empty());
        let stats = analytics
            .day_stats("exam-nope", "2026-07-18")
            .await
            .unwrap();
        assert_eq!(stats.planned, 0);
        assert_eq!(stats.completion_rate, 0.0);
        assert_eq!(stats.actual_duration_min, 0);
        assert!(analytics
            .weak_areas("exam-nope", 0.6, 10)
            .await
            .unwrap()
            .is_empty());
    }
}
