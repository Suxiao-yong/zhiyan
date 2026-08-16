# Cloud LLM Agent 简化改造修复计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** 修复 Cloud LLM Agent 简化改造当前的安全、审批、run 生命周期、测试和发布文档问题，使 Mandatory Task A/B/C 及最终发布门槛全部通过。

**Architecture:** 保留当前“Rust Planner/Tool Registry/Executor 唯一拥有云端能力、前端只负责展示和意图”的架构。先冻结当前脏工作树并验证 migration v11 是否曾被任何可分发版本应用，再按安全边界、run 终态、Draft 预览契约、上下文上限、兼容性和死代码清理分阶段修复。migration v1–10 的 SQL 不变；只有确认 v11 从未被分发或应用时，才撤回当前工作树中的未发布 v11 变更。

**Tech Stack:** Rust、Tauri、SQLite/sqlx、keyring、Vue 3、Pinia、TypeScript、Vitest、Vite、Cargo fmt/Clippy。

---

## 当前基线与不可违反的约束

以下是 2026-08-11 的历史基线，仅用于解释原始失败；执行本计划前必须重新采集一次基线，并以新的命令输出为准：

- npm.cmd test -- --run：12 个测试文件、65 个用例通过。
- npm.cmd run typecheck：通过。
- npm.cmd run build：通过，存在 chunk size 和动态导入 warning。
- cargo test --manifest-path src-tauri/Cargo.toml --lib：当时观测到 153 个测试，其中 151 个通过、2 个失败；该数量只是历史快照。
- cargo fmt --manifest-path src-tauri/Cargo.toml -- --check：失败。
- cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings：7 个错误。
- 当时工作树有 90 个变更条目，仍在 main 分支，不能使用 git reset --hard 或 git checkout -- 丢弃现有改造。执行前必须记录新的 `git rev-parse HEAD`、`git status --short` 和 `git diff --stat`；工作树数量和测试总数都不是固定验收条件。

执行期间必须遵守：

1. 不修改 src-tauri/src/db.rs 中 migration 1–10 的 SQL。只有在确认 `_sqlx_migrations` 和所有可分发数据库中不存在 v11 后，才删除当前工作树中的未发布 v11；否则保留 v11 并将其标记为兼容的 inert migration。
2. API Key 只允许 Rust/provider 读取；前端可以短暂提交用户刚输入的 key 到 Rust 的保存意图，但不得从 keyring 加载 key，也不得把已保存 key 放入 Pinia store、普通页面状态或消息。
3. 云端 LLM 不得生成 SQL、直接访问 SQLite 或绕过 Rust Tool Registry。
4. 所有模型写操作必须经过脱敏预览、用户确认、Rust 本地执行；不得恢复 agent_r2_auto_execute 自动写入。
5. 计划、历史表、ai_analyses、agent_memories、agent_jobs 保持可读；SQLite backup/restore 必须保留这些表。JSON 导出目前只承诺 ai_analyses，若要求 agent_memories/agent_jobs 也进入 JSON 导出，必须在本计划中同时修改 `src/services/export.ts` 和对应测试。
6. 每个任务只提交自己涉及的文件；禁止使用 git add -A。

## Task 1: 冻结脏工作树并固化可复现失败基线

**Files:**

- Modify: none
- Test: existing repository commands below

- [ ] **Step 1: 记录当前工作树，不把脏 main 误称为隔离分支**

先记录当前提交和所有未提交改造；`git switch -c` 只会改变分支指针，不会隔离当前 91 个（或执行时实际数量）未提交文件：

~~~powershell
git rev-parse HEAD
git status --short
git diff --stat
git branch --show-current
~~~

若用户明确要求分支隔离，且当前目录不是 linked worktree，再执行：

~~~powershell
git switch -c codex/cloud-llm-agent-fix-2026-08-11
~~~

若分支已存在，先确认不是其他 worktree 正在使用，再执行 `git switch codex/cloud-llm-agent-fix-2026-08-11`。任何情况下都不得 reset、checkout 丢弃当前改造。

- [ ] **Step 2: 记录修复前失败项和测试总数**

运行：

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list
~~~

将失败测试名称、Clippy 完整输出和测试总数保存到本次工作记录中。历史基线中的“2 个失败”和“7 个 Clippy 错误”只作参考；若当前失败集合变化，先更新基线再继续。

- [ ] **Step 3: 只读检查敏感信息和差异格式**

运行：

~~~powershell
rg -n -i 'sk-[A-Za-z0-9_-]{8,}|sk-ant-[A-Za-z0-9_-]{8,}|AIza[0-9A-Za-z_-]{20,}|Bearer\s+[A-Za-z0-9._-]{8,}|api[_-]?key|fallback' src src-tauri docs tests
git diff --check
~~~

扫描结果逐项确认：测试占位符可以保留，但真实 key、Authorization、URL 查询参数、日志和错误消息不得出现。`rg` 无匹配时退出码为 1，不把“无匹配”误判为扫描失败；只有退出码大于 1 才阻断执行。`git diff --check` 必须无错误。

## Task 2: 修复计划审批测试的日期依赖

**Files:**

- Modify: src-tauri/src/agent/runtime.rs:142-162
- Test: src-tauri/src/agent/runtime.rs:214-316

- [ ] **Step 1: 让测试数据相对业务日期生成未来旧计划**

