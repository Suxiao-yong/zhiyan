use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use serde_json::json;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;
use uuid::Uuid;
use zhiyan_lib::agent::error::AgentError;
use zhiyan_lib::agent::model::RunStatus;
use zhiyan_lib::agent::repository::AgentRepository;
use zhiyan_lib::db::AGENT_SCHEMA_SQL;

async fn test_pool() -> SqlitePool {
    let options = SqliteConnectOptions::from_str("sqlite::memory:")
        .unwrap()
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE TABLE exams (id TEXT PRIMARY KEY)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(AGENT_SCHEMA_SQL)
        .execute(&pool)
        .await
        .unwrap();
    pool
}

async fn event_count(pool: &SqlitePool, run_id: &str, event_type: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM agent_events WHERE run_id = ? AND event_type = ?")
        .bind(run_id)
        .bind(event_type)
        .fetch_one(pool)
        .await
        .unwrap()
}

struct FileDatabase {
    directory: PathBuf,
    path: PathBuf,
}

impl FileDatabase {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("zhiyan-agent-recovery-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("recovery.db");
        Self { directory, path }
    }

    fn cleanup(&self) {
        for path in [
            self.path.clone(),
            PathBuf::from(format!("{}-wal", self.path.display())),
            PathBuf::from(format!("{}-shm", self.path.display())),
        ] {
            remove_file_with_retry(&path).unwrap();
        }
        remove_dir_with_retry(&self.directory).unwrap();
    }
}

impl Drop for FileDatabase {
    fn drop(&mut self) {
        for path in [
            self.path.clone(),
            PathBuf::from(format!("{}-wal", self.path.display())),
            PathBuf::from(format!("{}-shm", self.path.display())),
        ] {
            let _ = remove_file_with_retry(&path);
        }
        let _ = remove_dir_with_retry(&self.directory);
    }
}

async fn wal_test_pool(database: &FileDatabase) -> SqlitePool {
    let options = SqliteConnectOptions::new()
        .filename(&database.path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(2));
    let pool = SqlitePoolOptions::new()
        .max_connections(3)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE TABLE exams (id TEXT PRIMARY KEY)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(AGENT_SCHEMA_SQL)
        .execute(&pool)
        .await
        .unwrap();
    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(journal_mode, "wal");
    pool
}

fn retry_io<F>(mut operation: F) -> std::io::Result<()>
where
    F: FnMut() -> std::io::Result<()>,
{
    let mut last_error = None;
    for attempt in 0..10 {
        match operation() {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error)
                if (error.kind() == std::io::ErrorKind::PermissionDenied
                    || error.raw_os_error() == Some(32))
                    && attempt < 9 =>
            {
                last_error = Some(error);
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.expect("retry loop must capture a permission error"))
}

fn remove_file_with_retry(path: &std::path::Path) -> std::io::Result<()> {
    retry_io(|| std::fs::remove_file(path))
}

fn remove_dir_with_retry(path: &std::path::Path) -> std::io::Result<()> {
    retry_io(|| std::fs::remove_dir(path))
}

fn remove_wal_with_retry(path: &std::path::Path) -> std::io::Result<()> {
    remove_file_with_retry(path)
}

#[tokio::test]
async fn creates_session_and_run_with_created_event() {
    let pool = test_pool().await;
    let repo = AgentRepository::new(pool.clone());

    repo.health().await.unwrap();
    let session = repo.create_session(None, "Runtime test").await.unwrap();
    let run = repo
        .create_run(&session.id, "Inspect today's plan", "user")
        .await
        .unwrap();

    assert_eq!(session.exam_id, None);
    assert_eq!(session.title, "Runtime test");
    assert_eq!(run.status, RunStatus::Queued);
    assert_eq!(run.started_at, None);
    assert_eq!(run.completed_at, None);

    let event: (String, String) =
        sqlx::query_as("SELECT event_type, payload_json FROM agent_events WHERE run_id = ?")
            .bind(&run.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(event.0, "run.created");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&event.1).unwrap(),
        json!({ "goal": "Inspect today's plan" })
    );
}

