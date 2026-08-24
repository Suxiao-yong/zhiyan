// Rust Scheduler (M4): background job lifecycle.
//
// Task 1 added the pause/state surface used by the tray. Task 2 adds the
// agent_jobs table (v8 migration), the tick loop with atomic claim, and
// per-type dispatch. Task 12 shrinks the runtime to the retained local
// reminders only (task_reminder, overdue_check); the daily_brief,
// weekly_report, retry_failed and cleanup_failed job types are deprecated —
// legacy rows still parse and list, but new code never schedules them and
// their dispatch is a no-op skip. The daily brief itself is an on-demand
// local read (see brief.rs), independent of the Scheduler.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::agent::error::AgentError;
use crate::analytics::Analytics;
use crate::notify::NotificationBus;

/// Reminder jobs are suppressed while `agent_reminders_paused` is `1`.
pub const REMINDERS_PAUSED_KEY: &str = "agent_reminders_paused";

/// Background job types. Since Task 12 only `task_reminder` and `overdue_check`
/// are scheduled and dispatched; the other variants are retained so legacy
/// `agent_jobs` rows (migration 8, never dropped) still parse and list. New
/// code must not schedule the deprecated types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobType {
    DailyBrief,
    TaskReminder,
    OverdueCheck,
    WeeklyReport,
    RetryFailed,
    CleanupFailed,
}

impl JobType {
    pub const ALL: [JobType; 6] = [
        JobType::DailyBrief,
        JobType::TaskReminder,
        JobType::OverdueCheck,
        JobType::WeeklyReport,
        JobType::RetryFailed,
        JobType::CleanupFailed,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            JobType::DailyBrief => "daily_brief",
            JobType::TaskReminder => "task_reminder",
            JobType::OverdueCheck => "overdue_check",
            JobType::WeeklyReport => "weekly_report",
            JobType::RetryFailed => "retry_failed",
            JobType::CleanupFailed => "cleanup_failed",
        }
    }

    pub fn parse(value: &str) -> Option<JobType> {
        JobType::ALL
            .into_iter()
            .find(|job_type| job_type.as_str() == value)
    }

    /// Whether the job is suppressed by the reminders pause. Task 12 also uses
    /// this to allow only reminder types through the schedule command.
    pub fn is_reminder(self) -> bool {
        matches!(self, JobType::TaskReminder | JobType::OverdueCheck)
    }
}

/// One row of agent_jobs, surfaced to the hidden debug page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: String,
    pub job_type: JobType,
    pub dedup_key: String,
    pub scheduled_at: String,
    pub status: String,
    pub last_result: Value,
    pub retry_at: Option<String>,
    pub runs: i64,
    pub last_run_at: Option<String>,
    pub created_at: String,
}

/// What a job handler produced: a completed run or a deliberately skipped run
/// (e.g. reminders paused, deprecated job type). No handler retries since
/// Task 12; the failed/retry path is gone.
enum JobOutcome {
    Done(Value),
    Skipped(Value),
}

#[derive(Clone)]
pub struct Scheduler {
    pool: SqlitePool,
    notifications: NotificationBus,
}

impl Scheduler {
    pub fn new(pool: SqlitePool, notifications: NotificationBus) -> Self {
        Self {
            pool,
            notifications,
        }
    }

