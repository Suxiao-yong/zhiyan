# Agent Model, Tool Loop, and Local Fallback — M3 Part 1

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the Rust runtime drive the model → tool loop over the two existing M2 tools (`plan.get_today`, `record.checkin_plan`), record per-call usage audit, and degrade to a local-mode turn when no LLM is configured — reachable only through the existing hidden `/agent-debug` contract. No production UI change, no ownership cutover, no new business write path.

**Why a slice, not the whole milestone:** The roadmap lists M3 as one milestone (model adapters, streaming tool loop, context audit, structured memory, local fallback). Mirroring how M2 shipped only the first two tools and deferred the rest to M6, this is **M3 Part 1**: the non-streaming OpenAI-compatible provider, the tool-call loop, usage audit, and the fallback gate. Parts 2–N (streaming `stream.rs`, the dedicated `agent_context_audit` table + Context Inspector UI, structured memory with `agent_memories` and the seven memory types, and the full Fallback Engine for overdue/stats/weakness/notifications) follow as separate sub-plans. **Ollama tool support is excluded by product decision — the product ships cloud LLMs only for the agent loop**; Ollama stays on the plain-chat TypeScript path and degrades to local mode in the Rust planner.

**Architecture:** A Rust `LlmProvider` trait abstracts the model call. `OpenAiCompatibleProvider` implements it with `reqwest`, mirroring the TypeScript `callLLMWithTools` request/response shape so existing user provider configs (DeepSeek/OpenAI/通义/Kimi/自定义) keep working without reconfiguration. A `Planner` owns the loop: it projects `ToolDescriptor`s into OpenAI tool schemas, calls the provider, routes each returned `tool_call` through `AgentRuntime::execute_tool` (the existing M2 boundary — the Planner never dispatches a tool itself), feeds each `ToolCallResponse` back as a `tool`-role message, and repeats until the model stops calling tools or a hard iteration cap is hit. Each provider call appends one `model.invoked` audit event to the existing `agent_events` table with token usage and the data-permission categories of the tools offered; no new table is introduced in this part. When the keyring has no API key, the provider returns a terminal error, or the run's soft token budget is exhausted, the Planner returns a deterministic local-mode turn that performs no model call and is explicitly marked `local` — never disguised as model output.

**Tech Stack:** Rust 2021, Tauri 2, reqwest 0.12 (rustls-tls), tokio, serde/serde_json, sqlx SQLite/WAL, Vue 3, TypeScript, Vitest, httpmock (dev only).

**Design source:** `docs/superpowers/specs/2026-07-17-rust-agent-os-redesign.md` §10 (Model & context), §12 (Local fallback).

**Milestone dependency:** Milestone 2 commit `c210f42` or a descendant containing migration v5, `AgentExecutor`, `AgentRuntime::execute_tool`, `ToolCallResponse`, and the two built-in tools.

---

## File map

### Rust provider, planner, and fallback

- Create `src-tauri/src/agent/llm/mod.rs`: `LlmProvider` trait, request/response DTOs, tool-schema projection, and the `ollama_unsupported_for_tools` guard.
- Create `src-tauri/src/agent/llm/openai_compatible.rs`: `OpenAiCompatibleProvider` with reqwest, retry/backoff, timeout, and redacted errors.
- Create `src-tauri/src/agent/planner.rs`: the tool-call loop over `AgentRuntime`, audit event emission, iteration cap, and local fallback.
- Modify `src-tauri/src/agent/error.rs`: add `ProviderUnavailable`, `ProviderRequestFailed`, `BudgetExhausted`, `MaxIterations` variants and codes.
- Modify `src-tauri/src/agent/commands.rs`: redact the new provider errors at the command boundary.
- Modify `src-tauri/src/agent/mod.rs`: export `llm` and `planner`.
- Modify `src-tauri/src/credentials.rs`: add a Rust-callable `api_key_for(provider)` helper reused by the provider.
- Modify `src-tauri/src/lib.rs`: construct and manage the planner, register the hidden command.
- Create `src-tauri/tests/agent_planner.rs`: provider contract tests against httpmock, the scripted-loop integration test, privacy redaction, and the offline fallback test.

### Frontend contract (hidden only)

- Modify `src/services/agent-client.ts`: typed `agentRunPlanner(goal, examId?)` invoke.
- Modify `src/services/agent-client.test.ts`: command-name and camelCase argument contract.
- Modify `src/pages/AgentDebug.vue` + `src/pages/AgentDebug.test.ts`: read-only "run planner turn" control that renders the trace and usage; no production navigation change.

