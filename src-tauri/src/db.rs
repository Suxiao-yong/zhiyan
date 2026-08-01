//! 数据库初始化与迁移。
//!
//! 使用 tauri-plugin-sql 管理 SQLite 连接；迁移在插件首次打开数据库时自动执行。
//! 时间基准统一为 datetime('now','localtime')，与本地业务日期、跨天 04:00 规则一致。

use tauri::{AppHandle, Manager};
use tauri_plugin_sql::{Migration, MigrationKind};

pub mod runtime;

/// 初始 schema：PRAGMA + 8 张表 + 6 个 updated_at 触发器 + 13 个索引。
///
/// 注意：tauri-plugin-sql 用连接池，PRAGMA foreign_keys 在事务内为 no-op，
/// 此处 PRAGMA 仅作兜底；真正的连接级外键强制由前端 `db.ts` 在 `getDb()` 后
/// 执行 `PRAGMA foreign_keys=ON`（以及连接 URL `?foreign_keys=on`）保证，
/// 并在 Step 9 实测级联删除是否生效。
pub fn migrations() -> Vec<Migration> {
    vec![
        Migration {
            version: 1,
            description: "initial schema: 8 tables, 6 updated_at triggers, 13 indexes",
            sql: SCHEMA_SQL,
            kind: MigrationKind::Up,
        },
        Migration {
            version: 2,
            description: "add sort_order to study_plans for drag-reorder (Phase 3)",
            sql: "ALTER TABLE study_plans ADD COLUMN sort_order INTEGER DEFAULT 0;",
            kind: MigrationKind::Up,
        },
        Migration {
            version: 3,
            description: "link study records to plan tasks for repeated check-ins",
            sql: r#"
                ALTER TABLE study_records
                    ADD COLUMN plan_id TEXT REFERENCES study_plans(id) ON DELETE SET NULL;
                CREATE INDEX IF NOT EXISTS idx_records_plan ON study_records(plan_id);
            "#,
            kind: MigrationKind::Up,
        },
        Migration {
            version: 4,
            description: "add persistent agent runtime foundation",
            sql: AGENT_SCHEMA_SQL,
            kind: MigrationKind::Up,
        },
        Migration {
            version: 5,
            description: "add agent tool policy receipts and ownership flags",
            sql: r#"
                ALTER TABLE agent_steps ADD COLUMN policy_json TEXT;
                ALTER TABLE agent_steps ADD COLUMN receipt_json TEXT;
                ALTER TABLE agent_steps ADD COLUMN undo_json TEXT;
                ALTER TABLE agent_steps ADD COLUMN undone_at TEXT;
                CREATE INDEX IF NOT EXISTS idx_agent_steps_tool_status ON agent_steps(tool_name, status);
                INSERT OR IGNORE INTO settings (key, value, description) VALUES
                    ('agent_tool_owner.plan.get_today','shadow','typescript|shadow|rust-owned; controls plan.get_today delivery'),
                    ('agent_tool_owner.record.checkin_plan','typescript','typescript|shadow|rust-owned; controls record.checkin_plan writes');
            "#,
            kind: MigrationKind::Up,
        },
        Migration {
            version: 6,
            description: "add agent context audit table (model-call data provenance)",
            sql: AGENT_CONTEXT_AUDIT_SQL,
            kind: MigrationKind::Up,
        },
        Migration {
            version: 7,
            description: "add structured long-term memory table",
            sql: AGENT_MEMORIES_SQL,
            kind: MigrationKind::Up,
        },
    ]
}

const SCHEMA_SQL: &str = r#"
PRAGMA foreign_keys = ON;

-- ============================================================
-- 时间基准统一：所有 DEFAULT 与触发器均使用 datetime('now','localtime')，
-- 与业务日期（本地日期）、跨天 04:00 规则（本地时间）保持一致。
-- SQLite 的 datetime('now') 默认返回 UTC，混用会导致日期错位。
-- ============================================================

-- 外键策略（与 PRAGMA foreign_keys = ON 配套，否则删除会抛约束错误）：
--   - 明细表对父表用 ON DELETE CASCADE：删考试 → 连带删除其科目/记录/计划/错题
--     （应用层必须在删除考试/科目时弹强确认框，告知将级联删除的数据量）
--   - 可空外键用 ON DELETE SET NULL：删知识点时记录/错题/计划保留、引用置空
--   - knowledge_points.parent_id 用 ON DELETE SET NULL：删某知识点时其子节点上浮为顶层，不连带删除整个子树

