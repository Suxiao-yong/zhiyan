use std::future::Future;

use chrono::DateTime;
use serde_json::Value;
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use zhiyan_lib::{
    agent::tools::plan::{self, PlanGetTodayInput},
    db,
};

const EXAM_ID: &str = "exam-1";
const BUSINESS_DATE: &str = "2026-07-17";

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
