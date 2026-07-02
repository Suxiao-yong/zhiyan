//! 数据库初始化与迁移。
//!
//! 使用 tauri-plugin-sql 管理 SQLite 连接；迁移在插件首次打开数据库时自动执行。
//! 时间基准统一为 datetime('now','localtime')，与本地业务日期、跨天 04:00 规则一致。

use tauri::{AppHandle, Manager};
use tauri_plugin_sql::{Migration, MigrationKind};

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

/// 确保应用数据目录存在（SQLite 数据库文件将落在此目录）。
pub fn init_db(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let app_data = app.path().app_data_dir()?;
    if !app_data.exists() {
        std::fs::create_dir_all(&app_data)?;
    }
    Ok(())
}
