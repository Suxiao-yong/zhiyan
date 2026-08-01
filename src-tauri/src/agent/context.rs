// Rust Context Builder (M3 Part 3). Records, per model call, the in-scope
// business data so the user can inspect what each call touched (spec §10.2).
// The audit carries record IDs + field names ONLY — never raw content (no
// plan tasks, record text, or wrong-question text) per the §10.2 privacy rule.
// This dedicated `agent_context_audit` table replaces the Part 1/2
// `model.invoked` event with structured per-call data provenance.

use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::agent::error::AgentError;
use crate::agent::llm::ProviderUsage;
use crate::agent::tools::plan;

/// The in-scope business data for a run, gathered once and reused on every
/// audit row of that run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextScope {
    pub exam_id: Option<String>,
    pub plan_ids: Vec<String>,
    pub subject_ids: Vec<String>,
}

impl ContextScope {
    fn categories(&self) -> Vec<&'static str> {
        let mut categories = Vec::new();
        if self.exam_id.is_some() {
            categories.push("exam");
        }
        if !self.plan_ids.is_empty() {
            categories.push("plan");
        }
        if !self.subject_ids.is_empty() {
            categories.push("subject");
        }
        categories
    }

    fn record_ids(&self) -> Value {
        let mut map = serde_json::Map::new();
        if let Some(exam_id) = &self.exam_id {
            map.insert("exam".into(), json!([exam_id]));
        }
        if !self.plan_ids.is_empty() {
            map.insert("plan".into(), json!(self.plan_ids));
        }
        if !self.subject_ids.is_empty() {
            map.insert("subject".into(), json!(self.subject_ids));
        }
        Value::Object(map)
    }

    fn field_sets(&self) -> Value {
        let mut map = serde_json::Map::new();
        if self.exam_id.is_some() {
            map.insert(
                "exam".into(),
                json!(["id", "name", "exam_date", "total_score"]),
            );
        }
        if !self.plan_ids.is_empty() {
            map.insert(
                "plan".into(),
                json!([
                    "id",
                    "date",
                    "subject_id",
                    "planned_tasks",
                    "planned_duration",
                    "status"
                ]),
            );
        }
        if !self.subject_ids.is_empty() {
            map.insert(
                "subject".into(),
                json!(["id", "name", "weight", "current_level"]),
            );
        }
        Value::Object(map)
    }
}

/// One row of the context audit, surfaced to the hidden inspector. JSON
/// columns are parsed; no raw business content is ever present.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextAuditRow {
    pub id: String,
    pub call_seq: i64,
    pub purpose: String,
    pub local: bool,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub tools_offered: Vec<String>,
    pub categories: Vec<String>,
    pub record_ids: Value,
    pub field_sets: Value,
    pub created_at: String,
}

#[derive(Clone)]
pub struct ContextAudit {
    pool: SqlitePool,
}