不要把测试绑定到日历日期，也不要直接用“自然日明天”表达业务日期（应用在凌晨 04:00 前仍属于前一天）。在 `draft_runtime()` 中只读取一次当前业务日期，再生成 30 天后的未来旧计划；这样跨午夜时测试仍不会变成过去计划。保持 SQL 参数化，使用如下结构替换当前内嵌的旧计划插入：

~~~rust
let business_date = crate::agent::tools::plan::business_date_at(
    chrono::Local::now().fixed_offset(),
);
let old_plan_date = chrono::NaiveDate::parse_from_str(&business_date, "%Y-%m-%d")
    .unwrap()
    .checked_add_days(chrono::Days::new(30))
    .unwrap()
    .format("%Y-%m-%d")
    .to_string();

sqlx::query(
    "INSERT INTO study_plans(id,exam_id,subject_id,date,planned_tasks,planned_duration,status)
     VALUES(?, ?, ?, ?, ?, ?, ?)",
)
.bind("plan-old")
.bind("exam-p")
.bind("sub-m")
.bind(&old_plan_date)
.bind("旧计划")
.bind(60_i64)
.bind("pending")
.execute(&pool)
.await
.unwrap();
~~~

保留其他考试、科目、知识点、session、run 的 fixture 内容不变。若要完全消除时钟依赖，下一步把 `business_date` 作为 `preview_generate`/`apply_preview` 的测试专用参数注入；本任务先保证 fixture 不依赖固定历史日期。

- [ ] **Step 2: 运行两个失败测试确认修复**

运行：

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib agent::runtime::tests::approving_apply_preview_writes_exactly_once_and_reject_writes_nothing
cargo test --manifest-path src-tauri/Cargo.toml --lib agent::runtime::tests::repeating_confirmation_never_double_writes
~~~

预期两个测试通过，审批确认后计划总数为 60，拒绝后仍为 1，重复确认返回 approval_invalid 且不增加计划行。

- [ ] **Step 3: 提交测试修复**

~~~powershell
git add src-tauri/src/agent/runtime.rs
git commit -m "test: make plan approval fixture date independent"
~~~

## Task 3: 补齐 agent_run_planner 的失败终态

**Files:**

- Modify: src-tauri/src/agent/commands.rs:394-440
- Modify: src-tauri/src/agent/runtime.rs:76-86
- Modify: src-tauri/src/agent/repository.rs:169-215
- Test: src-tauri/src/agent/commands.rs command-boundary tests
- Reference: src-tauri/src/agent/state.rs, src-tauri/src/agent/planner.rs

- [ ] **Step 1: 先添加 command boundary 的 preflight 失败测试**

增加测试用例，创建 running 状态的 run，在未写入 consent fingerprint 时调用与 `agent_run_planner` 相同的 preflight helper，断言返回错误、run 终态和数据库中的错误码：

~~~rust
assert_eq!(error.code(), "consent_required");
assert_eq!(run_status(&pool, "run-id").await, "failed");
assert_eq!(run_error_code(&pool, "run-id").await, "consent_required");
~~~

再覆盖三种 provider 情况：没有 cloud config 时允许明确的 local fallback；已有 cloud config 但 key 缺失时返回 `provider_unavailable`；settings 查询失败时返回 `persistence_error`。三种失败都必须断言原始错误码返回给调用方、数据库 `error_code` 保留原始错误码、run 不再是 `running`。

- [ ] **Step 2: 增加原子 fail_run 边界**

在 `src-tauri/src/agent/repository.rs` 增加只允许从 `running`/`waiting_approval` 进入
`failed` 的条件更新，并写入传入的稳定错误码；在 `runtime.rs` 暴露
`fail_run(run_id, error_code)`。条件更新影响行数不是 1 时返回 `Conflict`，不得静默忽略。
`agent_run_planner` 同时接收已由 lib.rs 管理的 `AgentRuntime` 和 `SqlitePool` state，
用参数化 SQL 只读取 `llm_provider` 是否为非空云端配置；不把这个查询塞进 Planner，避免
与 Task 6 的 prompt-boundary 改动产生同一文件的伪冲突。伪代码必须落成以下语义：

~~~rust
pub async fn fail_run(
    &self,
    run_id: &str,
    error_code: &str,
) -> Result<AgentRun, AgentError> {
    // UPDATE agent_runs SET status='failed', error_code=?,
    // completed_at=datetime('now','localtime')
    // WHERE id=? AND status IN ('running','waiting_approval')
    // 必须检查 rows_affected()==1，然后返回最新 AgentRun。
}
~~~

在 `commands.rs` 中把 consent 和 provider 构建包入一个非 Tauri helper。两个错误分支都通过 `fail_run` 终止 run；只有终止成功后才返回原始错误，终止失败则返回 `conflict`，避免对调用方声称失败但数据库仍是 `running`：

~~~rust
async fn prepare_provider(
    planner: &Planner,
    runtime: &AgentRuntime,
    pool: &SqlitePool,
    run_id: &str,
) -> Result<Option<LlmProvider>, CommandError> {
    if let Err(error) = planner.ensure_cloud_consent().await {
        runtime.fail_run(run_id, error.code()).await.map_err(CommandError::from)?;
        return Err(error.into());
    }
    let has_cloud_config = match sqlx::query_scalar::<_, Option<String>>(
        "SELECT value FROM settings WHERE key='llm_provider'",
    ).fetch_optional(pool).await {
        Ok(value) => value.flatten().is_some_and(|value| {
            let provider = value.trim();
            !provider.is_empty() && provider != "ollama"
        }),
        Err(error) => {
            let error = AgentError::Persistence(format!("provider settings read failed: {error}"));
            runtime.fail_run(run_id, error.code()).await
                .map_err(CommandError::from)?;
            return Err(error.into());
        }
    };
    match planner.build_provider().await {
        Ok(Some(provider)) => Ok(Some(provider)),
        Ok(None) if !has_cloud_config => Ok(None),
        Ok(None) => {
            runtime.fail_run(run_id, "provider_unavailable")
                .await.map_err(CommandError::from)?;
            Err(CommandError::from(AgentError::ProviderUnavailable))
        }
        Err(error) => {
            runtime.fail_run(run_id, error.code()).await.map_err(CommandError::from)?;
            Err(error.into())
        }
    }
}
~~~