-- 1. 考试配置表
CREATE TABLE IF NOT EXISTS exams (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    exam_type TEXT,
    exam_date TEXT NOT NULL,
    total_score REAL,
    description TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);

-- 2. 科目表
CREATE TABLE IF NOT EXISTS subjects (
    id TEXT PRIMARY KEY,
    exam_id TEXT NOT NULL REFERENCES exams(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    target_score REAL,
    current_level INTEGER DEFAULT 3 CHECK(current_level BETWEEN 1 AND 5),
    weight REAL DEFAULT 1.0,
    sort_order INTEGER DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);

-- 3. 知识点表（支持树形结构）
CREATE TABLE IF NOT EXISTS knowledge_points (
    id TEXT PRIMARY KEY,
    subject_id TEXT NOT NULL REFERENCES subjects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    parent_id TEXT REFERENCES knowledge_points(id) ON DELETE SET NULL,
    weight REAL DEFAULT 1.0,
    difficulty_level INTEGER DEFAULT 3 CHECK(difficulty_level BETWEEN 1 AND 5),
    current_mastery INTEGER DEFAULT 3 CHECK(current_mastery BETWEEN 1 AND 5),  -- 当前掌握度(1-5)：由 Welcome 自评初始化，由学习记录 mastery_rating 聚合更新
    chapter TEXT,
    sort_order INTEGER DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);

-- 4. 学习记录表
CREATE TABLE IF NOT EXISTS study_records (
    id TEXT PRIMARY KEY,
    date TEXT NOT NULL,
    subject_id TEXT NOT NULL REFERENCES subjects(id) ON DELETE CASCADE,
    knowledge_point_id TEXT REFERENCES knowledge_points(id) ON DELETE SET NULL,
    duration_min INTEGER NOT NULL CHECK(duration_min >= 0),
    content TEXT,
    questions_count INTEGER DEFAULT 0 CHECK(questions_count >= 0),
    correct_count INTEGER DEFAULT 0 CHECK(correct_count >= 0),
    mastery_rating INTEGER CHECK(mastery_rating IS NULL OR mastery_rating BETWEEN 1 AND 5),
    difficulty_notes TEXT,
    mood INTEGER CHECK(mood IS NULL OR mood BETWEEN 1 AND 5),
    session_time TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);

-- 5. 学习计划表（planned_duration/actual_duration 单位均为分钟，与 study_records.duration_min 一致）
CREATE TABLE IF NOT EXISTS study_plans (
    id TEXT PRIMARY KEY,
    exam_id TEXT NOT NULL REFERENCES exams(id) ON DELETE CASCADE,
    subject_id TEXT NOT NULL REFERENCES subjects(id) ON DELETE CASCADE,
    knowledge_point_id TEXT REFERENCES knowledge_points(id) ON DELETE SET NULL,
    date TEXT NOT NULL,
    planned_tasks TEXT,
    planned_duration INTEGER,
    actual_duration INTEGER,
    actual_tasks TEXT,
    status TEXT DEFAULT 'pending' CHECK(status IN ('pending','in_progress','completed','skipped')),
    generated_by TEXT DEFAULT 'ai' CHECK(generated_by IN ('ai','local')),
    ai_suggestion TEXT,
    user_modified INTEGER DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);

-- 6. 错题表
CREATE TABLE IF NOT EXISTS wrong_questions (
    id TEXT PRIMARY KEY,
    record_id TEXT REFERENCES study_records(id) ON DELETE SET NULL,  -- 记录删除时错题保留为独立条目
    subject_id TEXT NOT NULL REFERENCES subjects(id) ON DELETE CASCADE,
    knowledge_point_id TEXT REFERENCES knowledge_points(id) ON DELETE SET NULL,
    question_source TEXT,
    question_desc TEXT,
    correct_answer TEXT,
    my_answer TEXT,
    error_type TEXT,
    error_reason TEXT,
    review_count INTEGER DEFAULT 0,
    mastered INTEGER DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    last_review_at TEXT
);

-- 7. AI分析结果表（移除 confirmed_by：单用户本地应用无意义；user_confirmed: 0=未处理 1=确认 2=拒绝）
CREATE TABLE IF NOT EXISTS ai_analyses (
    id TEXT PRIMARY KEY,
    analysis_type TEXT NOT NULL CHECK(analysis_type IN ('daily','weekly','phase','prediction','adjustment')),
    period_start TEXT,
    period_end TEXT,
    subjects_analyzed TEXT,
    content TEXT,
    suggestions TEXT,
    scores_prediction TEXT,
    generated_by TEXT DEFAULT 'ai' CHECK(generated_by IN ('ai','local')),
    user_confirmed INTEGER DEFAULT 0 CHECK(user_confirmed IN (0,1,2)),
    applied INTEGER DEFAULT 0 CHECK(applied IN (0,1)),
    applied_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);

-- 8. 系统设置表（主键为 key，非 UUID——键值表例外）
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT,
    description TEXT,
    updated_at TEXT DEFAULT (datetime('now','localtime'))
);

