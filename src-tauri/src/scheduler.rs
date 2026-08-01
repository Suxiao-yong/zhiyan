// Rust Scheduler (M4): background job lifecycle.
//
// Task 1 added the pause/state surface used by the tray. Task 2 adds the
// agent_jobs table (v8 migration), the tick loop with atomic claim, retry with
// backoff, startup bootstrap catch-up, and per-type dispatch. Job handlers
// land in later M4 tasks: daily brief (Task 4), reminders/overdue (Task 5);
// weekly_report / retry_failed / cleanup_failed remain placeholder outcomes
// until M5, as planned in `2026-07-17-agent-tray-scheduler.md`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::agent::error::AgentError;
use crate::agent::memory::MemoryRepository;
use crate::agent::planner::Planner;
use crate::analytics::Analytics;
use crate::brief::{brief_payload, Brief, BriefBuilder};
use crate::notify::NotificationBus;

/// Reminder jobs are suppressed while `agent_reminders_paused` is `1`.
pub const REMINDERS_PAUSED_KEY: &str = "agent_reminders_paused";

/// Retry a failed job after this many minutes.
const RETRY_AFTER_MINUTES: i64 = 5;

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

    /// Whether the job is suppressed by the reminders pause.
    fn is_reminder(self) -> bool {
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

/// What a job handler produced: a completed run, a failed run that should
/// retry, or a deliberately skipped run (e.g. reminders paused).
enum JobOutcome {
    Done(Value),
    Retry(Value),
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
            self.record_outcome(&id, now, outcome).await?;
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

    /// Ensure today's daily jobs exist (daily brief at 08:00, overdue check at
    /// 09:00, task reminder at the configured reminder time). Called on every
    /// tick, so a restart or a day rollover re-creates the day's jobs exactly
    /// once (dedup keys are date-scoped). Returns how many were created.
    async fn ensure_today_jobs(&self, now: &str) -> Result<usize, AgentError> {
        let today = &now[..10];
        let reminder_time = self.reminder_time().await?;
        let mut created = 0;
        for (job_type, dedup, scheduled_at) in [
            (
                JobType::DailyBrief,
                format!("daily_brief:{today}"),
                format!("{today} 08:00:00"),
            ),
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
    /// `agent_reminder_time` setting, defaulting to `19:00`.
    async fn reminder_time(&self) -> Result<String, AgentError> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'agent_reminder_time'")
                .fetch_optional(&self.pool)
                .await
                .map_err(map_sqlx)?;
        Ok(value
            .filter(|raw| raw.len() == 5 && raw.as_bytes()[2] == b':')
            .unwrap_or_else(|| "19:00".to_owned()))
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
        if job_type.is_reminder() && self.reminders_paused().await.unwrap_or(false) {
            return JobOutcome::Skipped(json!({ "reason": "reminders paused" }));
        }
        match job_type {
            JobType::DailyBrief => {
                let today = &now[..10];
                let exam_id = self.active_exam_id().await.unwrap_or(None);
                let builder = BriefBuilder::new(
                    self.pool.clone(),
                    Analytics::new(self.pool.clone()),
                    MemoryRepository::new(self.pool.clone()),
                );
                let provider = Planner::build_provider_from(&self.pool)
                    .await
                    .ok()
                    .flatten();
                match builder
                    .build(exam_id.as_deref(), today, provider.as_ref())
                    .await
                {
                    Ok(brief) => {
                        let payload = brief_payload(&brief);
                        // The brief result is stored in last_result; pushing a
                        // UI event needs an AppHandle, which M4 deliberately
                        // avoids holding in Scheduler (see plan: event push is
                        // deferred to M5 via the command layer).
                        JobOutcome::Done(payload)
                    }
                    Err(_) => JobOutcome::Retry(json!({ "error": "brief build failed" })),
                }
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
                    let _ = self.notifications.send(
                        "今日任务提醒",
                        format!("今日还有 {unfinished} 项任务未完成。"),
                    );
                    JobOutcome::Done(json!({ "unfinished": unfinished }))
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
            JobType::WeeklyReport | JobType::RetryFailed | JobType::CleanupFailed => {
                let _ = now;
                JobOutcome::Skipped(json!({ "note": "handler lands in M5" }))
            }
        }
    }

    /// The exam the brief targets: the persisted `agent_active_exam_id`, or the
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

    /// On-demand brief for the hidden debug page. Resolves the exam (explicit
    /// id or active fallback), builds the brief with or without an LLM
    /// explanation, and returns it without touching the job table.
    pub(crate) async fn brief_preview(
        &self,
        exam_id: Option<&str>,
        today: &str,
    ) -> Result<Brief, AgentError> {
        let exam_id = match exam_id.map(str::trim).filter(|value| !value.is_empty()) {
            Some(id) => Some(id.to_owned()),
            None => self.active_exam_id().await?,
        };
        let builder = BriefBuilder::new(
            self.pool.clone(),
            Analytics::new(self.pool.clone()),
            MemoryRepository::new(self.pool.clone()),
        );
        let provider = Planner::build_provider_from(&self.pool)
            .await
            .ok()
            .flatten();
        builder
            .build(exam_id.as_deref(), today, provider.as_ref())
            .await
    }

    async fn record_outcome(
        &self,
        id: &str,
        now: &str,
        outcome: JobOutcome,
    ) -> Result<(), AgentError> {
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
            JobOutcome::Retry(payload) => {
                sqlx::query(
                    "UPDATE agent_jobs SET status = 'failed', last_result = ?, runs = runs + 1, \
                     retry_at = datetime(?, '+' || ? || ' minutes') WHERE id = ?",
                )
                .bind(payload.to_string())
                .bind(now)
                .bind(RETRY_AFTER_MINUTES)
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
                JobType::DailyBrief,
                "daily_brief:2026-07-18",
                "2026-07-18 08:00:00",
            )
            .await
            .unwrap();
        assert!(first.is_some());
        let duplicate = scheduler
            .schedule(
                JobType::DailyBrief,
                "daily_brief:2026-07-18",
                "2026-07-18 08:00:00",
            )
            .await
            .unwrap();
        assert!(duplicate.is_none());
        let rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_jobs WHERE dedup_key='daily_brief:2026-07-18'",
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
                JobType::DailyBrief,
                "daily_brief:2026-07-18",
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
    async fn paused_reminders_are_skipped_but_other_jobs_run() {
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
        // reminder:1 + daily_brief + the overdue_check created by ensure_today_jobs.
        assert_eq!(ran, 3);

        let reminder_result: String =
            sqlx::query_scalar("SELECT last_result FROM agent_jobs WHERE job_type='task_reminder'")
                .fetch_one(&scheduler.pool)
                .await
                .unwrap();
        assert!(reminder_result.contains("reminders paused"));
    }

    #[tokio::test]
    async fn bootstrap_creates_only_todays_missing_jobs() {
        let scheduler = scheduler().await;
        let created = scheduler.bootstrap("2026-07-18 07:30:00").await.unwrap();
        assert_eq!(created, 3); // daily brief + overdue check + task reminder

        // Second bootstrap (e.g. another restart the same day) creates nothing.
        let again = scheduler.bootstrap("2026-07-18 07:45:00").await.unwrap();
        assert_eq!(again, 0);

        // A different day creates the new day's trio.
        let tomorrow = scheduler.bootstrap("2026-07-19 07:00:00").await.unwrap();
        assert_eq!(tomorrow, 3);
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
        // reminder + daily_brief + overdue_check (all three today jobs run).
        assert_eq!(ran, 3);

        // The overdue check also fires for rp-old; collect both notifications.
        let mut titles = Vec::new();
        while let Ok(notification) = notifications.try_recv() {
            titles.push(notification.title.clone());
            if notification.title == "今日任务提醒" {
                assert!(notification.body.contains("2"));
                // Notification bodies never carry plan text.
                assert!(!notification.body.contains("rp-"));
            }
        }
        assert!(titles.contains(&"今日任务提醒".to_owned()));
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
        sqlx::query("INSERT INTO settings(key,value) VALUES('agent_reminder_time','21:30')")
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