`agent_run_planner` 的签名增加 `State<'_, AgentRuntime>` 与 `State<'_, SqlitePool>`，只调用
`prepare_provider(planner.inner(), runtime.inner(), pool.inner(), &run_id)`，保留已有 planner loop 错误分支和 waiting
approval 逻辑；loop 内 provider/schema/persistence 错误也使用同一个
`runtime.fail_run(run_id, error.code())`，不再用 `let _ = transition_run(...);`。如果终止本身失败，
返回 `conflict` 并记录原始错误，测试要覆盖“调用方收到 conflict、数据库没有继续保持 running”的情况。

- [ ] **Step 3: 运行状态相关测试**

~~~powershell
    cargo test --manifest-path src-tauri/Cargo.toml --lib agent::commands
    cargo test --manifest-path src-tauri/Cargo.toml --lib agent::repository
    cargo test --manifest-path src-tauri/Cargo.toml --lib agent::state
    cargo test --manifest-path src-tauri/Cargo.toml --lib agent::planner
~~~

预期 provider error、consent error、schema/persistence error、approval waiting 都有明确终态。

- [ ] **Step 4: 提交 run 生命周期修复**

~~~powershell
git add src-tauri/src/agent/commands.rs src-tauri/src/agent/runtime.rs src-tauri/src/agent/repository.rs
git commit -m "fix: terminate planner runs on preflight failures"
~~~

## Task 4: 收紧 API Key 边界并修复 Settings 测试流程

**Files:**

- Modify: src-tauri/src/credentials.rs:34-57
- Modify: src-tauri/src/lib.rs command registration
- Modify: src/stores/settings.ts:18-94
- Modify: src/services/db.ts:248-265
- Modify: src/pages/Settings.vue:20-145,250-310
- Modify: src/types/index.ts:127-134
- Test: src/stores/settings.test.ts
- Test: src/pages/Settings.test.ts

- [ ] **Step 1: 先改前端测试为“加载配置不返回密钥”**

将 src/stores/settings.test.ts 的断言改为只验证非敏感配置；mock 的 Tauri invoke
只允许收到 `has_api_key` 的布尔结果，不允许读取 key 内容：

~~~ts
expect(store.llmConfig).toEqual({
  provider: 'deepseek',
  baseUrl: 'https://api.deepseek.com',
  model: 'deepseek-chat',
  temperature: 0.7,
})
expect(invoke).toHaveBeenCalledWith('has_api_key', { provider: 'deepseek' })
expect(invoke).not.toHaveBeenCalledWith('load_api_key', expect.anything())
expect(JSON.stringify(store.$state)).not.toContain('configured-key-placeholder')
~~~

保存测试仍断言用户新输入的 key 只被传给 `store_api_key`，保存完成后 store、组件
props、错误消息和 agent preview 均不保留该字符串；空输入表示“沿用已配置 key”，
只有 `keyConfigured === false` 时才要求重新输入。

- [ ] **Step 2: 拆分非敏感配置和设置页临时输入**

将 `LLMConfig` 改为只包含 `provider/baseUrl/model/temperature`，在 Settings 页面使用
独立的 `apiKeyInput` ref 保存用户当前输入。`loadLlmConfig()` 只读取非敏感 settings，
另调用只返回布尔值的 `has_api_key`；成功保存后清空 `apiKeyInput`，仅显示
`keyConfigured: true` 或“需要重新输入”，不显示已保存 key。由于 App.vue 异步加载设置，
Settings.vue 必须在 store 加载完成后用 watch/显式 `syncFormFromStore()` 初始化表单，
不得把初始 `undefined` 当成用户要覆盖的配置。

- [ ] **Step 3: 删除前端 keyring 读取路径**

删除 settings.ts 对 `load_api_key` 的调用和 `loadApiKey()`。删除并取消注册 Rust 的
`load_api_key` Tauri command；增加只返回 `bool` 的 `has_api_key` command。保留
`api_key_for()` 作为 Rust provider 内部 API，保留 `store_api_key` 和 `delete_api_key`
供用户保存/删除意图使用。`src/services/db.ts` 增加 `hasSetting(key): Promise<boolean>`，
通过参数化的 `SELECT 1 ... LIMIT 1` 实现。

旧 fallback 只调用 `hasSetting(fallbackKey(provider))` 做存在性判断，不调用
`getSetting()`，不得将 fallback 字符串交给前端。

- [ ] **Step 4: 让 Test 按“已保存配置”工作**

Settings.vue 的 `test()` 在当前表单与最后保存的非敏感配置不一致，或 `apiKeyInput`
非空但尚未保存时，显示“请先保存配置”，不调用 `agent_test_provider`；不得再执行
`settingsStore.llmConfig = { ...form }`。保存成功后调用无参数的 Rust
`agent_test_provider`，由 Rust 从 settings + keyring 读取配置。保存必须先成功写 keyring，
再写非敏感 settings；任一步失败都不得更新 store 的“已保存配置”快照。