-- ============================================================
-- updated_at 自动更新触发器
-- SQLite 的 DEFAULT 仅在 INSERT 时求值，UPDATE 不会刷新 updated_at，
-- 且 SQLite 无 ON UPDATE 语法，必须用 AFTER UPDATE 触发器显式更新。
-- WHEN 条件避免在应用层已显式设置 updated_at 时被覆盖，也避免递归。
-- ============================================================
CREATE TRIGGER IF NOT EXISTS trg_exams_updated AFTER UPDATE ON exams
    FOR EACH ROW WHEN NEW.updated_at = OLD.updated_at
    BEGIN UPDATE exams SET updated_at = datetime('now','localtime') WHERE id = NEW.id; END;
CREATE TRIGGER IF NOT EXISTS trg_subjects_updated AFTER UPDATE ON subjects
    FOR EACH ROW WHEN NEW.updated_at = OLD.updated_at
    BEGIN UPDATE subjects SET updated_at = datetime('now','localtime') WHERE id = NEW.id; END;
CREATE TRIGGER IF NOT EXISTS trg_kp_updated AFTER UPDATE ON knowledge_points
    FOR EACH ROW WHEN NEW.updated_at = OLD.updated_at
    BEGIN UPDATE knowledge_points SET updated_at = datetime('now','localtime') WHERE id = NEW.id; END;
CREATE TRIGGER IF NOT EXISTS trg_records_updated AFTER UPDATE ON study_records
    FOR EACH ROW WHEN NEW.updated_at = OLD.updated_at
    BEGIN UPDATE study_records SET updated_at = datetime('now','localtime') WHERE id = NEW.id; END;
CREATE TRIGGER IF NOT EXISTS trg_plans_updated AFTER UPDATE ON study_plans
    FOR EACH ROW WHEN NEW.updated_at = OLD.updated_at
    BEGIN UPDATE study_plans SET updated_at = datetime('now','localtime') WHERE id = NEW.id; END;
CREATE TRIGGER IF NOT EXISTS trg_settings_updated AFTER UPDATE ON settings
    FOR EACH ROW WHEN NEW.updated_at = OLD.updated_at
    BEGIN UPDATE settings SET updated_at = datetime('now','localtime') WHERE key = NEW.key; END;

-- ============================================================
-- 性能索引（按高频查询场景补全，非仅 2 个）
-- ============================================================
CREATE INDEX IF NOT EXISTS idx_records_date_subject ON study_records(date, subject_id);
CREATE INDEX IF NOT EXISTS idx_records_subject ON study_records(subject_id);
CREATE INDEX IF NOT EXISTS idx_records_kp ON study_records(knowledge_point_id);
CREATE INDEX IF NOT EXISTS idx_plans_date_status ON study_plans(date, status);
CREATE INDEX IF NOT EXISTS idx_plans_exam_date ON study_plans(exam_id, date);
CREATE INDEX IF NOT EXISTS idx_plans_subject ON study_plans(subject_id);
CREATE INDEX IF NOT EXISTS idx_wrong_subject ON wrong_questions(subject_id);
CREATE INDEX IF NOT EXISTS idx_wrong_kp ON wrong_questions(knowledge_point_id);
CREATE INDEX IF NOT EXISTS idx_wrong_mastered ON wrong_questions(mastered);
CREATE INDEX IF NOT EXISTS idx_kp_subject ON knowledge_points(subject_id);
CREATE INDEX IF NOT EXISTS idx_kp_parent ON knowledge_points(parent_id);
CREATE INDEX IF NOT EXISTS idx_subjects_exam ON subjects(exam_id);
CREATE INDEX IF NOT EXISTS idx_analyses_type_date ON ai_analyses(analysis_type, created_at);
"#;