### Operations and milestone evidence

- Modify `docs/agent/feature-parity.md`: add a `Model adapter and tool loop` row marked `rust-owned` for the runtime slice and note the deferred M3 parts.
- Modify `docs/agent/migration-runbook.md`: M3 Part 1 adds no migration; record the audit-event approach, the provider error redaction, and the rollback note.

## Locked behavioral contract

- The provider request to an OpenAI-compatible base URL is exactly `POST {baseUrl trim trailing slash}/chat/completions` with JSON body `{model, messages, temperature, stream:false, tools}` and header `Authorization: Bearer {apiKey}`. This matches `src/services/llm-adapter.ts::callLLMWithTools` so an existing user configuration works unchanged.
- The provider parses `data.choices[0].message` into `{content: Option<String>, tool_calls: Vec<{id, function:{name, arguments}}>}` and `data.usage` into `{prompt_tokens, completion_tokens}`. Missing fields default to empty/zero, never panic.
- Ollama (`provider == "ollama"`) is rejected with `ProviderUnavailable` for any call that supplies tools. This is a permanent exclusion by product decision (cloud LLMs only for the agent loop), not a deferred task; Ollama stays on the TypeScript plain-chat path and degrades to local mode here.
- HTTP 401/403 map to `ProviderRequestFailed` with a safe message ("API Key 无效或无权限") and are not retried. 429 and 5xx and network errors map to `ProviderRequestFailed` and retry up to 3 times with 1s/2s backoff. The command-boundary message never contains the base URL, API key, request body, or response body.
- The Planner projects each `ToolDescriptor` into an OpenAI tool object `{type:"function", function:{name, description, parameters: input_schema}}`. It offers only tools whose ownership is `Shadow` or `RustOwned` (`Unavailable`/`Typescript` are hidden from the model). R1 tools are offered with an idempotency note in the description; the Planner synthesizes the idempotency key for every R1 call.
- The Planner routes every model `tool_call` through `AgentRuntime::execute_tool(ToolCallRequest{run_id, step_index, tool_name, tool_version, input, idempotency_key, approval_id})`. It never imports or calls a tool function directly. `ToolCallResponse::Completed` is fed back as `{role:"tool", tool_call_id, content: serialized output}`. `WaitingApproval`, `SummaryRequired`, and `NavigationRequired` stop the loop and are surfaced in the turn trace; the model is told the tool is awaiting user action.
- The loop stops when the model returns no `tool_calls`, when `WaitingApproval`/`NavigationRequired` is returned, or after `agent_planner_max_iterations` (default 6) calls. Hitting the cap returns `MaxIterations`.
- Each provider call appends exactly one `model.invoked` event to `agent_events` with `{provider, model, prompt_tokens, completion_tokens, tools_offered:[names], data_permissions:[categories], local:false}`. A local-mode turn appends one event with `local:true` and zero tokens. No prompt text is stored.
- When the keyring has no API key for the configured provider, or the provider returns a terminal error, or the run's soft token budget (`agent_planner_token_budget`, default 20000; summed `prompt_tokens + completion_tokens`) is exhausted before a call, the Planner returns a `PlannerTurn { mode:"local", final_text, trace }` with no model call. The local text explicitly says it is local, never claims model reasoning.
- The hidden `agent_run_planner` command is the only new entry point. It requires a non-empty goal, opens/uses an existing `run_id` (the caller creates the run via the existing `agent_create_run` + `agent_start_run`), and returns the trace + usage. Production Vue flows are unchanged.

## Task 1: Dependencies, keyring helper, and provider error types

**Files:**

- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/credentials.rs`
- Modify: `src-tauri/src/agent/error.rs`
- Modify: `src-tauri/src/agent/commands.rs`

- [ ] **Step 1: Add the HTTP client and dev mock server dependencies**

Append to `[dependencies]`:

```toml
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json"] }
```

Append to `[dev-dependencies]` (create the section if absent):

```toml
httpmock = "0.6"
```

Run `cargo check --manifest-path src-tauri\Cargo.toml`. Expected: exit 0 and an updated `Cargo.lock`.

- [ ] **Step 2: Write the keyring helper test first**

Add to `credentials.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::api_key_for;

    #[test]
    fn missing_api_key_resolves_to_none_without_panicking() {
        let provider = format!("test-{}", uuid::Uuid::new_v4());
        assert_eq!(api_key_for(&provider).unwrap(), None);
    }
}
```

Run `cargo test --manifest-path src-tauri\Cargo.toml credentials::tests -- --nocapture`. Expected: compile failure because `api_key_for` does not exist.

- [ ] **Step 3: Implement the Rust-callable keyring helper**

Add above the Tauri commands:

```rust
/// Rust-callable API key lookup reused by the LLM provider. Returns `None` on
/// `NoEntry`; surfaces other keyring errors so the caller can degrade.
pub fn api_key_for(provider: &str) -> Result<Option<String>, keyring::Error> {
    let entry = Entry::new("zhiyan", provider)?;
    match entry.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e),
    }
}
```

Run the test. Expected: pass.

- [ ] **Step 4: Write failing provider-error tests**

Add error variants and the command-boundary redaction tests together. In `error.rs` add (after `OwnershipUnavailable`):

```rust
#[error("llm provider is unavailable")]
ProviderUnavailable,
#[error("llm provider request failed")]
ProviderRequestFailed,
#[error("llm token budget exhausted")]
BudgetExhausted,
#[error("planner reached the maximum tool iterations")]
MaxIterations,
```

and four arms in `code()`:

```rust
Self::ProviderUnavailable => "provider_unavailable",
Self::ProviderRequestFailed => "provider_request_failed",
Self::BudgetExhausted => "budget_exhausted",
Self::max_iterations => "max_iterations",
```

> Fix the lowercase variant name to `Self::MaxIterations => "max_iterations"` when transcribing.

Add to `commands.rs` tests:

```rust
#[test]
fn provider_errors_are_redacted_and_safe() {
    for (error, code, message) in [
        (AgentError::ProviderUnavailable, "provider_unavailable", "llm provider is unavailable"),
        (AgentError::ProviderRequestFailed, "provider_request_failed", "llm provider request failed"),
        (AgentError::BudgetExhausted, "budget_exhausted", "llm token budget exhausted"),
        (AgentError::MaxIterations, "max_iterations", "planner reached the maximum tool iterations"),
    ] {
        let cmd = CommandError::from(error);
        assert_eq!(cmd.code, code);
        assert_eq!(cmd.message, message);
    }
}
```

Extend the `From<AgentError>` redaction arm so every provider variant maps to its safe `to_string()` (none carry secrets, so the existing `other => other.to_string()` already covers them — verify with the test).

- [ ] **Step 5: Verify and commit**

```powershell
cargo fmt --manifest-path src-tauri\Cargo.toml -- --check
cargo test --manifest-path src-tauri\Cargo.toml -- --test-threads=1
cargo clippy --manifest-path src-tauri\Cargo.toml --all-targets --all-features -- -D warnings
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/credentials.rs src-tauri/src/agent/error.rs src-tauri/src/agent/commands.rs
git commit -m "feat: add llm provider dependencies and redacted error types"
```

Expected: tests pass, Clippy exits 0.

## Task 2: OpenAI-compatible provider with contract tests

**Files:**

- Create: `src-tauri/src/agent/llm/mod.rs`
- Create: `src-tauri/src/agent/llm/openai_compatible.rs`
- Modify: `src-tauri/src/agent/mod.rs`
- Create: `src-tauri/tests/agent_planner.rs`

- [ ] **Step 1: Define the provider trait and DTOs**

`agent/llm/mod.rs`:

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::error::AgentError;

pub mod openai_compatible;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMessage {
    pub role: String, // "system" | "user" | "assistant" | "tool"
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ProviderToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "tool_call_id")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderToolCall {
    pub id: String,
    pub function: ProviderFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderUsage {
    #[serde(default, rename = "prompt_tokens")]
    pub prompt_tokens: i64,
    #[serde(default, rename = "completion_tokens")]
    pub completion_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ProviderToolCall>,
    pub usage: ProviderUsage,
}

/// The model-facing abstraction. Real impl: reqwest. Test impls in the
/// integration test script model responses without a network.
pub trait LlmProvider {
    fn chat(
        &self,
        messages: &[ProviderMessage],
        tools: &[Value],
    ) -> impl std::future::Future<Output = Result<ProviderResponse, AgentError>> + Send;
}

/// Project a tool descriptor into an OpenAI function-calling tool object.
pub fn tool_object(name: &str, description: &str, parameters: &Value) -> Value {
    serde_json::json!({
        "type": "function",
        "function": { "name": name, "description": description, "parameters": parameters }
    })
}
```

Export `pub mod llm;` from `agent/mod.rs`.

- [ ] **Step 2: Write the contract test against httpmock**

