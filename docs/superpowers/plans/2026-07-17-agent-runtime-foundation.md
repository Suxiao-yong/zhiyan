# Agent Runtime Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the persistent Rust Agent Runtime foundation without changing the current user-facing application flow.

**Architecture:** Keep the existing `tauri-plugin-sql` path active while adding a Rust `sqlx` pool against the same SQLite file. Agent sessions, runs, steps and events live in new version-4 tables. Pure state transitions stay independent of Tauri and SQL; repositories persist state; Tauri commands expose a small typed contract used only by a hidden debug page.

**Tech Stack:** Rust 2021, Tauri 2, sqlx 0.8 SQLite, Tokio, serde, thiserror, Vue 3, TypeScript, Pinia, Vitest.

---

## File map

### Create

- `docs/agent/feature-parity.md` — authoritative old/new capability ownership matrix.
- `docs/agent/migration-runbook.md` — migration, rollback and database backup procedure.
- `src-tauri/src/db/runtime.rs` — Rust SQLite pool construction and health check.
- `src-tauri/src/agent/mod.rs` — Agent module exports.
- `src-tauri/src/agent/error.rs` — stable internal and command-safe errors.
- `src-tauri/src/agent/model.rs` — persisted Agent types and statuses.
- `src-tauri/src/agent/state.rs` — pure Run transition rules.
- `src-tauri/src/agent/repository.rs` — Agent table persistence.
- `src-tauri/src/agent/runtime.rs` — orchestration over state and repository.
- `src-tauri/src/agent/commands.rs` — Tauri command boundary.
- `src-tauri/tests/agent_repository.rs` — in-memory SQLite repository integration tests.
- `src/services/agent-client.ts` — typed invoke wrapper.
- `src/services/agent-client.test.ts` — command contract tests.
- `src/pages/AgentDebug.vue` — hidden runtime inspection page.
- `src/pages/AgentDebug.test.ts` — hidden-page component test.

### Modify

- `src-tauri/Cargo.toml` — add async database, UUID, time and error dependencies.
- `src-tauri/src/db.rs` — add schema version 4 and expose the Agent schema for tests.
- `src-tauri/src/lib.rs` — manage application state, recover interrupted runs and register commands.
- `src/types/index.ts` — add Agent session/run/status DTOs.
- `src/router/index.ts` — add `/agent-debug` without adding it to the main menu.

## Task 1: Baseline and migration controls

**Files:**

- Create: `docs/agent/feature-parity.md`
- Create: `docs/agent/migration-runbook.md`

- [ ] **Step 1: Record the current baseline**

Create `docs/agent/feature-parity.md` with this exact initial matrix:

```markdown
# Agent OS Feature Parity

| Capability | Current owner | Target owner | Migration state | Regression command |
|---|---|---|---|---|
| Exam and subject configuration | TypeScript services | Rust tools | legacy | `npm.cmd test` |
| Plan generation and editing | TypeScript services | Rust tools | legacy | `npm.cmd test -- src/services/plan-generator.test.ts src/services/plan-service.test.ts` |
| Plan check-in and free record | TypeScript services | Rust tools | legacy | `npm.cmd test -- src/services/record-service.test.ts` |
| Wrong questions | TypeScript services | Rust tools | legacy | `npm.cmd test` |
| Analysis and prediction | TypeScript services | Rust tools | legacy | `npm.cmd test -- src/services/analyzer.test.ts` |
| Visualization datasets | TypeScript services | Rust tools | legacy | `npm.cmd test` |
| Import, export, backup and restore | TypeScript services plus Tauri plugins | Rust tools | legacy | `npm.cmd test -- src/services/export.test.ts` |
| Agent session and run state | none | Rust Runtime | foundation | `cargo test --manifest-path src-tauri/Cargo.toml` |

States: `legacy`, `shadow`, `rust-owned`, `retired`.
```

Create `docs/agent/migration-runbook.md`:

```markdown
# Agent OS Migration Runbook

## Safety rules

1. Never remove an old write path before its parity tests pass.
2. Never let TypeScript and Rust own the same write operation at the same time.
3. Back up `%APPDATA%\com.zhiyan.app\zhiyan.db` before a packaged migration test.
4. Treat migration failure as a release blocker; keep the original database untouched.

## Milestone 1 rollback

- Disable access to `/agent-debug`.
- Revert migration version 4 only on disposable test databases. Production rollback keeps the additive Agent tables.
- Remove Rust Agent state registration and commands.
- Existing business tables and TypeScript services remain unchanged.
```

- [ ] **Step 2: Run and record the baseline commands**

Run:

```powershell
npm.cmd test
npm.cmd run typecheck
npm.cmd run build
cargo test --manifest-path src-tauri\Cargo.toml
```

Expected: all commands exit `0`; Vitest reports the current 33 tests passing.

- [ ] **Step 3: Commit baseline documentation**

```powershell
git add docs/agent/feature-parity.md docs/agent/migration-runbook.md
git commit -m "docs: record agent migration baseline"
```

## Task 2: Add Rust runtime dependencies

**Files:**

- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`

- [ ] **Step 1: Add dependency declarations**

Append these dependencies under `[dependencies]`:

```toml
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "sqlite", "uuid", "chrono"] }
tokio = { version = "1", features = ["sync", "time"] }
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
thiserror = "2"
```

- [ ] **Step 2: Resolve and compile dependencies**

Run:

```powershell
cargo check --manifest-path src-tauri\Cargo.toml
```

Expected: exit `0` and update `src-tauri/Cargo.lock`.

- [ ] **Step 3: Commit dependencies**

```powershell
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "build: add Rust agent runtime dependencies"
```

## Task 3: Add the version-4 Agent schema

**Files:**

- Modify: `src-tauri/src/db.rs`

- [ ] **Step 1: Add a migration test that initially fails**

Add this test module at the end of `src-tauri/src/db.rs` before adding the migration:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_schema_contains_required_tables_and_constraints() {
        for table in [
            "agent_sessions",
            "agent_runs",
            "agent_steps",
            "agent_events",
            "agent_approvals",
        ] {
            assert!(AGENT_SCHEMA_SQL.contains(table), "missing {table}");
        }
        assert!(AGENT_SCHEMA_SQL.contains("UNIQUE(idempotency_key)"));
        assert!(AGENT_SCHEMA_SQL.contains("waiting_approval"));
    }

    #[test]
    fn migration_versions_are_strictly_increasing() {
        let versions: Vec<i64> = migrations().into_iter().map(|m| m.version).collect();
        assert!(versions.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(versions.last(), Some(&4));
    }
}
```

- [ ] **Step 2: Verify the test fails**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml db::tests -- --nocapture
```

Expected: compile failure because `AGENT_SCHEMA_SQL` and migration version 4 do not exist.

- [ ] **Step 3: Define the Agent schema**

Add `pub const AGENT_SCHEMA_SQL: &str` with these additive tables:

```rust
pub const AGENT_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS agent_sessions (
    id TEXT PRIMARY KEY,
    exam_id TEXT REFERENCES exams(id) ON DELETE SET NULL,
    title TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','archived')),
    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);

CREATE TABLE IF NOT EXISTS agent_runs (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
    goal TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued' CHECK(status IN (
        'queued','running','waiting_approval','completed','cancelled','failed','interrupted'
    )),
    trigger_source TEXT NOT NULL DEFAULT 'user' CHECK(trigger_source IN ('user','startup','schedule','recovery')),
    current_step INTEGER NOT NULL DEFAULT 0,
    error_code TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    started_at TEXT,
    completed_at TEXT
);

CREATE TABLE IF NOT EXISTS agent_steps (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    step_index INTEGER NOT NULL,
    tool_name TEXT NOT NULL,
    tool_version TEXT NOT NULL,
    risk INTEGER NOT NULL DEFAULT 0 CHECK(risk BETWEEN 0 AND 4),
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','running','waiting_approval','completed','failed','cancelled','interrupted')),
    input_json TEXT,
    output_json TEXT,
    error TEXT,
    idempotency_key TEXT,
    started_at TEXT,
    completed_at TEXT,
    UNIQUE(idempotency_key),
    UNIQUE(run_id, id),
    UNIQUE(run_id, step_index)
);

CREATE TABLE IF NOT EXISTS agent_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    step_id TEXT REFERENCES agent_steps(id) ON DELETE SET NULL,
    event_type TEXT NOT NULL,
    payload_json TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);

CREATE TABLE IF NOT EXISTS agent_approvals (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    step_id TEXT NOT NULL,
    risk INTEGER NOT NULL CHECK(risk BETWEEN 2 AND 4),
    preview_json TEXT,
    precondition_json TEXT,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','approved','rejected','expired')),
    expires_at TEXT NOT NULL,
    decided_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    FOREIGN KEY (run_id, step_id) REFERENCES agent_steps(run_id, id) ON DELETE CASCADE,
    UNIQUE(step_id)
);

CREATE INDEX IF NOT EXISTS idx_agent_runs_session_status ON agent_runs(session_id, status);
CREATE INDEX IF NOT EXISTS idx_agent_runs_status ON agent_runs(status);
CREATE INDEX IF NOT EXISTS idx_agent_steps_run_index ON agent_steps(run_id, step_index);
CREATE INDEX IF NOT EXISTS idx_agent_events_run_id ON agent_events(run_id, id);
CREATE INDEX IF NOT EXISTS idx_agent_approvals_status_expires ON agent_approvals(status, expires_at);

CREATE TRIGGER IF NOT EXISTS trg_agent_sessions_updated AFTER UPDATE ON agent_sessions
    FOR EACH ROW WHEN NEW.updated_at = OLD.updated_at
    BEGIN UPDATE agent_sessions SET updated_at = datetime('now','localtime') WHERE id = NEW.id; END;
CREATE TRIGGER IF NOT EXISTS trg_agent_runs_updated AFTER UPDATE ON agent_runs
    FOR EACH ROW WHEN NEW.updated_at = OLD.updated_at
    BEGIN UPDATE agent_runs SET updated_at = datetime('now','localtime') WHERE id = NEW.id; END;
"#;
```

Add migration version 4 after version 3:

```rust
Migration {
    version: 4,
    description: "add persistent agent runtime foundation",
    sql: AGENT_SCHEMA_SQL,
    kind: MigrationKind::Up,
},
```

- [ ] **Step 4: Run migration tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml db::tests -- --nocapture
```

Expected: both tests pass.

- [ ] **Step 5: Commit the schema**

```powershell
git add src-tauri/src/db.rs
git commit -m "feat: add persistent agent runtime schema"
```

## Task 4: Build the Rust database pool

**Files:**

- Create: `src-tauri/src/db/runtime.rs`
- Modify: `src-tauri/src/db.rs`

- [ ] **Step 1: Write pool option unit tests**

