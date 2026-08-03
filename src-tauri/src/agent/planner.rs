// Rust Planner (M3 Part 1/3): drives the model -> tool loop over AgentRuntime.
// The Planner is the only component that calls the provider and routes each
// returned tool_call through AgentRuntime::execute_tool. It never dispatches a
// tool itself (the executor's locked invariant) and records one
// agent_context_audit row per model call via the Context Builder.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::agent::context::{ContextAudit, ContextScope};
use crate::agent::error::AgentError;
use crate::agent::llm::tool_object;
use crate::agent::llm::{LlmProvider, ProviderMessage, ProviderResponse, ProviderUsage};
use crate::agent::memory::MemoryRepository;
use crate::agent::model::{ToolCallRequest, ToolCallResponse};
use crate::agent::runtime::AgentRuntime;
use crate::agent::tools::{Idempotency, ListedTool, RiskLevel, ToolOwnership};

const DEFAULT_MAX_ITERATIONS: i64 = 6;
const DEFAULT_TOKEN_BUDGET: i64 = 20000;
/// How many confirmed memories the Planner offers per run (spec §10.2:
/// "Runtime 按考试、类型、状态和最近使用时间选择少量相关记录").
const DEFAULT_MEMORY_LIMIT: i64 = 5;
const SYSTEM_PROMPT: &str = "你是智研的学习顾问助手。利用提供的工具回答用户目标；获取到信息后给出不含 tool_calls 的最终答复，使用中文。";