/// Persistent runtime state for agent sessions, runs, steps, events, and approvals.
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
    status TEXT NOT NULL DEFAULT 'queued' CHECK(status IN ('queued','running','waiting_approval','completed','cancelled','failed','interrupted')),
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

CREATE TABLE IF NOT EXISTS agent_context_audit (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    call_seq INTEGER NOT NULL,
    purpose TEXT NOT NULL,
    local INTEGER NOT NULL DEFAULT 0,
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    tools_offered_json TEXT NOT NULL DEFAULT '[]',
    categories_json TEXT NOT NULL DEFAULT '[]',
    record_ids_json TEXT NOT NULL DEFAULT '{}',
    field_sets_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);
CREATE INDEX IF NOT EXISTS idx_agent_context_audit_run ON agent_context_audit(run_id, call_seq);

CREATE TABLE IF NOT EXISTS agent_memories (
    id TEXT PRIMARY KEY,
    exam_id TEXT REFERENCES exams(id) ON DELETE SET NULL,
    memory_type TEXT NOT NULL CHECK(memory_type IN ('schedule_preference','daily_capacity','subject_preference','learning_constraint','reminder_preference','strategy_preference','confirmed_weakness')),
    content TEXT NOT NULL,
    source TEXT NOT NULL CHECK(source IN ('user_statement','behavior_inferred','model_candidate')),
    confidence REAL NOT NULL DEFAULT 0.5 CHECK(confidence BETWEEN 0 AND 1),
    status TEXT NOT NULL DEFAULT 'candidate' CHECK(status IN ('candidate','confirmed','inactive')),
    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    last_used_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_agent_memories_exam_type_status ON agent_memories(exam_id, memory_type, status);
CREATE INDEX IF NOT EXISTS idx_agent_memories_status_last_used ON agent_memories(status, last_used_at);

CREATE TRIGGER IF NOT EXISTS trg_agent_sessions_updated AFTER UPDATE ON agent_sessions
    FOR EACH ROW WHEN NEW.updated_at = OLD.updated_at
    BEGIN UPDATE agent_sessions SET updated_at = datetime('now','localtime') WHERE id = NEW.id; END;
CREATE TRIGGER IF NOT EXISTS trg_agent_runs_updated AFTER UPDATE ON agent_runs
    FOR EACH ROW WHEN NEW.updated_at = OLD.updated_at
    BEGIN UPDATE agent_runs SET updated_at = datetime('now','localtime') WHERE id = NEW.id; END;
CREATE TRIGGER IF NOT EXISTS trg_agent_memories_updated AFTER UPDATE ON agent_memories
    FOR EACH ROW WHEN NEW.updated_at = OLD.updated_at
    BEGIN UPDATE agent_memories SET updated_at = datetime('now','localtime') WHERE id = NEW.id; END;
"#;

/// Dedicated model-call data provenance table (M3 Part 3). Records, per model
/// call, the tools offered, the in-scope business data categories + record IDs +
/// field names, token usage, and local-mode flag. Stores NO raw content (no
/// plan tasks, record text, or wrong-question text) per the §10.2 privacy rule.
pub const AGENT_CONTEXT_AUDIT_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS agent_context_audit (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    call_seq INTEGER NOT NULL,
    purpose TEXT NOT NULL,
    local INTEGER NOT NULL DEFAULT 0,
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    tools_offered_json TEXT NOT NULL DEFAULT '[]',
    categories_json TEXT NOT NULL DEFAULT '[]',
    record_ids_json TEXT NOT NULL DEFAULT '{}',
    field_sets_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);
CREATE INDEX IF NOT EXISTS idx_agent_context_audit_run ON agent_context_audit(run_id, call_seq);
"#;

/// Structured long-term memory (M3 Part 3). Memories carry a type (one of the
/// seven spec §11 types), source (user statement / behavior inference / model
/// candidate), confidence, and a status gate: model- or inference-derived
/// memories start as `candidate` and require user confirmation; explicit user
/// statements are confirmed automatically. Content is plain text, never
/// vector-embedded; Runtime picks a few relevant records by exam, type, status,
/// and last use.
pub const AGENT_MEMORIES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS agent_memories (
    id TEXT PRIMARY KEY,
    exam_id TEXT REFERENCES exams(id) ON DELETE SET NULL,
    memory_type TEXT NOT NULL CHECK(memory_type IN ('schedule_preference','daily_capacity','subject_preference','learning_constraint','reminder_preference','strategy_preference','confirmed_weakness')),
    content TEXT NOT NULL,
    source TEXT NOT NULL CHECK(source IN ('user_statement','behavior_inferred','model_candidate')),
    confidence REAL NOT NULL DEFAULT 0.5 CHECK(confidence BETWEEN 0 AND 1),
    status TEXT NOT NULL DEFAULT 'candidate' CHECK(status IN ('candidate','confirmed','inactive')),
    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    last_used_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_agent_memories_exam_type_status ON agent_memories(exam_id, memory_type, status);
CREATE INDEX IF NOT EXISTS idx_agent_memories_status_last_used ON agent_memories(status, last_used_at);
"#;

/// 确保应用数据目录存在（SQLite 数据库文件将落在此目录）。
pub fn init_db(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let app_data = app.path().app_data_dir()?;
    if !app_data.exists() {
        std::fs::create_dir_all(&app_data)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use sqlx::{
        sqlite::{SqliteConnectOptions, SqlitePoolOptions},
        Row,
    };

    use super::{
        migrations, AGENT_CONTEXT_AUDIT_SQL, AGENT_MEMORIES_SQL, AGENT_SCHEMA_SQL, SCHEMA_SQL,
    };

    #[test]
    fn agent_schema_contains_required_tables_and_constraints() {
        for required_fragment in [
            "agent_sessions",
            "agent_runs",
            "agent_steps",
            "agent_events",
            "agent_approvals",
            "agent_context_audit",
            "agent_memories",
            "UNIQUE(idempotency_key)",
            "waiting_approval",
            "schedule_preference",
            "confirmed_weakness",
            "candidate",
        ] {
            assert!(
                AGENT_SCHEMA_SQL.contains(required_fragment),
                "agent schema is missing {required_fragment}"
            );
        }
    }

    #[test]
    fn migration_versions_are_strictly_increasing() {
        let versions: Vec<_> = migrations()
            .iter()
            .map(|migration| migration.version)
            .collect();

        assert!(versions.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(versions.last(), Some(&7));
    }

    #[test]
    fn migration_v5_adds_tool_receipts_and_ownership_flags() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let pool = SqlitePoolOptions::new()
                    .max_connections(1)
                    .connect("sqlite::memory:")
                    .await
                    .unwrap();
                let migration_list = migrations();

                sqlx::raw_sql(SCHEMA_SQL).execute(&pool).await.unwrap();
                for migration in migration_list.iter().skip(1) {
                    sqlx::raw_sql(migration.sql)
                        .execute(&pool)
                        .await
                        .unwrap();
                }

                let columns: Vec<String> = sqlx::query_scalar(
                    "SELECT name FROM pragma_table_info('agent_steps') ORDER BY cid",
                )
                .fetch_all(&pool)
                .await
                .unwrap();
                for receipt_column in ["policy_json", "receipt_json", "undo_json", "undone_at"] {
                    assert!(
                        columns.iter().any(|column| column == receipt_column),
                        "agent_steps.{receipt_column} must exist"
                    );
                }

                let tool_status_index_count: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_agent_steps_tool_status'",
                )
                .fetch_one(&pool)
                .await
                .unwrap();
                assert_eq!(tool_status_index_count, 1);

                let tool_status_index_columns: Vec<String> = sqlx::query_scalar(
                    "SELECT name FROM pragma_index_info('idx_agent_steps_tool_status') ORDER BY seqno",
                )
                .fetch_all(&pool)
                .await
                .unwrap();
                assert_eq!(tool_status_index_columns, vec!["tool_name", "status"]);

                let owners: Vec<(String, String)> = sqlx::query_as(
                    "SELECT key, value FROM settings WHERE key LIKE 'agent_tool_owner.%' ORDER BY key",
                )
                .fetch_all(&pool)
                .await
                .unwrap();
                assert_eq!(
                    owners,
                    vec![
                        (
                            "agent_tool_owner.plan.get_today".to_owned(),
                            "shadow".to_owned(),
                        ),
                        (
                            "agent_tool_owner.record.checkin_plan".to_owned(),
                            "typescript".to_owned(),
                        ),
                    ]
                );
                assert_eq!(
                    migration_list.last().map(|migration| migration.version),
                    Some(7)
                );
            });
    }

    #[test]
    fn migration_v6_adds_context_audit_table() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let pool = SqlitePoolOptions::new()
                    .max_connections(1)
                    .connect("sqlite::memory:")
                    .await
                    .unwrap();
                for migration in &migrations() {
                    sqlx::raw_sql(migration.sql).execute(&pool).await.unwrap();
                }

                let table_exists: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='agent_context_audit'",
                )
                .fetch_one(&pool)
                .await
                .unwrap();
                assert_eq!(table_exists, 1);

                let columns: Vec<String> =
                    sqlx::query_scalar("SELECT name FROM pragma_table_info('agent_context_audit')")
                        .fetch_all(&pool)
                        .await
                        .unwrap();
                for column in [
                    "id",
                    "run_id",
                    "call_seq",
                    "purpose",
                    "local",
                    "prompt_tokens",
                    "completion_tokens",
                    "tools_offered_json",
                    "categories_json",
                    "record_ids_json",
                    "field_sets_json",
                    "created_at",
                ] {
                    assert!(
                        columns.iter().any(|c| c == column),
                        "agent_context_audit.{column} must exist"
                    );
                }

                // v6 is additive CREATE TABLE; v4 rows survive a v6 upgrade.
                sqlx::query("INSERT INTO agent_sessions (id, title) VALUES ('session-v6', 'V6')")
                    .execute(&pool)
                    .await
                    .unwrap();
                sqlx::query("INSERT INTO agent_runs (id, session_id, goal) VALUES ('run-v6', 'session-v6', 'survive')")
                    .execute(&pool)
                    .await
                    .unwrap();
                sqlx::raw_sql(AGENT_CONTEXT_AUDIT_SQL).execute(&pool).await.unwrap();
                let survived: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM agent_runs WHERE id='run-v6'")
                        .fetch_one(&pool)
                        .await
                        .unwrap();
                assert_eq!(survived, 1);
            });
    }

    #[test]
    fn migration_v7_adds_memories_table() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let pool = SqlitePoolOptions::new()
                    .max_connections(1)
                    .connect("sqlite::memory:")
                    .await
                    .unwrap();
                for migration in &migrations() {
                    sqlx::raw_sql(migration.sql).execute(&pool).await.unwrap();
                }

                let table_exists: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='agent_memories'",
                )
                .fetch_one(&pool)
                .await
                .unwrap();
                assert_eq!(table_exists, 1);

                let columns: Vec<String> =
                    sqlx::query_scalar("SELECT name FROM pragma_table_info('agent_memories')")
                        .fetch_all(&pool)
                        .await
                        .unwrap();
                for column in [
                    "id",
                    "exam_id",
                    "memory_type",
                    "content",
                    "source",
                    "confidence",
                    "status",
                    "created_at",
                    "updated_at",
                    "last_used_at",
                ] {
                    assert!(
                        columns.iter().any(|c| c == column),
                        "agent_memories.{column} must exist"
                    );
                }

                // CHECKs reject bad type/source/status/confidence values.
                let bad_type = sqlx::query(
                    "INSERT INTO agent_memories (id, memory_type, content) VALUES ('m-bad', 'unknown_type', 'x')",
                )
                .execute(&pool)
                .await;
                assert!(bad_type.is_err());
                let bad_status = sqlx::query(
                    "INSERT INTO agent_memories (id, memory_type, content, status) VALUES ('m-bad', 'schedule_preference', 'x', 'deleted')",
                )
                .execute(&pool)
                .await;
                assert!(bad_status.is_err());
                let bad_confidence = sqlx::query(
                    "INSERT INTO agent_memories (id, memory_type, content, confidence) VALUES ('m-bad', 'schedule_preference', 'x', 1.5)",
                )
                .execute(&pool)
                .await;
                assert!(bad_confidence.is_err());

                // v7 is additive CREATE TABLE; v6-era rows survive a v7 upgrade.
                sqlx::query("INSERT INTO agent_sessions (id, title) VALUES ('session-v7', 'V7')")
                    .execute(&pool)
                    .await
                    .unwrap();
                sqlx::query("INSERT INTO agent_runs (id, session_id, goal) VALUES ('run-v7', 'session-v7', 'survive')")
                    .execute(&pool)
                    .await
                    .unwrap();
                sqlx::raw_sql(AGENT_MEMORIES_SQL).execute(&pool).await.unwrap();
                let survived: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM agent_runs WHERE id='run-v7'")
                        .fetch_one(&pool)
                        .await
                        .unwrap();
                assert_eq!(survived, 1);
            });
    }

    async fn seed_v4_rows(pool: &sqlx::SqlitePool) {
        sqlx::raw_sql(
            r#"
                INSERT INTO exams (id, name, exam_date) VALUES ('exam-seeded', 'Seeded', '2030-01-01');
                INSERT INTO subjects (id, exam_id, name) VALUES ('subject-seeded', 'exam-seeded', 'Seeded');
                INSERT INTO study_plans (
                    id, exam_id, subject_id, date, planned_tasks
                ) VALUES (
                    'plan-seeded', 'exam-seeded', 'subject-seeded', '2030-01-01', 'Keep me'
                );
                INSERT INTO agent_sessions (id, title) VALUES ('session-seeded', 'Seeded');
                INSERT INTO agent_runs (id, session_id, goal) VALUES (
                    'run-seeded', 'session-seeded', 'Keep agent rows'
                );
                INSERT INTO agent_steps (
                    id, run_id, step_index, tool_name, tool_version, input_json, output_json,
                    idempotency_key
                ) VALUES (
                    'step-seeded', 'run-seeded', 0, 'plan.get_today', '1', '{"date":"2030-01-01"}',
                    '{"plans":[]}', 'seeded-key'
                );
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
    }

    async fn assert_v4_rows_preserved(pool: &sqlx::SqlitePool) {
        let plan_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM study_plans WHERE id = 'plan-seeded'")
                .fetch_one(pool)
                .await
                .unwrap();
        let agent_rows: (String, String, String) = sqlx::query_as(
            r#"
                SELECT agent_sessions.id, agent_runs.id, agent_steps.id
                FROM agent_sessions
                JOIN agent_runs ON agent_runs.session_id = agent_sessions.id
                JOIN agent_steps ON agent_steps.run_id = agent_runs.id
                WHERE agent_steps.id = 'step-seeded'
            "#,
        )
        .fetch_one(pool)
        .await
        .unwrap();

        assert_eq!(plan_count, 1);
        assert_eq!(
            agent_rows,
            (
                "session-seeded".to_owned(),
                "run-seeded".to_owned(),
                "step-seeded".to_owned(),
            )
        );
    }

    #[test]
    fn migration_v5_preserves_v4_rows_on_upgrade_and_full_initialization() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let migration_list = migrations();
                let migration_v5 = migration_list
                    .iter()
                    .find(|migration| migration.version == 5)
                    .expect("migration v5 must exist");

                let upgrade_pool = SqlitePoolOptions::new()
                    .max_connections(1)
                    .connect("sqlite::memory:")
                    .await
                    .unwrap();
                sqlx::raw_sql(SCHEMA_SQL)
                    .execute(&upgrade_pool)
                    .await
                    .unwrap();
                for migration in migration_list
                    .iter()
                    .filter(|migration| (2..=4).contains(&migration.version))
                {
                    sqlx::raw_sql(migration.sql)
                        .execute(&upgrade_pool)
                        .await
                        .unwrap();
                }
                seed_v4_rows(&upgrade_pool).await;
                sqlx::raw_sql(migration_v5.sql)
                    .execute(&upgrade_pool)
                    .await
                    .unwrap();
                assert_v4_rows_preserved(&upgrade_pool).await;

                let full_migration_pool = SqlitePoolOptions::new()
                    .max_connections(1)
                    .connect("sqlite::memory:")
                    .await
                    .unwrap();
                for migration in &migration_list {
                    sqlx::raw_sql(migration.sql)
                        .execute(&full_migration_pool)
                        .await
                        .unwrap();
                    if migration.version == 4 {
                        seed_v4_rows(&full_migration_pool).await;
                    }
                }
                assert_v4_rows_preserved(&full_migration_pool).await;
            });
    }

    #[test]
    fn agent_schema_executes_on_sqlite() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let pool = SqlitePoolOptions::new()
                    .max_connections(1)
                    .connect("sqlite::memory:")
                    .await
                    .unwrap();

                sqlx::raw_sql(SCHEMA_SQL).execute(&pool).await.unwrap();
                sqlx::raw_sql(AGENT_SCHEMA_SQL)
                    .execute(&pool)
                    .await
                    .unwrap();

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
                    "agent_context_audit",
                ] {
                    assert!(tables.iter().any(|table| table == expected_table));
                }
            });
    }

    #[test]
    fn agent_schema_requires_approval_expiration() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let pool = SqlitePoolOptions::new()
                    .max_connections(1)
                    .connect("sqlite::memory:")
                    .await
                    .unwrap();

                sqlx::raw_sql(SCHEMA_SQL).execute(&pool).await.unwrap();
                sqlx::raw_sql(AGENT_SCHEMA_SQL)
                    .execute(&pool)
                    .await
                    .unwrap();

                let columns = sqlx::query("PRAGMA table_info(agent_approvals)")
                    .fetch_all(&pool)
                    .await
                    .unwrap();
                let expires_at = columns
                    .iter()
                    .find(|column| column.get::<String, _>("name") == "expires_at")
                    .expect("agent_approvals.expires_at must exist");

                assert_eq!(expires_at.get::<i64, _>("notnull"), 1);
            });
    }

    #[test]
    fn agent_schema_rejects_approval_for_a_step_from_another_run() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let options = SqliteConnectOptions::from_str("sqlite::memory:")
                    .unwrap()
                    .foreign_keys(true);
                let pool = SqlitePoolOptions::new()
                    .max_connections(1)
                    .connect_with(options)
                    .await
                    .unwrap();
                let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
                assert_eq!(foreign_keys, 1);

                sqlx::raw_sql(SCHEMA_SQL).execute(&pool).await.unwrap();
                sqlx::raw_sql(AGENT_SCHEMA_SQL)
                    .execute(&pool)
                    .await
                    .unwrap();
                sqlx::query(
                    "INSERT INTO agent_sessions (id, title) VALUES ('session-a', 'A'), ('session-b', 'B')",
                )
                .execute(&pool)
                .await
                .unwrap();
                sqlx::query(
                    "INSERT INTO agent_runs (id, session_id, goal) VALUES ('run-a', 'session-a', 'A'), ('run-b', 'session-b', 'B')",
                )
                .execute(&pool)
                .await
                .unwrap();
                sqlx::query(
                    "INSERT INTO agent_steps (id, run_id, step_index, tool_name, tool_version) VALUES ('step-a', 'run-a', 0, 'tool', '1')",
                )
                .execute(&pool)
                .await
                .unwrap();

                let mismatched_approval = sqlx::query(
                    "INSERT INTO agent_approvals (id, run_id, step_id, risk, expires_at) VALUES ('approval-a', 'run-b', 'step-a', 2, '2030-01-01')",
                )
                .execute(&pool)
                .await;

                assert!(
                    mismatched_approval.is_err(),
                    "an approval run_id must match its step's run_id"
                );
            });
    }

    #[test]
    fn agent_schema_indexes_run_status_for_recovery() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let pool = SqlitePoolOptions::new()
                    .max_connections(1)
                    .connect("sqlite::memory:")
                    .await
                    .unwrap();

                sqlx::raw_sql(SCHEMA_SQL).execute(&pool).await.unwrap();
                sqlx::raw_sql(AGENT_SCHEMA_SQL)
                    .execute(&pool)
                    .await
                    .unwrap();

                let index_count: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_agent_runs_status'",
                )
                .fetch_one(&pool)
                .await
                .unwrap();
                assert_eq!(index_count, 1);
            });
    }
}