In `tests/agent_planner.rs` (test-only `ScriptedProvider` is added in Task 4; this task covers the real provider):

```rust
use httpmock::MockServer;
use zhiyan_lib::agent::error::AgentError;
use zhiyan_lib::agent::llm::openai_compatible::OpenAiCompatibleProvider;
use zhiyan_lib::agent::llm::{LlmProvider, ProviderMessage};

fn provider(server: &MockServer) -> OpenAiCompatibleProvider {
    OpenAiCompatibleProvider::new(
        server.base_url(),
        "test-model".into(),
        "sk-test".into(),
        0.2,
    )
}

#[tokio::test]
async fn parses_content_tool_calls_and_usage_from_chat_completions() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(httpmock::Method::POST).path("/chat/completions");
        then.status(200).body_json(serde_json::json!({
            "choices": [{ "message": {
                "content": "今日计划如下",
                "tool_calls": [{ "id": "call_1", "function": { "name": "plan.get_today", "arguments": "{\"exam_id\":\"e1\"}" } }]
            }}],
            "usage": { "prompt_tokens": 120, "completion_tokens": 8 }
        }));
    });
    let resp = provider(&server)
        .chat(&[ProviderMessage { role: "user".into(), content: Some("看今天".into()), tool_calls: None, tool_call_id: None }], &[])
        .await
        .unwrap();
    assert_eq!(resp.content.as_deref(), Some("今日计划如下"));
    assert_eq!(resp.tool_calls[0].function.name, "plan.get_today");
    assert_eq!(resp.usage.prompt_tokens, 120);
    assert_eq!(resp.usage.completion_tokens, 8);
}
```

Run and observe RED (compile failure: module absent).

- [ ] **Step 3: Implement `OpenAiCompatibleProvider`**

`openai_compatible.rs` builds the request body identical to the TypeScript layer, POSTs with reqwest, parses `choices[0].message`, maps status codes to `AgentError::ProviderRequestFailed` with safe messages, retries 429/5xx/network up to 3 times with 1s/2s backoff, never retries 401/403. The base URL's trailing slash is trimmed. `provider == "ollama"` is handled by the caller (Task 4 fallback); the provider itself targets `/chat/completions` only. Errors carry no URL/key/body text.

```rust
use serde_json::Value;
use std::time::Duration;

use super::{LlmProvider, ProviderMessage, ProviderResponse, ProviderUsage};
use crate::agent::error::AgentError;

#[derive(Clone)]
pub struct OpenAiCompatibleProvider {
    base_url: String,
    model: String,
    api_key: String,
    temperature: f32,
}

impl OpenAiCompatibleProvider {
    pub fn new(base_url: String, model: String, api_key: String, temperature: f32) -> Self {
        Self { base_url: base_url.trim_end_matches('/').to_owned(), model, api_key, temperature }
    }
}

impl LlmProvider for OpenAiCompatibleProvider {
    async fn chat(&self, messages: &[ProviderMessage], tools: &[Value]) -> Result<ProviderResponse, AgentError> {
        // build body, loop with retry/backoff, map status, parse, redact.
        todo!("see locked contract; mirror llm-adapter.ts")
    }
}
```

- [ ] **Step 4: Add redaction and retry tests**

Assert: a 401 response maps to `ProviderRequestFailed` and is not retried (server mock hit once); a 500-then-200 sequence retries and succeeds (mock hit twice); the `AgentError::ProviderRequestFailed` string contains neither the base URL nor `sk-test`.

- [ ] **Step 5: Verify and commit**

```powershell
cargo fmt --manifest-path src-tauri\Cargo.toml -- --check
cargo test --manifest-path src-tauri\Cargo.toml --test agent_planner -- --test-threads=1
cargo clippy --manifest-path src-tauri\Cargo.toml --all-targets --all-features -- -D warnings
git add src-tauri/src/agent/mod.rs src-tauri/src/agent/llm src-tauri/tests/agent_planner.rs
git commit -m "feat: add openai-compatible rust llm provider"
```

## Task 3: Tool-list projection and iteration cap

**Files:**

- Modify: `src-tauri/src/agent/planner.rs`
- Modify: `src-tauri/tests/agent_planner.rs`

- [ ] **Step 1: Write the projection test**

Assert `Planner::tool_offering(&runtime)` returns only `Shadow`/`RustOwned` tools, each projected via `tool_object` with the descriptor's `input_schema` as `parameters`, and that an R1 tool's projected description carries an idempotency note.