Create `src-tauri/src/db/runtime.rs` with the test first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pool_enforces_foreign_keys() {
        let path = std::env::temp_dir().join(format!("zhiyan-agent-{}.db", uuid::Uuid::new_v4()));
        let pool = connect(&path).await.unwrap();
        let (enabled,): (i64,) = sqlx::query_as("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(enabled, 1);
        pool.close().await;
        std::fs::remove_file(path).unwrap();
    }
}
```

Expose the module from `src-tauri/src/db.rs`:

```rust
pub mod runtime;
```

- [ ] **Step 2: Verify the test fails**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml db::runtime::tests -- --nocapture
```

Expected: compile failure because `sqlite_options` does not exist.

- [ ] **Step 3: Implement the pool**

Add this implementation above the test module:

```rust
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
    sqlx::raw_sql(super::AGENT_SCHEMA_SQL).execute(&pool).await?;
    Ok(pool)
}

pub async fn health(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1").execute(pool).await?;
    Ok(())
}
```

- [ ] **Step 4: Run formatting and tests**

```powershell
cargo fmt --manifest-path src-tauri\Cargo.toml
cargo test --manifest-path src-tauri\Cargo.toml db::runtime::tests -- --nocapture
```

Expected: test passes.

- [ ] **Step 5: Commit the pool**

```powershell
git add src-tauri/src/db.rs src-tauri/src/db/runtime.rs
git commit -m "feat: add Rust SQLite runtime pool"
```

## Task 5: Define Agent models and pure transitions

**Files:**

- Create: `src-tauri/src/agent/mod.rs`
- Create: `src-tauri/src/agent/error.rs`
- Create: `src-tauri/src/agent/model.rs`
- Create: `src-tauri/src/agent/state.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write transition tests**

Create `src-tauri/src/agent/state.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_happy_path_and_approval_resume() {
        assert_eq!(transition(RunStatus::Queued, RunEvent::Start).unwrap(), RunStatus::Running);
        assert_eq!(
            transition(RunStatus::Running, RunEvent::RequestApproval).unwrap(),
            RunStatus::WaitingApproval
        );
        assert_eq!(
            transition(RunStatus::WaitingApproval, RunEvent::Approve).unwrap(),
            RunStatus::Running
        );
        assert_eq!(
            transition(RunStatus::Running, RunEvent::Complete).unwrap(),
            RunStatus::Completed
        );
    }

    #[test]
    fn rejects_transition_from_terminal_state() {
        let error = transition(RunStatus::Completed, RunEvent::Start).unwrap_err();
        assert_eq!(error.code(), "invalid_transition");
    }

    #[test]
    fn restart_interrupts_only_active_runs() {
        assert_eq!(transition(RunStatus::Running, RunEvent::Interrupt).unwrap(), RunStatus::Interrupted);
        assert!(transition(RunStatus::WaitingApproval, RunEvent::Interrupt).is_err());
    }
}
```

- [ ] **Step 2: Verify compile failure**

Change `mod db;` to `pub mod db;`, add `pub mod agent;` to `lib.rs`, create `agent/mod.rs` with `pub mod state;`, then run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml agent::state::tests -- --nocapture
```

Expected: compile failure for missing types and `transition`.

- [ ] **Step 3: Implement models and errors**

Create `agent/error.rs`:

```rust
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AgentError {
    #[error("invalid transition from {from} using {event}")]
    InvalidTransition { from: String, event: String },
    #[error("agent record not found: {0}")]
    NotFound(String),
    #[error("agent state changed before the operation completed")]
    Conflict,
    #[error("agent persistence failed: {0}")]
    Persistence(String),
}

impl AgentError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidTransition { .. } => "invalid_transition",
            Self::NotFound(_) => "not_found",
            Self::Conflict => "conflict",
            Self::Persistence(_) => "persistence_error",
        }
    }
}
```

Create `agent/model.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    WaitingApproval,
    Completed,
    Cancelled,
    Failed,
    Interrupted,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::WaitingApproval => "waiting_approval",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunEvent {
    Start,
    RequestApproval,
    Approve,
    Reject,
    Complete,
    Fail,
    Cancel,
    Interrupt,
    Resume,
}

impl std::fmt::Display for RunEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AgentSession {
    pub id: String,
    pub exam_id: Option<String>,
    pub title: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AgentRun {
    pub id: String,
    pub session_id: String,
    pub goal: String,
    pub status: RunStatus,
    pub trigger_source: String,
    pub current_step: i64,
    pub error_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}
```

Implement `transition` in `agent/state.rs`:

```rust
use super::error::AgentError;
use super::model::{RunEvent, RunStatus};

pub fn transition(from: RunStatus, event: RunEvent) -> Result<RunStatus, AgentError> {
    use RunEvent::*;
    use RunStatus::*;
    let next = match (from, event) {
        (Queued, Start) | (Interrupted, Resume) => Running,
        (Running, RequestApproval) => WaitingApproval,
        (WaitingApproval, Approve) => Running,
        (WaitingApproval, Reject) => Cancelled,
        (Queued, Cancel) | (Running, Cancel) | (WaitingApproval, Cancel) | (Interrupted, Cancel) => Cancelled,
        (Running, Complete) => Completed,
        (Running, Fail) | (WaitingApproval, Fail) => Failed,
        (Running, Interrupt) => Interrupted,
        _ => {
            return Err(AgentError::InvalidTransition {
                from: from.to_string(),
                event: event.to_string(),
            })
        }
    };
    Ok(next)
}
```

Export modules in `agent/mod.rs`:

```rust
pub mod error;
pub mod model;
pub mod state;
```

- [ ] **Step 4: Run tests and clippy**

