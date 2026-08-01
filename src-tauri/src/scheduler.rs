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
/// retry, or a deliberately skipped run (e.g. reminders paused). `Done` and
/// `Retry` are constructed by the Task 4/5 handlers; Task 2's placeholder
/// handlers only skip.
#[allow(dead_code)]
enum JobOutcome {
    Done(Value),
    Retry(Value),
    Skipped(Value),
}

#[derive(Clone)]
pub struct Scheduler {
    pool: SqlitePool,
}

impl Scheduler {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
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

    /// Run every due job once. `now` is passed in so tests control the clock
    /// (`"YYYY-MM-DD HH:MM:SS"`, local time, same format as the DB defaults).
    /// Returns how many jobs ran (including skips).
    pub async fn tick(&self, now: &str) -> Result<usize, AgentError> {
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

    /// Startup catch-up: re-create today's meaningful jobs (daily brief,
    /// overdue check) when their dedup keys are absent after a restart or
    /// sleep/wake. Never replays failed user-visible writes.
    pub async fn bootstrap(&self, now: &str) -> Result<usize, AgentError> {
        let today = &now[..10];
        let mut created = 0;
        if self
            .schedule(
                JobType::DailyBrief,
                &format!("daily_brief:{today}"),
                &format!("{today} 08:00:00"),
            )
            .await?
            .is_some()
        {
            created += 1;
        }
        if self
            .schedule(
                JobType::OverdueCheck,
                &format!("overdue_check:{today}"),
                &format!("{today} 09:00:00"),
            )
            .await?
            .is_some()
        {
            created += 1;
        }
        Ok(created)
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
                // Task 4 replaces this with brief.rs.
                let _ = now;
                JobOutcome::Skipped(json!({ "note": "daily brief handler lands in M4 Task 4" }))
            }
            JobType::TaskReminder => {
                // Task 5 replaces this with the notification path.
                let _ = now;
                JobOutcome::Skipped(json!({ "note": "task reminder handler lands in M4 Task 5" }))
            }
            JobType::OverdueCheck => {
                // Task 5 replaces this with the notification path.
                let _ = now;
                JobOutcome::Skipped(json!({ "note": "overdue check handler lands in M4 Task 5" }))
            }
            JobType::WeeklyReport | JobType::RetryFailed | JobType::CleanupFailed => {
                let _ = now;
                JobOutcome::Skipped(json!({ "note": "handler lands in M5" }))
            }
        }
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

    async fn scheduler() -> Scheduler {
        Scheduler::new(scheduler_pool().await)
    }

    #[tokio::test]
    async fn reminders_default_to_enabled_and_toggle_round_trips() {
        let pool = scheduler_pool().await;
        let scheduler = Scheduler::new(pool.clone());
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
        assert_eq!(ran, 2);

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
        assert_eq!(created, 2);

        // Second bootstrap (e.g. another restart the same day) creates nothing.
        let again = scheduler.bootstrap("2026-07-18 07:45:00").await.unwrap();
        assert_eq!(again, 0);

        // A different day creates the new day's pair.
        let tomorrow = scheduler.bootstrap("2026-07-19 07:00:00").await.unwrap();
        assert_eq!(tomorrow, 2);
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