#[tokio::test]
async fn create_run_rolls_back_when_its_audit_event_fails() {
    let pool = test_pool().await;
    let repo = AgentRepository::new(pool.clone());
    let session = repo.create_session(None, "Atomic create").await.unwrap();
    sqlx::raw_sql(
        "CREATE TRIGGER reject_created_event BEFORE INSERT ON agent_events \
         WHEN NEW.event_type = 'run.created' BEGIN SELECT RAISE(ABORT, 'event rejected'); END;",
    )
    .execute(&pool)
    .await
    .unwrap();

    let result = repo.create_run(&session.id, "must roll back", "user").await;

    assert!(matches!(result, Err(AgentError::Persistence(_))));
    let run_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM agent_runs WHERE goal = 'must roll back'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(run_count, 0);
}

#[tokio::test]
async fn update_run_status_updates_lifecycle_without_a_transition_event() {
    let pool = test_pool().await;
    let repo = AgentRepository::new(pool.clone());
    let session = repo.create_session(None, "Update helper").await.unwrap();
    let run = repo
        .create_run(&session.id, "Update me", "recovery")
        .await
        .unwrap();

    let running = repo
        .update_run_status(&run.id, RunStatus::Running, None)
        .await
        .unwrap();
    assert_eq!(running.status, RunStatus::Running);
    assert!(running.started_at.is_some());
    assert_eq!(running.completed_at, None);

    let completed = repo
        .update_run_status(&run.id, RunStatus::Completed, None)
        .await
        .unwrap();
    assert!(completed.completed_at.is_some());
    assert_eq!(event_count(&pool, &run.id, "run.status_changed").await, 0);

    let error = repo
        .update_run_status("missing-run", RunStatus::Running, None)
        .await
        .unwrap_err();
    assert_eq!(error, AgentError::NotFound("missing-run".to_owned()));
}

#[tokio::test]
async fn transitions_status_and_event_atomically_with_complete_lifecycle_timestamps() {
    let pool = test_pool().await;
    let repo = AgentRepository::new(pool.clone());
    let session = repo.create_session(None, "Transitions").await.unwrap();
    let run = repo
        .create_run(&session.id, "Transition me", "user")
        .await
        .unwrap();

    let running = repo
        .transition_run_status(&run.id, RunStatus::Queued, RunStatus::Running, "start")
        .await
        .unwrap();
    running
        .started_at
        .as_ref()
        .expect("running sets started_at");
    assert_eq!(running.completed_at, None);
    let first_started_at = "2000-01-02 03:04:05";
    sqlx::query("UPDATE agent_runs SET started_at = ? WHERE id = ?")
        .bind(first_started_at)
        .bind(&run.id)
        .execute(&pool)
        .await
        .unwrap();

    repo.transition_run_status(
        &run.id,
        RunStatus::Running,
        RunStatus::WaitingApproval,
        "request_approval",
    )
    .await
    .unwrap();
    let resumed = repo
        .transition_run_status(
            &run.id,
            RunStatus::WaitingApproval,
            RunStatus::Running,
            "approve",
        )
        .await
        .unwrap();
    assert_eq!(resumed.started_at.as_deref(), Some(first_started_at));

    let completed = repo
        .transition_run_status(
            &run.id,
            RunStatus::Running,
            RunStatus::Completed,
            "complete",
        )
        .await
        .unwrap();
    assert!(completed.completed_at.is_some());

    let payloads: Vec<String> = sqlx::query_scalar(
        "SELECT payload_json FROM agent_events \
         WHERE run_id = ? AND event_type = 'run.status_changed' ORDER BY id",
    )
    .bind(&run.id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(payloads.len(), 4);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&payloads[0]).unwrap(),
        json!({ "from": "queued", "to": "running", "event": "start" })
    );
}