```powershell
cargo fmt --manifest-path src-tauri\Cargo.toml
cargo test --manifest-path src-tauri\Cargo.toml agent::state::tests -- --nocapture
cargo clippy --manifest-path src-tauri\Cargo.toml -- -D warnings
```

Expected: all commands exit `0`.

- [ ] **Step 5: Commit state machine**

```powershell
git add src-tauri/src/agent src-tauri/src/lib.rs
git commit -m "feat: add agent run state machine"
```

## Task 6: Persist sessions, runs and recovery

**Files:**

- Create: `src-tauri/src/agent/repository.rs`
- Create: `src-tauri/tests/agent_repository.rs`
- Modify: `src-tauri/src/agent/mod.rs`

- [ ] **Step 1: Write repository integration tests**

Create the complete `src-tauri/tests/agent_repository.rs`:

```rust
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use zhiyan_lib::agent::model::RunStatus;
use zhiyan_lib::agent::repository::AgentRepository;
use zhiyan_lib::db::AGENT_SCHEMA_SQL;

async fn test_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql(AGENT_SCHEMA_SQL).execute(&pool).await.unwrap();
    pool
}

#[tokio::test]
async fn creates_session_and_run_then_updates_status() {
    let pool = test_pool().await;
    let repo = AgentRepository::new(pool);
    let session = repo.create_session(None, "Runtime test").await.unwrap();
    let run = repo.create_run(&session.id, "Inspect today's plan", "user").await.unwrap();
    assert_eq!(run.status, RunStatus::Queued);

    let running = repo.update_run_status(&run.id, RunStatus::Running, None).await.unwrap();
    assert_eq!(running.status, RunStatus::Running);
}

#[tokio::test]
async fn recovery_marks_only_active_runs_interrupted() {
    let pool = test_pool().await;
    let repo = AgentRepository::new(pool);
    let session = repo.create_session(None, "Recovery").await.unwrap();
    let active = repo.create_run(&session.id, "active", "user").await.unwrap();
    repo.update_run_status(&active.id, RunStatus::Running, None).await.unwrap();
    let completed = repo.create_run(&session.id, "done", "user").await.unwrap();
    repo.update_run_status(&completed.id, RunStatus::Running, None).await.unwrap();
    repo.update_run_status(&completed.id, RunStatus::Completed, None).await.unwrap();
    let approval = repo.create_run(&session.id, "approval", "user").await.unwrap();
    repo.update_run_status(&approval.id, RunStatus::WaitingApproval, None).await.unwrap();

    assert_eq!(repo.interrupt_active_runs().await.unwrap(), 1);
    assert_eq!(repo.get_run(&active.id).await.unwrap().status, RunStatus::Interrupted);
    assert_eq!(repo.get_run(&completed.id).await.unwrap().status, RunStatus::Completed);
    assert_eq!(repo.get_run(&approval.id).await.unwrap().status, RunStatus::WaitingApproval);
}
```

- [ ] **Step 2: Verify tests fail**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --test agent_repository -- --nocapture
```

Expected: compile failure because `AgentRepository` does not exist.

- [ ] **Step 3: Implement the repository**

Create the complete `src-tauri/src/agent/repository.rs`:

```rust
use super::error::AgentError;
use super::model::{AgentRun, AgentSession, RunStatus};
use serde_json::Value;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

#[derive(Clone)]
pub struct AgentRepository {
    pool: SqlitePool,
}

