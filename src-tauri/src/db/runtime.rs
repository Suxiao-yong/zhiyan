use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::Path;
use std::time::Duration;

pub fn sqlite_options(path: &Path) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5))
}

pub async fn connect(path: &Path) -> Result<SqlitePool, sqlx::Error> {
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(sqlite_options(path))
        .await?;
    sqlx::raw_sql(super::AGENT_SCHEMA_SQL)
        .execute(&pool)
        .await?;
    Ok(pool)
}

pub async fn health(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1").execute(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::Duration;

    use uuid::Uuid;

    use super::{connect, health};

    struct TempDatabase {
        directory: PathBuf,
        path: PathBuf,
    }

    impl TempDatabase {
        fn new() -> Self {
            let directory =
                std::env::temp_dir().join(format!("zhiyan-agent-runtime-{}", Uuid::new_v4()));
            std::fs::create_dir(&directory).unwrap();
            let path = directory.join("runtime.sqlite");
            Self { directory, path }
        }

        fn path(&self) -> &Path {
            &self.path
        }

        fn cleanup(&self) -> std::io::Result<()> {
            let mut last_error = None;
            for _ in 0..10 {
                match std::fs::remove_dir_all(&self.directory) {
                    Ok(()) => return Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                    Err(error) => {
                        last_error = Some(error);
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            }
            Err(last_error.expect("cleanup retry must capture an error"))
        }
    }

    impl Drop for TempDatabase {
        fn drop(&mut self) {
            let _ = self.cleanup();
        }
    }

    #[test]
    fn pool_enforces_foreign_keys() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let database = TempDatabase::new();
                let pool = connect(database.path()).await.unwrap();

                let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
                assert_eq!(foreign_keys, 1);

                let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
                assert_eq!(journal_mode, "wal");

                health(&pool).await.unwrap();

                let tables: Vec<String> = sqlx::query_scalar(
                    "SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'agent_%'",
                )
                .fetch_all(&pool)
                .await
                .unwrap();
                for expected_table in [
                    "agent_sessions",
                    "agent_runs",
                    "agent_steps",
                    "agent_events",
                    "agent_approvals",
                ] {
                    assert!(tables.iter().any(|table| table == expected_table));
                }

                pool.close().await;
                drop(pool);
                database.cleanup().unwrap();
                assert!(!database.directory.exists());
            });
    }
}