#[tokio::test]
async fn transition_conflict_changes_neither_status_nor_events() {
    let pool = test_pool().await;
    let repo = AgentRepository::new(pool.clone());
    let session = repo.create_session(None, "Conflict").await.unwrap();
    let run = repo
        .create_run(&session.id, "Remain queued", "user")
        .await
        .unwrap();

    let result = repo
        .transition_run_status(
            &run.id,
            RunStatus::Running,
            RunStatus::Completed,
            "complete",
        )
        .await;

    assert_eq!(result.unwrap_err(), AgentError::Conflict);
    assert_eq!(
        repo.get_run(&run.id).await.unwrap().status,
        RunStatus::Queued
    );
    assert_eq!(event_count(&pool, &run.id, "run.status_changed").await, 0);
}

#[tokio::test]
async fn transition_rolls_back_status_when_event_insert_fails() {
    let pool = test_pool().await;
    let repo = AgentRepository::new(pool.clone());
    let session = repo
        .create_session(None, "Atomic transition")
        .await
        .unwrap();
    let run = repo
        .create_run(&session.id, "Remain queued", "user")
        .await
        .unwrap();
    sqlx::raw_sql(
        "CREATE TRIGGER reject_status_event BEFORE INSERT ON agent_events \
         WHEN NEW.event_type = 'run.status_changed' BEGIN SELECT RAISE(ABORT, 'event rejected'); END;",
    )
    .execute(&pool)
    .await
    .unwrap();

    let result = repo
        .transition_run_status(&run.id, RunStatus::Queued, RunStatus::Running, "start")
        .await;

    assert!(matches!(result, Err(AgentError::Persistence(_))));
    let stored = repo.get_run(&run.id).await.unwrap();
    assert_eq!(stored.status, RunStatus::Queued);
    assert_eq!(stored.started_at, None);
}

#[tokio::test]
async fn recovery_interrupts_only_running_runs_and_is_idempotent() {
    let pool = test_pool().await;
    let repo = AgentRepository::new(pool.clone());
    let session = repo.create_session(None, "Recovery").await.unwrap();

    let active = repo
        .create_run(&session.id, "active", "user")
        .await
        .unwrap();
    repo.update_run_status(&active.id, RunStatus::Running, None)
        .await
        .unwrap();
    let completed = repo.create_run(&session.id, "done", "user").await.unwrap();
    repo.update_run_status(&completed.id, RunStatus::Completed, None)
        .await
        .unwrap();
    let approval = repo
        .create_run(&session.id, "approval", "user")
        .await
        .unwrap();
    repo.update_run_status(&approval.id, RunStatus::WaitingApproval, None)
        .await
        .unwrap();

    assert_eq!(repo.interrupt_active_runs().await.unwrap(), 1);
    assert_eq!(
        repo.get_run(&active.id).await.unwrap().status,
        RunStatus::Interrupted
    );
    assert_eq!(repo.get_run(&active.id).await.unwrap().completed_at, None);
    assert_eq!(
        repo.get_run(&completed.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(
        repo.get_run(&approval.id).await.unwrap().status,
        RunStatus::WaitingApproval
    );

    let payload: String = sqlx::query_scalar(
        "SELECT payload_json FROM agent_events WHERE run_id = ? AND event_type = 'run.interrupted'",
    )
    .bind(&active.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&payload).unwrap(),
        json!({ "reason": "application_restart" })
    );

    assert_eq!(repo.interrupt_active_runs().await.unwrap(), 0);
    assert_eq!(event_count(&pool, &active.id, "run.interrupted").await, 1);
}

#[tokio::test]
async fn recovery_waits_for_wal_writer_and_uses_the_latest_status() {
    let database = FileDatabase::new();
    let pool = wal_test_pool(&database).await;
    let repo = AgentRepository::new(pool.clone());
    let session = repo.create_session(None, "WAL recovery").await.unwrap();
    let run = repo
        .create_run(&session.id, "concurrent completion", "user")
        .await
        .unwrap();
    repo.update_run_status(&run.id, RunStatus::Running, None)
        .await
        .unwrap();

    let mut writer = pool.acquire().await.unwrap();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *writer)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE agent_runs SET status = 'completed', \
         completed_at = datetime('now','localtime') WHERE id = ?",
    )
    .bind(&run.id)
    .execute(&mut *writer)
    .await
    .unwrap();

    let recovery_repo = repo.clone();
    let recovery = tokio::spawn(async move { recovery_repo.interrupt_active_runs().await });
    tokio::task::yield_now().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    sqlx::query("COMMIT").execute(&mut *writer).await.unwrap();
    drop(writer);
    let interrupted = tokio::time::timeout(Duration::from_secs(3), recovery)
        .await
        .expect("recovery should finish after the writer commits")
        .unwrap()
        .unwrap();

    assert_eq!(interrupted, 0);
    assert_eq!(
        repo.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(event_count(&pool, &run.id, "run.interrupted").await, 0);

    drop(repo);
    pool.close().await;
    drop(pool);
    database.cleanup();
}