impl AgentRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn health(&self) -> Result<(), AgentError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(map_sqlx)?;
        Ok(())
    }

    pub async fn create_session(
        &self,
        exam_id: Option<&str>,
        title: &str,
    ) -> Result<AgentSession, AgentError> {
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO agent_sessions (id, exam_id, title) VALUES (?, ?, ?)")
            .bind(&id)
            .bind(exam_id)
            .bind(title)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx)?;
        sqlx::query_as::<_, AgentSession>("SELECT * FROM agent_sessions WHERE id = ?")
            .bind(&id)
            .fetch_one(&self.pool)
            .await
            .map_err(map_sqlx)
    }

    pub async fn create_run(
        &self,
        session_id: &str,
        goal: &str,
        trigger_source: &str,
    ) -> Result<AgentRun, AgentError> {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO agent_runs (id, session_id, goal, status, trigger_source) \
             VALUES (?, ?, ?, 'queued', ?)",
        )
        .bind(&id)
        .bind(session_id)
        .bind(goal)
        .bind(trigger_source)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx)?;
        self.append_event(&id, "run.created", &serde_json::json!({ "goal": goal }))
            .await?;
        self.get_run(&id).await
    }

    pub async fn get_run(&self, id: &str) -> Result<AgentRun, AgentError> {
        sqlx::query_as::<_, AgentRun>("SELECT * FROM agent_runs WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(map_sqlx)
    }

    pub async fn update_run_status(
        &self,
        id: &str,
        status: RunStatus,
        error_code: Option<&str>,
    ) -> Result<AgentRun, AgentError> {
        let status = status.to_string();
        let result = sqlx::query(
            "UPDATE agent_runs SET status = ?, error_code = ?, \
             completed_at = CASE WHEN ? IN ('completed','cancelled','failed') \
                 THEN datetime('now','localtime') ELSE NULL END \
             WHERE id = ?",
        )
        .bind(&status)
        .bind(error_code)
        .bind(&status)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx)?;
        if result.rows_affected() == 0 {
            return Err(AgentError::NotFound(id.to_string()));
        }
        self.get_run(id).await
    }

    pub async fn transition_run_status(
        &self,
        id: &str,
        expected: RunStatus,
        next: RunStatus,
        event: &str,
    ) -> Result<AgentRun, AgentError> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx)?;
        let next_value = next.to_string();
        let result = sqlx::query(
            "UPDATE agent_runs SET status = ?, \
             completed_at = CASE WHEN ? IN ('completed','cancelled','failed') \
                 THEN datetime('now','localtime') ELSE NULL END \
             WHERE id = ? AND status = ?",
        )
        .bind(&next_value)
        .bind(&next_value)
        .bind(id)
        .bind(expected.to_string())
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        if result.rows_affected() == 0 {
            tx.rollback().await.map_err(map_sqlx)?;
            return Err(AgentError::Conflict);
        }
        let payload = serde_json::json!({
            "from": expected.to_string(),
            "to": next_value,
            "event": event,
        });
        sqlx::query(
            "INSERT INTO agent_events (run_id, event_type, payload_json) \
             VALUES (?, 'run.status_changed', ?)",
        )
        .bind(id)
        .bind(payload.to_string())
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        tx.commit().await.map_err(map_sqlx)?;
        self.get_run(id).await
    }

    pub async fn append_event(
        &self,
        run_id: &str,
        event_type: &str,
        payload: &Value,
    ) -> Result<(), AgentError> {
        sqlx::query(
            "INSERT INTO agent_events (run_id, event_type, payload_json) VALUES (?, ?, ?)",
        )
        .bind(run_id)
        .bind(event_type)
        .bind(payload.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    pub async fn interrupt_active_runs(&self) -> Result<u64, AgentError> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx)?;
        let ids: Vec<String> = sqlx::query("SELECT id FROM agent_runs WHERE status = 'running'")
            .fetch_all(&mut *tx)
            .await
            .map_err(map_sqlx)?
            .into_iter()
            .map(|row| row.get::<String, _>("id"))
            .collect();
        for id in &ids {
            sqlx::query("UPDATE agent_runs SET status = 'interrupted' WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx)?;
            sqlx::query(
                "INSERT INTO agent_events (run_id, event_type, payload_json) \
                 VALUES (?, 'run.interrupted', '{\"reason\":\"application_restart\"}')",
            )
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx)?;
        }
        tx.commit().await.map_err(map_sqlx)?;
        Ok(ids.len() as u64)
    }
}

fn map_sqlx(error: sqlx::Error) -> AgentError {
    match error {
        sqlx::Error::RowNotFound => AgentError::NotFound("agent record".to_string()),
        other => AgentError::Persistence(other.to_string()),
    }
}
```

The persistence error may contain the database error class and message but must not include bound parameter values.

Export the repository in `agent/mod.rs`:

```rust
pub mod repository;
```

- [ ] **Step 4: Run repository and full Rust tests**

```powershell
cargo fmt --manifest-path src-tauri\Cargo.toml
cargo test --manifest-path src-tauri\Cargo.toml --test agent_repository -- --nocapture
cargo test --manifest-path src-tauri\Cargo.toml
```

Expected: all tests pass.

- [ ] **Step 5: Commit repository**

```powershell
git add src-tauri/src/agent src-tauri/tests/agent_repository.rs
git commit -m "feat: persist agent sessions and runs"
```

## Task 7: Add Runtime and Tauri commands

**Files:**

- Create: `src-tauri/src/agent/runtime.rs`
- Create: `src-tauri/src/agent/commands.rs`
- Modify: `src-tauri/src/agent/mod.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write Runtime tests and implementation**

Create the complete `src-tauri/src/agent/runtime.rs`:

```rust
use super::error::AgentError;
use super::model::{AgentRun, AgentSession, RunEvent};
use super::repository::AgentRepository;
use super::state::transition;

#[derive(Clone)]
pub struct AgentRuntime {
    repository: AgentRepository,
}

impl AgentRuntime {
    pub fn new(repository: AgentRepository) -> Self {
        Self { repository }
    }

    pub async fn create_session(
        &self,
        exam_id: Option<&str>,
        title: &str,
    ) -> Result<AgentSession, AgentError> {
        self.repository.create_session(exam_id, title).await
    }

    pub async fn create_run(&self, session_id: &str, goal: &str) -> Result<AgentRun, AgentError> {
        self.repository.create_run(session_id, goal, "user").await
    }

    pub async fn transition_run(
        &self,
        run_id: &str,
        event: RunEvent,
    ) -> Result<AgentRun, AgentError> {
        let current = self.repository.get_run(run_id).await?;
        let next = transition(current.status, event)?;
        self.repository
            .transition_run_status(run_id, current.status, next, &event.to_string())
            .await
    }

    pub async fn recover_interrupted(&self) -> Result<u64, AgentError> {
        self.repository.interrupt_active_runs().await
    }

    pub async fn health(&self) -> Result<(), AgentError> {
        self.repository.health().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::model::RunStatus;
    use crate::db::AGENT_SCHEMA_SQL;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn runtime() -> AgentRuntime {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(AGENT_SCHEMA_SQL).execute(&pool).await.unwrap();
        AgentRuntime::new(AgentRepository::new(pool))
    }

    #[tokio::test]
    async fn creates_starts_and_cancels_run() {
        let runtime = runtime().await;
        let session = runtime.create_session(None, "Today").await.unwrap();
        let run = runtime.create_run(&session.id, "Inspect plan").await.unwrap();
        assert_eq!(run.status, RunStatus::Queued);
        let running = runtime.transition_run(&run.id, RunEvent::Start).await.unwrap();
        assert_eq!(running.status, RunStatus::Running);
        let cancelled = runtime.transition_run(&run.id, RunEvent::Cancel).await.unwrap();
        assert_eq!(cancelled.status, RunStatus::Cancelled);
    }

    #[tokio::test]
    async fn recovery_interrupts_running_run() {
        let runtime = runtime().await;
        let session = runtime.create_session(None, "Recovery").await.unwrap();
        let run = runtime.create_run(&session.id, "Recover me").await.unwrap();
        runtime.transition_run(&run.id, RunEvent::Start).await.unwrap();
        assert_eq!(runtime.recover_interrupted().await.unwrap(), 1);
    }
}
```