- [ ] **Step 2: Implement projection + cap read**

`Planner::tool_offering` calls `runtime.list_tools()`, filters ownership, projects each descriptor with `llm::tool_object`, and appends an R1 idempotency note to the description. `max_iterations()` reads `agent_planner_max_iterations` (default 6) from settings.

- [ ] **Step 3: Verify and commit**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --test agent_planner -- --test-threads=1
git add src-tauri/src/agent/planner.rs src-tauri/tests/agent_planner.rs
git commit -m "feat: project agent tool registry into model tool list"
```

## Task 4: The tool-call loop over AgentRuntime with audit

**Files:**

- Modify: `src-tauri/src/agent/planner.rs`
- Modify: `src-tauri/tests/agent_planner.rs`

- [ ] **Step 1: Add a scripted test provider**

In the test file, a `ScriptedProvider` implementing `LlmProvider` returns a pre-seeded `Vec<ProviderResponse>` queue, popping one per call. This tests the loop without a network.

- [ ] **Step 2: Write the end-to-end loop test**

Seed a run + today's plan in an in-memory pool. Script the provider to return one `plan.get_today` tool call, then a no-tool final message. Assert: the planner executed exactly one tool through `AgentRuntime` (one completed `agent_steps` row), fed its output back, returned the final text, emitted exactly one `model.invoked` event with the correct usage and `local:false`, and stopped cleanly.

- [ ] **Step 3: Implement `Planner::run`**

```rust
pub async fn run<P: LlmProvider>(
    &self, provider: &P, run_id: &str, goal: &str,
) -> Result<PlannerTurn, AgentError> { /* loop, route, audit, cap */ }
```

The loop: build messages (`system` + goal `user`), offer tools, call `provider.chat`, emit `model.invoked`, branch on `tool_calls` (route each through `runtime.execute_tool`, push `tool` message) vs no-tool (return final text). `WaitingApproval`/`NavigationRequired`/`SummaryRequired` stop the loop and are recorded in the trace. Synthesize an idempotency key `planner/{run_id}/{step_index}` for R1 calls. The Planner holds only an `AgentRuntime` clone.

- [ ] **Step 4: Verify and commit**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --test agent_planner -- --test-threads=1
git add src-tauri/src/agent/planner.rs src-tauri/tests/agent_planner.rs
git commit -m "feat: drive agent tool loop from the rust planner"
```

## Task 5: Local fallback and soft token budget

**Files:**

- Modify: `src-tauri/src/agent/planner.rs`
- Modify: `src-tauri/tests/agent_planner.rs`

- [ ] **Step 1: Write the fallback tests**

Assert: when `api_key_for` returns `None`, `Planner::run` makes zero provider calls and returns `PlannerTurn { mode: "local", .. }` with text that contains "本地" and does not claim model reasoning; one `model.invoked` event is emitted with `local:true` and zero tokens. Assert: when the run's summed tokens already meet `agent_planner_token_budget`, the next call is skipped and the turn is local. Assert: a terminal `ProviderRequestFailed` after retries degrades to local-mode text rather than surfacing the error to the caller.

- [ ] **Step 2: Implement the fallback gate**

Before each `provider.chat`, check keyring + budget; on failure or terminal provider error, return the local turn. Track summed tokens across the run on the Planner.