测试结果只显示稳定 error_code、模型名、延迟和能力布尔值，绝不显示 key、URL 或响应体。

- [ ] **Step 5: 增加安全回归扫描和测试**

~~~powershell
npm.cmd test -- --run src/stores/settings.test.ts src/pages/Settings.test.ts
npm.cmd run typecheck
rg -n "load_api_key|llmConfig.*apiKey|apiKey.*llmConfig|getSetting\(fallbackKey|loadApiKey" src/stores src/services src/pages/Settings.vue src/types/index.ts src-tauri
~~~

预期前端不存在 `load_api_key` 调用，`LLMConfig` 不再包含 API Key，fallback 只返回
布尔存在性；Rust provider 测试仍能从 keyring 读取。扫描命令退出码为 1（无匹配）视为
通过，只有退出码大于 1 才失败。

- [ ] **Step 6: 提交 credential boundary 修复**

~~~powershell
    git add src-tauri/src/credentials.rs src-tauri/src/lib.rs src/stores/settings.ts src/services/db.ts src/pages/Settings.vue src/types/index.ts src/stores/settings.test.ts src/pages/Settings.test.ts
git commit -m "fix: keep provider credentials inside Rust"
~~~

## Task 5: 完成 Draft 预览、确认和撤销闭环

**Files:**

- Modify: src-tauri/src/agent/executor.rs:2300-2478
- Modify: src-tauri/src/agent/tools/plan.rs:512-690
- Reference: src-tauri/src/agent/plan_draft.rs:60-80 (use existing DraftDay/DraftTask fields)
- Modify: src/types/index.ts:260-290
- Modify: src/components/agent/AgentActionPreview.vue
- Modify: src/components/agent/ApprovalCard.vue
- Modify: src/stores/agent.ts
- Modify: src/services/agent-client.ts
- Test: src/pages/AgentHome.test.ts
- Test: src-tauri/src/agent/executor.rs

- [ ] **Step 1: 先增加后端 preview contract 测试**

针对 `plan.apply_preview` 的预览断言。测试先从 fixture 展平真实 Draft 行，
并以同一套“冲突策略投影”计算预期写入数量；数量不能使用天数：

~~~rust
let rows = draft.daily_plans.iter().flat_map(|day| {
    day.tasks.iter().map(|task| (day.date.clone(), task.subject_name.clone(), task.task.clone(), task.duration_min))
}).collect::<Vec<_>>();
let projected = project_apply_rows(&draft, &existing_plans, business_date);
assert_eq!(preview["affected_count"], json!(projected.writable_rows.len()));
assert_eq!(preview["fields"]["draft_row_count"], json!(rows.len()));
assert!(preview["fields"]["rows"].as_array().unwrap().iter().all(|row| {
    row.get("date").is_some() && row.get("subject_name").is_some()
        && row.get("planned_tasks").is_some() && row.get("planned_duration").is_some()
}));
assert!(preview["fields"].get("precondition_hash").is_some());
assert!(preview["fields"].get("preview_step_id").is_none());
~~~

同时断言 preview 不含 API Key、完整请求体、模型原始输出或未列入白名单的 input 字段。
以 30 天 × 2 任务 fixture 为例，`draft_row_count` 应为 60；若存在保留科目，
`affected_count` 必须等于实际可写入行数，而不是固定写成 60。

- [ ] **Step 2: 让 Rust 生成脱敏的真实 Draft 行**

将 `build_approval_preview()` 的 match 返回值扩展为包含 `fields` 的结果结构；
`plan.apply_preview` 分支反序列化现有 `PlanDraft`，调用与真正 apply 共用的
`project_apply_rows()`，在 Draft 作用域内构造脱敏 fields。`DraftDay` 只有 `date` 和
`tasks`，`DraftTask` 使用 `subject_name/task/duration_min`；不得再写不存在的
`row.subject_name`、`row.planned_tasks` 或 `row.planned_duration`。`affected_count`
定义为 projection 中实际可写入的任务行数，`draft_row_count` 定义为 Draft 展平后的行数。
fields 只返回白名单字段 `date/subject_name/planned_tasks/planned_duration`、
`draft_row_count`、`precondition_hash`、`kept_subjects` 和冲突摘要。禁止直接
`serde_json::to_value(input)` 回显完整请求：

~~~rust
let rows = draft.daily_plans.iter().flat_map(|day| {
    day.tasks.iter().map(|task| json!({
        "date": day.date,
        "subject_name": task.subject_name,
        "planned_tasks": task.task,
        "planned_duration": task.duration_min,
    }))
}).collect::<Vec<_>>();
let projection = project_apply_rows(&draft, &existing_plans, business_date);
let sanitized_fields = json!({
    "precondition_hash": draft.precondition_hash,
    "draft_row_count": rows.len(),
    "rows": rows,
    "kept_subjects": projection.kept_subjects,
    "conflicts": draft.conflicts,
});
// 返回 { affected_count: projection.writable_rows.len(), fields: sanitized_fields }。
// apply 与 preview 必须共用 projection，避免 UI 数字与实际写入数量漂移。
~~~

测试还要验证 fields 序列化后不包含 `preview_step_id`、API Key、原始 input 或模型响应。

- [ ] **Step 3: 补齐前端 Draft 行和 precondition 展示**