- [ ] **Step 2: Verify Runtime tests fail**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml agent::runtime::tests -- --nocapture
```

Expected: compile failure because Runtime is not implemented.

- [ ] **Step 3: Run Runtime tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml agent::runtime::tests -- --nocapture
```

Expected: both Runtime tests pass.

- [ ] **Step 4: Define command DTOs and safe errors**

Create the complete `agent/commands.rs`:

```rust
use super::error::AgentError;
use super::model::{AgentRun, AgentSession, RunEvent};
use super::runtime::AgentRuntime;

#[derive(Debug, serde::Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl From<AgentError> for CommandError {
    fn from(value: AgentError) -> Self {
        Self { code: value.code().to_string(), message: value.to_string() }
    }
}

#[tauri::command]
pub async fn agent_health(runtime: tauri::State<'_, AgentRuntime>) -> Result<(), CommandError> {
    runtime.health().await.map_err(Into::into)
}

#[tauri::command]
pub async fn agent_create_session(
    runtime: tauri::State<'_, AgentRuntime>,
    exam_id: Option<String>,
    title: String,
) -> Result<AgentSession, CommandError> {
    if title.trim().is_empty() {
        return Err(validation_error("title is required"));
    }
    runtime
        .create_session(exam_id.as_deref(), title.trim())
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn agent_create_run(
    runtime: tauri::State<'_, AgentRuntime>,
    session_id: String,
    goal: String,
) -> Result<AgentRun, CommandError> {
    if goal.trim().is_empty() {
        return Err(validation_error("goal is required"));
    }
    runtime
        .create_run(&session_id, goal.trim())
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn agent_start_run(
    runtime: tauri::State<'_, AgentRuntime>,
    run_id: String,
) -> Result<AgentRun, CommandError> {
    runtime
        .transition_run(&run_id, RunEvent::Start)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn agent_cancel_run(
    runtime: tauri::State<'_, AgentRuntime>,
    run_id: String,
) -> Result<AgentRun, CommandError> {
    runtime
        .transition_run(&run_id, RunEvent::Cancel)
        .await
        .map_err(Into::into)
}

fn validation_error(message: &str) -> CommandError {
    CommandError {
        code: "validation_error".to_string(),
        message: message.to_string(),
    }
}
```

Export `commands`, `repository` and `runtime` from `agent/mod.rs`.

- [ ] **Step 5: Wire application state**

Replace `src-tauri/src/lib.rs` with the same plugin list plus this Agent wiring:

```rust
pub mod agent;
mod credentials;
pub mod db;

use agent::repository::AgentRepository;
use agent::runtime::AgentRuntime;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_sql::Builder::default()
                .add_migrations("sqlite:zhiyan.db", db::migrations())
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            credentials::store_api_key,
            credentials::load_api_key,
            credentials::delete_api_key,
            agent::commands::agent_health,
            agent::commands::agent_create_session,
            agent::commands::agent_create_run,
            agent::commands::agent_start_run,
            agent::commands::agent_cancel_run,
        ])
        .setup(|app| {
            db::init_db(app.handle())?;
            let db_path = app.path().app_data_dir()?.join("zhiyan.db");
            let pool = tauri::async_runtime::block_on(db::runtime::connect(&db_path))
                .map_err(|error| std::io::Error::other(format!("agent database: {error}")))?;
            let runtime = AgentRuntime::new(AgentRepository::new(pool));
            tauri::async_runtime::block_on(runtime.recover_interrupted())
                .map_err(|error| std::io::Error::other(format!("agent recovery: {error}")))?;
            app.manage(runtime);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running zhiyan application");
}
```

Startup recovery errors abort setup; the application must not continue with inconsistent Run state.

- [ ] **Step 6: Run Rust verification**

```powershell
cargo fmt --manifest-path src-tauri\Cargo.toml
cargo test --manifest-path src-tauri\Cargo.toml
cargo clippy --manifest-path src-tauri\Cargo.toml -- -D warnings
```

Expected: all commands exit `0`.

- [ ] **Step 7: Commit Runtime and commands**

```powershell
git add src-tauri/src/agent src-tauri/src/lib.rs
git commit -m "feat: expose persistent agent runtime commands"
```

## Task 8: Add the typed frontend contract

**Files:**

- Modify: `src/types/index.ts`
- Create: `src/services/agent-client.ts`
- Create: `src/services/agent-client.test.ts`

- [ ] **Step 1: Write failing invoke contract tests**

Create `agent-client.test.ts`:

```typescript
import { beforeEach, describe, expect, it, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

import { cancelAgentRun, createAgentRun, createAgentSession, startAgentRun } from './agent-client'

describe('agent client', () => {
  beforeEach(() => invoke.mockReset())

  it('uses camelCase Tauri arguments', async () => {
    invoke.mockResolvedValue({ id: 'session-1' })
    await createAgentSession('exam-1', 'Today')
    expect(invoke).toHaveBeenCalledWith('agent_create_session', {
      examId: 'exam-1',
      title: 'Today',
    })
  })

  it.each([
    ['create', () => createAgentRun('session-1', 'Inspect plan'), 'agent_create_run', { sessionId: 'session-1', goal: 'Inspect plan' }],
    ['start', () => startAgentRun('run-1'), 'agent_start_run', { runId: 'run-1' }],
    ['cancel', () => cancelAgentRun('run-1'), 'agent_cancel_run', { runId: 'run-1' }],
  ])('%s command preserves the command contract', async (_name, call, command, args) => {
    invoke.mockResolvedValue({ id: 'run-1' })
    await call()
    expect(invoke).toHaveBeenCalledWith(command, args)
  })
})
```

- [ ] **Step 2: Verify tests fail**

```powershell
npm.cmd test -- src/services/agent-client.test.ts
```

Expected: failure because `agent-client.ts` does not exist.

- [ ] **Step 3: Add Agent DTOs**

Append to `src/types/index.ts`:

```typescript
export type AgentRunStatus =
  | 'queued'
  | 'running'
  | 'waiting_approval'
  | 'completed'
  | 'cancelled'
  | 'failed'
  | 'interrupted'

export interface AgentSession {
  id: string
  exam_id: string | null
  title: string
  status: 'active' | 'archived'
  created_at: string
  updated_at: string
}

export interface AgentRun {
  id: string
  session_id: string
  goal: string
  status: AgentRunStatus
  trigger_source: 'user' | 'startup' | 'schedule' | 'recovery'
  current_step: number
  error_code: string | null
  created_at: string
  updated_at: string
  completed_at: string | null
}
```

- [ ] **Step 4: Implement the client**

Create `agent-client.ts`:

```typescript
import { invoke } from '@tauri-apps/api/core'
import type { AgentRun, AgentSession } from '@/types'

export function agentHealth(): Promise<void> {
  return invoke('agent_health')
}

export function createAgentSession(examId: string | null, title: string): Promise<AgentSession> {
  return invoke('agent_create_session', { examId, title })
}

export function createAgentRun(sessionId: string, goal: string): Promise<AgentRun> {
  return invoke('agent_create_run', { sessionId, goal })
}

export function startAgentRun(runId: string): Promise<AgentRun> {
  return invoke('agent_start_run', { runId })
}

export function cancelAgentRun(runId: string): Promise<AgentRun> {
  return invoke('agent_cancel_run', { runId })
}
```

- [ ] **Step 5: Run frontend tests and typecheck**

```powershell
npm.cmd test -- src/services/agent-client.test.ts
npm.cmd run typecheck
```

Expected: all tests pass and typecheck exits `0`.

- [ ] **Step 6: Commit the contract**

```powershell
git add src/types/index.ts src/services/agent-client.ts src/services/agent-client.test.ts
git commit -m "feat: add typed agent runtime client"
```

## Task 9: Add the hidden Agent debug page

**Files:**

- Create: `src/pages/AgentDebug.vue`
- Modify: `src/router/index.ts`

- [ ] **Step 1: Add a component smoke test**

Create `src/pages/AgentDebug.test.ts`:

```typescript
// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { AgentRun, AgentSession } from '@/types'

const mocks = vi.hoisted(() => ({
  health: vi.fn(),
  createSession: vi.fn(),
  createRun: vi.fn(),
  startRun: vi.fn(),
  cancelRun: vi.fn(),
}))

vi.mock('@/services/agent-client', () => ({
  agentHealth: mocks.health,
  createAgentSession: mocks.createSession,
  createAgentRun: mocks.createRun,
  startAgentRun: mocks.startRun,
  cancelAgentRun: mocks.cancelRun,
}))

import AgentDebug from './AgentDebug.vue'
import { useExamStore } from '@/stores/exam'

const session: AgentSession = {
  id: 'session-1',
  exam_id: 'exam-1',
  title: 'Runtime test',
  status: 'active',
  created_at: '2026-07-17 10:00:00',
  updated_at: '2026-07-17 10:00:00',
}

const queued: AgentRun = {
  id: 'run-1',
  session_id: 'session-1',
  goal: 'Inspect today plan',
  status: 'queued',
  trigger_source: 'user',
  current_step: 0,
  error_code: null,
  created_at: '2026-07-17 10:00:00',
  updated_at: '2026-07-17 10:00:00',
  completed_at: null,
}

describe('AgentDebug', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.health.mockResolvedValue(undefined)
    mocks.createSession.mockResolvedValue(session)
    mocks.createRun.mockResolvedValue(queued)
    mocks.startRun.mockResolvedValue({ ...queued, status: 'running' })
  })

  it('creates a session and starts a run', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    useExamStore().setActiveExam('exam-1')
    const wrapper = mount(AgentDebug, { global: { plugins: [pinia] } })
    await flushPromises()
    expect(wrapper.get('[data-test="health"]').text()).toContain('可用')

    await wrapper.get('[data-test="create-session"]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test="start-run"]').trigger('click')
    await flushPromises()

    expect(mocks.createSession).toHaveBeenCalledWith('exam-1', 'Runtime test')
    expect(mocks.createRun).toHaveBeenCalledWith('session-1', 'Inspect today plan')
    expect(mocks.startRun).toHaveBeenCalledWith('run-1')
    expect(wrapper.get('[data-test="run-status"]').text()).toContain('running')
  })
})
```

- [ ] **Step 2: Verify the test fails**

```powershell
npm.cmd test -- src/pages/AgentDebug.test.ts
```

Expected: failure because `AgentDebug.vue` does not exist.

- [ ] **Step 3: Implement the debug page**