- [ ] **Step 3: Verify and commit**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --test agent_planner -- --test-threads=1
git add src-tauri/src/agent/planner.rs src-tauri/tests/agent_planner.rs
git commit -m "feat: degrade agent planner to local mode on llm failure"
```

## Task 6: Hidden Tauri command and debug-page wiring

**Files:**

- Modify: `src-tauri/src/agent/commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/services/agent-client.ts`
- Modify: `src/services/agent-client.test.ts`
- Modify: `src/pages/AgentDebug.vue`
- Modify: `src/pages/AgentDebug.test.ts`

- [ ] **Step 1: Add the command**

`agent_run_planner(runtime, run_id, goal) -> PlannerTurn` trims both inputs, calls `runtime.run_planner(...)`, redacts errors. The `Planner` is constructed in `lib.rs` from the same pool and managed as `tauri::State<Planner>` (or the runtime owns it — pick the smaller diff). Register the command in `invoke_handler`.

- [ ] **Step 2: Frontend contract + hidden page**

Add `agentRunPlanner(runId, goal)` to `agent-client.ts`; assert the command name and `{ runId, goal }` camelCase args in the test. Add a read-only "运行一轮 Planner" control to `AgentDebug.vue` rendering the trace + usage; assert it invokes the client and renders the final text in the component test.

- [ ] **Step 3: Verify and commit**

```powershell
npm.cmd test -- src/services/agent-client.test.ts src/pages/AgentDebug.test.ts
npm.cmd run typecheck
cargo fmt --manifest-path src-tauri\Cargo.toml -- --check
cargo clippy --manifest-path src-tauri\Cargo.toml --all-targets --all-features -- -D warnings
git add src-tauri/src/agent/commands.rs src-tauri/src/lib.rs src/services/agent-client.ts src/services/agent-client.test.ts src/pages/AgentDebug.vue src/pages/AgentDebug.test.ts
git commit -m "feat: expose hidden agent planner debug command"
```

## Task 7: Milestone-slice verification and evidence

**Files:**

- Modify: `docs/agent/feature-parity.md`
- Modify: `docs/agent/migration-runbook.md`

- [ ] **Step 1: Run every automated gate from a clean worktree**

```powershell
npm.cmd test
npm.cmd run typecheck
npm.cmd run build
cargo fmt --manifest-path src-tauri\Cargo.toml -- --check
cargo test --manifest-path src-tauri\Cargo.toml
cargo test --manifest-path src-tauri\Cargo.toml -- --test-threads=1
cargo clippy --manifest-path src-tauri\Cargo.toml --all-targets --all-features -- -D warnings
git diff --check
```

Required: zero Vitest failures, zero Rust failures in parallel and serial, no type/build errors, no formatter/Clippy/diff warnings.

- [ ] **Step 2: Record honest ownership**

In `feature-parity.md` add a `Model adapter and tool loop (Rust, non-streaming)` row marked `rust-owned` (the slice is Rust-owned runtime code reachable only via debug), and a note that streaming, the dedicated context-audit table + inspector, structured memory, and the full fallback engine remain M3 parts 2–N. Ollama tool support is excluded (cloud LLMs only).

- [ ] **Step 3: Runbook note**

In `migration-runbook.md` record: M3 Part 1 adds no migration; model usage audit lives in `agent_events` as `model.invoked`; provider errors are redacted at the command boundary; rollback is disabling the hidden command and the `Planner` state (the existing app is unchanged).

- [ ] **Step 4: Commit milestone evidence**

```powershell
git add docs/agent/feature-parity.md docs/agent/migration-runbook.md
git commit -m "docs: record m3 part 1 model loop milestone"
git status --short
```

## Exit criteria

- The Rust OpenAI-compatible provider mirrors the TypeScript request/response shape; an existing user provider config works without reconfiguration (verified by contract test).
- The Planner drives the model → tool loop entirely through `AgentRuntime::execute_tool`; it never dispatches a tool itself.
- Each model call produces one redacted `model.invoked` audit event with token usage and the data-permission categories offered; local-mode turns are marked `local:true` and never impersonate model output.
- No API key, provider failure, or exhausted soft budget degrades to a deterministic local turn instead of surfacing an error.
- The slice is reachable only through the hidden `agent_run_planner` command; production Vue flows, tool ownership, and the write path are unchanged.
- Full automated verification passes (Vitest, typecheck, build, cargo test parallel/serial, fmt, Clippy).

## Deferred to M3 parts 2–N (not in this plan)

- ~~Streaming text (`llm/stream.rs`) and the streaming tool loop.~~ **Shipped as M3 Part 2**: `OpenAiCompatibleProvider::chat_stream` parses SSE, forwards content deltas via an `on_chunk` callback, reassembles tool_calls by index, captures the final `usage` chunk; `Planner::run` streams each turn; `agent_run_planner` emits `agent-planner-chunk` events rendered live on `/agent-debug`.
- The dedicated `agent_context_audit` table (§7.3) and the Context Inspector UI (§10.2), replacing the `model.invoked` event with structured per-call data categories and single-use authorization for sensitive fields.
- Structured long-term memory: `agent_memories`, the seven memory types, candidate→confirmed flow, and the memory management UI (§11).
- The full Fallback Engine: overdue detection, daily/weekly stats, rule-based weakness identification, reminders, and workbench navigation (§12) — beyond the no-LLM local turn shipped here.

## Excluded by product decision (cloud LLMs only)

- Ollama tool-calling support. The product ships cloud LLMs for the agent loop; Ollama stays on the TypeScript plain-chat path and degrades to local mode in the Rust planner (`provider == "ollama"` → `ProviderUnavailable` for tools). Revisit only if local models become a product requirement.