#[derive(Clone)]
pub struct Planner {
    pool: SqlitePool,
    runtime: AgentRuntime,
    context: ContextAudit,
    memory: MemoryRepository,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerTurn {
    pub mode: String, // "model" | "local"
    pub final_text: String,
    pub iterations: i64,
    pub model_calls: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    /// USD estimate from the settings rates (defaults 0.002/0.006 per 1k).
    pub estimated_cost_usd: f64,
    pub trace: Vec<TraceEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TraceEntry {
    ToolCalled {
        name: String,
        step_id: String,
        replayed: bool,
    },
    ToolWaitingApproval {
        name: String,
        approval_id: String,
    },
    ToolNavigationRequired {
        name: String,
        route: String,
    },
    ToolSummaryRequired {
        name: String,
    },
    MaxIterations,
    LocalFallback {
        reason: String,
    },
}

/// Accumulated per-loop accounting so the loop and the local-fallback helper
/// pass a single value instead of five counters (keeps clippy honest).
#[derive(Default)]
struct LoopAccumulator {
    iterations: i64,
    model_calls: i64,
    audit_seq: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    trace: Vec<TraceEntry>,
}

impl LoopAccumulator {
    fn into_turn(self, mode: &str, final_text: String, rates: (f64, f64)) -> PlannerTurn {
        let estimated_cost_usd = self.prompt_tokens as f64 / 1000.0 * rates.0
            + self.completion_tokens as f64 / 1000.0 * rates.1;
        PlannerTurn {
            mode: mode.to_owned(),
            final_text,
            iterations: self.iterations,
            model_calls: self.model_calls,
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens,
            estimated_cost_usd,
            trace: self.trace,
        }
    }
}

impl Planner {
    pub fn new(pool: SqlitePool, runtime: AgentRuntime, memory: MemoryRepository) -> Self {
        let context = ContextAudit::new(pool.clone());
        Self {
            pool,
            runtime,
            context,
            memory,
        }
    }

    /// Project the registry into OpenAI tool objects, offering only tools the
    /// Rust runtime may execute (Shadow reads or RustOwned writes). Typescript
    /// and Unavailable tools are hidden from the model.
    pub async fn tool_offering(&self) -> Result<Vec<Value>, AgentError> {
        let offered = self.offered_listed().await?;
        Ok(offered.iter().map(project_tool).collect())
    }

    async fn offered_listed(&self) -> Result<Vec<ListedTool>, AgentError> {
        let listed = self.runtime.list_tools().await?;
        Ok(listed
            .into_iter()
            .filter(|tool| {
                matches!(
                    tool.ownership,
                    ToolOwnership::Shadow | ToolOwnership::RustOwned
                )
            })
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

    /// Drive the model -> tool loop. The provider is supplied per call (its
    /// config is read from settings at the command boundary); the loop feeds
    /// each tool result back until the model stops calling tools, a tool needs
    /// user approval/navigation, or the iteration cap is hit. When no provider
    /// is configured, the provider fails terminally, or the soft token budget
    /// is exhausted before a call, the Planner returns a local-mode turn that
    /// performs no successful model call and is explicitly marked `local`.
    pub(crate) async fn run(
        &self,
        provider: Option<&LlmProvider>,
        run_id: &str,
        goal: &str,
        on_chunk: &mut (dyn FnMut(&str) + Send),
    ) -> Result<PlannerTurn, AgentError> {
        let turn = self.run_inner(provider, run_id, goal, on_chunk).await?;
        self.record_messages(run_id, goal, &turn).await?;
        Ok(turn)
    }

    async fn run_inner(
        &self,
        provider: Option<&LlmProvider>,
        run_id: &str,
        goal: &str,
        on_chunk: &mut (dyn FnMut(&str) + Send),
    ) -> Result<PlannerTurn, AgentError> {
        let Some(provider) = provider else {
            return self
                .local_turn(
                    run_id,
                    "no llm provider configured",
                    LoopAccumulator::default(),
                )
                .await;
        };
        let offered = self.offered_listed().await?;
        let mut by_name: BTreeMap<&'static str, &ListedTool> = BTreeMap::new();
        let mut tools: Vec<Value> = Vec::with_capacity(offered.len());
        let mut tools_offered: Vec<&'static str> = Vec::with_capacity(offered.len());
        for tool in &offered {
            by_name.insert(tool.descriptor.name, tool);
            tools.push(project_tool(tool));
            tools_offered.push(tool.descriptor.name);
        }
        let mut scope = self.context.gather(run_id).await?;

        // Confirmed long-term memories (spec §10.2) become part of the system
        // context; exam-scoped memories first, then global ones, ordered by
        // last use. Only confirmed memories are offered; candidates and
        // inactive ones stay out until the user confirms or reactivates them.
        let memories = self
            .memory
            .relevant(scope.exam_id.as_deref(), DEFAULT_MEMORY_LIMIT)
            .await?;
        scope.memory_ids = memories.iter().map(|entry| entry.id.clone()).collect();
        let mut system_content = SYSTEM_PROMPT.to_owned();
        if !memories.is_empty() {
            let lines: Vec<String> = memories
                .iter()
                .map(|entry| format!("- [{}] {}", entry.memory_type.as_str(), entry.content))
                .collect();
            system_content
                .push_str("\n\n用户已确认的长期记忆（仅作参考，冲突时以用户最新指令为准）：\n");
            system_content.push_str(&lines.join("\n"));
            for entry in &memories {
                self.memory.touch(&entry.id).await?;
            }
        }

        let mut messages = vec![
            ProviderMessage {
                role: "system".into(),
                content: Some(system_content),
                tool_calls: None,
                tool_call_id: None,
            },
            ProviderMessage {
                role: "user".into(),
                content: Some(goal.to_owned()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let max_iterations = self.max_iterations().await?;
        let budget = self.token_budget().await?;
        let rates = self.cost_rates().await?;
        let mut step_index = 0_i64;
        let mut acc = LoopAccumulator::default();

        loop {
            if acc.iterations >= max_iterations {
                acc.trace.push(TraceEntry::MaxIterations);
                return Err(AgentError::MaxIterations);
            }
            if budget > 0 && acc.prompt_tokens + acc.completion_tokens >= budget {
                return self.local_turn(run_id, "token budget exhausted", acc).await;
            }
            let response = match provider.chat_stream(&messages, &tools, on_chunk).await {
                Ok(response) => response,
                Err(AgentError::ProviderRequestFailed | AgentError::ProviderUnavailable) => {
                    return self
                        .local_turn(run_id, "llm provider unavailable", acc)
                        .await;
                }
                Err(error) => return Err(error),
            };
            acc.model_calls += 1;
            acc.audit_seq += 1;
            acc.prompt_tokens += response.usage.prompt_tokens;
            acc.completion_tokens += response.usage.completion_tokens;
            self.context
                .record(
                    run_id,
                    acc.audit_seq,
                    &scope,
                    &response.usage,
                    false,
                    &tools_offered,
                )
                .await?;

            // Echo the assistant turn (with tool_calls) so the conversation stays
            // well-formed before the tool-result messages.
            let assistant_tool_calls = if response.tool_calls.is_empty() {
                None
            } else {
                Some(response.tool_calls.clone())
            };
            messages.push(ProviderMessage {
                role: "assistant".into(),
                content: response.content.clone(),
                tool_calls: assistant_tool_calls,
                tool_call_id: None,
            });

            if response.tool_calls.is_empty() {
                return Ok(acc.into_turn("model", response.content.unwrap_or_default(), rates));
            }
            acc.iterations += 1;

            for call in &response.tool_calls {
                let Some(entry) = by_name.get(call.function.name.as_str()).copied() else {
                    // Unknown tool: tell the model and let it recover.
                    messages.push(tool_message(
                        &call.id,
                        json!({ "error": "unknown tool" }).to_string(),
                    ));
                    continue;
                };
                let input: Value =
                    serde_json::from_str(&call.function.arguments).unwrap_or_else(|_| json!({}));
                let idempotency_key = if matches!(
                    entry.descriptor.idempotency,
                    Idempotency::RequiredExactlyOnce
                ) {
                    Some(format!("planner/{run_id}/{step_index}"))
                } else {
                    None
                };
                let request = ToolCallRequest {
                    run_id: run_id.to_owned(),
                    step_index,
                    tool_name: call.function.name.clone(),
                    tool_version: entry.descriptor.version.to_owned(),
                    input,
                    idempotency_key,
                    approval_id: None,
                };
                let tool_response = self.runtime.execute_tool(request).await?;
                match tool_response {
                    ToolCallResponse::Completed {
                        output,
                        step_id,
                        replayed,
                        ..
                    } => {
                        acc.trace.push(TraceEntry::ToolCalled {
                            name: call.function.name.clone(),
                            step_id,
                            replayed,
                        });
                        messages.push(tool_message(&call.id, output.to_string()));
                        step_index += 1;
                    }
                    ToolCallResponse::WaitingApproval { approval_id, .. } => {
                        acc.trace.push(TraceEntry::ToolWaitingApproval {
                            name: call.function.name.clone(),
                            approval_id,
                        });
                        return Ok(acc.into_turn(
                            "model",
                            response.content.unwrap_or_default(),
                            rates,
                        ));
                    }
                    ToolCallResponse::NavigationRequired { route, .. } => {
                        acc.trace.push(TraceEntry::ToolNavigationRequired {
                            name: call.function.name.clone(),
                            route,
                        });
                        return Ok(acc.into_turn(
                            "model",
                            response.content.unwrap_or_default(),
                            rates,
                        ));
                    }
                    ToolCallResponse::SummaryRequired { .. } => {
                        acc.trace.push(TraceEntry::ToolSummaryRequired {
                            name: call.function.name.clone(),
                        });
                        return Ok(acc.into_turn(
                            "model",
                            response.content.unwrap_or_default(),
                            rates,
                        ));
                    }
                }
            }
        }
    }

    /// Soft token budget. Defaults to 20000; `0` or negative means unlimited.
    pub async fn token_budget(&self) -> Result<i64, AgentError> {
        let value: Option<String> = sqlx::query_scalar(
            "SELECT value FROM settings WHERE key = 'agent_planner_token_budget'",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx)?;
        Ok(value
            .and_then(|raw| raw.parse().ok())
            .filter(|parsed: &i64| *parsed > 0)
            .unwrap_or(DEFAULT_TOKEN_BUDGET))
    }

    /// Per-1k-token USD rates used for `estimated_cost_usd`, from settings
    /// (`agent_cost_per_1k_prompt_tokens`, `agent_cost_per_1k_completion_tokens`)
    /// with conservative public-cloud defaults (0.002 / 0.006).
    pub(crate) async fn cost_rates(&self) -> Result<(f64, f64), AgentError> {
        async fn rate(pool: &SqlitePool, key: &str, default: f64) -> Result<f64, AgentError> {
            let value: Option<String> =
                sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
                    .bind(key)
                    .fetch_optional(pool)
                    .await
                    .map_err(map_sqlx)?;
            Ok(value
                .and_then(|raw| raw.parse::<f64>().ok())
                .filter(|rate| *rate >= 0.0)
                .unwrap_or(default))
        }
        Ok((
            rate(&self.pool, "agent_cost_per_1k_prompt_tokens", 0.002).await?,
            rate(&self.pool, "agent_cost_per_1k_completion_tokens", 0.006).await?,
        ))
    }

    /// Persist a turn as user + assistant messages (M5 conversation). Local
    /// turns record zero tokens. Runs without a session (or a missing run row)
    /// are skipped safely.
    async fn record_messages(
        &self,
        run_id: &str,
        goal: &str,
        turn: &PlannerTurn,
    ) -> Result<(), AgentError> {
        let session_id: Option<String> =
            sqlx::query_scalar("SELECT session_id FROM agent_runs WHERE id = ?")
                .bind(run_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(map_sqlx)?;
        let Some(session_id) = session_id else {
            return Ok(());
        };
        let mut tx = self.pool.begin().await.map_err(map_sqlx)?;
        let user_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO agent_messages (id, session_id, run_id, role, text) \
             VALUES (?, ?, ?, 'user', ?)",
        )
        .bind(&user_id)
        .bind(&session_id)
        .bind(run_id)
        .bind(goal)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let assistant_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO agent_messages (id, session_id, run_id, role, text, \
             prompt_tokens, completion_tokens) \
             VALUES (?, ?, ?, 'assistant', ?, ?, ?)",
        )
        .bind(&assistant_id)
        .bind(&session_id)
        .bind(run_id)
        .bind(&turn.final_text)
        .bind(turn.prompt_tokens)
        .bind(turn.completion_tokens)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        tx.commit().await.map_err(map_sqlx)?;
        Ok(())
    }

    /// Read the LLM config from settings + keyring. Returns None when no
    /// provider is configured, the key is absent, or the provider is Ollama
    /// (Ollama has no tool-calling support and degrades to local mode).
    pub(crate) async fn build_provider(&self) -> Result<Option<LlmProvider>, AgentError> {
        Self::build_provider_from(&self.pool).await
    }

    /// Provider construction shared by the Planner loop, the brief job, and
    /// any other M4 background handler that may call the LLM.
    pub(crate) async fn build_provider_from(
        pool: &SqlitePool,
    ) -> Result<Option<LlmProvider>, AgentError> {
        let provider: Option<String> =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'llm_provider'")
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx)?;
        let Some(provider) = provider.filter(|p| !p.trim().is_empty()) else {
            return Ok(None);
        };
        if provider == "ollama" {
            return Ok(None);
        }
        let base_url: String =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'llm_base_url'")
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx)?
                .unwrap_or_default();
        let model: String =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'llm_model'")
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx)?
                .unwrap_or_default();
        if base_url.trim().is_empty() || model.trim().is_empty() {
            return Ok(None);
        }
        let temperature: f32 =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'llm_temperature'")
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx)?
                .and_then(|raw: String| raw.parse().ok())
                .unwrap_or(0.7);
        let Some(api_key) =
            crate::api_key_for(&provider).map_err(|_| AgentError::ProviderUnavailable)?
        else {
            return Ok(None);
        };
        Ok(Some(LlmProvider::OpenAiCompatible(
            crate::agent::llm::openai_compatible::OpenAiCompatibleProvider::new(
                base_url,
                model,
                api_key,
                temperature,
            ),
        )))
    }

    /// Produce a deterministic local-mode turn. Records one `agent_context_audit`
    /// row marked `local:true` (zero tokens, no scope); the turn carries whatever
    /// successful calls already happened so callers see honest usage. The text
    /// names the failure reason and never claims model output.
    async fn local_turn(
        &self,
        run_id: &str,
        reason: &str,
        mut acc: LoopAccumulator,
    ) -> Result<PlannerTurn, AgentError> {
        let response = ProviderResponse {
            content: Some(format!("（本地模式）{reason}，跳过模型推理。")),
            tool_calls: Vec::new(),
            usage: ProviderUsage::default(),
        };
        let rates = self.cost_rates().await?;
        acc.audit_seq += 1;
        self.context
            .record(
                run_id,
                acc.audit_seq,
                &ContextScope::default(),
                &response.usage,
                true,
                &[],
            )
            .await?;
        acc.trace.push(TraceEntry::LocalFallback {
            reason: reason.to_owned(),
        });
        Ok(acc.into_turn("local", response.content.unwrap_or_default(), rates))
    }
}

fn tool_message(tool_call_id: &str, content: String) -> ProviderMessage {
    ProviderMessage {
        role: "tool".into(),
        content: Some(content),
        tool_calls: None,
        tool_call_id: Some(tool_call_id.to_owned()),
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
    use crate::agent::llm::{LlmProvider, ProviderResponse, ProviderUsage, SyntheticProvider};
    use crate::agent::memory::{MemoryRepository, MemorySource, MemoryType};
    use crate::agent::model::RunEvent;
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
        let memory = MemoryRepository::new(pool.clone());
        (Planner::new(pool.clone(), runtime, memory), pool)
    }

    async fn started_run(pool: &SqlitePool, planner: &Planner) -> String {
        let session = planner.runtime.create_session(None, "loop").await.unwrap();
        let run = planner
            .runtime
            .create_run(&session.id, "inspect today")
            .await
            .unwrap();
        planner
            .runtime
            .transition_run(&run.id, RunEvent::Start)
            .await
            .unwrap();
        // Seed an exam + plan so plan.get_today returns one row through the real tool.
        sqlx::raw_sql(
            r#"
            INSERT INTO exams(id,name,exam_date) VALUES('exam-loop','Loop','2030-01-01');
            INSERT INTO subjects(id,exam_id,name) VALUES('subject-loop','exam-loop','Loop');
            INSERT INTO study_plans(id,exam_id,subject_id,date,planned_tasks,planned_duration,status,generated_by,sort_order)
                VALUES('plan-loop','exam-loop','subject-loop','1970-01-01','复习','30','pending','local',0);
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
        run.id
    }

    fn call_tool(id: &str, name: &str, arguments: &str) -> crate::agent::llm::ProviderToolCall {
        crate::agent::llm::ProviderToolCall {
            id: id.into(),
            kind: "function".into(),
            function: crate::agent::llm::ProviderFunction {
                name: name.into(),
                arguments: arguments.into(),
            },
        }
    }

    #[tokio::test]
    async fn tool_offering_defaults_to_shadow_reads_only() {
        let (planner, _pool) = planner().await;
        let offering = planner.tool_offering().await.unwrap();
        assert_eq!(offering.len(), 1);
        assert_eq!(offering[0]["function"]["name"], "plan.get_today");
        assert_eq!(offering[0]["function"]["parameters"]["type"], "object");
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

    #[tokio::test]
    async fn loop_executes_tool_then_stops_on_final_answer() {
        let (planner, pool) = planner().await;
        let run_id = started_run(&pool, &planner).await;

        // Scripted provider: first turn calls plan.get_today, second turn answers.
        let provider = LlmProvider::Synthetic(SyntheticProvider::scripted(vec![
            ProviderResponse {
                content: Some("我先查一下今日计划。".into()),
                tool_calls: vec![call_tool(
                    "call-1",
                    "plan.get_today",
                    "{\"exam_id\":\"exam-loop\"}",
                )],
                usage: ProviderUsage {
                    prompt_tokens: 100,
                    completion_tokens: 5,
                },
            },
            ProviderResponse {
                content: Some("今日有一项复习任务。".into()),
                tool_calls: Vec::new(),
                usage: ProviderUsage {
                    prompt_tokens: 200,
                    completion_tokens: 10,
                },
            },
        ]));

        let mut chunks: Vec<String> = Vec::new();
        let turn = planner
            .run(
                Some(&provider),
                &run_id,
                "看今天的计划",
                &mut |chunk| chunks.push(chunk.to_owned()),
            )
            .await
            .unwrap();

        assert_eq!(turn.mode, "model");
        // Streaming: both assistant turns' content were forwarded as chunks.
        assert!(chunks.iter().any(|c| c.contains("我先查一下今日计划。")));
        assert!(chunks.iter().any(|c| c.contains("今日有一项复习任务。")));
        assert_eq!(turn.final_text, "今日有一项复习任务。");
        assert_eq!(turn.iterations, 1);
        assert_eq!(turn.model_calls, 2);
        assert_eq!(turn.prompt_tokens, 300);
        assert_eq!(turn.completion_tokens, 15);
        // Default rates 0.002/0.006 per 1k: 300/1000*0.002 + 15/1000*0.006.
        assert!((turn.estimated_cost_usd - 0.00069).abs() < 1e-9);
        assert_eq!(turn.trace.len(), 1);
        assert!(
            matches!(turn.trace[0], TraceEntry::ToolCalled { ref name, .. } if name == "plan.get_today")
        );

        // The tool ran exactly once through the executor (one completed step).
        let completed_steps: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_steps WHERE run_id=? AND status='completed'",
        )
        .bind(&run_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(completed_steps, 1);
        // One agent_context_audit row per provider call (local:false).
        let audit_rows: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_context_audit WHERE run_id=?")
                .bind(&run_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(audit_rows, 2);
        let local_rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_context_audit WHERE run_id=? AND local=1",
        )
        .bind(&run_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(local_rows, 0);
    }

    #[tokio::test]
    async fn loop_hits_max_iterations_when_model_never_stops() {
        let (planner, pool) = planner().await;
        let run_id = started_run(&pool, &planner).await;
        sqlx::query("INSERT INTO settings(key,value) VALUES('agent_planner_max_iterations','2')")
            .execute(&pool)
            .await
            .unwrap();

        // Every turn calls the same read tool; the model never answers.
        let repeating = ProviderResponse {
            content: Some("再查一次。".into()),
            tool_calls: vec![call_tool(
                "call-1",
                "plan.get_today",
                "{\"exam_id\":\"exam-loop\"}",
            )],
            usage: ProviderUsage::default(),
        };
        let provider = LlmProvider::Synthetic(SyntheticProvider::scripted(vec![
            repeating.clone(),
            repeating.clone(),
            repeating,
        ]));

        let error = planner
            .run(Some(&provider), &run_id, "不停查", &mut |_| {})
            .await
            .unwrap_err();
        assert_eq!(error.code(), "max_iterations");
    }

    #[tokio::test]
    async fn returns_local_turn_when_no_provider_configured() {
        let (planner, pool) = planner().await;
        let run_id = started_run(&pool, &planner).await;

        let turn = planner
            .run(None, &run_id, "看今天的计划", &mut |_| {})
            .await
            .unwrap();

        assert_eq!(turn.mode, "local");
        assert_eq!(turn.model_calls, 0);
        assert!(turn.final_text.contains("本地模式"));
        assert!(turn.final_text.contains("no llm provider configured"));
        assert!(matches!(
            turn.trace.first(),
            Some(TraceEntry::LocalFallback { .. })
        ));
        // One local agent_context_audit row, zero tokens.
        let local_events: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_context_audit WHERE run_id=? AND local=1",
        )
        .bind(&run_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(local_events, 1);
    }

    #[tokio::test]
    async fn returns_local_turn_when_terminal_provider_error() {
        let (planner, pool) = planner().await;
        let run_id = started_run(&pool, &planner).await;

        // Real provider against a 401 mock -> terminal ProviderRequestFailed -> local.
        let server = httpmock::MockServer::start();
        server.mock(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions");
            then.status(401).body("unauthorized");
        });
        let provider = LlmProvider::OpenAiCompatible(
            crate::agent::llm::openai_compatible::OpenAiCompatibleProvider::new(
                server.base_url(),
                "test-model".into(),
                "sk-test".into(),
                0.2,
            )
            .with_retry_delay(std::time::Duration::from_millis(5)),
        );

        let turn = planner
            .run(Some(&provider), &run_id, "看今天的计划", &mut |_| {})
            .await
            .unwrap();

        assert_eq!(turn.mode, "local");
        assert_eq!(turn.model_calls, 0);
        assert!(turn.final_text.contains("llm provider unavailable"));
        let local_events: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_context_audit WHERE run_id=? AND local=1",
        )
        .bind(&run_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(local_events, 1);
    }

    #[tokio::test]
    async fn returns_local_turn_when_token_budget_exhausted_mid_loop() {
        let (planner, pool) = planner().await;
        let run_id = started_run(&pool, &planner).await;
        // Budget so low the first 100-token call exhausts it before the second call.
        sqlx::query("INSERT INTO settings(key,value) VALUES('agent_planner_token_budget','1')")
            .execute(&pool)
            .await
            .unwrap();

        let provider =
            LlmProvider::Synthetic(SyntheticProvider::scripted(vec![ProviderResponse {
                content: Some("查一下。".into()),
                tool_calls: vec![call_tool(
                    "call-1",
                    "plan.get_today",
                    "{\"exam_id\":\"exam-loop\"}",
                )],
                usage: ProviderUsage {
                    prompt_tokens: 100,
                    completion_tokens: 0,
                },
            }]));

        let turn = planner
            .run(Some(&provider), &run_id, "看今天的计划", &mut |_| {})
            .await
            .unwrap();

        assert_eq!(turn.mode, "local");
        assert_eq!(turn.model_calls, 1);
        assert!(turn.final_text.contains("token budget exhausted"));
        // One non-local audit row (the call that ran) + one local row (the fallback).
        let non_local: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_context_audit WHERE run_id=? AND local=0",
        )
        .bind(&run_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let local: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_context_audit WHERE run_id=? AND local=1",
        )
        .bind(&run_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(non_local, 1);
        assert_eq!(local, 1);
    }

    #[tokio::test]
    async fn confirmed_memories_are_offered_in_system_prompt_and_audited() {
        let (planner, pool) = planner().await;
        let run_id = started_run(&pool, &planner).await;

        // One confirmed memory bound to the run's exam, one candidate, one inactive.
        let memory = MemoryRepository::new(pool.clone());
        let confirmed = memory
            .create(
                Some("exam-loop"),
                MemoryType::DailyCapacity,
                "每天最多学习两小时",
                MemorySource::UserStatement,
                1.0,
            )
            .await
            .unwrap();
        let candidate = memory
            .create(
                None,
                MemoryType::SubjectPreference,
                "晚上更常做数学",
                MemorySource::BehaviorInferred,
                0.6,
            )
            .await
            .unwrap();
        let inactive = memory
            .create(
                None,
                MemoryType::ReminderPreference,
                "旧的提醒偏好",
                MemorySource::UserStatement,
                1.0,
            )
            .await
            .unwrap();
        memory.deactivate(&inactive.id).await.unwrap();

        let provider =
            LlmProvider::Synthetic(SyntheticProvider::scripted(vec![ProviderResponse {
                content: Some("好的，我了解了。".into()),
                tool_calls: Vec::new(),
                usage: ProviderUsage {
                    prompt_tokens: 10,
                    completion_tokens: 3,
                },
            }]));

        let turn = planner
            .run(Some(&provider), &run_id, "看今天的计划", &mut |_| {})
            .await
            .unwrap();
        assert_eq!(turn.mode, "model");

        // The system prompt offers only the confirmed memory.
        let request = match &provider {
            LlmProvider::Synthetic(synthetic) => synthetic.last_request().unwrap(),
            _ => unreachable!(),
        };
        let system = request[0].content.as_deref().unwrap();
        assert!(system.contains("每天最多学习两小时"));
        assert!(!system.contains("晚上更常做数学"));
        assert!(!system.contains("旧的提醒偏好"));

        // The offered memory was marked as used.
        let last_used: Option<String> =
            sqlx::query_scalar("SELECT last_used_at FROM agent_memories WHERE id=?")
                .bind(&confirmed.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(last_used.is_some());

        // The audit row records the memory category and id, never its content,
        // and only for the confirmed memory actually offered.
        let (categories, record_ids): (String, String) = sqlx::query_as(
            "SELECT categories_json, record_ids_json FROM agent_context_audit \
             WHERE run_id=? ORDER BY call_seq LIMIT 1",
        )
        .bind(&run_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(categories.contains("memory"));
        assert!(record_ids.contains(&confirmed.id));
        assert!(!record_ids.contains(&candidate.id));
        assert!(!record_ids.contains("每天最多学习两小时"));
    }

    #[tokio::test]
    async fn run_persists_user_and_assistant_messages() {
        let (planner, pool) = planner().await;
        let run_id = started_run(&pool, &planner).await;

        let provider =
            LlmProvider::Synthetic(SyntheticProvider::scripted(vec![ProviderResponse {
                content: Some("今日有一项复习任务。".into()),
                tool_calls: Vec::new(),
                usage: ProviderUsage {
                    prompt_tokens: 40,
                    completion_tokens: 5,
                },
            }]));

        let turn = planner
            .run(Some(&provider), &run_id, "看今天的计划", &mut |_| {})
            .await
            .unwrap();
        assert_eq!(turn.mode, "model");

        let messages: Vec<(String, String, i64, i64)> = sqlx::query_as(
            "SELECT role, text, prompt_tokens, completion_tokens \
             FROM agent_messages WHERE run_id = ? ORDER BY created_at, rowid",
        )
        .bind(&run_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].0, "user");
        assert_eq!(messages[0].1, "看今天的计划");
        assert_eq!(messages[0].2, 0);
        assert_eq!(messages[1].0, "assistant");
        assert_eq!(messages[1].1, "今日有一项复习任务。");
        assert_eq!(messages[1].2, 40);
        assert_eq!(messages[1].3, 5);
    }

    #[tokio::test]
    async fn local_turn_persists_messages_with_zero_tokens() {
        let (planner, pool) = planner().await;
        let run_id = started_run(&pool, &planner).await;

        let turn = planner
            .run(None, &run_id, "看今天的计划", &mut |_| {})
            .await
            .unwrap();
        assert_eq!(turn.mode, "local");

        let messages: Vec<(String, i64, i64)> = sqlx::query_as(
            "SELECT role, prompt_tokens, completion_tokens \
             FROM agent_messages WHERE run_id = ?",
        )
        .bind(&run_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].0, "user");
        assert_eq!(messages[1].0, "assistant");
        assert_eq!(messages[1].1, 0);
        assert_eq!(messages[1].2, 0);
    }
}