Create `src/pages/AgentDebug.vue`:

```vue
<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useExamStore } from '@/stores/exam'
import {
  agentHealth,
  cancelAgentRun,
  createAgentRun,
  createAgentSession,
  startAgentRun,
} from '@/services/agent-client'
import type { AgentRun, AgentSession } from '@/types'

const examStore = useExamStore()
const healthy = ref(false)
const session = ref<AgentSession | null>(null)
const run = ref<AgentRun | null>(null)
const goal = ref('Inspect today plan')
const error = ref('')

async function perform(action: () => Promise<void>) {
  error.value = ''
  try {
    await action()
  } catch (cause) {
    error.value = (cause as { message?: string })?.message ?? String(cause)
  }
}

async function createSession() {
  await perform(async () => {
    session.value = await createAgentSession(examStore.activeExamId, 'Runtime test')
  })
}

async function createAndStartRun() {
  if (!session.value) return
  await perform(async () => {
    run.value = await createAgentRun(session.value!.id, goal.value)
    run.value = await startAgentRun(run.value.id)
  })
}

async function cancelRun() {
  if (!run.value) return
  await perform(async () => {
    run.value = await cancelAgentRun(run.value!.id)
  })
}

onMounted(() =>
  perform(async () => {
    await agentHealth()
    healthy.value = true
  }),
)
</script>

<template>
  <section class="agent-debug">
    <h1>Agent Runtime Debug</h1>
    <p data-test="health">Runtime：{{ healthy ? '可用' : '不可用' }}</p>
    <button data-test="create-session" type="button" @click="createSession">创建测试会话</button>
    <label>
      Goal
      <input v-model="goal" data-test="goal" />
    </label>
    <button data-test="start-run" type="button" :disabled="!session" @click="createAndStartRun">
      创建并启动 Run
    </button>
    <button data-test="cancel-run" type="button" :disabled="!run" @click="cancelRun">
      取消 Run
    </button>
    <dl>
      <dt>Session</dt><dd>{{ session?.id ?? '—' }}</dd>
      <dt>Run</dt><dd>{{ run?.id ?? '—' }}</dd>
      <dt>Status</dt><dd data-test="run-status">{{ run?.status ?? '—' }}</dd>
    </dl>
    <p v-if="error" role="alert">{{ error }}</p>
  </section>
</template>

<style scoped>
.agent-debug { display: grid; gap: 16px; max-width: 720px; }
button, input { min-height: 36px; }
dl { display: grid; grid-template-columns: 100px 1fr; gap: 8px; }
dt { font-weight: 600; }
</style>
```

Do not add this page to `AppLayout.vue` navigation.

- [ ] **Step 4: Add the hidden route**

Add before the catch-all route in `src/router/index.ts`:

```typescript
{
  path: '/agent-debug',
  name: 'agent-debug',
  component: () => import('@/pages/AgentDebug.vue'),
},
```

- [ ] **Step 5: Run page tests, typecheck and build**

```powershell
npm.cmd test -- src/pages/AgentDebug.test.ts src/services/agent-client.test.ts
npm.cmd run typecheck
npm.cmd run build
```

Expected: all commands exit `0`.

- [ ] **Step 6: Commit the debug page**

```powershell
git add src/pages/AgentDebug.vue src/pages/AgentDebug.test.ts src/router/index.ts
git commit -m "feat: add hidden agent runtime debug page"
```

## Task 10: Milestone verification and handoff

**Files:**

- Modify: `docs/agent/feature-parity.md`
- Modify: `docs/agent/migration-runbook.md`
- Modify: `MANUAL_TEST.md`

- [ ] **Step 1: Update ownership state**

Change “Agent session and run state” from `foundation` to `rust-owned`. Add Milestone 1 manual steps:

```markdown
## Agent Runtime Foundation

1. Start the Tauri application and open `/agent-debug` directly.
2. Confirm Runtime health is available.
3. Create a test session and Run.
4. Start the Run and confirm its status becomes `running`.
5. Restart the app and confirm the unfinished Run is `interrupted`.
6. Confirm dashboard, plan check-in, records, analysis and settings still work.
```

- [ ] **Step 2: Run the full verification suite**

```powershell
npm.cmd test
npm.cmd run typecheck
npm.cmd run build
cargo fmt --manifest-path src-tauri\Cargo.toml -- --check
cargo test --manifest-path src-tauri\Cargo.toml
cargo clippy --manifest-path src-tauri\Cargo.toml -- -D warnings
git diff --check
```

Expected:

- Vitest includes the original 33 tests plus new agent-client and debug-page tests.
- Rust unit and integration tests pass.
- Typecheck and both builds exit `0`.
- Clippy reports no warnings.
- `git diff --check` prints nothing.

- [ ] **Step 3: Test the packaged database path manually**

Run:

```powershell
npm.cmd run tauri dev
```

Open `/agent-debug`, create and start a Run, close and reopen the application, and confirm recovery marks it `interrupted`. Confirm existing study records remain unchanged.

- [ ] **Step 4: Commit milestone documentation**

```powershell
git add docs/agent/feature-parity.md docs/agent/migration-runbook.md MANUAL_TEST.md
git commit -m "docs: complete agent runtime foundation milestone"
```

- [ ] **Step 5: Create the next detailed plan**

Write `docs/superpowers/plans/2026-07-17-agent-tools-policy.md` from the verified Rust repository and Runtime APIs. Its first vertical slice must be `plan.get_today` plus `record.checkin_plan`, including TypeScript/Rust parity tests and R1 idempotency.
