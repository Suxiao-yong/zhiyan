use std::borrow::Cow;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::DateTime;
use serde_json::Value;
use sqlx::{
    migrate::{Migration as SqlxMigration, MigrationType, Migrator},
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    SqlitePool,
};
use tokio::sync::Barrier;
use uuid::Uuid;
use zhiyan_lib::{
    agent::commands::CommandError,
    agent::error::AgentError,
    agent::model::{ToolCallRequest, ToolCallResponse},
    agent::tools::{
        knowledge_point_admin, material,
        plan::{self, PlanGetTodayInput},
    },
    agent::{
        executor::{AgentExecutor, RecordCheckinExecutionRequest},
        repository::AgentRepository,
        runtime::AgentRuntime,
        tools::record::{self, RecordCheckinPlanInput},
    },
    db,
};

const EXAM_ID: &str = "exam-1";
const BUSINESS_DATE: &str = "2026-07-17";
type StoredRecordMetrics = (
    String,
    String,
    Option<String>,
    i64,
    Option<String>,
    i64,
    i64,
    Option<i64>,
    Option<String>,
    Option<i64>,
    Option<String>,
);
type StoredWrongQuestion = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn block_on(future: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future);
}

async fn migrated_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let migrations = db::migrations();
    let versions = migrations
        .iter()
        .map(|migration| migration.version)
        .collect::<Vec<_>>();
    assert!(
        versions.starts_with(&[1, 2, 3, 4, 5]),
        "plan.get_today requires schema migrations 1 through 5"
    );
    for migration in migrations {
        sqlx::raw_sql(migration.sql).execute(&pool).await.unwrap();
    }
    pool
}

struct WalDatabase {
    directory: PathBuf,
    path: PathBuf,
}

impl WalDatabase {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("zhiyan-agent-tools-wal-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("agent-tools.sqlite");
        Self { directory, path }
    }

    async fn migrated_pool(&self) -> SqlitePool {
        let pool = self.open_pool().await;
        for migration in db::migrations() {
            sqlx::raw_sql(migration.sql).execute(&pool).await.unwrap();
        }
        pool
    }

    async fn open_pool(&self) -> SqlitePool {
        let options = SqliteConnectOptions::new()
            .filename(&self.path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
                .fetch_one(&pool)
                .await
                .unwrap(),
            "wal"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("PRAGMA busy_timeout")
                .fetch_one(&pool)
                .await
                .unwrap(),
            5_000
        );
        pool
    }
}

impl Drop for WalDatabase {
    fn drop(&mut self) {
        for _ in 0..20 {
            match std::fs::remove_dir_all(&self.directory) {
                Ok(()) => return,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
    }
}

fn migrator_through(max_version: i64) -> Migrator {
    let migrations = db::migrations()
        .into_iter()
        .filter(|migration| migration.version <= max_version)
        .map(|migration| {
            SqlxMigration::new(
                migration.version,
                Cow::Borrowed(migration.description),
                MigrationType::ReversibleUp,
                Cow::Borrowed(migration.sql),
                false,
            )
        })
        .collect::<Vec<_>>();
    Migrator {
        migrations: Cow::Owned(migrations),
        ignore_missing: false,
        locking: true,
        no_tx: false,
    }
}

async fn seed_versioned_database(pool: &SqlitePool, version: i64, label: &str) {
    sqlx::query("INSERT INTO exams(id,name,exam_date) VALUES(?,?, '2030-01-01')")
        .bind(format!("exam-{label}"))
        .bind(format!("Exam {label}"))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO subjects(id,exam_id,name) VALUES(?,?,?)")
        .bind(format!("subject-{label}"))
        .bind(format!("exam-{label}"))
        .bind(format!("Subject {label}"))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO study_plans(id,exam_id,subject_id,date,planned_tasks) VALUES(?,?,?,'2030-01-01',?)",
    )
    .bind(format!("plan-{label}"))
    .bind(format!("exam-{label}"))
    .bind(format!("subject-{label}"))
    .bind(format!("Keep plan {label}"))
    .execute(pool)
    .await
    .unwrap();
    if version >= 3 {
        sqlx::query(
            "INSERT INTO study_records(id,plan_id,date,subject_id,duration_min,content) VALUES(?,?, '2030-01-01',?,15,?)",
        )
        .bind(format!("record-{label}"))
        .bind(format!("plan-{label}"))
        .bind(format!("subject-{label}"))
        .bind(format!("Keep record {label}"))
        .execute(pool)
        .await
        .unwrap();
    } else {
        sqlx::query(
            "INSERT INTO study_records(id,date,subject_id,duration_min,content) VALUES(?,'2030-01-01',?,15,?)",
        )
        .bind(format!("record-{label}"))
        .bind(format!("subject-{label}"))
        .bind(format!("Keep record {label}"))
        .execute(pool)
        .await
        .unwrap();
    }
    if version >= 4 {
        sqlx::query("INSERT INTO agent_sessions(id,title) VALUES(?,?)")
            .bind(format!("session-{label}"))
            .bind(format!("Session {label}"))
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO agent_runs(id,session_id,goal) VALUES(?,?,?)")
            .bind(format!("run-{label}"))
            .bind(format!("session-{label}"))
            .bind(format!("Keep run {label}"))
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO agent_steps(id,run_id,step_index,tool_name,tool_version,status) VALUES(?,?,0,'plan.get_today','1','completed')",
        )
        .bind(format!("step-{label}"))
        .bind(format!("run-{label}"))
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO agent_events(run_id,step_id,event_type,payload_json) VALUES(?,?,'tool.completed','{}')",
        )
        .bind(format!("run-{label}"))
        .bind(format!("step-{label}"))
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO agent_approvals(id,run_id,step_id,risk,status,expires_at) VALUES(?,?,?,3,'approved','2030-01-01T00:00:00Z')",
        )
        .bind(format!("approval-{label}"))
        .bind(format!("run-{label}"))
        .bind(format!("step-{label}"))
        .execute(pool)
        .await
        .unwrap();
    }
}