在 AgentActionPreview.vue 中将 fields.rows 转成只读 compact rows，显示日期、科目、任务和分钟数；显示 precondition 是否存在以及冲突详情。组件仍不调用数据库，只通过事件向父层发出动作。

- [ ] **Step 4: 接通实际 undo command**

在 `plan.rs` 为 `plan.apply_preview` 生成 `PlanApplyUndoReceipt`，写入现有
`agent_steps.undo_json`：至少包含被替换/删除的完整 `study_plans` 行、被 detach 的
`study_records.plan_id` 关系、此次插入的 plan id，以及 apply 前后的投影 hash。
`DispatchResult` 返回该 receipt，`apply_preview_descriptor().supports_undo` 改为 true；
`executor.rs::undo_in_transaction()` 按 tool name 分支处理该 receipt：先验证插入行仍
存在且快照涉及的计划/记录没有被外部修改，再事务内删除新行、恢复旧行和原有 plan_id，
最后写 `undone_at`。任何快照缺失、重复撤销或外部变更均返回 `conflict`，不能静默覆盖用户修改，
也不新增 migration。为 `record.checkin_plan` 保留原有 undo 分支。

在 `stores/agent.ts` 增加 `undoTool(stepId)` 调用现有 `undoAgentTool()`，成功后刷新
approvals/messages/brief。ApprovalCard.vue 在审批完成且 `preview.undo_available` 为
true 时显示“撤销”按钮，传入 approval.step_id；按钮调用 store，不做 state-only 假更新。

- [ ] **Step 5: 增加前端审批回归测试**

AgentHome.test.ts 必须覆盖：真实 Draft 行显示、确认调用 agent_resolve_approval、取消不写入、
plan.apply_preview 撤销恢复原计划且在外部修改后拒绝撤销、record.checkin_plan 仍可撤销、
重复确认不重复刷新业务数据。对同一 approval 连续双击确认只能发出一次 resolve invoke；
ApprovalCard 或 store 必须有 per-approval busy/claim guard。

- [ ] **Step 6: 运行 Mandatory Task C 门槛**

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib agent::tools::plan
cargo test --manifest-path src-tauri/Cargo.toml --lib agent::executor
npm.cmd test -- --run src/pages/AgentHome.test.ts
npm.cmd run typecheck
~~~

- [ ] **Step 7: 提交审批闭环修复**

~~~powershell
    git add src-tauri/src/agent/executor.rs src-tauri/src/agent/tools/plan.rs src/types/index.ts src/components/agent/AgentActionPreview.vue src/components/agent/ApprovalCard.vue src/stores/agent.ts src/services/agent-client.ts src/pages/AgentHome.test.ts
git commit -m "fix: expose complete draft preview and undo flow"
~~~

## Task 6: 补齐 ContextSnapshot 字节上限和 provider capability 错误

**Files:**

- Modify: src-tauri/src/agent/context_snapshot.rs:27-242
- Modify: src-tauri/src/agent/planner.rs:224-253
- Modify: src-tauri/src/agent/llm/openai_compatible.rs:107-141
- Test: src-tauri/src/agent/context_snapshot.rs
- Test: src-tauri/src/agent/llm/openai_compatible.rs
- Test: src-tauri/src/agent/planner.rs

- [ ] **Step 1: 先增加 UTF-8 字节截断测试**

加入包含中文和 emoji 的字符串测试，断言截断结果始终是合法 UTF-8，并且字段、
snapshot 和最终 provider 请求分别满足字节上限：

~~~rust
assert!(value.as_bytes().len() <= MAX_FIELD_BYTES);
assert!(snapshot.to_system_text().as_bytes().len() <= MAX_SNAPSHOT_BYTES);
assert!(serialized_messages_content_bytes(&messages) <= MAX_PROMPT_BYTES);
assert!(serialized_request_body(&request).len() <= MAX_REQUEST_BYTES);
~~~

再构造超长 tool output、history、system prompt 和 user goal，断言最终 provider request 的
消息内容总字节数不超过约定上限，且 truncation 计数增加；还要验证工具 schema 不会使
最终序列化 request body 超过 `MAX_REQUEST_BYTES`。测试数据只能使用固定假数据，不得放真实 key。

- [ ] **Step 2: 使用明确的字节上限**

保留现有分类数量限制，新增并集中定义：

~~~rust
pub const MAX_FIELD_BYTES: usize = 800;
pub const MAX_TOOL_OUTPUT_BYTES: usize = 8 * 1024;
pub const MAX_SNAPSHOT_BYTES: usize = 12 * 1024;
pub const MAX_PROMPT_BYTES: usize = 64 * 1024;
pub const MAX_REQUEST_BYTES: usize = 96 * 1024;
~~~

用按 UTF-8 边界截断的 helper 替换 `chars().take()` 作为最终限制：

~~~rust
fn truncate_utf8_prefix(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes { return value.to_owned(); }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) { end -= 1; }
    value[..end].to_owned()
}
~~~

保留“优先保留尾部”的场景，但尾部截断同样必须回退到 `is_char_boundary()`。
在 Planner 调用 provider 前，对 system、user、assistant、tool output 的累计消息内容
做总上限裁剪，按优先级从最旧 history、较早 tool output 到当前目标裁剪，并记录本地
`truncated_fields`/`truncated_bytes` 计数；随后对完整序列化 request body 做第二道上限检查。
若序列化 body 仍超限，只能继续丢弃低优先级 history/tool output；如果只剩固定诊断 prompt、
工具 schema 和必要消息仍超限，则在发送前返回已有的 `provider_protocol_error`，不得发送超限
请求，也不得截断 JSON/schema/key 的中间字节。
不得把 ContextAudit 当作上下文来源，也不得为了满足上限把工具 schema 或 API Key 放入日志。