impl ContextAudit {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Gather the run's in-scope business data: the exam bound to the run's
    /// session, today's plans for that exam, and the exam's subjects. Gathered
    /// once per run via indexed lookups.
    pub async fn gather(&self, run_id: &str) -> Result<ContextScope, AgentError> {
        let exam_id: Option<Option<String>> = sqlx::query_scalar(
            "SELECT s.exam_id FROM agent_runs r \
             JOIN agent_sessions s ON s.id = r.session_id WHERE r.id = ?",
        )
        .bind(run_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx)?;
        let Some(exam_id) = exam_id.flatten() else {
            return Ok(ContextScope::default());
        };
        let business_date = plan::business_date_at(Local::now().fixed_offset());
        let plan_ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM study_plans WHERE exam_id = ? AND date = ? ORDER BY id",
        )
        .bind(&exam_id)
        .bind(&business_date)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx)?;
        let subject_ids: Vec<String> =
            sqlx::query_scalar("SELECT id FROM subjects WHERE exam_id = ? ORDER BY id")
                .bind(&exam_id)
                .fetch_all(&self.pool)
                .await
                .map_err(map_sqlx)?;
        Ok(ContextScope {
            exam_id: Some(exam_id),
            plan_ids,
            subject_ids,
        })
    }

    /// Record one audit row for a model call. `call_seq` identifies the call
    /// within the run; `tools_offered` lists the tool names the model could call.
    pub async fn record(
        &self,
        run_id: &str,
        call_seq: i64,
        scope: &ContextScope,
        usage: &ProviderUsage,
        local: bool,
        tools_offered: &[&str],
    ) -> Result<(), AgentError> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO agent_context_audit \
             (id, run_id, call_seq, purpose, local, prompt_tokens, completion_tokens, \
              tools_offered_json, categories_json, record_ids_json, field_sets_json) \
             VALUES (?, ?, ?, 'planner_turn', ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(run_id)
        .bind(call_seq)
        .bind(if local { 1 } else { 0 })
        .bind(usage.prompt_tokens)
        .bind(usage.completion_tokens)
        .bind(json!(tools_offered).to_string())
        .bind(json!(scope.categories()).to_string())
        .bind(scope.record_ids().to_string())
        .bind(scope.field_sets().to_string())
        .execute(&self.pool)
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    /// Read every audit row of a run, oldest call first, for the hidden
    /// Context Inspector. JSON columns are parsed back into structured values;
    /// no raw business content is ever present.
    pub async fn list(&self, run_id: &str) -> Result<Vec<ContextAuditRow>, AgentError> {
        #[derive(sqlx::FromRow)]
        struct Raw {
            id: String,
            call_seq: i64,
            purpose: String,
            local: i64,
            prompt_tokens: i64,
            completion_tokens: i64,
            tools_offered_json: String,
            categories_json: String,
            record_ids_json: String,
            field_sets_json: String,
            created_at: String,
        }
        let rows = sqlx::query_as::<_, Raw>(
            "SELECT id, call_seq, purpose, local, prompt_tokens, completion_tokens, \
             tools_offered_json, categories_json, record_ids_json, field_sets_json, created_at \
             FROM agent_context_audit WHERE run_id = ? ORDER BY call_seq",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx)?;
        rows.into_iter()
            .map(|row| {
                Ok(ContextAuditRow {
                    id: row.id,
                    call_seq: row.call_seq,
                    purpose: row.purpose,
                    local: row.local != 0,
                    prompt_tokens: row.prompt_tokens,
                    completion_tokens: row.completion_tokens,
                    tools_offered: json_or_persistence(serde_json::from_str(
                        &row.tools_offered_json,
                    ))?,
                    categories: json_or_persistence(serde_json::from_str(&row.categories_json))?,
                    record_ids: json_or_persistence(serde_json::from_str(&row.record_ids_json))?,
                    field_sets: json_or_persistence(serde_json::from_str(&row.field_sets_json))?,
                    created_at: row.created_at,
                })
            })
            .collect()
    }
}

fn map_sqlx(_error: sqlx::Error) -> AgentError {
    AgentError::Persistence("context audit failed".to_owned())
}

