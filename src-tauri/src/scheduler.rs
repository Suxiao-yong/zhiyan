// Rust Scheduler (M4): background job lifecycle.
//
// Task 1 adds the pause/state surface the tray uses (settings-backed). Task 2
// adds the agent_jobs table, tick loop, and job dispatch on top of this struct
// (M4 plan: `2026-07-17-agent-tray-scheduler.md`).

use sqlx::SqlitePool;

use crate::agent::error::AgentError;

/// Reminder jobs are suppressed while `agent_reminders_paused` is `1`.
pub const REMINDERS_PAUSED_KEY: &str = "agent_reminders_paused";

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
}

fn map_sqlx(_error: sqlx::Error) -> AgentError {
    AgentError::Persistence("scheduler settings failed".to_owned())
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

    #[tokio::test]
    async fn reminders_default_to_enabled() {
        let pool = scheduler_pool().await;
        let scheduler = Scheduler::new(pool);
        assert!(!scheduler.reminders_paused().await.unwrap());
    }

    #[tokio::test]
    async fn pause_toggle_round_trips_through_settings() {
        let pool = scheduler_pool().await;
        let scheduler = Scheduler::new(pool.clone());

        assert!(scheduler.set_reminders_paused(true).await.unwrap());
        assert!(scheduler.reminders_paused().await.unwrap());

        assert!(!scheduler.set_reminders_paused(false).await.unwrap());
        assert!(!scheduler.reminders_paused().await.unwrap());

        let stored: String = sqlx::query_scalar("SELECT value FROM settings WHERE key=?")
            .bind(REMINDERS_PAUSED_KEY)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(stored, "0");
    }
}