- [ ] **Step 3: 把 capability 失败映射为稳定错误**

把两个诊断请求拆成互不矛盾的固定 prompt：文本探测使用
`“请只回复 OK，不要调用工具。”`，工具探测使用
`“请调用 diagnostics.ping，不要输出文本。”`，并分别携带最小固定 schema；
禁止用“不要使用工具”的 prompt 去验证 tool-call。若 text stream 缺失/中断，或
diagnostics.ping 没有真正的 tool-call，`test_provider()` 返回稳定的
`provider_protocol_error`，不再返回 `error_code=None` 搭配 false 能力。补齐空响应、
无 tool-call、stream 中断和合法响应的 mock 测试；响应体、prompt、schema 均不得包含
业务数据或 key。

- [ ] **Step 4: 修复 Clippy 触发项并格式化**

按已确认的触发点逐项处理，不使用“以后按报告处理”的占位语句：删除
`planner.rs:659` 不可能的 enum else 分支；为 `context_snapshot.rs` 和
`tools/plan.rs` 的 SQL row tuple 定义命名 row type；移除 `executor.rs` 不必要借用；
把 provider 测试改为 `std::slice::from_ref`；修正文档列表缩进。然后运行：

~~~powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings
~~~

预期 fmt check 和 Clippy 均通过；若需要格式化，只对本 Task 列出的 Rust 文件执行
rustfmt，并在提交前用 `git diff --name-only` 检查没有带入无关文件。

- [ ] **Step 5: 提交上下文和诊断修复**

~~~powershell
git add src-tauri/src/agent/context_snapshot.rs src-tauri/src/agent/planner.rs src-tauri/src/agent/llm/openai_compatible.rs src-tauri/src/agent/executor.rs src-tauri/src/agent/tools/plan.rs
git commit -m "fix: enforce bounded prompts and provider capability errors"
~~~

## Task 7: 恢复 migration 与兼容文档的一致性

**Files:**

- Modify: src-tauri/src/db.rs:104-113
- Modify: docs/agent/migration-runbook.md:162-169
- Modify: docs/agent/cloud-llm-cutover-migration.md
- Test: src-tauri/src/db.rs

- [ ] **Step 1: 先确认 v11 是否已经对外应用，再处理 source**

先保存 `git diff -- src-tauri/src/db.rs`，并在一次性数据库副本和打包数据库（若存在）中
读取 `_sqlx_migrations` 的已应用版本；检查不能只看当前源码。若所有可支持数据库都没有
v11，删除当前工作树中仅用于写入 `agent_tool_owner.plan.preview_generate` 和
`agent_tool_owner.plan.apply_preview` 的未发布 v11，并保留 v1–v10 SQL 不变。若任一已
发布数据库已经有 v11，则不得删除或重编号 v11；保留兼容 migration，并把最新版本、测试
和文档统一记为 v11。无论哪条分支，工具 ownership 都继续由 Rust 静态 registry 决定，
不得依赖这些 inert settings。

- [ ] **Step 2: 修正 migration 测试语义**

根据 Step 1 的实际结果更新版本断言：未发布分支断言最新版本为 v10，已发布分支断言
最新版本为 v11；两条分支都不能修改 v1–v10 的 SQL。保留“完整 migrations 创建运行时表”
和“旧表行可读”测试，但将旧数据测试调整为：先创建 v10 schema（已发布分支再应用
v11），插入 `ai_analyses`、`agent_memories`、`agent_jobs`、旧 ownership、旧 consent
fingerprint 和旧 fallback 行，随后运行当前启动/迁移入口，最后断言这些行的 value 未改变，
新 Agent 不读取已废弃的 analysis/memory/job/ownership/fallback 值。测试不能只是在完整
migrations 执行后才插入旧行。

- [ ] **Step 3: 校正文档版本与策略**

统一写明 Step 1 选择的版本策略：v1–v10 不变，本次发布不新增 migration；如果 v11
已经发布则 v11 也不可改写。历史表保留且可读；核心 Agent 不再写 analysis/memory/job
旧 API，但提醒 scheduler 允许继续写入其内部 reminder job。JSON 导出/导入白名单只承诺
现有 `ai_analyses`，SQLite backup/restore 才承诺完整数据库保真；不要笼统写成“所有
历史表都可 JSON 导出”。

- [ ] **Step 4: 运行数据库兼容性门槛**

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib db
cargo test --manifest-path src-tauri/Cargo.toml --lib db::tests::migration_versions_are_monotonic
~~~

预期所有 db 测试通过，且测试结果能证明旧数据升级后仍可读；版本断言必须与 Step 1
记录的分支一致，不能同时声称 v10 和 v11 都是最新。

- [ ] **Step 5: 提交兼容性修复**

~~~powershell
    git add src-tauri/src/db.rs docs/agent/migration-runbook.md docs/agent/cloud-llm-cutover-migration.md
git commit -m "fix: restore migration compatibility contract"
~~~

## Task 8: 清理死的旧分析、Memory 和公开 Job API 引用

**Files:**