fn json_or_persistence<T: serde::de::DeserializeOwned>(
    value: Result<T, serde_json::Error>,
) -> Result<T, AgentError> {
    value.map_err(|_| AgentError::Persistence("context audit failed".to_owned()))
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::agent::tools::plan;

    async fn audit_pool() -> SqlitePool {
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

    async fn run_for(pool: &SqlitePool, exam_id: Option<&str>) -> String {
        sqlx::query("INSERT INTO agent_sessions (id, title, exam_id) VALUES ('session-a', 'A', ?)")
            .bind(exam_id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO agent_runs (id, session_id, goal, status) VALUES ('run-a', 'session-a', 'g', 'running')")
            .execute(pool)
            .await
            .unwrap();
        "run-a".to_owned()
    }

    #[tokio::test]
    async fn gather_is_empty_when_no_exam_is_bound() {
        let pool = audit_pool().await;
        run_for(&pool, None).await;
        let audit = ContextAudit::new(pool.clone());
        let scope = audit.gather("run-a").await.unwrap();
        assert!(scope.exam_id.is_none());
        assert!(scope.plan_ids.is_empty());
        assert!(scope.subject_ids.is_empty());
        assert!(scope.categories().is_empty());
    }

    #[tokio::test]
    async fn gather_collects_today_plans_and_subjects_for_the_bound_exam() {
        let pool = audit_pool().await;
        sqlx::raw_sql(
            r#"
            INSERT INTO exams (id, name, exam_date) VALUES ('exam-1', 'Math', '2030-01-01');
            INSERT INTO subjects (id, exam_id, name) VALUES ('subject-math', 'exam-1', 'Math');
            INSERT INTO study_plans (id, exam_id, subject_id, date, planned_tasks) VALUES
                ('plan-today', 'exam-1', 'subject-math', '1970-01-01', 'x');
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        // The seeded plan date is '1970-01-01'; plant a plan for the real business date too.
        let today = plan::business_date_at(Local::now().fixed_offset());
        sqlx::query("INSERT INTO study_plans (id, exam_id, subject_id, date, planned_tasks) VALUES ('plan-real', 'exam-1', 'subject-math', ?, 'today')")
            .bind(&today)
            .execute(&pool)
            .await
            .unwrap();
        run_for(&pool, Some("exam-1")).await;

        let audit = ContextAudit::new(pool.clone());
        let scope = audit.gather("run-a").await.unwrap();
        assert_eq!(scope.exam_id.as_deref(), Some("exam-1"));
        assert_eq!(scope.plan_ids, vec!["plan-real".to_owned()]);
        assert_eq!(scope.subject_ids, vec!["subject-math".to_owned()]);
        assert_eq!(scope.categories(), vec!["exam", "plan", "subject"]);
        assert_eq!(scope.record_ids()["exam"][0], "exam-1");
        assert_eq!(scope.record_ids()["plan"][0], "plan-real");
        assert_eq!(
            scope.field_sets()["exam"],
            json!(["id", "name", "exam_date", "total_score"])
        );
    }

    #[tokio::test]
    async fn record_writes_a_row_with_scope_tokens_and_tools_offered() {
        let pool = audit_pool().await;
        run_for(&pool, None).await;
        let audit = ContextAudit::new(pool.clone());
        let scope = audit.gather("run-a").await.unwrap();
        let usage = ProviderUsage {
            prompt_tokens: 42,
            completion_tokens: 7,
        };
        audit
            .record("run-a", 1, &scope, &usage, false, &["plan.get_today"])
            .await
            .unwrap();

        let row: (i64, i64, i64, String, String, String, String) = sqlx::query_as(
            "SELECT call_seq, local, prompt_tokens, tools_offered_json, categories_json, record_ids_json, field_sets_json \
             FROM agent_context_audit WHERE run_id='run-a'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, 1);
        assert_eq!(row.1, 0);
        assert_eq!(row.2, 42);
        assert_eq!(row.3, r#"["plan.get_today"]"#);
        assert_eq!(row.4, "[]");
        assert_eq!(row.5, "{}");
        assert_eq!(row.6, "{}");
    }

    #[tokio::test]
    async fn list_returns_rows_oldest_call_first_with_parsed_columns() {
        let pool = audit_pool().await;
        run_for(&pool, None).await;
        let audit = ContextAudit::new(pool.clone());
        let scope = audit.gather("run-a").await.unwrap();
        let usage = ProviderUsage {
            prompt_tokens: 10,
            completion_tokens: 2,
        };
        audit
            .record("run-a", 1, &scope, &usage, false, &["plan.get_today"])
            .await
            .unwrap();
        audit
            .record("run-a", 2, &scope, &usage, true, &[])
            .await
            .unwrap();

        let rows = audit.list("run-a").await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].call_seq, 1);
        assert!(!rows[0].local);
        assert_eq!(rows[0].purpose, "planner_turn");
        assert_eq!(rows[0].tools_offered, vec!["plan.get_today".to_owned()]);
        assert_eq!(rows[0].record_ids, Value::Object(serde_json::Map::new()));
        assert!(!rows[0].id.is_empty());
        assert!(!rows[0].created_at.is_empty());
        assert_eq!(rows[1].call_seq, 2);
        assert!(rows[1].local);
        assert!(rows[1].tools_offered.is_empty());
    }
}
