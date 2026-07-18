use chrono::DateTime;
use serde_json::Value;
use sqlx::sqlite::SqlitePoolOptions;
use zhiyan_lib::{
    agent::tools::plan::{self, PlanGetTodayInput},
    db,
};

#[test]
fn plan_get_today_uses_the_0400_business_day_boundary() {
    let before_boundary = DateTime::parse_from_rfc3339("2026-07-18T03:59:59+08:00").unwrap();
    let at_boundary = DateTime::parse_from_rfc3339("2026-07-18T04:00:00+08:00").unwrap();

    assert_eq!(plan::business_date_at(before_boundary), "2026-07-17");
    assert_eq!(plan::business_date_at(at_boundary), "2026-07-18");
}

#[test]
fn plan_get_today_matches_the_shared_fixture_without_updating_study_plans() {
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
            let migrations = db::migrations();
            assert_eq!(
                migrations
                    .iter()
                    .map(|migration| migration.version)
                    .collect::<Vec<_>>(),
                vec![1, 2, 3, 4, 5]
            );
            for migration in migrations {
                sqlx::raw_sql(migration.sql).execute(&pool).await.unwrap();
            }

            sqlx::raw_sql(
                r#"
                INSERT INTO exams (id, name, exam_date)
                VALUES ('exam-1', 'Fixture exam', '2026-12-31');

                INSERT INTO subjects (id, exam_id, name)
                VALUES ('subject-math', 'exam-1', '数学');

                INSERT INTO knowledge_points (id, subject_id, name)
                VALUES ('kp-function', 'subject-math', '函数');

                INSERT INTO study_plans (
                    id, exam_id, subject_id, knowledge_point_id, date,
                    planned_tasks, planned_duration, actual_duration, actual_tasks,
                    status, generated_by, ai_suggestion, user_modified, sort_order,
                    created_at, updated_at
                ) VALUES (
                    'plan-1', 'exam-1', 'subject-math', 'kp-function', '2026-07-17',
                    '复习函数', 60, NULL, NULL,
                    'pending', 'local', NULL, 0, 0,
                    '2026-07-16 09:00:00', '2026-07-17 21:00:00'
                );

                INSERT INTO study_records (
                    id, plan_id, date, subject_id, knowledge_point_id,
                    duration_min, content, created_at, updated_at
                ) VALUES (
                    'record-1', 'plan-1', '2026-07-17', 'subject-math', 'kp-function',
                    30, '完成第一节', '2026-07-17 20:00:00', '2026-07-17 20:00:00'
                );
                "#,
            )
            .execute(&pool)
            .await
            .unwrap();

            let before: (Option<i64>, Option<String>, String) = sqlx::query_as(
                "SELECT actual_duration, actual_tasks, status FROM study_plans WHERE id = 'plan-1'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();

            let output = plan::get_today(
                &pool,
                PlanGetTodayInput {
                    exam_id: "exam-1".to_owned(),
                },
                "2026-07-17",
            )
            .await
            .unwrap();

            let fixture: Value = serde_json::from_str(include_str!(
                "../../tests/fixtures/agent-tools/plan-get-today.json"
            ))
            .unwrap();
            assert_eq!(
                serde_json::to_value(output).unwrap(),
                fixture["expected_output"]
            );

            let after: (Option<i64>, Option<String>, String) = sqlx::query_as(
                "SELECT actual_duration, actual_tasks, status FROM study_plans WHERE id = 'plan-1'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(after, before, "plan.get_today must remain read-only");
        });
}