- Delete after zero-reference scan: src/pages/Dashboard.vue
- Delete after zero-reference scan: src/components/dashboard/AiDailySummary.vue
- Delete after zero-reference scan: src/components/dashboard/TodayTodoList.vue
- Delete after zero-reference scan: src/components/charts/WeeklyTrendChart.vue
- Delete after zero-reference scan: src/components/charts/SubjectRatioChart.vue
- Delete after zero-reference scan: src-tauri/src/agent/memory.rs
- Modify: src-tauri/src/agent/mod.rs
- Modify: src/services/agent-client.ts
- Modify: src/stores/record.ts (remove dead dashboard stats state/helpers only)
- Modify: src/services/record-service.ts (remove dead dashboard stats queries only)
- Modify: src/types/index.ts
- Modify: src-tauri/src/agent/commands.rs
- Modify: src-tauri/src/lib.rs
- Test: src/router/index.test.ts, src/pages/AgentHome.test.ts, src-tauri/src/db.rs

- [ ] **Step 1: 扫描旧分析和 dashboard 引用**

~~~powershell
rg -n "Dashboard|AiDailySummary|TodayTodoList|WeeklyTrendChart|SubjectRatioChart|AiAnalysis|AnalysisType|loadDashboardStats|getWeeklyTrend|getSubjectRatio|agent_job_|agent_memory_|MemoryRepository|mod memory" src src-tauri
~~~

只保留 `ai_analyses` 作为 export/import/db compatibility 表名，保留 scheduler 的内部
reminder job 实现和 `/dashboard` redirect/test；生产组件、页面、类型、store/service
统计 helper、公开 command/client 无引用后才删除。扫描命令退出码为 1（无匹配）视为
通过，只有退出码大于 1 才失败。

- [ ] **Step 2: 删除未路由的旧 dashboard 分支**

删除上述未引用页面、组件和两张已不属于核心统计的 chart；同时删除
`record.ts`/`record-service.ts` 中只服务旧 Dashboard 的统计 state、query 和类型，
保留仍被 Agent/记录页使用的通用记录查询。保留 DurationTrendChart 与 KpHeatmapChart，
确认它们仍由按需可视化路径使用；不要删除 `/dashboard` redirect 测试。

- [ ] **Step 3: 删除未使用的 MemoryRepository 模块**

确认 MemoryRepository 只有模块自测和兼容 fixture 使用后，删除 `pub mod memory;` 与
`memory.rs` 及仅供该模块的测试/类型；更新 `mod.rs`、命令注册和引用。保留
`AGENT_MEMORIES_SQL`、migration 测试和旧表导出/读取兼容，不删除数据库表，也不把旧表
读取重新接回新 Agent。

- [ ] **Step 4: 删除未使用的公开 Job list/schedule API**

agent_job_list、agent_job_schedule、AgentJob、AgentJobType 当前没有前端调用；删除对应 Tauri command 注册、client 函数和 TypeScript 类型。保留 scheduler.rs 对提醒/逾期任务的内部调度、历史 job 表和 deprecated job 的 skip 行为。

- [ ] **Step 5: 执行死引用扫描**

~~~powershell
rg -n "highlight\.js|marked|@tauri-apps/plugin-http|tauri-plugin-http|agent-engine|plan-generator|plan-chat-agent|MemoryRepository|agent_job_|agent_memory_|AiAnalysis|AnalysisType|Dashboard" src src-tauri package.json src-tauri/Cargo.toml
~~~

预期只剩 redirect/test、历史表名、scheduler 内部实现和文档说明；不得有已删除组件的
import、route component、动态引用、死 store/service helper 或公开 job/memory command。

- [ ] **Step 6: 提交死代码清理**

~~~powershell
    git add src/pages/Dashboard.vue src/components/dashboard/AiDailySummary.vue src/components/dashboard/TodayTodoList.vue src/components/charts/WeeklyTrendChart.vue src/components/charts/SubjectRatioChart.vue src-tauri/src/agent/mod.rs src-tauri/src/agent/memory.rs src/services/agent-client.ts src/stores/record.ts src/services/record-service.ts src/types/index.ts src-tauri/src/agent/commands.rs src-tauri/src/lib.rs src/router/index.test.ts src/pages/AgentHome.test.ts src-tauri/src/db.rs
git commit -m "chore: remove unreachable legacy agent surfaces"
~~~

## Task 9: 修正状态文档并完成最终自动化门槛

**Files:**

- Modify: PROJECT_STATUS.md
- Modify: docs/agent/feature-parity.md
- Modify: docs/agent/cloud-llm-cutover-test-matrix.md
- Modify: MANUAL_TEST.md

- [ ] **Step 1: 更新测试事实**

把 PROJECT_STATUS.md 和 docs/agent/feature-parity.md 中固定的“Rust 153 全部通过”改为
本次实际命令输出：记录测试命令、日期、退出码和观察到的通过/失败数量，不把历史数量
当成门禁。只有完整 Rust、fmt、Clippy、前端测试、typecheck、build 全部通过后，才能写成通过。

- [ ] **Step 2: 重新填写自动化验收矩阵**

只勾选已经实际执行且有命令输出的项目。未执行的 Tauri 打包、首次启动、数据库备份恢复、
真实 OpenAI-compatible provider、断网/401/429/超时和跨考试隔离保持未勾选，不得用静态
检查代替手测；每个勾选项必须链接到命令输出或 MANUAL_TEST.md 的记录。

- [ ] **Step 3: 运行完整自动化发布命令**