    /// Whether reminder-type jobs are paused.
    pub async fn reminders_paused(&self) -> Result<bool, AgentError> {
        let value: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
            .bind(REMINDERS_PAUSED_KEY)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_sqlx)?;
        Ok(value.as_deref() == Some("1"))
    }

    /// Set the pause flag and return the new value.
    pub async fn set_reminders_paused(&self, paused: bool) -> Result<bool, AgentError> {
        let value = if paused { "1" } else { "0" };
        sqlx::query(
            "INSERT INTO settings (key, value, description) VALUES (?, ?, ?) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(REMINDERS_PAUSED_KEY)
        .bind(value)
        .bind("1=pause reminder jobs (tray toggle)")
        .execute(&self.pool)
        .await
        .map_err(map_sqlx)?;
        Ok(paused)
    }

    /// Schedule a job instance. The dedup key is globally unique: scheduling
    /// an existing key is a no-op (INSERT OR IGNORE), so repeated ticks or
    /// restarts never double-create the same logical job.
    pub async fn schedule(
        &self,
        job_type: JobType,
        dedup_key: &str,
        scheduled_at: &str,
    ) -> Result<Option<String>, AgentError> {
        let id = uuid::Uuid::new_v4().to_string();
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO agent_jobs (id, job_type, dedup_key, scheduled_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(job_type.as_str())
        .bind(dedup_key)
        .bind(scheduled_at)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx)?
        .rows_affected();
        if inserted == 0 {
            return Ok(None);
        }
        Ok(Some(id))
    }

    /// Run every due job once, after ensuring today's daily jobs are scheduled.
    /// `now` is passed in so tests control the clock (`"YYYY-MM-DD HH:MM:SS"`,
    /// local time, same format as the DB defaults). Returns how many jobs ran
    /// (including skips).
    pub async fn tick(&self, now: &str) -> Result<usize, AgentError> {
        self.ensure_today_jobs(now).await?;
        let due: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, job_type FROM agent_jobs \
             WHERE status = 'scheduled' AND scheduled_at <= ? \
             ORDER BY scheduled_at",
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx)?;

        let mut ran = 0;
        for (id, job_type) in due {
            let Some(job_type) = JobType::parse(&job_type) else {
                continue;
            };
            // Atomic claim: only one runner transitions scheduled -> running.
            let claimed = sqlx::query(
                "UPDATE agent_jobs SET status = 'running', last_run_at = ? \
                 WHERE id = ? AND status = 'scheduled'",
            )
            .bind(now)
            .bind(&id)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx)?
            .rows_affected();
            if claimed == 0 {
                continue;
            }
            let outcome = self.dispatch(&job_type, now).await;
            self.record_outcome(&id, outcome).await?;
            ran += 1;
        }
        Ok(ran)
    }

    /// Startup catch-up: ensure today's daily jobs (brief, overdue check, task
    /// reminder) exist after a restart or sleep/wake. Never replays failed
    /// user-visible writes. `tick` also calls the same ensure, so day rollover
    /// while running is covered too.
    pub async fn bootstrap(&self, now: &str) -> Result<usize, AgentError> {
        self.ensure_today_jobs(now).await
    }

    /// Ensure today's reminder jobs exist (overdue check at 09:00, task
    /// reminder at the configured reminder time). Called on every tick, so a
    /// restart or a day rollover re-creates the day's jobs exactly once
    /// (dedup keys are date-scoped). Task 12: daily_brief / weekly_report are
    /// no longer scheduled. Returns how many were created.
    async fn ensure_today_jobs(&self, now: &str) -> Result<usize, AgentError> {
        let today = &now[..10];
        let reminder_time = self.reminder_time().await?;
        let mut created = 0;
        for (job_type, dedup, scheduled_at) in [
            (
                JobType::OverdueCheck,
                format!("overdue_check:{today}"),
                format!("{today} 09:00:00"),
            ),
            (
                JobType::TaskReminder,
                format!("task_reminder:{today}"),
                format!("{today} {reminder_time}:00"),
            ),
        ] {
            if self
                .schedule(job_type, &dedup, &scheduled_at)
                .await?
                .is_some()
            {
                created += 1;
            }
        }
        Ok(created)
    }

    /// The daily task-reminder clock time (`HH:MM`), from the
    /// `reminder_time` setting (the key the frontend Settings store writes),
    /// defaulting to `19:00`.
    async fn reminder_time(&self) -> Result<String, AgentError> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'reminder_time'")
                .fetch_optional(&self.pool)
                .await
                .map_err(map_sqlx)?;
        Ok(value
            .filter(|raw| raw.len() == 5 && raw.as_bytes()[2] == b':')
            .unwrap_or_else(|| "19:00".to_owned()))
    }

    /// The user-facing notification switch (`notification_enabled`, default
    /// on, same semantics as the frontend: anything except `"false"` is on).
    /// Reminders stay scheduled but never notify when the user turned
    /// notifications off in Settings.
    async fn notifications_enabled(&self) -> Result<bool, AgentError> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'notification_enabled'")
                .fetch_optional(&self.pool)
                .await
                .map_err(map_sqlx)?;
        Ok(value.map(|raw| raw != "false").unwrap_or(true))
    }

    /// Every job on record, newest first, for the hidden debug page.
    pub async fn list(&self, limit: i64) -> Result<Vec<JobRecord>, AgentError> {
        let rows = sqlx::query_as::<_, JobRow>(
            "SELECT id, job_type, dedup_key, scheduled_at, status, last_result, retry_at, \
             runs, last_run_at, created_at FROM agent_jobs ORDER BY rowid DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx)?;
        rows.into_iter().map(|row| row.try_into()).collect()
    }

    async fn dispatch(&self, job_type: &JobType, now: &str) -> JobOutcome {
        if job_type.is_reminder() {
            if self.reminders_paused().await.unwrap_or(false) {
                return JobOutcome::Skipped(json!({ "reason": "reminders paused" }));
            }
            if !self.notifications_enabled().await.unwrap_or(true) {
                return JobOutcome::Skipped(json!({ "reason": "notifications disabled" }));
            }
        }
        match job_type {
            // Task 12: the daily brief is an on-demand read; legacy scheduled
            // daily_brief rows are skipped, never executed.
            JobType::DailyBrief => {
                JobOutcome::Skipped(json!({ "note": "daily brief is on-demand" }))
            }
            JobType::TaskReminder => {
                let today = &now[..10];
                let analytics = Analytics::new(self.pool.clone());
                let exam_id = self.active_exam_id().await.unwrap_or(None);
                let Some(exam_id) = exam_id else {
                    return JobOutcome::Skipped(json!({ "reason": "no exam" }));
                };
                let stats = analytics
                    .day_stats(&exam_id, today)
                    .await
                    .unwrap_or_default();
                let unfinished = stats.planned - stats.completed - stats.skipped;
                if unfinished > 0 {
                    // Task 8: mention due reviews when any exist; a failed count
                    // silently skips the extra line so the main reminder holds.
                    let mut body = format!("今日还有 {unfinished} 项任务未完成。");
                    let due_reviews = crate::agent::tools::review::count_due(&self.pool, &exam_id)
                        .await
                        .unwrap_or(0);
                    if due_reviews > 0 {
                        body.push_str(&format!("今天还有 {due_reviews} 道错题待复习。"));
                    }
                    let _ = self.notifications.send("今日任务提醒", body);
                    JobOutcome::Done(
                        json!({ "unfinished": unfinished, "due_reviews": due_reviews }),
                    )
                } else {
                    JobOutcome::Done(json!({ "unfinished": 0, "note": "all done" }))
                }
            }
            JobType::OverdueCheck => {
                let today = &now[..10];
                let analytics = Analytics::new(self.pool.clone());
                let exam_id = self.active_exam_id().await.unwrap_or(None);
                let Some(exam_id) = exam_id else {
                    return JobOutcome::Skipped(json!({ "reason": "no exam" }));
                };
                let overdue = analytics
                    .overdue_plans(&exam_id, today)
                    .await
                    .unwrap_or_default();
                if !overdue.is_empty() {
                    let earliest = overdue
                        .iter()
                        .map(|plan| plan.date.as_str())
                        .min()
                        .unwrap_or("");
                    let _ = self.notifications.send(
                        "逾期计划提醒",
                        format!(
                            "有 {} 项逾期计划尚未完成（最早：{earliest}）。",
                            overdue.len()
                        ),
                    );
                    JobOutcome::Done(
                        json!({ "overdue_count": overdue.len(), "earliest": earliest }),
                    )
                } else {
                    JobOutcome::Done(json!({ "overdue_count": 0, "note": "none overdue" }))
                }
            }
            // Task 12: weekly reports are removed; legacy rows are skipped.
            JobType::WeeklyReport => {
                JobOutcome::Skipped(json!({ "note": "weekly report removed" }))
            }
            JobType::RetryFailed | JobType::CleanupFailed => {
                let _ = now;
                JobOutcome::Skipped(json!({ "note": "handler lands in M6" }))
            }
        }
    }

    /// The exam reminders target: the persisted `agent_active_exam_id`, or the
    /// most recently active exam as a fallback.
    async fn active_exam_id(&self) -> Result<Option<String>, AgentError> {
        let configured: Option<String> =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'agent_active_exam_id'")
                .fetch_optional(&self.pool)
                .await
                .map_err(map_sqlx)?;
        if let Some(exam_id) = configured.filter(|value| !value.trim().is_empty()) {
            return Ok(Some(exam_id));
        }
        let latest: Option<String> =
            sqlx::query_scalar("SELECT id FROM exams ORDER BY updated_at DESC, rowid DESC LIMIT 1")
                .fetch_optional(&self.pool)
                .await
                .map_err(map_sqlx)?;
        Ok(latest)
    }

    async fn record_outcome(&self, id: &str, outcome: JobOutcome) -> Result<(), AgentError> {
        match outcome {
            JobOutcome::Done(payload) => {
                sqlx::query(
                    "UPDATE agent_jobs SET status = 'completed', last_result = ?, runs = runs + 1, \
                     retry_at = NULL WHERE id = ?",
                )
                .bind(payload.to_string())
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(map_sqlx)?;
            }
            JobOutcome::Skipped(payload) => {
                sqlx::query(
                    "UPDATE agent_jobs SET status = 'completed', last_result = ?, runs = runs + 1 \
                     WHERE id = ?",
                )
                .bind(payload.to_string())
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(map_sqlx)?;
            }
        }
        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct JobRow {
    id: String,
    job_type: String,
    dedup_key: String,
    scheduled_at: String,
    status: String,
    last_result: Option<String>,
    retry_at: Option<String>,
    runs: i64,
    last_run_at: Option<String>,
    created_at: String,
}

impl TryFrom<JobRow> for JobRecord {
    type Error = AgentError;

    fn try_from(row: JobRow) -> Result<Self, Self::Error> {
        let job_type = JobType::parse(&row.job_type)
            .ok_or_else(|| AgentError::Persistence("invalid job type".to_owned()))?;
        Ok(JobRecord {
            id: row.id,
            job_type,
            dedup_key: row.dedup_key,
            scheduled_at: row.scheduled_at,
            status: row.status,
            last_result: row
                .last_result
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .unwrap_or(Value::Null),
            retry_at: row.retry_at,
            runs: row.runs,
            last_run_at: row.last_run_at,
            created_at: row.created_at,
        })
    }
}

fn map_sqlx(_error: sqlx::Error) -> AgentError {
    AgentError::Persistence("scheduler operation failed".to_owned())
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::notify::Notification;

    async fn scheduler_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        for migration in crate::db::migrations() {
            sqlx::raw_sql(migration.sql).execute(&pool).await.unwrap();
        }
        pool
    }

    /// A scheduler whose notifications are collected into a test channel.
    fn test_scheduler(pool: SqlitePool) -> (Scheduler, tokio::sync::mpsc::Receiver<Notification>) {
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        (Scheduler::new(pool, NotificationBus(tx)), rx)
    }

    async fn scheduler() -> Scheduler {
        test_scheduler(scheduler_pool().await).0
    }

    #[tokio::test]
    async fn reminders_default_to_enabled_and_toggle_round_trips() {
        let pool = scheduler_pool().await;
        let scheduler = test_scheduler(pool.clone()).0;
        assert!(!scheduler.reminders_paused().await.unwrap());
        assert!(scheduler.set_reminders_paused(true).await.unwrap());
        assert!(scheduler.reminders_paused().await.unwrap());
        assert!(!scheduler.set_reminders_paused(false).await.unwrap());
        assert!(!scheduler.reminders_paused().await.unwrap());
    }

    #[tokio::test]
    async fn schedule_dedups_on_the_global_key() {
        let scheduler = scheduler().await;
        let first = scheduler
            .schedule(
                JobType::TaskReminder,
                "task_reminder:2026-07-18",
                "2026-07-18 19:00:00",
            )
            .await
            .unwrap();
        assert!(first.is_some());
        let duplicate = scheduler
            .schedule(
                JobType::TaskReminder,
                "task_reminder:2026-07-18",
                "2026-07-18 19:00:00",
            )
            .await
            .unwrap();
        assert!(duplicate.is_none());
        let rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_jobs WHERE dedup_key='task_reminder:2026-07-18'",
        )
        .fetch_one(&scheduler.pool)
        .await
        .unwrap();
        assert_eq!(rows, 1);
    }

    #[tokio::test]
    async fn tick_runs_due_jobs_once_with_atomic_claim() {
        let scheduler = scheduler().await;
        scheduler
            .schedule(
                JobType::OverdueCheck,
                "overdue_check:2026-07-18",
                "2026-07-18 09:00:00",
            )
            .await
            .unwrap();
        scheduler
            .schedule(
                JobType::TaskReminder,
                "task_reminder:2026-07-18",
                "2026-07-18 08:00:00",
            )
            .await
            .unwrap();

        // Past-due: both run exactly once per tick; a future job stays queued.
        let ran = scheduler.tick("2026-07-18 10:00:00").await.unwrap();
        assert_eq!(ran, 2);
        let ran_again = scheduler.tick("2026-07-18 11:00:00").await.unwrap();
        assert_eq!(ran_again, 0);

        let completed: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_jobs WHERE status='completed'")
                .fetch_one(&scheduler.pool)
                .await
                .unwrap();
        assert_eq!(completed, 2);

        scheduler
            .schedule(JobType::CleanupFailed, "cleanup:1", "2026-07-18 12:00:00")
            .await
            .unwrap();
        let future = scheduler.tick("2026-07-18 11:00:00").await.unwrap();
        assert_eq!(future, 0);
        let due = scheduler.tick("2026-07-18 12:30:00").await.unwrap();
        assert_eq!(due, 1);
    }

    #[tokio::test]
    async fn paused_and_deprecated_jobs_are_skipped() {
        let scheduler = scheduler().await;
        scheduler.set_reminders_paused(true).await.unwrap();
        scheduler
            .schedule(JobType::TaskReminder, "reminder:1", "2026-07-18 09:00:00")
            .await
            .unwrap();
        scheduler
            .schedule(
                JobType::DailyBrief,
                "daily_brief:2026-07-18",
                "2026-07-18 08:00:00",
            )
            .await
            .unwrap();

        let ran = scheduler.tick("2026-07-18 10:00:00").await.unwrap();
        // reminder:1 (paused) + daily_brief (deprecated, on-demand) + the
        // overdue_check created by ensure_today_jobs (paused).
        assert_eq!(ran, 3);

        let reminder_result: String =
            sqlx::query_scalar("SELECT last_result FROM agent_jobs WHERE job_type='task_reminder'")
                .fetch_one(&scheduler.pool)
                .await
                .unwrap();
        assert!(reminder_result.contains("reminders paused"));

        // Deprecated types are never executed, even when not paused.
        let brief_result: String =
            sqlx::query_scalar("SELECT last_result FROM agent_jobs WHERE job_type='daily_brief'")
                .fetch_one(&scheduler.pool)
                .await
                .unwrap();
        assert!(brief_result.contains("on-demand"));
    }

    #[tokio::test]
    async fn bootstrap_creates_only_todays_missing_jobs() {
        let scheduler = scheduler().await;
        let created = scheduler.bootstrap("2026-07-18 07:30:00").await.unwrap();
        assert_eq!(created, 2); // overdue check + task reminder

        // Second bootstrap (e.g. another restart the same day) creates nothing.
        let again = scheduler.bootstrap("2026-07-18 07:45:00").await.unwrap();
        assert_eq!(again, 0);

        // A different day creates the new day's pair.
        let tomorrow = scheduler.bootstrap("2026-07-19 07:00:00").await.unwrap();
        assert_eq!(tomorrow, 2);
    }

    async fn seed_exam_with_plans(pool: &sqlx::SqlitePool) {
        sqlx::raw_sql(
            r#"
            INSERT INTO exams (id, name, exam_date) VALUES ('exam-r', 'R', '2030-06-01');
            INSERT INTO subjects (id, exam_id, name) VALUES ('sub-r', 'exam-r', 'Math');
            INSERT INTO study_plans (id, exam_id, subject_id, date, planned_duration, status) VALUES
                ('rp-1', 'exam-r', 'sub-r', '2026-07-18', 60, 'pending'),
                ('rp-2', 'exam-r', 'sub-r', '2026-07-18', 30, 'pending'),
                ('rp-old', 'exam-r', 'sub-r', '2026-07-15', 45, 'pending');
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn task_reminder_notifies_about_unfinished_tasks() {
        let pool = scheduler_pool().await;
        seed_exam_with_plans(&pool).await;
        sqlx::query("INSERT INTO settings(key,value) VALUES('agent_active_exam_id','exam-r')")
            .execute(&pool)
            .await
            .unwrap();
        let (scheduler, mut notifications) = test_scheduler(pool.clone());
        scheduler
            .schedule(
                JobType::TaskReminder,
                "task_reminder:2026-07-18",
                "2026-07-18 19:00:00",
            )
            .await
            .unwrap();

        let ran = scheduler.tick("2026-07-18 20:00:00").await.unwrap();
        // reminder + overdue_check (the two today jobs run; daily_brief is gone).
        assert_eq!(ran, 2);

        // The overdue check also fires for rp-old; collect both notifications.
        let mut titles = Vec::new();
        while let Ok(notification) = notifications.try_recv() {
            titles.push(notification.title.clone());
            if notification.title == "今日任务提醒" {
                assert!(notification.body.contains("2"));
                // Notification bodies never carry plan text.
                assert!(!notification.body.contains("rp-"));
                // No due reviews seeded: the review line must be absent.
                assert!(!notification.body.contains("待复习"));
            }
        }
        assert!(titles.contains(&"今日任务提醒".to_owned()));
    }

    #[tokio::test]
    async fn task_reminder_mentions_due_reviews() {
        let pool = scheduler_pool().await;
        seed_exam_with_plans(&pool).await;
        // Two due wrong questions for the active exam: one overdue, one never
        // scheduled (NULL next_review_at also counts as due).
        sqlx::raw_sql(
            r#"
            INSERT INTO wrong_questions (id, subject_id, question_desc, next_review_at)
                VALUES ('wq-due-1', 'sub-r', 'due past', '2000-01-01'),
                       ('wq-due-2', 'sub-r', 'never scheduled', NULL);
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO settings(key,value) VALUES('agent_active_exam_id','exam-r')")
            .execute(&pool)
            .await
            .unwrap();
        let (scheduler, mut notifications) = test_scheduler(pool.clone());
        scheduler
            .schedule(
                JobType::TaskReminder,
                "task_reminder:2026-07-18",
                "2026-07-18 19:00:00",
            )
            .await
            .unwrap();

        scheduler.tick("2026-07-18 20:00:00").await.unwrap();

        let mut mentioned = false;
        while let Ok(notification) = notifications.try_recv() {
            if notification.title == "今日任务提醒" {
                assert!(notification.body.contains("待复习"));
                assert!(notification.body.contains("2 道错题"));
                mentioned = true;
            }
        }
        assert!(mentioned, "task reminder must mention due reviews");
    }

    #[tokio::test]
    async fn overdue_check_notifies_with_count_and_earliest_date() {
        let pool = scheduler_pool().await;
        seed_exam_with_plans(&pool).await;
        sqlx::query("INSERT INTO settings(key,value) VALUES('agent_active_exam_id','exam-r')")
            .execute(&pool)
            .await
            .unwrap();
        let (scheduler, mut notifications) = test_scheduler(pool.clone());
        scheduler
            .schedule(
                JobType::OverdueCheck,
                "overdue_check:2026-07-18",
                "2026-07-18 09:00:00",
            )
            .await
            .unwrap();

        scheduler.tick("2026-07-18 10:00:00").await.unwrap();

        let notification = notifications.recv().await.unwrap();
        assert_eq!(notification.title, "逾期计划提醒");
        assert!(notification.body.contains("1"));
        assert!(notification.body.contains("2026-07-15"));
    }

    #[tokio::test]
    async fn reminders_stay_silent_when_paused_or_nothing_due() {
        let pool = scheduler_pool().await;
        seed_exam_with_plans(&pool).await;
        sqlx::query("INSERT INTO settings(key,value) VALUES('agent_active_exam_id','exam-r')")
            .execute(&pool)
            .await
            .unwrap();
        let (scheduler, mut notifications) = test_scheduler(pool.clone());
        scheduler.set_reminders_paused(true).await.unwrap();
        scheduler
            .schedule(
                JobType::TaskReminder,
                "task_reminder:2026-07-18",
                "2026-07-18 19:00:00",
            )
            .await
            .unwrap();

        scheduler.tick("2026-07-18 20:00:00").await.unwrap();
        // Paused: the reminder is skipped, no notification is queued.
        assert!(notifications.try_recv().is_err());

        // Resume and mark today's plans completed -> still no notification.
        scheduler.set_reminders_paused(false).await.unwrap();
        sqlx::query("UPDATE study_plans SET status='completed' WHERE id='rp-1'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE study_plans SET status='completed' WHERE id='rp-2'")
            .execute(&pool)
            .await
            .unwrap();
        scheduler
            .schedule(
                JobType::TaskReminder,
                "task_reminder:2026-07-18-2",
                "2026-07-18 20:00:00",
            )
            .await
            .unwrap();
        scheduler.tick("2026-07-18 21:00:00").await.unwrap();
        assert!(notifications.try_recv().is_err());
    }

    #[tokio::test]
    async fn reminder_time_setting_controls_the_daily_schedule() {
        let scheduler = scheduler().await;
        sqlx::query("INSERT INTO settings(key,value) VALUES('reminder_time','21:30')")
            .execute(&scheduler.pool)
            .await
            .unwrap();
        scheduler.bootstrap("2026-07-18 07:00:00").await.unwrap();
        let scheduled_at: String = sqlx::query_scalar(
            "SELECT scheduled_at FROM agent_jobs WHERE job_type='task_reminder'",
        )
        .fetch_one(&scheduler.pool)
        .await
        .unwrap();
        assert_eq!(scheduled_at, "2026-07-18 21:30:00");
    }

    #[tokio::test]
    async fn deprecated_weekly_and_brief_jobs_are_skipped_without_side_effects() {
        let pool = scheduler_pool().await;
        seed_exam_with_plans(&pool).await;
        sqlx::query("INSERT INTO settings(key,value) VALUES('agent_active_exam_id','exam-r')")
            .execute(&pool)
            .await
            .unwrap();
        let (scheduler, mut notifications) = test_scheduler(pool);
        scheduler
            .schedule(
                JobType::WeeklyReport,
                "weekly_report:2026-07-13",
                "2026-07-13 08:00:00",
            )
            .await
            .unwrap();

        let ran = scheduler.tick("2026-07-18 08:30:00").await.unwrap();
        // weekly (deprecated, skipped) runs; overdue_check is at 09:00 and the
        // task reminder at 19:00 are not due yet.
        assert_eq!(ran, 1);

        let result: String =
            sqlx::query_scalar("SELECT last_result FROM agent_jobs WHERE job_type='weekly_report'")
                .fetch_one(&scheduler.pool)
                .await
                .unwrap();
        assert!(result.contains("removed"));
        // No notification is produced by a deprecated job.
        assert!(notifications.try_recv().is_err());
    }

    #[tokio::test]
    async fn reminders_are_silent_when_notifications_are_disabled() {
        let pool = scheduler_pool().await;
        seed_exam_with_plans(&pool).await;
        sqlx::query("INSERT INTO settings(key,value) VALUES('agent_active_exam_id','exam-r')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO settings(key,value) VALUES('notification_enabled','false')")
            .execute(&pool)
            .await
            .unwrap();
        let (scheduler, mut notifications) = test_scheduler(pool.clone());
        scheduler
            .schedule(
                JobType::TaskReminder,
                "task_reminder:2026-07-18",
                "2026-07-18 19:00:00",
            )
            .await
            .unwrap();

        let ran = scheduler.tick("2026-07-18 20:00:00").await.unwrap();
        // The task reminder runs but is skipped (notifications disabled);
        // ensure_today_jobs also created the 09:00 overdue_check, which is
        // due too, so two jobs ran in total.
        assert_eq!(ran, 2);
        let result: String =
            sqlx::query_scalar("SELECT last_result FROM agent_jobs WHERE job_type='task_reminder'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(result.contains("notifications disabled"));
        assert!(notifications.try_recv().is_err());
    }

    #[tokio::test]
    async fn list_returns_records_newest_first() {
        let scheduler = scheduler().await;
        scheduler
            .schedule(JobType::CleanupFailed, "a:1", "2026-07-18 08:00:00")
            .await
            .unwrap();
        scheduler
            .schedule(JobType::CleanupFailed, "b:2", "2026-07-18 09:00:00")
            .await
            .unwrap();
        let jobs = scheduler.list(10).await.unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].dedup_key, "b:2");
        assert_eq!(jobs[0].job_type, JobType::CleanupFailed);
        assert_eq!(jobs[0].status, "scheduled");
    }
}
