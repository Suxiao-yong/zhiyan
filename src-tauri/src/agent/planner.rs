// Rust Planner (M3 Part 1): drives the model -> tool loop over AgentRuntime.
// Task 3 covers tool-list projection and the iteration cap; Task 4 adds the loop.

use serde_json::Value;
use sqlx::SqlitePool;

use crate::agent::error::AgentError;
use crate::agent::llm::tool_object;
use crate::agent::runtime::AgentRuntime;
use crate::agent::tools::{ListedTool, RiskLevel, ToolOwnership};

const DEFAULT_MAX_ITERATIONS: i64 = 6;

#[derive(Clone)]
pub struct Planner {
    pool: SqlitePool,
    runtime: AgentRuntime,
}

impl Planner {
    pub fn new(pool: SqlitePool, runtime: AgentRuntime) -> Self {
        Self { pool, runtime }
    }

    /// Project the registry into OpenAI tool objects, offering only tools the
    /// Rust runtime may execute (Shadow reads or RustOwned writes). Typescript
    /// and Unavailable tools are hidden from the model.
    pub async fn tool_offering(&self) -> Result<Vec<Value>, AgentError> {
        let listed = self.runtime.list_tools().await?;
        Ok(listed
            .into_iter()
            .filter(|tool| {
                matches!(
                    tool.ownership,
                    ToolOwnership::Shadow | ToolOwnership::RustOwned
                )
            })
            .map(|tool| project_tool(&tool))
            .collect())
    }

    /// Soft iteration cap. Defaults to 6; a positive `agent_planner_max_iterations`
    /// setting overrides it so a runaway loop is bounded.
    pub async fn max_iterations(&self) -> Result<i64, AgentError> {
        let value: Option<String> = sqlx::query_scalar(
            "SELECT value FROM settings WHERE key = 'agent_planner_max_iterations'",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx)?;
        Ok(value
            .and_then(|raw| raw.parse().ok())
            .filter(|parsed: &i64| *parsed > 0)
            .unwrap_or(DEFAULT_MAX_ITERATIONS))
    }
}

fn project_tool(tool: &ListedTool) -> Value {
    let descriptor = &tool.descriptor;
    let mut description = format!(
        "Call the {} tool. Arguments must match the JSON schema.",
        descriptor.name
    );
    if descriptor.risk == RiskLevel::R1 {
        description.push_str(
            " Exactly-once: an idempotency key is supplied automatically; do not include one.",
        );
    }
    tool_object(descriptor.name, &description, &descriptor.input_schema)
}

fn map_sqlx(_error: sqlx::Error) -> AgentError {
    AgentError::Persistence("planner settings read failed".to_owned())
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::agent::executor::AgentExecutor;
    use crate::agent::repository::AgentRepository;
    use crate::agent::runtime::AgentRuntime;

    async fn planner() -> (Planner, SqlitePool) {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        for migration in crate::db::migrations() {
            sqlx::raw_sql(migration.sql).execute(&pool).await.unwrap();
        }
        let runtime = AgentRuntime::new(
            AgentRepository::new(pool.clone()),
            AgentExecutor::new(pool.clone()),
        );
        (Planner::new(pool.clone(), runtime), pool)
    }

    #[tokio::test]
    async fn tool_offering_defaults_to_shadow_reads_only() {
        // Migration v5 seeds plan.get_today=shadow, record.checkin_plan=typescript.
        let (planner, _pool) = planner().await;
        let offering = planner.tool_offering().await.unwrap();
        assert_eq!(offering.len(), 1);
        assert_eq!(offering[0]["function"]["name"], "plan.get_today");
        assert_eq!(offering[0]["function"]["parameters"]["type"], "object");
        // R0 read tool carries no idempotency note.
        let description = offering[0]["function"]["description"].as_str().unwrap();
        assert!(!description.contains("idempotency key is supplied"));
    }

    #[tokio::test]
    async fn rust_owned_r1_tool_is_offered_with_idempotency_note() {
        let (planner, pool) = planner().await;
        sqlx::query("UPDATE settings SET value='rust-owned' WHERE key='agent_tool_owner.record.checkin_plan'")
            .execute(&pool)
            .await
            .unwrap();
        let offering = planner.tool_offering().await.unwrap();
        let checkin = offering
            .iter()
            .find(|t| t["function"]["name"] == "record.checkin_plan")
            .expect("rust-owned checkin must be offered");
        let description = checkin["function"]["description"].as_str().unwrap();
        assert!(description.contains("idempotency key is supplied automatically"));
    }

    #[tokio::test]
    async fn max_iterations_defaults_to_six_and_reads_setting() {
        let (planner, pool) = planner().await;
        assert_eq!(planner.max_iterations().await.unwrap(), 6);
        sqlx::query("INSERT INTO settings(key,value) VALUES('agent_planner_max_iterations','3')")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(planner.max_iterations().await.unwrap(), 3);
    }
}