~~~powershell
git diff --check
npm.cmd test -- --run
npm.cmd run typecheck
npm.cmd run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --lib
    $secret_scan = rg -n --glob '!docs/superpowers/plans/**' 'sk-[A-Za-z0-9]+|Bearer\s+[A-Za-z0-9._-]+' src src-tauri docs tests
if ($LASTEXITCODE -gt 1) { throw "secret scan failed: rg exit $LASTEXITCODE" }
if ($secret_scan) { $secret_scan; throw "possible secret found" }
~~~

预期每条构建/测试命令退出码为 0；Rust 不再硬编码 153/153，使用实际输出；敏感信息
扫描无匹配。若扫描命令无匹配而 rg 退出码为 1，视为通过；退出码大于 1 才是扫描工具失败。

- [ ] **Step 4: 运行 Tauri 手工/打包门槛**

~~~powershell
npm.cmd run tauri build
~~~

使用产品支持的 app-data override 指向一次性临时目录；如果产品没有该 override，则使用
一次性 Windows 用户配置文件，测试结束后整体删除该目录/用户。按 MANUAL_TEST.md 逐项执行：
首次启动、重启持久化、旧路由、provider 错误、审批取消/确认/重复确认/precondition 变化、
导出/导入、SQLite backup/restore 和提醒场景。真实 key 只通过本机凭据管理器配置，不写
仓库、截图或日志；测试前后保存数据库版本、备份文件校验值和关键行计数。

- [ ] **Step 5: 最终文档自洽检查**

~~~powershell
$stale = rg -n --glob '!docs/superpowers/plans/**' "153 通过|全部 20|v9 是最新|migration v1.*v9|apiKey.*llmConfig|load_api_key|TODO|TBD|待补|删除或取消注册|以 .* 类型为准|按 Clippy 报告" PROJECT_STATUS.md docs MANUAL_TEST.md src src-tauri
if ($LASTEXITCODE -gt 1) { throw "stale scan failed: rg exit $LASTEXITCODE" }
if ($stale) { $stale; throw "stale claim or placeholder found" }
$fixed_count = rg -n --glob '!docs/superpowers/plans/**' "153/153|Rust 153" PROJECT_STATUS.md docs MANUAL_TEST.md src src-tauri
if ($LASTEXITCODE -gt 1) { throw "fixed-count scan failed: rg exit $LASTEXITCODE" }
if ($fixed_count) { $fixed_count; throw "fixed test count claim found" }
rg -n --glob '!docs/superpowers/plans/**' "v10 为当前最新|v1.?v10|v10.*最新" PROJECT_STATUS.md docs MANUAL_TEST.md src src-tauri
git status --short
~~~

预期不存在过时的通过声明、v9 最新声明、前端 keyring 读取或计划占位词；版本 grep 的
结果必须与 Task 7 的 v10/v11 分支记录一致。工作树应与 Task 1 保存的 dirty baseline
逐项比较，不能要求把用户原有未提交变更误判成此次修复。

- [ ] **Step 6: 提交最终文档与验收结果**

~~~powershell
    git add PROJECT_STATUS.md docs/agent/feature-parity.md docs/agent/cloud-llm-cutover-test-matrix.md MANUAL_TEST.md
git commit -m "docs: finalize cloud llm agent repair verification"
~~~

## 计划自检

在执行前和每次任务提交前逐项检查：

- [ ] Goal 的每一项都有对应 Task、文件、测试和验收证据：run 终态（Task 3）、API Key
  边界（Task 4）、Draft/审批/撤销（Task 5）、prompt/provider 协议（Task 6）、迁移兼容
  （Task 7）、死代码（Task 8）和发布证据（Task 9）。
- [ ] 所有删除动作都已先做零引用扫描；所有共享文件（例如 `types/index.ts`、
  `executor.rs`、`tools/plan.rs`、`lib.rs`）提交前用 `git diff` 核对，只暂存当前任务的
  hunks，不能覆盖前序任务或用户的 dirty baseline。
- [ ] 计划中没有“按报告处理”“字段以类型为准”这类未决实现；Draft 字段、字节上限、
  stable error code、migration v10/v11 分支和测试退出码均已定义。
- [ ] 最终扫描排除本计划文件本身，避免示例命令中的 `load_api_key`、历史失败数字和
  扫描正则被误判为项目残留；项目源代码与正式文档仍必须无匹配。

## 发布判定

只有以下条件全部满足，才能把计划和项目状态标记为完成：

- Rust lib tests、前端测试、typecheck、build、fmt 和 Clippy 的实际命令全部通过；测试
  数量以本次输出为准，不使用固定的 153。
- API Key 不再从 Rust 返回到前端 store/UI；Rust provider 和保存 command 测试通过。
- consent/provider preflight 错误不会留下 running run。
- Draft preview 显示真实脱敏行、影响数量、冲突和 precondition；确认、取消、重复确认、撤销均有测试。
- ContextSnapshot、每个 tool output 和总 prompt 都有 UTF-8 字节上限，且有超限测试。
- migration 1–10 未被修改，本次没有新增 migration；若 Task 7 证明 v11 未发布，则源码
  最新版本为 v10；若证明 v11 已发布，则保留 v11，测试和文档一致。
- 旧 dashboard/analysis/memory/job 公开死引用扫描通过，保留历史表和内部提醒调度。
- 前端测试、typecheck、build、Tauri 打包和手工验收矩阵均有真实证据。
- PROJECT_STATUS.md 不再宣称未通过的测试已通过。