async fn assert_v5_database(pool: &SqlitePool, label: &str, expected_agent_rows: i64) {
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM study_plans WHERE id=?")
            .bind(format!("plan-{label}"))
            .fetch_one(pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM study_records WHERE id=?")
            .bind(format!("record-{label}"))
            .fetch_one(pool)
            .await
            .unwrap(),
        1
    );
    for table in [
        "agent_sessions",
        "agent_runs",
        "agent_steps",
        "agent_events",
        "agent_approvals",
    ] {
        let exists: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?")
                .bind(table)
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(exists, 1, "missing {table}");
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_steps WHERE id=?")
            .bind(format!("step-{label}"))
            .fetch_one(pool)
            .await
            .unwrap(),
        expected_agent_rows
    );
    let columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('agent_steps')")
            .fetch_all(pool)
            .await
            .unwrap();
    for column in ["policy_json", "receipt_json", "undo_json", "undone_at"] {
        assert!(columns.iter().any(|actual| actual == column));
    }
    let owners: Vec<(String, String)> = sqlx::query_as(
        "SELECT key,value FROM settings WHERE key LIKE 'agent_tool_owner.%' ORDER BY key",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(
        owners,
        [
            (
                "agent_tool_owner.exam.get_active".to_owned(),
                "rust-owned".to_owned()
            ),
            (
                "agent_tool_owner.plan.generate".to_owned(),
                "rust-owned".to_owned()
            ),
            (
                "agent_tool_owner.plan.get_range".to_owned(),
                "rust-owned".to_owned()
            ),
            (
                "agent_tool_owner.plan.get_today".to_owned(),
                "shadow".to_owned()
            ),
            (
                "agent_tool_owner.record.checkin_plan".to_owned(),
                "typescript".to_owned()
            ),
            (
                "agent_tool_owner.record.create_free".to_owned(),
                "rust-owned".to_owned()
            ),
            (
                "agent_tool_owner.record.get_history".to_owned(),
                "rust-owned".to_owned()
            ),
            (
                "agent_tool_owner.wrong_question.create".to_owned(),
                "rust-owned".to_owned()
            ),
            (
                "agent_tool_owner.wrong_question.mark_mastered".to_owned(),
                "rust-owned".to_owned()
            )
        ]
    );
}

async fn seed_exam_tree(pool: &SqlitePool) {
    sqlx::raw_sql(
        r#"
        INSERT INTO exams (id, name, exam_date)
        VALUES ('exam-1', 'Fixture exam', '2026-12-31');
        INSERT INTO subjects (id, exam_id, name)
        VALUES ('subject-math', 'exam-1', '数学');
        INSERT INTO knowledge_points (id, subject_id, name)
        VALUES ('kp-function', 'subject-math', '函数');
        "#,
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_checkin_fixture(pool: &SqlitePool) -> Value {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/agent-tools/record-checkin-plan.json"
    ))
    .unwrap();
    seed_exam_tree(pool).await;
    let plan = &fixture["plan"];
    sqlx::query(
        r#"
        INSERT INTO study_plans (
            id, exam_id, subject_id, knowledge_point_id, date, planned_tasks,
            planned_duration, status, generated_by, sort_order, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'local', 0,
                  '2026-07-16 09:00:00', '2026-07-16 09:00:00')
        "#,
    )
    .bind(plan["id"].as_str().unwrap())
    .bind(plan["exam_id"].as_str().unwrap())
    .bind(plan["subject_id"].as_str().unwrap())
    .bind(plan["knowledge_point_id"].as_str().unwrap())
    .bind(plan["date"].as_str().unwrap())
    .bind(plan["planned_tasks"].as_str().unwrap())
    .bind(plan["planned_duration"].as_i64().unwrap())
    .bind(plan["status"].as_str().unwrap())
    .execute(pool)
    .await
    .unwrap();

    let old = &fixture["existing_records"][0];
    sqlx::query(
        r#"
        INSERT INTO study_records (
            id, plan_id, date, subject_id, knowledge_point_id, duration_min,
            content, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?,
                  '2026-07-17 09:00:00', '2026-07-17 09:00:00')
        "#,
    )
    .bind(old["id"].as_str().unwrap())
    .bind(plan["id"].as_str().unwrap())
    .bind(plan["date"].as_str().unwrap())
    .bind(plan["subject_id"].as_str().unwrap())
    .bind(plan["knowledge_point_id"].as_str().unwrap())
    .bind(old["duration_min"].as_i64().unwrap())
    .bind(old["content"].as_str().unwrap())
    .execute(pool)
    .await
    .unwrap();
    fixture
}

async fn seed_agent_run(pool: &SqlitePool) {
    sqlx::raw_sql(
        r#"
        INSERT INTO agent_sessions (id, exam_id, title)
        VALUES ('session-checkin', 'exam-1', 'Check-in session');
        INSERT INTO agent_runs (id, session_id, goal, status, trigger_source)
        VALUES ('run-checkin', 'session-checkin', 'Check in', 'running', 'user');
        UPDATE settings SET value='rust-owned'
        WHERE key='agent_tool_owner.record.checkin_plan';
        "#,
    )
    .execute(pool)
    .await
    .unwrap();
}

fn fixture_checkin_input(fixture: &Value) -> RecordCheckinPlanInput {
    serde_json::from_value(fixture["input"].clone()).unwrap()
}

fn execution_request(
    input: RecordCheckinPlanInput,
    key: &str,
    step_index: i64,
) -> RecordCheckinExecutionRequest {
    RecordCheckinExecutionRequest {
        run_id: "run-checkin".to_owned(),
        step_index,
        input,
        business_date: BUSINESS_DATE.to_owned(),
        idempotency_key: Some(key.to_owned()),
    }
}

#[test]
fn record_checkin_plan_matches_the_shared_fixture_and_copies_locked_fields() {
    block_on(async {
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        let input = fixture_checkin_input(&fixture);
        let expected = &fixture["expected"];
        let mut tx = pool.begin().await.unwrap();

        let output = record::checkin_plan(&mut tx, input, BUSINESS_DATE, "record-new")
            .await
            .unwrap();

        assert_eq!(output.record_id, "record-new");
        assert_eq!(output.plan_id, fixture["plan"]["id"]);
        assert_eq!(output.date, expected["date"]);
        assert_eq!(output.subject_id, expected["subject_id"]);
        assert_eq!(
            output.knowledge_point_id.as_deref(),
            expected["knowledge_point_id"].as_str()
        );
        assert_eq!(output.actual_duration, expected["actual_duration"]);
        assert_eq!(
            output.actual_tasks.as_deref(),
            expected["actual_tasks"].as_str()
        );
        assert_eq!(output.status, expected["status"]);
        assert_eq!(output.wrong_question_ids.len(), 1);

        let record_row: StoredRecordMetrics = sqlx::query_as(
            r#"
            SELECT date, subject_id, knowledge_point_id, duration_min, content,
                   questions_count, correct_count, mastery_rating, difficulty_notes,
                   mood, session_time
            FROM study_records WHERE id = 'record-new'
            "#,
        )
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(record_row.0, expected["date"]);
        assert_eq!(record_row.1, expected["subject_id"]);
        assert_eq!(
            record_row.2.as_deref(),
            expected["knowledge_point_id"].as_str()
        );
        assert_eq!(record_row.3, fixture["input"]["duration_min"]);
        assert_eq!(
            record_row.4.as_deref(),
            fixture["input"]["content"].as_str()
        );
        assert_eq!(record_row.5, expected["questions_count"]);
        assert_eq!(record_row.6, expected["correct_count"]);
        assert_eq!(record_row.7, expected["mastery_rating"].as_i64());
        assert_eq!(
            record_row.8.as_deref(),
            fixture["input"]["difficulty_notes"].as_str()
        );
        assert_eq!(record_row.9, fixture["input"]["mood"].as_i64());
        assert_eq!(
            record_row.10.as_deref(),
            fixture["input"]["session_time"].as_str()
        );

        let wrong_row: StoredWrongQuestion = sqlx::query_as(
            r#"
                SELECT subject_id, knowledge_point_id, question_source, question_desc,
                       correct_answer, my_answer, error_type, error_reason
                FROM wrong_questions WHERE record_id = 'record-new'
                "#,
        )
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(wrong_row.0, expected["subject_id"]);
        assert_eq!(
            wrong_row.1.as_deref(),
            expected["knowledge_point_id"].as_str()
        );
        for (actual, field) in [
            (wrong_row.2.as_deref(), "question_source"),
            (wrong_row.3.as_deref(), "question_desc"),
            (wrong_row.4.as_deref(), "correct_answer"),
            (wrong_row.5.as_deref(), "my_answer"),
            (wrong_row.6.as_deref(), "error_type"),
            (wrong_row.7.as_deref(), "error_reason"),
        ] {
            assert_eq!(
                actual,
                fixture["input"]["wrong_questions"][0][field].as_str()
            );
        }
        tx.rollback().await.unwrap();
    });
}

#[test]
fn record_checkin_plan_rejects_invalid_inputs_without_changing_business_rows() {
    block_on(async {
        for case in [
            "missing_plan",
            "skipped_plan",
            "future_plan",
            "zero_duration",
            "negative_duration",
            "negative_questions",
            "negative_correct",
            "correct_above_questions",
            "low_mastery",
            "high_mastery",
            "low_mood",
            "high_mood",
            "invalid_session",
        ] {
            let pool = migrated_pool().await;
            let fixture = seed_checkin_fixture(&pool).await;
            let mut input = fixture_checkin_input(&fixture);
            match case {
                "missing_plan" => input.plan_id = "missing".to_owned(),
                "skipped_plan" => {
                    sqlx::query("UPDATE study_plans SET status = 'skipped' WHERE id = 'plan-1'")
                        .execute(&pool)
                        .await
                        .unwrap();
                }
                "future_plan" => {
                    sqlx::query("UPDATE study_plans SET date = '2026-07-18' WHERE id = 'plan-1'")
                        .execute(&pool)
                        .await
                        .unwrap();
                }
                "zero_duration" => input.duration_min = 0,
                "negative_duration" => input.duration_min = -1,
                "negative_questions" => input.questions_count = -1,
                "negative_correct" => input.correct_count = -1,
                "correct_above_questions" => input.correct_count = input.questions_count + 1,
                "low_mastery" => input.mastery_rating = Some(0),
                "high_mastery" => input.mastery_rating = Some(6),
                "low_mood" => input.mood = Some(0),
                "high_mood" => input.mood = Some(6),
                "invalid_session" => input.session_time = Some("night".to_owned()),
                _ => unreachable!(),
            }
            let before_plan: (Option<i64>, Option<String>, String, String) = sqlx::query_as(
                "SELECT actual_duration, actual_tasks, status, date FROM study_plans WHERE id='plan-1'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            let before_records: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records")
                .fetch_one(&pool)
                .await
                .unwrap();
            let mut tx = pool.begin().await.unwrap();
            let result =
                record::checkin_plan(&mut tx, input, BUSINESS_DATE, "record-rejected").await;
            assert!(result.is_err(), "case {case} must fail");
            tx.rollback().await.unwrap();

            let after_plan: (Option<i64>, Option<String>, String, String) = sqlx::query_as(
                "SELECT actual_duration, actual_tasks, status, date FROM study_plans WHERE id='plan-1'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            let after_records: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records")
                .fetch_one(&pool)
                .await
                .unwrap();
            let wrong_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM wrong_questions")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(after_plan, before_plan, "case {case} changed plan");
            assert_eq!(after_records, before_records, "case {case} inserted record");
            assert_eq!(wrong_count, 0, "case {case} inserted wrong question");
        }
    });
}

#[test]
fn checkin_idempotency_replays_one_atomic_receipt_and_conflicts_on_other_input() {
    block_on(async {
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        seed_agent_run(&pool).await;
        let executor = AgentExecutor::new(pool.clone());
        let request = execution_request(fixture_checkin_input(&fixture), "checkin/device-a/42", 0);

        let first = executor
            .execute_record_checkin_plan(request.clone())
            .await
            .unwrap();
        let second = executor.execute_record_checkin_plan(request).await.unwrap();
        assert_eq!(first.output, second.output);
        assert!(!first.replayed);
        assert!(second.replayed);
        assert_eq!(first.step_id, second.step_id);

        let record_id = first.output.record_id.clone();
        let record_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM study_records WHERE id = ?")
                .bind(&record_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let wrong_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM wrong_questions WHERE record_id = ?")
                .bind(&record_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let step_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_steps WHERE idempotency_key='checkin/device-a/42'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let event_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_events WHERE step_id = ? AND event_type='tool.completed'",
        )
        .bind(&first.step_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            (record_count, wrong_count, step_count, event_count),
            (1, 1, 1, 1)
        );

        let mut conflicting = fixture_checkin_input(&fixture);
        conflicting.duration_min += 1;
        let error = executor
            .execute_record_checkin_plan(execution_request(conflicting, "checkin/device-a/42", 1))
            .await
            .unwrap_err();
        assert_eq!(error.code(), "idempotency_conflict");
        let records_after_conflict: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(records_after_conflict, 2);
    });
}

#[test]
fn checkin_idempotency_requires_a_non_empty_key_before_writing() {
    block_on(async {
        for key in [None, Some(""), Some("   ")] {
            let pool = migrated_pool().await;
            let fixture = seed_checkin_fixture(&pool).await;
            seed_agent_run(&pool).await;
            let executor = AgentExecutor::new(pool.clone());
            let mut request = execution_request(fixture_checkin_input(&fixture), "unused", 0);
            request.idempotency_key = key.map(str::to_owned);

            let error = executor
                .execute_record_checkin_plan(request)
                .await
                .unwrap_err();
            assert_eq!(error.code(), "idempotency_required");
            let steps: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_steps")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(steps, 0);
        }
    });
}

#[test]
fn checkin_idempotency_replay_after_undo_keeps_original_output_without_new_writes() {
    block_on(async {
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        seed_agent_run(&pool).await;
        let executor = AgentExecutor::new(pool.clone());
        let request = execution_request(fixture_checkin_input(&fixture), "checkin/undo/replay", 0);
        let completed = executor
            .execute_record_checkin_plan(request.clone())
            .await
            .unwrap();
        executor.undo(&completed.step_id).await.unwrap();
        let records_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records")
            .fetch_one(&pool)
            .await
            .unwrap();
        let wrongs_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM wrong_questions")
            .fetch_one(&pool)
            .await
            .unwrap();
        let steps_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_steps")
            .fetch_one(&pool)
            .await
            .unwrap();
        let events_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_events")
            .fetch_one(&pool)
            .await
            .unwrap();

        let replay = executor.execute_record_checkin_plan(request).await.unwrap();

        assert_eq!(replay.output, completed.output);
        assert!(replay.replayed);
        assert!(!replay.undo_available);
        let records_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records")
            .fetch_one(&pool)
            .await
            .unwrap();
        let wrongs_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM wrong_questions")
            .fetch_one(&pool)
            .await
            .unwrap();
        let steps_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_steps")
            .fetch_one(&pool)
            .await
            .unwrap();
        let events_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(records_after, records_before);
        assert_eq!(wrongs_after, wrongs_before);
        assert_eq!(steps_after, steps_before);
        assert_eq!(events_after, events_before);
    });
}

#[test]
fn checkin_crash_window_rolls_back_business_aggregate_step_and_event() {
    block_on(async {
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        seed_agent_run(&pool).await;
        sqlx::raw_sql(
            r#"
            CREATE TRIGGER reject_tool_complete BEFORE INSERT ON agent_events
            WHEN NEW.event_type='tool.completed'
            BEGIN SELECT RAISE(ABORT,'crash window'); END;
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        let executor = AgentExecutor::new(pool.clone());

        let error = executor
            .execute_record_checkin_plan(execution_request(
                fixture_checkin_input(&fixture),
                "checkin/device-a/43",
                0,
            ))
            .await
            .unwrap_err();
        assert_eq!(error.code(), "persistence_error");
        assert_eq!(
            error.to_string(),
            "agent persistence failed: tool transaction failed"
        );
        assert!(!error.to_string().contains("crash window"));
        assert!(!error.to_string().contains("CREATE TRIGGER"));
        assert!(!error
            .to_string()
            .contains(fixture["input"]["content"].as_str().unwrap()));

        let record_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records")
            .fetch_one(&pool)
            .await
            .unwrap();
        let plan: (Option<i64>, Option<String>, String) = sqlx::query_as(
            "SELECT actual_duration, actual_tasks, status FROM study_plans WHERE id='plan-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let step_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_steps")
            .fetch_one(&pool)
            .await
            .unwrap();
        let event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(record_count, 1);
        assert_eq!(plan, (None, None, "pending".to_owned()));
        assert_eq!((step_count, event_count), (0, 0));
    });
}

#[test]
fn checkin_undo_is_exactly_once_and_restores_aggregate_from_remaining_records() {
    block_on(async {
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        seed_agent_run(&pool).await;
        let executor = AgentExecutor::new(pool.clone());
        let completed = executor
            .execute_record_checkin_plan(execution_request(
                fixture_checkin_input(&fixture),
                "checkin/device-a/44",
                0,
            ))
            .await
            .unwrap();

        let first = executor.undo(&completed.step_id).await.unwrap();
        let second = executor.undo(&completed.step_id).await.unwrap();
        assert_eq!(first, second);
        assert_eq!(first.output["actual_duration"].as_i64().unwrap(), 20);
        assert_eq!(first.output["actual_tasks"].as_str(), Some("热身"));
        assert_eq!(first.output["status"].as_str().unwrap(), "in_progress");

        let new_record_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM study_records WHERE id = ?")
                .bind(&completed.output.record_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let wrong_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM wrong_questions WHERE record_id = ?")
                .bind(&completed.output.record_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let aggregate: (Option<i64>, Option<String>, String) = sqlx::query_as(
            "SELECT actual_duration, actual_tasks, status FROM study_plans WHERE id='plan-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let undo_events: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_events WHERE step_id=? AND event_type='tool.undone'",
        )
        .bind(&completed.step_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((new_record_count, wrong_count, undo_events), (0, 0, 1));
        assert_eq!(
            aggregate,
            (Some(20), Some("热身".to_owned()), "in_progress".to_owned())
        );
    });
}

#[test]
fn checkin_undo_without_remaining_records_restores_planned_pending_aggregate() {
    block_on(async {
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        sqlx::query("DELETE FROM study_records WHERE id='record-old'")
            .execute(&pool)
            .await
            .unwrap();
        seed_agent_run(&pool).await;
        let executor = AgentExecutor::new(pool.clone());
        let completed = executor
            .execute_record_checkin_plan(execution_request(
                fixture_checkin_input(&fixture),
                "checkin/device-a/45",
                0,
            ))
            .await
            .unwrap();

        let undone = executor.undo(&completed.step_id).await.unwrap();
        assert_eq!(undone.output["actual_duration"].as_i64().unwrap(), 0);
        assert_eq!(
            undone.output["actual_tasks"].as_str(),
            fixture["plan"]["planned_tasks"].as_str()
        );
        assert_eq!(undone.output["status"].as_str().unwrap(), "pending");
    });
}

#[test]
fn checkin_undo_payload_keeps_exact_v1_keys_and_provenance_in_receipt_metadata() {
    block_on(async {
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        seed_agent_run(&pool).await;
        let executor = AgentExecutor::new(pool.clone());
        let mut input = fixture_checkin_input(&fixture);
        input.finish = true;
        let completed = executor
            .execute_record_checkin_plan(execution_request(input, "checkin/payload/exact-v1", 0))
            .await
            .unwrap();
        let (undo_json, receipt_json): (String, String) =
            sqlx::query_as("SELECT undo_json, receipt_json FROM agent_steps WHERE id = ?")
                .bind(&completed.step_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let undo: Value = serde_json::from_str(&undo_json).unwrap();
        let keys = undo.as_object().unwrap().keys().collect::<Vec<_>>();

        assert_eq!(keys.len(), 4);
        for required in ["kind", "record_id", "plan_id", "wrong_question_ids"] {
            assert!(undo.get(required).is_some(), "missing undo key {required}");
        }
        let receipt: Value = serde_json::from_str(&receipt_json).unwrap();
        assert_eq!(receipt["compensation"]["finish"], true);
        assert_eq!(receipt["compensation"]["baseline_completed"], false);
        assert!(receipt.get("undo_result").is_some());
    });
}

#[test]
fn checkin_undo_removes_receipted_orphans_after_external_record_deletion() {
    block_on(async {
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        seed_agent_run(&pool).await;
        let executor = AgentExecutor::new(pool.clone());
        let completed = executor
            .execute_record_checkin_plan(execution_request(
                fixture_checkin_input(&fixture),
                "checkin/orphan/1",
                0,
            ))
            .await
            .unwrap();
        sqlx::query("DELETE FROM study_records WHERE id = ?")
            .bind(&completed.output.record_id)
            .execute(&pool)
            .await
            .unwrap();
        let orphan_record_id: Option<String> =
            sqlx::query_scalar("SELECT record_id FROM wrong_questions WHERE id = ?")
                .bind(&completed.output.wrong_question_ids[0])
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(orphan_record_id, None);

        let undone = executor.undo(&completed.step_id).await.unwrap();

        assert_eq!(
            serde_json::from_value::<Vec<String>>(
                undone.output["removed_wrong_question_ids"].clone()
            )
            .unwrap(),
            completed.output.wrong_question_ids
        );
        assert_eq!(undone.output["actual_duration"].as_i64().unwrap(), 20);
        assert_eq!(undone.output["actual_tasks"].as_str(), Some("热身"));
        assert_eq!(undone.output["status"].as_str().unwrap(), "in_progress");
        let wrong_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM wrong_questions WHERE id = ?")
                .bind(&completed.output.wrong_question_ids[0])
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(wrong_count, 0);
    });
}

#[test]
fn checkin_undo_conflicts_on_missing_or_reassigned_wrong_question_without_damage() {
    block_on(async {
        for tamper in ["missing", "reassigned"] {
            let pool = migrated_pool().await;
            let fixture = seed_checkin_fixture(&pool).await;
            seed_agent_run(&pool).await;
            let executor = AgentExecutor::new(pool.clone());
            let completed = executor
                .execute_record_checkin_plan(execution_request(
                    fixture_checkin_input(&fixture),
                    &format!("checkin/integrity/{tamper}"),
                    0,
                ))
                .await
                .unwrap();
            let wrong_id = &completed.output.wrong_question_ids[0];
            match tamper {
                "missing" => {
                    sqlx::query("DELETE FROM wrong_questions WHERE id = ?")
                        .bind(wrong_id)
                        .execute(&pool)
                        .await
                        .unwrap();
                }
                "reassigned" => {
                    sqlx::query("UPDATE wrong_questions SET record_id='record-old' WHERE id = ?")
                        .bind(wrong_id)
                        .execute(&pool)
                        .await
                        .unwrap();
                }
                _ => unreachable!(),
            }
            let plan_before: (Option<i64>, Option<String>, String) = sqlx::query_as(
                "SELECT actual_duration, actual_tasks, status FROM study_plans WHERE id='plan-1'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            let receipt_before: (Option<String>, Option<String>) =
                sqlx::query_as("SELECT receipt_json, undone_at FROM agent_steps WHERE id = ?")
                    .bind(&completed.step_id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            let events_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_events")
                .fetch_one(&pool)
                .await
                .unwrap();

            let error = executor.undo(&completed.step_id).await.unwrap_err();

            assert_eq!(error.code(), "conflict", "tamper={tamper}");
            let target_count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM study_records WHERE id = ?")
                    .bind(&completed.output.record_id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            let plan_after: (Option<i64>, Option<String>, String) = sqlx::query_as(
                "SELECT actual_duration, actual_tasks, status FROM study_plans WHERE id='plan-1'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            let receipt_after: (Option<String>, Option<String>) =
                sqlx::query_as("SELECT receipt_json, undone_at FROM agent_steps WHERE id = ?")
                    .bind(&completed.step_id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            let events_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_events")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(target_count, 1, "tamper={tamper}");
            assert_eq!(plan_after, plan_before, "tamper={tamper}");
            assert_eq!(receipt_after, receipt_before, "tamper={tamper}");
            assert_eq!(events_after, events_before, "tamper={tamper}");
            if tamper == "reassigned" {
                let record_id: Option<String> =
                    sqlx::query_scalar("SELECT record_id FROM wrong_questions WHERE id = ?")
                        .bind(wrong_id)
                        .fetch_one(&pool)
                        .await
                        .unwrap();
                assert_eq!(record_id.as_deref(), Some("record-old"));
            }
        }
    });
}

#[test]
fn checkin_undo_preserves_other_finish_receipts_and_recalculates_after_they_are_undone() {
    block_on(async {
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        seed_agent_run(&pool).await;
        let executor = AgentExecutor::new(pool.clone());
        let mut input_a = fixture_checkin_input(&fixture);
        input_a.content = Some("A task".to_owned());
        input_a.finish = false;
        let a = executor
            .execute_record_checkin_plan(execution_request(input_a, "checkin/status/a", 0))
            .await
            .unwrap();
        let mut input_b = fixture_checkin_input(&fixture);
        input_b.content = Some("B task".to_owned());
        input_b.finish = true;
        let b = executor
            .execute_record_checkin_plan(execution_request(input_b, "checkin/status/b", 1))
            .await
            .unwrap();
        assert_eq!(b.output.status, "completed");

        let undo_a = executor.undo(&a.step_id).await.unwrap();

        assert_eq!(undo_a.output["actual_duration"].as_i64().unwrap(), 50);
        assert_eq!(undo_a.output["actual_tasks"].as_str(), Some("B task"));
        assert_eq!(undo_a.output["status"].as_str().unwrap(), "completed");

        let undo_b = executor.undo(&b.step_id).await.unwrap();
        assert_eq!(undo_b.output["actual_duration"].as_i64().unwrap(), 20);
        assert_eq!(undo_b.output["actual_tasks"].as_str(), Some("热身"));
        assert_eq!(undo_b.output["status"].as_str().unwrap(), "in_progress");
    });
}

#[test]
fn checkin_undo_does_not_promote_a_prior_agent_finish_to_legacy_completed_baseline() {
    block_on(async {
        for keep_old_record in [true, false] {
            let pool = migrated_pool().await;
            let fixture = seed_checkin_fixture(&pool).await;
            if !keep_old_record {
                sqlx::query("DELETE FROM study_records WHERE id='record-old'")
                    .execute(&pool)
                    .await
                    .unwrap();
            }
            seed_agent_run(&pool).await;
            let executor = AgentExecutor::new(pool.clone());
            let mut input_a = fixture_checkin_input(&fixture);
            input_a.content = Some("finish A".to_owned());
            input_a.finish = true;
            let a = executor
                .execute_record_checkin_plan(execution_request(
                    input_a,
                    &format!("checkin/baseline/a/{keep_old_record}"),
                    0,
                ))
                .await
                .unwrap();
            let mut input_b = fixture_checkin_input(&fixture);
            input_b.content = Some("non-finish B".to_owned());
            input_b.finish = false;
            let b = executor
                .execute_record_checkin_plan(execution_request(
                    input_b,
                    &format!("checkin/baseline/b/{keep_old_record}"),
                    1,
                ))
                .await
                .unwrap();
            let b_receipt: String =
                sqlx::query_scalar("SELECT receipt_json FROM agent_steps WHERE id = ?")
                    .bind(&b.step_id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            let b_receipt: Value = serde_json::from_str(&b_receipt).unwrap();
            assert_eq!(
                b_receipt["compensation"]["baseline_completed"], false,
                "keep_old_record={keep_old_record}"
            );

            let undo_a = executor.undo(&a.step_id).await.unwrap();
            assert_eq!(undo_a.output["status"].as_str().unwrap(), "in_progress");

            let undo_b = executor.undo(&b.step_id).await.unwrap();
            assert_eq!(
                undo_b.output["status"].as_str().unwrap(),
                if keep_old_record {
                    "in_progress"
                } else {
                    "pending"
                }
            );
        }
    });
}

#[test]
fn checkin_undo_ignores_stale_finish_receipts_whose_record_no_longer_exists() {
    block_on(async {
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        seed_agent_run(&pool).await;
        let executor = AgentExecutor::new(pool.clone());
        let mut input_a = fixture_checkin_input(&fixture);
        input_a.content = Some("stale finish A".to_owned());
        input_a.finish = true;
        let a = executor
            .execute_record_checkin_plan(execution_request(input_a, "checkin/stale-finish/a", 0))
            .await
            .unwrap();
        let mut input_b = fixture_checkin_input(&fixture);
        input_b.content = Some("non-finish B".to_owned());
        input_b.finish = false;
        let b = executor
            .execute_record_checkin_plan(execution_request(input_b, "checkin/stale-finish/b", 1))
            .await
            .unwrap();
        sqlx::query("DELETE FROM study_records WHERE id = ?")
            .bind(&a.output.record_id)
            .execute(&pool)
            .await
            .unwrap();

        let undone_b = executor.undo(&b.step_id).await.unwrap();

        assert_eq!(undone_b.output["actual_duration"].as_i64().unwrap(), 20);
        assert_eq!(undone_b.output["actual_tasks"].as_str(), Some("热身"));
        assert_eq!(undone_b.output["status"].as_str().unwrap(), "in_progress");
    });
}

#[test]
fn checkin_undo_treats_missing_receipt_metadata_as_false_and_still_replays_result() {
    block_on(async {
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        seed_agent_run(&pool).await;
        let executor = AgentExecutor::new(pool.clone());
        let completed = executor
            .execute_record_checkin_plan(execution_request(
                fixture_checkin_input(&fixture),
                "checkin/legacy-receipt",
                0,
            ))
            .await
            .unwrap();
        let undo_json: String =
            sqlx::query_scalar("SELECT undo_json FROM agent_steps WHERE id = ?")
                .bind(&completed.step_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let mut legacy_undo: Value = serde_json::from_str(&undo_json).unwrap();
        legacy_undo["finish"] = Value::Bool(true);
        legacy_undo["plan_was_completed"] = Value::Bool(true);
        sqlx::query(
            "UPDATE agent_steps SET undo_json=?, receipt_json='{\"undo_result\":null}' WHERE id=?",
        )
        .bind(legacy_undo.to_string())
        .bind(&completed.step_id)
        .execute(&pool)
        .await
        .unwrap();

        let first = executor.undo(&completed.step_id).await.unwrap();
        let replay = executor.undo(&completed.step_id).await.unwrap();

        assert_eq!(first, replay);
        assert_eq!(first.output["status"].as_str().unwrap(), "in_progress");
        let receipt_json: String =
            sqlx::query_scalar("SELECT receipt_json FROM agent_steps WHERE id = ?")
                .bind(&completed.step_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let receipt: Value = serde_json::from_str(&receipt_json).unwrap();
        assert!(receipt.get("compensation").is_none());
        assert!(receipt["undo_result"].is_object());
    });
}

#[test]
fn checkin_undo_preserves_a_plan_that_was_already_completed_before_execution() {
    block_on(async {
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        sqlx::query("UPDATE study_plans SET status='completed' WHERE id='plan-1'")
            .execute(&pool)
            .await
            .unwrap();
        seed_agent_run(&pool).await;
        let executor = AgentExecutor::new(pool.clone());
        let mut input = fixture_checkin_input(&fixture);
        input.finish = true;
        let completed = executor
            .execute_record_checkin_plan(execution_request(input, "checkin/status/precompleted", 0))
            .await
            .unwrap();

        let undone = executor.undo(&completed.step_id).await.unwrap();

        assert_eq!(undone.output["actual_duration"].as_i64().unwrap(), 20);
        assert_eq!(undone.output["actual_tasks"].as_str(), Some("热身"));
        assert_eq!(undone.output["status"].as_str().unwrap(), "completed");
    });
}

#[test]
fn checkin_undo_propagates_legacy_completed_baseline_through_active_finish_receipts() {
    block_on(async {
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        sqlx::query("UPDATE study_plans SET status='completed' WHERE id='plan-1'")
            .execute(&pool)
            .await
            .unwrap();
        seed_agent_run(&pool).await;
        let executor = AgentExecutor::new(pool.clone());
        let mut input_a = fixture_checkin_input(&fixture);
        input_a.content = Some("legacy finish A".to_owned());
        input_a.finish = true;
        let a = executor
            .execute_record_checkin_plan(execution_request(input_a, "checkin/legacy-chain/a", 0))
            .await
            .unwrap();
        let mut input_b = fixture_checkin_input(&fixture);
        input_b.content = Some("legacy non-finish B".to_owned());
        input_b.finish = false;
        let b = executor
            .execute_record_checkin_plan(execution_request(input_b, "checkin/legacy-chain/b", 1))
            .await
            .unwrap();

        let undo_a = executor.undo(&a.step_id).await.unwrap();
        assert_eq!(undo_a.output["status"].as_str().unwrap(), "completed");
        let undo_b = executor.undo(&b.step_id).await.unwrap();

        assert_eq!(undo_b.output["actual_duration"].as_i64().unwrap(), 20);
        assert_eq!(undo_b.output["actual_tasks"].as_str(), Some("热身"));
        assert_eq!(undo_b.output["status"].as_str().unwrap(), "completed");
    });
}

#[test]
fn checkin_undo_audit_failure_rolls_back_every_compensation_write() {
    block_on(async {
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        seed_agent_run(&pool).await;
        let executor = AgentExecutor::new(pool.clone());
        let completed = executor
            .execute_record_checkin_plan(execution_request(
                fixture_checkin_input(&fixture),
                "checkin/undo/audit-failure",
                0,
            ))
            .await
            .unwrap();
        sqlx::raw_sql(
            r#"
            CREATE TRIGGER reject_tool_undone BEFORE INSERT ON agent_events
            WHEN NEW.event_type='tool.undone'
            BEGIN SELECT RAISE(ABORT,'undo crash window'); END;
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        let plan_before: (Option<i64>, Option<String>, String) = sqlx::query_as(
            "SELECT actual_duration, actual_tasks, status FROM study_plans WHERE id='plan-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let step_before: (Option<String>, Option<String>) =
            sqlx::query_as("SELECT receipt_json, undone_at FROM agent_steps WHERE id = ?")
                .bind(&completed.step_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let events_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_events")
            .fetch_one(&pool)
            .await
            .unwrap();

        let error = executor.undo(&completed.step_id).await.unwrap_err();

        assert_eq!(error.code(), "persistence_error");
        let record_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM study_records WHERE id = ?")
                .bind(&completed.output.record_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let wrong_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM wrong_questions WHERE record_id = ?")
                .bind(&completed.output.record_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let plan_after: (Option<i64>, Option<String>, String) = sqlx::query_as(
            "SELECT actual_duration, actual_tasks, status FROM study_plans WHERE id='plan-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let step_after: (Option<String>, Option<String>) =
            sqlx::query_as("SELECT receipt_json, undone_at FROM agent_steps WHERE id = ?")
                .bind(&completed.step_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let events_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!((record_count, wrong_count), (1, 1));
        assert_eq!(plan_after, plan_before);
        assert_eq!(step_after, step_before);
        assert_eq!(events_after, events_before);
    });
}

#[test]
fn plan_get_today_uses_the_0400_business_day_boundary() {
    let before_boundary = DateTime::parse_from_rfc3339("2026-07-18T03:59:59+08:00").unwrap();
    let at_boundary = DateTime::parse_from_rfc3339("2026-07-18T04:00:00+08:00").unwrap();

    assert_eq!(plan::business_date_at(before_boundary), "2026-07-17");
    assert_eq!(plan::business_date_at(at_boundary), "2026-07-18");
}

#[test]
fn plan_get_today_matches_the_shared_fixture_without_updating_study_plans() {
    block_on(async {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/agent-tools/plan-get-today.json"
        ))
        .unwrap();
        let now_local = DateTime::parse_from_rfc3339(fixture["now_local"].as_str().unwrap())
            .expect("fixture now_local must be RFC 3339");
        let business_date = fixture["business_date"].as_str().unwrap();
        let exam_id = fixture["input"]["exam_id"].as_str().unwrap();
        let expected_plan = &fixture["expected_output"]["plans"][0];

        assert_eq!(plan::business_date_at(now_local), business_date);
        assert_eq!(fixture["expected_output"]["business_date"], business_date);

        let pool = migrated_pool().await;
        sqlx::query("INSERT INTO exams (id, name, exam_date) VALUES (?, ?, ?)")
            .bind(exam_id)
            .bind("Fixture exam")
            .bind("2026-12-31")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO subjects (id, exam_id, name) VALUES (?, ?, ?)")
            .bind(expected_plan["subject_id"].as_str().unwrap())
            .bind(exam_id)
            .bind(expected_plan["subject_name"].as_str().unwrap())
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO knowledge_points (id, subject_id, name) VALUES (?, ?, ?)")
            .bind(expected_plan["knowledge_point_id"].as_str().unwrap())
            .bind(expected_plan["subject_id"].as_str().unwrap())
            .bind(expected_plan["knowledge_point_name"].as_str().unwrap())
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            r#"
            INSERT INTO study_plans (
                id, exam_id, subject_id, knowledge_point_id, date,
                planned_tasks, planned_duration, actual_duration, actual_tasks,
                status, generated_by, ai_suggestion, user_modified, sort_order,
                created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, NULL, NULL, 'pending', ?, NULL, ?, ?, ?, ?)
            "#,
        )
        .bind(expected_plan["id"].as_str().unwrap())
        .bind(exam_id)
        .bind(expected_plan["subject_id"].as_str().unwrap())
        .bind(expected_plan["knowledge_point_id"].as_str().unwrap())
        .bind(business_date)
        .bind(expected_plan["planned_tasks"].as_str().unwrap())
        .bind(expected_plan["planned_duration"].as_i64().unwrap())
        .bind(expected_plan["generated_by"].as_str().unwrap())
        .bind(expected_plan["user_modified"].as_i64().unwrap())
        .bind(expected_plan["sort_order"].as_i64().unwrap())
        .bind(expected_plan["created_at"].as_str().unwrap())
        .bind(expected_plan["updated_at"].as_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO study_records (
                id, plan_id, date, subject_id, knowledge_point_id,
                duration_min, content, created_at, updated_at
            ) VALUES ('record-1', ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(expected_plan["id"].as_str().unwrap())
        .bind(business_date)
        .bind(expected_plan["subject_id"].as_str().unwrap())
        .bind(expected_plan["knowledge_point_id"].as_str().unwrap())
        .bind(expected_plan["actual_duration"].as_i64().unwrap())
        .bind(expected_plan["actual_tasks"].as_str().unwrap())
        .bind("2026-07-17 20:00:00")
        .bind("2026-07-17 20:00:00")
        .execute(&pool)
        .await
        .unwrap();

        let before: (Option<i64>, Option<String>, String) = sqlx::query_as(
            "SELECT actual_duration, actual_tasks, status FROM study_plans WHERE id = ?",
        )
        .bind(expected_plan["id"].as_str().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();

        sqlx::query("PRAGMA query_only = ON")
            .execute(&pool)
            .await
            .unwrap();
        let query_only: i64 = sqlx::query_scalar("PRAGMA query_only")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(query_only, 1, "fixture connection must reject writes");

        let output = plan::get_today(
            &pool,
            PlanGetTodayInput {
                exam_id: exam_id.to_owned(),
            },
            business_date,
        )
        .await
        .unwrap();

        assert_eq!(
            serde_json::to_value(output).unwrap(),
            fixture["expected_output"]
        );

        let after: (Option<i64>, Option<String>, String) = sqlx::query_as(
            "SELECT actual_duration, actual_tasks, status FROM study_plans WHERE id = ?",
        )
        .bind(expected_plan["id"].as_str().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(after, before, "plan.get_today must remain read-only");
    });
}

#[test]
fn plan_get_today_sums_all_records_and_uses_the_latest_non_empty_content() {
    block_on(async {
        let pool = migrated_pool().await;
        seed_exam_tree(&pool).await;
        sqlx::raw_sql(
            r#"
            INSERT INTO study_plans (
                id, exam_id, subject_id, knowledge_point_id, date, planned_tasks,
                actual_duration, actual_tasks, status, generated_by, sort_order,
                created_at, updated_at
            ) VALUES (
                'plan-aggregate', 'exam-1', 'subject-math', 'kp-function', '2026-07-17',
                'Fallback task', 999, 'Stale task', 'pending', 'local', 0,
                '2026-07-16 09:00:00', '2026-07-16 09:00:00'
            );
            INSERT INTO study_records
                (id, plan_id, date, subject_id, duration_min, content, created_at)
            VALUES
                ('record-older', 'plan-aggregate', '2026-07-17', 'subject-math', 20,
                 'Older content', '2026-07-17 09:00:00'),
                ('record-newer', 'plan-aggregate', '2026-07-17', 'subject-math', 30,
                 'Newest valid content', '2026-07-17 10:00:00'),
                ('record-null', 'plan-aggregate', '2026-07-17', 'subject-math', 5,
                 NULL, '2026-07-17 11:00:00'),
                ('record-empty', 'plan-aggregate', '2026-07-17', 'subject-math', 7,
                 '', '2026-07-17 12:00:00');
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let output = plan::get_today(
            &pool,
            PlanGetTodayInput {
                exam_id: EXAM_ID.to_owned(),
            },
            BUSINESS_DATE,
        )
        .await
        .unwrap();
        let actual = &output.plans[0];

        assert_eq!(actual.actual_duration, Some(62));
        assert_eq!(actual.record_count, 4);
        assert_eq!(actual.actual_tasks.as_deref(), Some("Newest valid content"));
        assert_eq!(actual.status, "in_progress");
    });
}

#[test]
fn plan_get_today_falls_back_and_preserves_unrecorded_or_terminal_statuses() {
    block_on(async {
        let pool = migrated_pool().await;
        seed_exam_tree(&pool).await;
        sqlx::raw_sql(
            r#"
            INSERT INTO study_plans
                (id, exam_id, subject_id, date, planned_tasks, actual_duration, actual_tasks,
                 status, generated_by, sort_order, created_at, updated_at)
            VALUES
                ('plan-fallback', 'exam-1', 'subject-math', '2026-07-17', 'Planned fallback',
                 NULL, NULL, 'pending', 'local', 0, '2026-07-16 08:00:00', '2026-07-16 08:00:00'),
                ('plan-unrecorded', 'exam-1', 'subject-math', '2026-07-17', 'Unrecorded',
                 12, 'Stored actual task', 'pending', 'local', 1, '2026-07-16 08:00:00', '2026-07-16 08:00:00'),
                ('plan-completed', 'exam-1', 'subject-math', '2026-07-17', 'Completed',
                 NULL, NULL, 'completed', 'local', 2, '2026-07-16 08:00:00', '2026-07-16 08:00:00'),
                ('plan-skipped', 'exam-1', 'subject-math', '2026-07-17', 'Skipped',
                 NULL, NULL, 'skipped', 'local', 3, '2026-07-16 08:00:00', '2026-07-16 08:00:00');

            INSERT INTO study_records
                (id, plan_id, date, subject_id, duration_min, content, created_at)
            VALUES
                ('fallback-null', 'plan-fallback', '2026-07-17', 'subject-math', 10,
                 NULL, '2026-07-17 09:00:00'),
                ('fallback-empty', 'plan-fallback', '2026-07-17', 'subject-math', 15,
                 '', '2026-07-17 10:00:00'),
                ('completed-record', 'plan-completed', '2026-07-17', 'subject-math', 20,
                 'Completed record', '2026-07-17 09:00:00'),
                ('skipped-record', 'plan-skipped', '2026-07-17', 'subject-math', 5,
                 'Skipped record', '2026-07-17 09:00:00');
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let output = plan::get_today(
            &pool,
            PlanGetTodayInput {
                exam_id: EXAM_ID.to_owned(),
            },
            BUSINESS_DATE,
        )
        .await
        .unwrap();
        let find = |id: &str| output.plans.iter().find(|plan| plan.id == id).unwrap();

        let fallback = find("plan-fallback");
        assert_eq!(fallback.actual_duration, Some(25));
        assert_eq!(fallback.actual_tasks.as_deref(), Some("Planned fallback"));
        assert_eq!(fallback.status, "in_progress");

        let unrecorded = find("plan-unrecorded");
        assert_eq!(unrecorded.record_count, 0);
        assert_eq!(unrecorded.actual_duration, Some(12));
        assert_eq!(
            unrecorded.actual_tasks.as_deref(),
            Some("Stored actual task")
        );
        assert_eq!(unrecorded.status, "pending");

        assert_eq!(find("plan-completed").status, "completed");
        assert_eq!(find("plan-skipped").status, "skipped");
    });
}

#[test]
fn plan_get_today_orders_by_date_sort_order_and_created_at() {
    block_on(async {
        let pool = migrated_pool().await;
        seed_exam_tree(&pool).await;
        sqlx::raw_sql(
            r#"
            INSERT INTO study_plans
                (id, exam_id, subject_id, date, status, generated_by, sort_order, created_at, updated_at)
            VALUES
                ('plan-created-late', 'exam-1', 'subject-math', '2026-07-17', 'pending', 'local', 0,
                 '2026-07-16 10:00:00', '2026-07-16 10:00:00'),
                ('plan-sort-second', 'exam-1', 'subject-math', '2026-07-17', 'pending', 'local', 1,
                 '2026-07-16 08:00:00', '2026-07-16 08:00:00'),
                ('plan-created-early', 'exam-1', 'subject-math', '2026-07-17', 'pending', 'local', 0,
                 '2026-07-16 09:00:00', '2026-07-16 09:00:00'),
                ('plan-other-date', 'exam-1', 'subject-math', '2026-07-18', 'pending', 'local', -1,
                 '2026-07-15 08:00:00', '2026-07-15 08:00:00');
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let output = plan::get_today(
            &pool,
            PlanGetTodayInput {
                exam_id: EXAM_ID.to_owned(),
            },
            BUSINESS_DATE,
        )
        .await
        .unwrap();

        assert!(output.plans.iter().all(|plan| plan.date == BUSINESS_DATE));
        assert_eq!(
            output
                .plans
                .iter()
                .map(|plan| plan.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "plan-created-early",
                "plan-created-late",
                "plan-sort-second"
            ]
        );
    });
}

#[test]
fn plan_get_today_content_ties_keep_created_at_as_the_only_order_key() {
    let source = include_str!("../src/agent/tools/plan.rs")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(source.contains("ORDER BY latest.created_at DESC LIMIT 1"));
    assert!(!source.contains("ORDER BY latest.created_at DESC, latest.id"));
    assert!(source.contains("ORDER BY p.date, p.sort_order, p.created_at"));
    assert!(!source.contains("ORDER BY p.date, p.sort_order, p.created_at, p.id"));

    block_on(async {
        let pool = migrated_pool().await;
        seed_exam_tree(&pool).await;
        sqlx::raw_sql(
            r#"
            INSERT INTO study_plans
                (id, exam_id, subject_id, date, status, generated_by, created_at, updated_at)
            VALUES
                ('plan-tie', 'exam-1', 'subject-math', '2026-07-17', 'pending', 'local',
                 '2026-07-16 09:00:00', '2026-07-16 09:00:00');
            INSERT INTO study_records
                (id, plan_id, date, subject_id, duration_min, content, created_at)
            VALUES
                ('record-z', 'plan-tie', '2026-07-17', 'subject-math', 10,
                 'Tie Z', '2026-07-17 10:00:00'),
                ('record-a', 'plan-tie', '2026-07-17', 'subject-math', 20,
                 'Tie A', '2026-07-17 10:00:00');
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let output = plan::get_today(
            &pool,
            PlanGetTodayInput {
                exam_id: EXAM_ID.to_owned(),
            },
            BUSINESS_DATE,
        )
        .await
        .unwrap();
        let actual_tasks = output.plans[0].actual_tasks.as_deref().unwrap();

        assert!(matches!(actual_tasks, "Tie Z" | "Tie A"));
        assert_eq!(output.plans[0].actual_duration, Some(30));
    });
}

#[test]
fn executor_dto_contract_has_exact_tagged_wire_shape() {
    let request = ToolCallRequest {
        run_id: "run-1".to_owned(),
        step_index: 2,
        tool_name: "plan.get_today".to_owned(),
        tool_version: "1".to_owned(),
        input: serde_json::json!({"exam_id":"exam-1"}),
        idempotency_key: None,
        approval_id: None,
    };
    let request_json = serde_json::to_value(request).unwrap();
    assert_eq!(
        request_json
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        [
            "approval_id",
            "idempotency_key",
            "input",
            "run_id",
            "step_index",
            "tool_name",
            "tool_version",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    );

    let response = ToolCallResponse::Completed {
        step_id: "step-1".to_owned(),
        output: serde_json::json!({"ok":true}),
        replayed: false,
        undo_available: true,
    };
    assert_eq!(
        serde_json::to_value(response).unwrap(),
        serde_json::json!({
            "state":"completed",
            "step_id":"step-1",
            "output":{"ok":true},
            "replayed":false,
            "undo_available":true
        })
    );
}

#[test]
fn executor_rejects_request_policy_fields_as_tool_input() {
    let registry = zhiyan_lib::agent::tools::ToolRegistry::built_in();
    for forbidden in ["risk", "permissions", "policy", "data_permissions"] {
        let mut input = serde_json::json!({"exam_id":"exam-1"});
        input[forbidden] = serde_json::json!("attacker-controlled");
        assert_eq!(
            registry
                .validate_input("plan.get_today", "1", &input)
                .unwrap_err()
                .code(),
            "tool_schema_invalid",
            "descriptor field {forbidden} must not be accepted in input"
        );
    }
}

#[test]
fn executor_lists_static_rust_ownership_and_ignores_legacy_settings() {
    block_on(async {
        let pool = migrated_pool().await;
        let executor = AgentExecutor::new(pool.clone());

        // Every tool is Rust-owned by the static registry; the legacy
        // `agent_tool_owner.*` settings are inert and never rewrite it.
        let listed = executor.list_tools().await.unwrap();
        let ownership = |name: &str| {
            listed
                .iter()
                .find(|tool| tool.descriptor.name == name)
                .unwrap()
                .ownership
        };
        for name in [
            "plan.get_today",
            "record.checkin_plan",
            "exam.get_active",
            "plan.get_range",
            "record.get_history",
        ] {
            assert_eq!(
                ownership(name),
                zhiyan_lib::agent::tools::ToolOwnership::RustOwned
            );
        }

        // Legacy ownership settings are inert: deleting or rewriting them must
        // not change the static ownership, and list_tools stays healthy.
        sqlx::query("DELETE FROM settings WHERE key = ?")
            .bind("agent_tool_owner.plan.get_today")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE settings SET value='future-owner' WHERE key = ?")
            .bind("agent_tool_owner.record.checkin_plan")
            .execute(&pool)
            .await
            .unwrap();
        let listed = executor.list_tools().await.unwrap();
        assert!(listed
            .iter()
            .all(|tool| { tool.ownership == zhiyan_lib::agent::tools::ToolOwnership::RustOwned }));

        // The registry never reads the settings table for ownership, so a
        // dropped settings table cannot break listing.
        sqlx::query("DROP TABLE settings")
            .execute(&pool)
            .await
            .unwrap();
        let listed = executor.list_tools().await.unwrap();
        assert!(listed
            .iter()
            .all(|tool| { tool.ownership == zhiyan_lib::agent::tools::ToolOwnership::RustOwned }));
    });
}

#[test]
fn executor_runs_rust_read_with_trusted_local_business_date() {
    block_on(async {
        let pool = migrated_pool().await;
        seed_exam_tree(&pool).await;
        seed_agent_run(&pool).await;
        let business_date = plan::business_date_at(chrono::Local::now().fixed_offset());
        sqlx::query(
            "INSERT INTO study_plans (id, exam_id, subject_id, date) VALUES ('plan-today', ?, 'subject-math', ?)",
        )
        .bind(EXAM_ID)
        .bind(&business_date)
        .execute(&pool)
        .await
        .unwrap();

        let response = AgentExecutor::new(pool.clone())
            .execute(ToolCallRequest {
                run_id: "run-checkin".to_owned(),
                step_index: 0,
                tool_name: "plan.get_today".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({"exam_id":EXAM_ID}),
                idempotency_key: None,
                approval_id: None,
            })
            .await
            .unwrap();

        let ToolCallResponse::Completed {
            output,
            replayed,
            undo_available,
            ..
        } = response
        else {
            panic!("R0 must complete")
        };
        assert_eq!(output["business_date"], business_date);
        assert_eq!(output["plans"][0]["id"], "plan-today");
        assert!(!replayed);
        assert!(!undo_available);
        let (risk, policy_json, receipt_json): (i64, String, String) = sqlx::query_as(
            "SELECT risk, policy_json, receipt_json FROM agent_steps WHERE run_id='run-checkin' AND step_index=0",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(risk, 0);
        let policy: Value = serde_json::from_str(&policy_json).unwrap();
        assert_eq!(policy["delivery"], "rust");
        assert_eq!(policy["decision"], "execute");
        assert_eq!(
            serde_json::from_str::<Value>(&receipt_json).unwrap()["delivery"],
            "rust"
        );
        let events: Vec<String> = sqlx::query_scalar(
            "SELECT event_type FROM agent_events WHERE run_id='run-checkin' ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(events, ["tool.requested", "tool.completed"]);
    });
}

#[test]
fn executor_legacy_ownership_values_are_inert_and_never_gate_writes() {
    block_on(async {
        // Legacy ownership settings no longer gate execution: every value
        // (typescript/shadow/invalid) is inert, the write is gated by the R3
        // approval flow exactly the same way, and the setting is never
        // rewritten by the runtime.
        for owner in ["typescript", "shadow", "invalid"] {
            let pool = migrated_pool().await;
            let fixture = seed_checkin_fixture(&pool).await;
            seed_agent_run(&pool).await;
            sqlx::query(
                "UPDATE settings SET value=? WHERE key='agent_tool_owner.record.checkin_plan'",
            )
            .bind(owner)
            .execute(&pool)
            .await
            .unwrap();
            let executor = AgentExecutor::new(pool.clone());
            let request = ToolCallRequest {
                run_id: "run-checkin".to_owned(),
                step_index: 0,
                tool_name: "record.checkin_plan".to_owned(),
                tool_version: "1".to_owned(),
                input: fixture["input"].clone(),
                idempotency_key: Some(format!("owner/{owner}")),
                approval_id: None,
            };

            // The write waits for approval — never an ownership error, and no
            // business row is written before the user confirms.
            let ToolCallResponse::WaitingApproval { approval_id, .. } =
                executor.execute(request.clone()).await.unwrap()
            else {
                panic!("check-in must request approval, owner={owner}")
            };
            let records: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(records, 1, "owner={owner}: no write before approval");

            // Approval dispatches the write regardless of the legacy value.
            let approved = executor.resolve_approval(&approval_id, true).await.unwrap();
            assert_eq!(approved.status, "approved");
            let records: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(records, 2, "owner={owner}: approved write lands");

            // The legacy setting survives untouched (inert, never rewritten).
            let stored: String = sqlx::query_scalar(
                "SELECT value FROM settings WHERE key='agent_tool_owner.record.checkin_plan'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(stored, owner, "owner={owner}: setting stays inert");
        }
    });
}

#[test]
fn executor_r3_write_dispatches_once_after_approval_and_undo_restores() {
    block_on(async {
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        seed_agent_run(&pool).await;
        let request = ToolCallRequest {
            run_id: "run-checkin".to_owned(),
            step_index: 0,
            tool_name: "record.checkin_plan".to_owned(),
            tool_version: "1".to_owned(),
            input: fixture["input"].clone(),
            idempotency_key: Some("generic/r3/once".to_owned()),
            approval_id: None,
        };
        let executor = AgentExecutor::new(pool.clone());

        // The R3 write waits for approval and performs no business write yet.
        let ToolCallResponse::WaitingApproval { approval_id, .. } =
            executor.execute(request).await.unwrap()
        else {
            panic!("check-in must request approval")
        };
        let records: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(records, 1);

        // Approving dispatches exactly once with the undo receipt.
        let approved = executor.resolve_approval(&approval_id, true).await.unwrap();
        assert_eq!(approved.status, "approved");
        let step_id = approved.step_id;
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM study_records")
                .fetch_one(&pool)
                .await
                .unwrap(),
            2
        );

        // Re-confirming the consumed approval is rejected; no second write.
        let error = executor
            .resolve_approval(&approval_id, true)
            .await
            .unwrap_err();
        assert_eq!(error.code(), "approval_invalid");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM study_records")
                .fetch_one(&pool)
                .await
                .unwrap(),
            2
        );

        // Undo restores the pre-write state.
        executor.undo(&step_id).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM study_records")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
    });
}

#[test]
fn concurrent_checkin_same_key_on_wal_completes_once_and_replays_once() {
    block_on(async {
        let database = WalDatabase::new();
        let pool = database.migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        seed_agent_run(&pool).await;
        let request =
            execution_request(fixture_checkin_input(&fixture), "generic/r1/wal-barrier", 0);
        let executor = AgentExecutor::new(pool.clone());
        let barrier = Arc::new(Barrier::new(2));
        let left = {
            let barrier = barrier.clone();
            let executor = executor.clone();
            let request = request.clone();
            tokio::spawn(async move {
                barrier.wait().await;
                executor.execute_record_checkin_plan(request).await
            })
        };
        let right = {
            let barrier = barrier.clone();
            let executor = executor.clone();
            let request = request.clone();
            tokio::spawn(async move {
                barrier.wait().await;
                executor.execute_record_checkin_plan(request).await
            })
        };

        let responses = [left.await.unwrap().unwrap(), right.await.unwrap().unwrap()];

        assert_eq!(
            responses
                .iter()
                .filter(|response| response.replayed)
                .count(),
            1
        );
        assert_eq!(
            responses
                .iter()
                .filter(|response| !response.replayed)
                .count(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM study_records WHERE plan_id='plan-1'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM agent_steps WHERE idempotency_key='generic/r1/wal-barrier'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM agent_events WHERE event_type='tool.completed'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );

        pool.close().await;
        drop(pool);
        drop(database);
    });
}

#[test]
fn checkin_same_run_step_with_different_key_is_a_conflict() {
    block_on(async {
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        seed_agent_run(&pool).await;
        let executor = AgentExecutor::new(pool.clone());
        let input = fixture_checkin_input(&fixture);

        executor
            .execute_record_checkin_plan(execution_request(
                input.clone(),
                "generic/r1/run-step-first",
                0,
            ))
            .await
            .unwrap();
        let error = executor
            .execute_record_checkin_plan(execution_request(input, "generic/r1/run-step-second", 0))
            .await
            .unwrap_err();

        assert_eq!(error.code(), "conflict");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_steps")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM study_records")
                .fetch_one(&pool)
                .await
                .unwrap(),
            2
        );
    });
}

#[test]
fn executor_receipts_list_descriptor_permissions_and_events_redact_free_text() {
    block_on(async {
        const SECRET: &str = "SECRET_MARKER";
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        seed_agent_run(&pool).await;
        let mut input = fixture["input"].clone();
        input["content"] = serde_json::json!(format!("{SECRET} private content"));
        input["difficulty_notes"] = serde_json::json!(format!("{SECRET} private notes"));
        input["wrong_questions"][0]["error_reason"] =
            serde_json::json!(format!("{SECRET} private answer notes"));

        let executor = AgentExecutor::new(pool.clone());
        let ToolCallResponse::WaitingApproval { approval_id, .. } = executor
            .execute(ToolCallRequest {
                run_id: "run-checkin".to_owned(),
                step_index: 0,
                tool_name: "record.checkin_plan".to_owned(),
                tool_version: "1".to_owned(),
                input,
                idempotency_key: Some("generic/r1/privacy".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap()
        else {
            panic!("check-in must request approval")
        };
        executor.resolve_approval(&approval_id, true).await.unwrap();

        // The step keeps the full canonical input (free text included) so the
        // approval replay can re-dispatch the exact same write.
        let (input_json, receipt_json): (String, String) = sqlx::query_as(
            "SELECT input_json,receipt_json FROM agent_steps WHERE idempotency_key='generic/r1/privacy'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(input_json.contains(SECRET));
        let snapshot: Value = serde_json::from_str(&input_json).unwrap();
        assert_eq!(snapshot["plan_id"], "plan-1");
        assert_eq!(snapshot["duration_min"], 30);
        assert_eq!(snapshot["finish"], false);
        let receipt: Value = serde_json::from_str(&receipt_json).unwrap();
        assert_eq!(
            receipt["permissions"],
            serde_json::json!([
                "study_plans:read_write",
                "study_records:write",
                "wrong_questions:write",
                "agent_audit:write"
            ])
        );
        // Event payloads stay structured and never carry the free text.
        let events: Vec<String> =
            sqlx::query_scalar("SELECT payload_json FROM agent_events ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(events.iter().all(|event| !event.contains(SECRET)));
    });
}

#[test]
fn record_replays_after_pool_reopen_and_conflicts_on_free_text_change() {
    block_on(async {
        let database = WalDatabase::new();
        let pool = database.migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        seed_agent_run(&pool).await;
        let request = execution_request(
            fixture_checkin_input(&fixture),
            "generic/r1/reopen-fingerprint",
            0,
        );
        let first = AgentExecutor::new(pool.clone())
            .execute_record_checkin_plan(request.clone())
            .await
            .unwrap();
        assert!(!first.replayed);
        pool.close().await;
        drop(pool);

        let reopened = database.open_pool().await;
        let replay = AgentExecutor::new(reopened.clone())
            .execute_record_checkin_plan(request.clone())
            .await
            .unwrap();
        assert!(replay.replayed);
        assert_eq!(first.output, replay.output);

        let mut changed = request;
        changed.input.difficulty_notes = Some("different private free text".to_owned());
        let error = AgentExecutor::new(reopened.clone())
            .execute_record_checkin_plan(changed)
            .await
            .unwrap_err();
        assert_eq!(error.code(), "idempotency_conflict");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM agent_steps WHERE idempotency_key='generic/r1/reopen-fingerprint'"
            )
            .fetch_one(&reopened)
            .await
            .unwrap(),
            1
        );
        reopened.close().await;
        drop(reopened);
        drop(database);
    });
}

#[test]
fn sql_failures_and_command_errors_never_expose_private_diagnostics() {
    block_on(async {
        const SECRET: &str = "SECRET_MARKER";
        const SQL_TEXT: &str = "SELECT secret FROM private_table";
        const ABSOLUTE_PATH: &str = r"C:\private\zhiyan.db";
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        seed_agent_run(&pool).await;
        let mut input = fixture["input"].clone();
        input["difficulty_notes"] =
            serde_json::json!(format!("{SECRET} {SQL_TEXT} %APPDATA% {ABSOLUTE_PATH}"));
        sqlx::raw_sql(
            r#"
            CREATE TRIGGER reject_private_completed BEFORE INSERT ON agent_events
            WHEN NEW.event_type='tool.completed'
            BEGIN SELECT RAISE(ABORT,'SECRET_MARKER SELECT secret FROM private_table %APPDATA% C:\private\zhiyan.db'); END;
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let error = AgentExecutor::new(pool.clone())
            .execute(ToolCallRequest {
                run_id: "run-checkin".to_owned(),
                step_index: 0,
                tool_name: "record.checkin_plan".to_owned(),
                tool_version: "1".to_owned(),
                input,
                idempotency_key: Some("generic/r1/private-error".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap();
        let ToolCallResponse::WaitingApproval { approval_id, .. } = error else {
            panic!("check-in must request approval")
        };
        let error = AgentExecutor::new(pool.clone())
            .resolve_approval(&approval_id, true)
            .await
            .unwrap_err();
        let command_error = CommandError::from(AgentError::Persistence(format!(
            "{SECRET} {SQL_TEXT} %APPDATA% {ABSOLUTE_PATH}"
        )));
        let stored: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT policy_json FROM agent_steps
            UNION ALL SELECT error FROM agent_steps
            UNION ALL SELECT receipt_json FROM agent_steps
            UNION ALL SELECT payload_json FROM agent_events
            "#,
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        assert_eq!(error.code(), "persistence_error");
        assert_eq!(command_error.code, "persistence_error");
        assert_eq!(command_error.message, "agent persistence failed");
        for serialized in stored.into_iter().chain([command_error.message]) {
            assert!(!serialized.contains(SECRET));
            assert!(!serialized.contains(SQL_TEXT));
            assert!(!serialized.contains("%APPDATA%"));
            assert!(!serialized.contains(ABSOLUTE_PATH));
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM study_records")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        let (status, error_code): (String, String) =
            sqlx::query_as("SELECT status, error FROM agent_steps")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            (status.as_str(), error_code.as_str()),
            ("failed", "persistence_error")
        );
        let conflict = CommandError::from(AgentError::IdempotencyConflict);
        assert_eq!(conflict.code, "idempotency_conflict");
        assert_eq!(
            conflict.message,
            "idempotency key is already being resolved; retry"
        );
        assert!(!conflict.message.contains("constraint"));
    });
}

#[test]
fn file_databases_from_v1_through_v4_upgrade_once_and_reopen_through_both_pools() {
    block_on(async {
        for version in 1..=4 {
            let database = WalDatabase::new();
            let seed_pool = database.open_pool().await;
            migrator_through(version).run(&seed_pool).await.unwrap();
            let label = format!("v{version}");
            seed_versioned_database(&seed_pool, version, &label).await;
            sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
                .execute(&seed_pool)
                .await
                .unwrap();
            seed_pool.close().await;
            drop(seed_pool);

            let plugin_pool = database.open_pool().await;
            migrator_through(10).run(&plugin_pool).await.unwrap();
            let runtime_pool = db::runtime::connect(&database.path).await.unwrap();
            assert_v5_database(&plugin_pool, &label, i64::from(version == 4)).await;
            assert_v5_database(&runtime_pool, &label, i64::from(version == 4)).await;
            assert_eq!(
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM _sqlx_migrations WHERE version=5 AND success=1"
                )
                .fetch_one(&plugin_pool)
                .await
                .unwrap(),
                1
            );
            migrator_through(10).run(&plugin_pool).await.unwrap();
            assert_eq!(
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM _sqlx_migrations WHERE version=5 AND success=1"
                )
                .fetch_one(&plugin_pool)
                .await
                .unwrap(),
                1
            );
            runtime_pool.close().await;
            plugin_pool.close().await;
            drop(runtime_pool);
            drop(plugin_pool);
            drop(database);
        }
    });
}

#[test]
fn prepared_restore_replaces_with_v4_backup_and_relaunch_upgrades_it_once() {
    block_on(async {
        let primary = WalDatabase::new();
        let backup = WalDatabase::new();

        let backup_pool = backup.open_pool().await;
        migrator_through(4).run(&backup_pool).await.unwrap();
        seed_versioned_database(&backup_pool, 4, "backup").await;
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&backup_pool)
            .await
            .unwrap();
        backup_pool.close().await;
        drop(backup_pool);

        let plugin_pool = primary.open_pool().await;
        migrator_through(10).run(&plugin_pool).await.unwrap();
        seed_versioned_database(&plugin_pool, 5, "primary").await;
        let runtime_pool = db::runtime::connect(&primary.path).await.unwrap();
        let runtime = AgentRuntime::new(
            AgentRepository::new(runtime_pool.clone()),
            AgentExecutor::new(runtime_pool.clone()),
        );

        runtime.prepare_database_restore().await.unwrap();
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&plugin_pool)
            .await
            .unwrap();
        plugin_pool.close().await;
        drop(runtime_pool);
        drop(plugin_pool);
        std::fs::copy(&backup.path, &primary.path).unwrap();

        let relaunched_plugin = primary.open_pool().await;
        migrator_through(10).run(&relaunched_plugin).await.unwrap();
        let relaunched_runtime = db::runtime::connect(&primary.path).await.unwrap();
        assert_v5_database(&relaunched_plugin, "backup", 1).await;
        assert_v5_database(&relaunched_runtime, "backup", 1).await;
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM study_plans WHERE id='plan-primary'"
            )
            .fetch_one(&relaunched_plugin)
            .await
            .unwrap(),
            0
        );
        migrator_through(10).run(&relaunched_plugin).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM _sqlx_migrations WHERE version=5 AND success=1"
            )
            .fetch_one(&relaunched_plugin)
            .await
            .unwrap(),
            1
        );

        relaunched_runtime.close().await;
        relaunched_plugin.close().await;
        drop(relaunched_runtime);
        drop(relaunched_plugin);
        drop(backup);
        drop(primary);
    });
}

#[test]
fn executor_approval_dispatch_failure_rolls_back_business_and_step() {
    block_on(async {
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        seed_agent_run(&pool).await;
        sqlx::raw_sql(
            r#"
            CREATE TRIGGER reject_generic_completed BEFORE INSERT ON agent_events
            WHEN NEW.event_type='tool.completed'
            BEGIN SELECT RAISE(ABORT,'generic audit failure'); END;
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let executor = AgentExecutor::new(pool.clone());
        let ToolCallResponse::WaitingApproval { approval_id, .. } = executor
            .execute(ToolCallRequest {
                run_id: "run-checkin".to_owned(),
                step_index: 0,
                tool_name: "record.checkin_plan".to_owned(),
                tool_version: "1".to_owned(),
                input: fixture["input"].clone(),
                idempotency_key: Some("generic/audit/rollback".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap()
        else {
            panic!("check-in must request approval")
        };
        let error = executor
            .resolve_approval(&approval_id, true)
            .await
            .unwrap_err();
        assert_eq!(error.code(), "persistence_error");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM study_records")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        let (status, error_code): (String, String) =
            sqlx::query_as("SELECT status, error FROM agent_steps")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            (status.as_str(), error_code.as_str()),
            ("failed", "persistence_error")
        );
        let events: Vec<(String, String)> =
            sqlx::query_as("SELECT event_type, payload_json FROM agent_events ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(
            !events.iter().any(|event| event.0 == "tool.completed"),
            "the aborted completion event must never land"
        );
        assert!(events
            .iter()
            .all(|event| !event.1.contains("generic audit failure")));
    });
}

#[test]
fn executor_run_gate_blocks_non_running_or_wrong_current_step_before_business() {
    block_on(async {
        for (status, current_step) in [
            ("queued", 0_i64),
            ("cancelled", 0),
            ("interrupted", 0),
            ("running", 1),
        ] {
            let pool = migrated_pool().await;
            let fixture = seed_checkin_fixture(&pool).await;
            seed_agent_run(&pool).await;
            sqlx::query(
                "UPDATE settings SET value='rust-owned' WHERE key='agent_tool_owner.record.checkin_plan'",
            )
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query("UPDATE agent_runs SET status=?, current_step=? WHERE id='run-checkin'")
                .bind(status)
                .bind(current_step)
                .execute(&pool)
                .await
                .unwrap();
            let records_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM study_records")
                .fetch_one(&pool)
                .await
                .unwrap();

            let error = AgentExecutor::new(pool.clone())
                .execute(ToolCallRequest {
                    run_id: "run-checkin".to_owned(),
                    step_index: 0,
                    tool_name: "record.checkin_plan".to_owned(),
                    tool_version: "1".to_owned(),
                    input: fixture["input"].clone(),
                    idempotency_key: Some(format!("run-gate/{status}/{current_step}")),
                    approval_id: None,
                })
                .await
                .unwrap_err();

            assert_eq!(error.code(), "conflict");
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM study_records")
                    .fetch_one(&pool)
                    .await
                    .unwrap(),
                records_before
            );
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_steps")
                    .fetch_one(&pool)
                    .await
                    .unwrap(),
                0
            );
        }
    });
}

#[test]
fn executor_approval_dispatch_advances_run_once_and_repeat_wait_does_not_advance_again() {
    block_on(async {
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        seed_agent_run(&pool).await;
        let request = ToolCallRequest {
            run_id: "run-checkin".to_owned(),
            step_index: 0,
            tool_name: "record.checkin_plan".to_owned(),
            tool_version: "1".to_owned(),
            input: fixture["input"].clone(),
            idempotency_key: Some("run-advance/once".to_owned()),
            approval_id: None,
        };
        let executor = AgentExecutor::new(pool.clone());

        // Waiting for approval never executes the step or advances the run.
        let ToolCallResponse::WaitingApproval { approval_id, .. } =
            executor.execute(request.clone()).await.unwrap()
        else {
            panic!("check-in must request approval")
        };
        let (run_status, current_step): (String, i64) =
            sqlx::query_as("SELECT status, current_step FROM agent_runs WHERE id='run-checkin'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!((run_status.as_str(), current_step), ("waiting_approval", 0));

        // Approving dispatches the write and completes the run exactly once.
        // The approved write is the run's last step: the run transitions
        // waiting_approval -> completed (current_step is not advanced).
        let approved = executor.resolve_approval(&approval_id, true).await.unwrap();
        assert_eq!(approved.status, "approved");
        let (run_status, current_step): (String, i64) =
            sqlx::query_as("SELECT status, current_step FROM agent_runs WHERE id='run-checkin'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!((run_status.as_str(), current_step), ("completed", 0));

        // Re-sending the same key replays the completed step and never
        // changes the run state a second time.
        let ToolCallResponse::Completed { replayed, .. } =
            executor.execute(request.clone()).await.unwrap()
        else {
            panic!("completed check-in must replay")
        };
        assert!(replayed);
        let (run_status, current_step): (String, i64) =
            sqlx::query_as("SELECT status, current_step FROM agent_runs WHERE id='run-checkin'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!((run_status.as_str(), current_step), ("completed", 0));
    });
}

#[test]
fn executor_undo_ignores_inert_legacy_ownership() {
    block_on(async {
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        seed_agent_run(&pool).await;
        let executor = AgentExecutor::new(pool.clone());
        let completed = executor
            .execute_record_checkin_plan(execution_request(
                fixture_checkin_input(&fixture),
                "undo/owner-gate",
                0,
            ))
            .await
            .unwrap();
        sqlx::query(
            "UPDATE settings SET value='typescript' WHERE key='agent_tool_owner.record.checkin_plan'",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Ownership is static now: the legacy setting is never re-checked
        // inside the compensation transaction.
        executor.undo(&completed.step_id).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM study_records")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
    });
}

#[test]
fn legacy_specialized_write_api_is_rust_owned_with_inert_legacy_settings() {
    block_on(async {
        let pool = migrated_pool().await;
        let fixture = seed_checkin_fixture(&pool).await;
        seed_agent_run(&pool).await;
        sqlx::query(
            "UPDATE settings SET value='typescript' WHERE key='agent_tool_owner.record.checkin_plan'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let executor = AgentExecutor::new(pool.clone());

        // The dedicated write API is Rust-owned regardless of the legacy
        // setting, which is never read or rewritten.
        let completed = executor
            .execute_record_checkin_plan(execution_request(
                fixture_checkin_input(&fixture),
                "legacy/ownership/gate",
                0,
            ))
            .await
            .unwrap();
        assert!(!completed.replayed);
        assert!(completed.undo_available);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM study_records")
                .fetch_one(&pool)
                .await
                .unwrap(),
            2
        );
        let stored: String = sqlx::query_scalar(
            "SELECT value FROM settings WHERE key='agent_tool_owner.record.checkin_plan'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored, "typescript");

        executor.undo(&completed.step_id).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM study_records")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
    });
}

// --- knowledge point mastery recompute (learning-effectiveness-loop Task 5) ---

async fn seed_mastery_fixture(pool: &SqlitePool) {
    seed_exam_tree(pool).await;
    sqlx::query(
        r#"
        INSERT INTO study_plans (
            id, exam_id, subject_id, knowledge_point_id, date, planned_tasks,
            planned_duration, status, generated_by, sort_order, created_at, updated_at
        ) VALUES ('plan-mastery', 'exam-1', 'subject-math', 'kp-function',
                  '2026-07-17', '复习函数', 60, 'pending', 'local', 0,
                  '2026-07-16 09:00:00', '2026-07-16 09:00:00')
        "#,
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_history_rating(
    pool: &SqlitePool,
    id: &str,
    date: &str,
    created_at: &str,
    rating: Option<i64>,
) {
    sqlx::query(
        r#"
        INSERT INTO study_records (
            id, date, subject_id, knowledge_point_id, duration_min,
            mastery_rating, created_at, updated_at
        ) VALUES (?, ?, 'subject-math', 'kp-function', 30, ?, ?, ?)
        "#,
    )
    .bind(id)
    .bind(date)
    .bind(rating)
    .bind(created_at)
    .bind(created_at)
    .execute(pool)
    .await
    .unwrap();
}

fn mastery_checkin_input(mastery_rating: Option<i64>) -> RecordCheckinPlanInput {
    RecordCheckinPlanInput {
        plan_id: "plan-mastery".to_owned(),
        duration_min: 30,
        content: None,
        questions_count: 0,
        correct_count: 0,
        mastery_rating,
        difficulty_notes: None,
        mood: None,
        session_time: None,
        finish: false,
        wrong_questions: Vec::new(),
    }
}

#[test]
fn checkin_updates_kp_mastery_from_recent_ratings() {
    block_on(async {
        let pool = migrated_pool().await;
        seed_mastery_fixture(&pool).await;
        // Two rated history rows plus the new rating=1 checkin: the three most
        // recent rated records are 1 (2026-07-17), 5 (07-16), 5 (07-15).
        seed_history_rating(
            &pool,
            "record-h1",
            "2026-07-15",
            "2026-07-15 09:00:00",
            Some(5),
        )
        .await;
        seed_history_rating(
            &pool,
            "record-h2",
            "2026-07-16",
            "2026-07-16 09:00:00",
            Some(5),
        )
        .await;

        let mut tx = pool.begin().await.unwrap();
        record::checkin_plan(
            &mut tx,
            mastery_checkin_input(Some(1)),
            BUSINESS_DATE,
            "record-mastery-new",
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let m: i64 = sqlx::query_scalar(
            "SELECT current_mastery FROM knowledge_points WHERE id = 'kp-function'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        // ROUND(AVG(1, 5, 5)) = ROUND(3.67) = 4.
        assert_eq!(m, 4);
    });
}

#[test]
fn checkin_without_any_rated_records_keeps_mastery_untouched() {
    block_on(async {
        let pool = migrated_pool().await;
        seed_mastery_fixture(&pool).await;
        // History exists but carries no mastery ratings: the aggregate must
        // keep the self-assessed baseline (default 3).
        seed_history_rating(
            &pool,
            "record-h1",
            "2026-07-15",
            "2026-07-15 09:00:00",
            None,
        )
        .await;

        let mut tx = pool.begin().await.unwrap();
        record::checkin_plan(
            &mut tx,
            mastery_checkin_input(None),
            BUSINESS_DATE,
            "record-mastery-null",
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let m: i64 = sqlx::query_scalar(
            "SELECT current_mastery FROM knowledge_points WHERE id = 'kp-function'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(m, 3);
    });
}

#[test]
fn first_rated_checkin_sets_mastery_directly() {
    block_on(async {
        let pool = migrated_pool().await;
        seed_mastery_fixture(&pool).await;

        let mut tx = pool.begin().await.unwrap();
        record::checkin_plan(
            &mut tx,
            mastery_checkin_input(Some(2)),
            BUSINESS_DATE,
            "record-mastery-first",
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let m: i64 = sqlx::query_scalar(
            "SELECT current_mastery FROM knowledge_points WHERE id = 'kp-function'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(m, 2);
    });
}

#[test]
fn create_free_with_kp_recomputes_mastery_too() {
    block_on(async {
        let pool = migrated_pool().await;
        seed_exam_tree(&pool).await;

        let mut tx = pool.begin().await.unwrap();
        record::create_free(
            &mut tx,
            record::RecordCreateFreeInput {
                exam_id: EXAM_ID.to_owned(),
                date: BUSINESS_DATE.to_owned(),
                subject_id: "subject-math".to_owned(),
                knowledge_point_id: Some("kp-function".to_owned()),
                duration_min: 45,
                content: None,
                questions_count: None,
                correct_count: None,
                mastery_rating: Some(4),
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let m: i64 = sqlx::query_scalar(
            "SELECT current_mastery FROM knowledge_points WHERE id = 'kp-function'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(m, 4);
    });
}

// --- review.get_due / review.complete (learning-effectiveness-loop Task 4) ---

async fn seed_review_fixture(pool: &SqlitePool) {
    seed_exam_tree(pool).await;
    seed_agent_run(pool).await;
    // Four wrong questions under the bound exam:
    // - wq-due: overdue (next_review_at in the past)
    // - wq-null: never scheduled (NULL next_review_at counts as due)
    // - wq-future: scheduled in the future (not due)
    // - wq-done: mastered, excluded even though overdue
    sqlx::query(
        r#"
        INSERT INTO wrong_questions (id, subject_id, question_desc, next_review_at) VALUES
            ('wq-due', 'subject-math', 'Overdue question', '2000-01-01'),
            ('wq-null', 'subject-math', 'Never scheduled', NULL),
            ('wq-future', 'subject-math', 'Future question', '2999-01-01');
        INSERT INTO wrong_questions (id, subject_id, question_desc, mastered, next_review_at)
            VALUES ('wq-done', 'subject-math', 'Mastered question', 1, '2000-01-01');
        "#,
    )
    .execute(pool)
    .await
    .unwrap();
}

#[test]
fn review_get_due_returns_due_and_unscheduled_only() {
    block_on(async {
        let pool = migrated_pool().await;
        seed_review_fixture(&pool).await;

        let response = AgentExecutor::new(pool.clone())
            .execute(ToolCallRequest {
                run_id: "run-checkin".to_owned(),
                step_index: 0,
                tool_name: "review.get_due".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({"exam_id": EXAM_ID}),
                idempotency_key: None,
                approval_id: None,
            })
            .await
            .unwrap();
        let ToolCallResponse::Completed {
            output,
            replayed,
            undo_available,
            ..
        } = response
        else {
            panic!("R0 must complete")
        };
        assert!(!replayed);
        assert!(!undo_available);
        assert_eq!(output["count"], 2);
        let ids: Vec<&str> = output["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect();
        // Never-scheduled first, then oldest next_review_at.
        assert_eq!(ids, vec!["wq-null", "wq-due"]);
        assert_eq!(output["items"][0]["subject_name"], "数学");
        assert!(output["items"][0]["knowledge_point_name"].is_null());
    });
}

#[test]
fn review_complete_first_success_schedules_one_day_and_is_undoable() {
    block_on(async {
        let pool = migrated_pool().await;
        seed_review_fixture(&pool).await;
        let executor = AgentExecutor::new(pool.clone());

        let response = executor
            .execute(ToolCallRequest {
                run_id: "run-checkin".to_owned(),
                step_index: 0,
                tool_name: "review.complete".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({"wrong_question_id": "wq-due", "quality": 4}),
                idempotency_key: None,
                approval_id: None,
            })
            .await
            .unwrap();
        let ToolCallResponse::Completed {
            step_id,
            output,
            replayed,
            undo_available,
        } = response
        else {
            panic!("R1 must complete")
        };
        assert!(!replayed);
        assert!(undo_available);
        assert_eq!(output["interval_days"], 1.0);

        let (count, ease): (i64, f64) = sqlx::query_as(
            "SELECT review_count, ease_factor FROM wrong_questions WHERE id='wq-due'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1);
        assert!((ease - 2.5).abs() < 1e-9); // q=4 keeps the initial ease
        let last_review: String =
            sqlx::query_scalar("SELECT last_review_at FROM wrong_questions WHERE id='wq-due'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(!last_review.is_empty());
        let due_offset: i64 = sqlx::query_scalar(
            "SELECT CAST(julianday(next_review_at) - julianday(date('now','localtime')) AS INTEGER) \
             FROM wrong_questions WHERE id='wq-due'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(due_offset, 1);

        // Undo restores the pre-review scheduling state.
        executor.undo(&step_id).await.unwrap();
        let restored: (i64, f64, f64, Option<String>) = sqlx::query_as(
            "SELECT review_count, ease_factor, review_interval_days, next_review_at \
             FROM wrong_questions WHERE id='wq-due'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(restored.0, 0);
        assert!((restored.1 - 2.5).abs() < 1e-9);
        assert!((restored.2 - 0.0).abs() < 1e-9);
        assert_eq!(restored.3.as_deref(), Some("2000-01-01"));
    });
}

#[test]
fn review_complete_rejects_mastered_question() {
    block_on(async {
        let pool = migrated_pool().await;
        seed_review_fixture(&pool).await;

        let error = AgentExecutor::new(pool.clone())
            .execute(ToolCallRequest {
                run_id: "run-checkin".to_owned(),
                step_index: 0,
                tool_name: "review.complete".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({"wrong_question_id": "wq-done", "quality": 4}),
                idempotency_key: None,
                approval_id: None,
            })
            .await
            .unwrap_err();
        assert_eq!(error.code(), "persistence_error");
    });
}

#[test]
fn review_complete_rejects_quality_out_of_range() {
    block_on(async {
        let pool = migrated_pool().await;
        seed_review_fixture(&pool).await;

        let error = AgentExecutor::new(pool.clone())
            .execute(ToolCallRequest {
                run_id: "run-checkin".to_owned(),
                step_index: 0,
                tool_name: "review.complete".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({"wrong_question_id": "wq-due", "quality": 9}),
                idempotency_key: None,
                approval_id: None,
            })
            .await
            .unwrap_err();
        assert_eq!(error.code(), "tool_schema_invalid");
    });
}

// --- material.create (v0.3.0 Task 3, R3 paste-text import) ---

#[test]
fn material_create_applies_with_approval_and_stores_content() {
    block_on(async {
        let pool = migrated_pool().await;
        seed_exam_tree(&pool).await;
        seed_agent_run(&pool).await;
        let executor = AgentExecutor::new(pool.clone());

        // 200 chars: long enough to exercise the 120-char preview excerpt cap
        // while the full body must never appear in preview or tool output.
        let content = "甲".repeat(200);
        let char_count = content.chars().count();
        let ToolCallResponse::WaitingApproval {
            approval_id,
            preview,
            ..
        } = executor
            .execute(ToolCallRequest {
                run_id: "run-checkin".to_owned(),
                step_index: 0,
                tool_name: "material.create".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({
                    "exam_id": EXAM_ID,
                    "subject_id": "subject-math",
                    "title": "函数笔记",
                    "content": content,
                }),
                idempotency_key: Some("material/create/1".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap()
        else {
            panic!("material.create must request approval")
        };
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM materials")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0,
            "no business write before approval"
        );

        // The approval preview shows title + char count + a 120-char excerpt,
        // never the full content.
        let preview_json = serde_json::to_string(&preview).unwrap();
        let excerpt: String = content.chars().take(120).collect();
        assert_eq!(preview["fields"]["title"].as_str().unwrap(), "函数笔记");
        assert_eq!(
            preview["fields"]["char_count"].as_i64().unwrap(),
            char_count as i64
        );
        assert_eq!(preview["fields"]["excerpt"].as_str().unwrap(), excerpt);
        assert!(
            !preview_json.contains(&content),
            "preview must not contain the full content"
        );

        let approved = executor.resolve_approval(&approval_id, true).await.unwrap();
        assert_eq!(approved.status, "approved");

        let stored: (String, String, String) =
            sqlx::query_as("SELECT exam_id, title, content FROM materials")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored.0, EXAM_ID);
        assert_eq!(stored.1, "函数笔记");
        assert_eq!(stored.2, content);

        // Tool output carries material_id/title/char_count only — never the
        // content itself.
        let output_json: String =
            sqlx::query_scalar("SELECT output_json FROM agent_steps WHERE id = ?")
                .bind(&approved.step_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let output: Value = serde_json::from_str(&output_json).unwrap();
        assert!(!output["material_id"].as_str().unwrap().is_empty());
        assert_eq!(output["title"].as_str().unwrap(), "函数笔记");
        assert_eq!(output["char_count"].as_i64().unwrap(), char_count as i64);
        assert!(
            !output_json.contains("甲"),
            "tool output must not echo the content"
        );
    });
}

#[test]
fn material_create_rejects_oversized_content() {
    block_on(async {
        let pool = migrated_pool().await;
        seed_exam_tree(&pool).await;
        seed_agent_run(&pool).await;
        let executor = AgentExecutor::new(pool.clone());

        for content in ["a".repeat(50_001), "中".repeat(50_001)] {
            let error = executor
                .execute(ToolCallRequest {
                    run_id: "run-checkin".to_owned(),
                    step_index: 0,
                    tool_name: "material.create".to_owned(),
                    tool_version: "1".to_owned(),
                    input: serde_json::json!({
                        "exam_id": EXAM_ID,
                        "subject_id": "subject-math",
                        "title": "超长资料",
                        "content": content,
                    }),
                    idempotency_key: None,
                    approval_id: None,
                })
                .await
                .unwrap_err();
            assert_eq!(error.code(), "tool_schema_invalid");
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM materials")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0,
            "oversized content must never be written"
        );
    });
}

#[test]
fn material_create_rejects_subject_not_in_exam() {
    block_on(async {
        let pool = migrated_pool().await;
        seed_exam_tree(&pool).await;
        sqlx::raw_sql(
            r#"
            INSERT INTO exams (id, name, exam_date) VALUES ('exam-2', 'Other', '2027-01-01');
            INSERT INTO subjects (id, exam_id, name) VALUES ('subject-other', 'exam-2', '英语');
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        let error = material::create(
            &mut tx,
            material::MaterialCreateInput {
                exam_id: Some(EXAM_ID.to_owned()),
                subject_id: "subject-other".to_owned(),
                title: "越权资料".to_owned(),
                content: "不应写入".to_owned(),
            },
        )
        .await
        .unwrap_err();
        tx.rollback().await.unwrap();
        assert_eq!(error.code(), "persistence_error");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM materials")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
    });
}

// --- knowledge_point.create_batch (v0.3.0 Task 4, R3 bulk kp import) ---

#[test]
fn kp_create_batch_applies_with_material_source() {
    block_on(async {
        let pool = migrated_pool().await;
        seed_exam_tree(&pool).await;
        sqlx::query(
            "INSERT INTO materials(id,exam_id,subject_id,title,content) VALUES('mat-1',?, 'subject-math','泰勒级数讲义','正文')",
        )
        .bind(EXAM_ID)
        .execute(&pool)
        .await
        .unwrap();
        seed_agent_run(&pool).await;
        let executor = AgentExecutor::new(pool.clone());

        let ToolCallResponse::WaitingApproval { approval_id, .. } = executor
            .execute(ToolCallRequest {
                run_id: "run-checkin".to_owned(),
                step_index: 0,
                tool_name: "knowledge_point.create_batch".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({
                    "exam_id": EXAM_ID,
                    "subject_id": "subject-math",
                    "material_id": "mat-1",
                    "concepts": [
                        {"name": "泰勒展开", "parent_name": null, "source_ref": "§1-§2"},
                        {"name": "余项估计", "parent_name": "泰勒展开", "source_ref": "§3"}
                    ]
                }),
                idempotency_key: Some("kp/create/1".to_owned()),
                approval_id: None,
            })
            .await
            .unwrap()
        else {
            panic!("knowledge_point.create_batch must request approval")
        };
        // No business write before approval (the seeded 'kp-function' row is the only kp).
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM knowledge_points")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );

        let approved = executor.resolve_approval(&approval_id, true).await.unwrap();
        assert_eq!(approved.status, "approved");

        let rows: Vec<(
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        )> = sqlx::query_as(
            r#"
                SELECT id, name, parent_id, material_id, source_ref
                FROM knowledge_points
                WHERE subject_id = 'subject-math' AND name IN ('泰勒展开', '余项估计')
                ORDER BY rowid
                "#,
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows.len(), 2);
        let parent = &rows[0];
        let child = &rows[1];
        assert_eq!(parent.1, "泰勒展开");
        assert_eq!(parent.2, None);
        assert_eq!(child.1, "余项估计");
        assert_eq!(child.2.as_deref(), Some(parent.0.as_str()));
        for row in &rows {
            assert_eq!(row.3.as_deref(), Some("mat-1"));
        }
        assert_eq!(parent.4.as_deref(), Some("§1-§2"));
        assert_eq!(child.4.as_deref(), Some("§3"));

        // Tool output: {created:[{id,name,parent_id}], count}
        let output_json: String =
            sqlx::query_scalar("SELECT output_json FROM agent_steps WHERE id = ?")
                .bind(&approved.step_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let output: Value = serde_json::from_str(&output_json).unwrap();
        assert_eq!(output["count"].as_i64().unwrap(), 2);
        let created = output["created"].as_array().unwrap();
        assert_eq!(created.len(), 2);
        assert_eq!(created[0]["id"].as_str().unwrap(), parent.0);
        assert_eq!(created[0]["name"], "泰勒展开");
        assert_eq!(created[0]["parent_id"], Value::Null);
        assert_eq!(created[1]["id"].as_str().unwrap(), child.0);
        assert_eq!(created[1]["name"], "余项估计");
        assert_eq!(created[1]["parent_id"].as_str().unwrap(), parent.0.as_str());
    });
}

#[test]
fn kp_create_batch_rejects_unknown_parent() {
    block_on(async {
        let pool = migrated_pool().await;
        seed_exam_tree(&pool).await;
        let mut tx = pool.begin().await.unwrap();

        // '不存在节点' is neither earlier in this batch nor in the subject's
        // existing kp tree ('函数') — must be rejected without any write.
        let error = knowledge_point_admin::create_batch(
            &mut tx,
            knowledge_point_admin::KnowledgePointCreateBatchInput {
                exam_id: None,
                subject_id: "subject-math".to_owned(),
                material_id: None,
                concepts: vec![knowledge_point_admin::BatchConcept {
                    name: "洛必达法则".to_owned(),
                    parent_name: Some("不存在节点".to_owned()),
                    source_ref: None,
                }],
            },
        )
        .await
        .unwrap_err();
        tx.rollback().await.unwrap();
        assert_eq!(error.code(), "persistence_error");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM knowledge_points WHERE name='洛必达法则'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
    });
}

#[test]
fn kp_create_batch_rejects_empty_and_oversized_batch() {
    block_on(async {
        let pool = migrated_pool().await;
        seed_exam_tree(&pool).await;
        let mut tx = pool.begin().await.unwrap();

        for concepts in [
            Vec::new(),
            std::iter::repeat_n(
                knowledge_point_admin::BatchConcept {
                    name: "知识点".to_owned(),
                    parent_name: None,
                    source_ref: None,
                },
                51,
            )
            .collect::<Vec<_>>(),
        ] {
            let error = knowledge_point_admin::create_batch(
                &mut tx,
                knowledge_point_admin::KnowledgePointCreateBatchInput {
                    exam_id: None,
                    subject_id: "subject-math".to_owned(),
                    material_id: None,
                    concepts,
                },
            )
            .await
            .unwrap_err();
            assert_eq!(error.code(), "tool_schema_invalid");
        }
    });

    block_on(async {
        // Schema layer rejects the same shapes at the dispatcher boundary.
        let pool = migrated_pool().await;
        seed_exam_tree(&pool).await;
        seed_agent_run(&pool).await;
        let executor = AgentExecutor::new(pool.clone());
        let oversized: Vec<Value> =
            std::iter::repeat_n(serde_json::json!({"name": "知识点"}), 51).collect();
        for concepts in [serde_json::json!([]), serde_json::json!(oversized)] {
            let error = executor
                .execute(ToolCallRequest {
                    run_id: "run-checkin".to_owned(),
                    step_index: 0,
                    tool_name: "knowledge_point.create_batch".to_owned(),
                    tool_version: "1".to_owned(),
                    input: serde_json::json!({
                        "exam_id": EXAM_ID,
                        "subject_id": "subject-math",
                        "concepts": concepts,
                    }),
                    idempotency_key: None,
                    approval_id: None,
                })
                .await
                .unwrap_err();
            assert_eq!(error.code(), "tool_schema_invalid");
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM knowledge_points")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
    });
}

#[test]
fn kp_create_batch_rejects_bad_source_ref_format() {
    block_on(async {
        // Schema layer: dispatcher pattern check.
        let pool = migrated_pool().await;
        seed_exam_tree(&pool).await;
        seed_agent_run(&pool).await;
        let executor = AgentExecutor::new(pool.clone());
        let error = executor
            .execute(ToolCallRequest {
                run_id: "run-checkin".to_owned(),
                step_index: 0,
                tool_name: "knowledge_point.create_batch".to_owned(),
                tool_version: "1".to_owned(),
                input: serde_json::json!({
                    "exam_id": EXAM_ID,
                    "subject_id": "subject-math",
                    "concepts": [{"name": "泰勒展开", "source_ref": "第3段"}],
                }),
                idempotency_key: None,
                approval_id: None,
            })
            .await
            .unwrap_err();
        assert_eq!(error.code(), "tool_schema_invalid");
    });

    block_on(async {
        // Execution layer: direct calls must hit the same guard.
        let pool = migrated_pool().await;
        seed_exam_tree(&pool).await;
        let mut tx = pool.begin().await.unwrap();
        let error = knowledge_point_admin::create_batch(
            &mut tx,
            knowledge_point_admin::KnowledgePointCreateBatchInput {
                exam_id: None,
                subject_id: "subject-math".to_owned(),
                material_id: None,
                concepts: vec![knowledge_point_admin::BatchConcept {
                    name: "泰勒展开".to_owned(),
                    parent_name: None,
                    source_ref: Some("第3段".to_owned()),
                }],
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error.code(), "tool_schema_invalid");
        tx.rollback().await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM knowledge_points")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
    });
}