#[tokio::test]
async fn foreign_keys_reject_runs_for_missing_sessions() {
    let pool = test_pool().await;
    let repo = AgentRepository::new(pool.clone());
    let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(foreign_keys, 1);

    let error = repo
        .create_run("missing-session", "invalid parent", "user")
        .await
        .unwrap_err();

    assert!(matches!(error, AgentError::Persistence(_)));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_runs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn persistence_errors_do_not_disclose_bound_values() {
    let pool = test_pool().await;
    let repo = AgentRepository::new(pool);
    let session = repo.create_session(None, "Safe errors").await.unwrap();
    let secret_value = "secret-trigger-value-9f35";

    let error = repo
        .create_run(&session.id, "invalid trigger", secret_value)
        .await
        .unwrap_err();

    let AgentError::Persistence(message) = error else {
        panic!("invalid trigger source should be a persistence error");
    };
    assert!(!message.contains(secret_value));
}

#[tokio::test]
async fn prepare_database_restore_checkpoints_wal_and_closes_pool() {
    let database = FileDatabase::new();
    let pool = wal_test_pool(&database).await;
    sqlx::query("PRAGMA wal_autocheckpoint = 0")
        .execute(&pool)
        .await
        .unwrap();
    let repo = AgentRepository::new(pool.clone());
    let session = repo.create_session(None, "Restore").await.unwrap();
    repo.create_run(&session.id, "Restore me", "user")
        .await
        .unwrap();

    let wal_path = PathBuf::from(format!("{}-wal", database.path.display()));
    assert!(wal_path.exists(), "writes should produce a WAL file");

    repo.prepare_database_restore().await.unwrap();

    assert!(repo.health().await.is_err(), "pool must be closed");
    drop(repo);
    drop(pool);
    std::fs::write(&database.path, b"replacement database")
        .expect("closed pool must release the database file");
    remove_wal_with_retry(&wal_path).expect("checkpointed WAL must be removable");
    database.cleanup();
}

#[tokio::test]
async fn append_event_persists_json_and_missing_get_maps_to_not_found() {
    let pool = test_pool().await;
    let repo = AgentRepository::new(pool.clone());
    let session = repo.create_session(None, "Events").await.unwrap();
    let run = repo
        .create_run(&session.id, "Events", "user")
        .await
        .unwrap();

    repo.append_event(&run.id, "run.note", &json!({ "safe": true }))
        .await
        .unwrap();
    assert_eq!(event_count(&pool, &run.id, "run.note").await, 1);

    assert!(matches!(
        repo.get_run("missing").await,
        Err(AgentError::NotFound(_))
    ));
}
