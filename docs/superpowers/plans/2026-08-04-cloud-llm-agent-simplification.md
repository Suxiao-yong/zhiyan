# Cloud LLM Agent Product Simplification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** 将智研从“旧学习管理系统 + Agent 外壳 + 多套 AI 链路”收敛为一个以 Agent 为入口、云端 LLM 负责理解与生成、本地 Rust/SQLite 负责事实与提交的简洁学习产品。

**Architecture:** 保留本地考试、科目、知识点、计划、学习记录和错题作为唯一事实源；由 Rust Agent Planner 统一调用唯一的 OpenAI-compatible 云端 LLM；LLM 只能查询本地上下文、生成回答或生成待确认动作，所有写入经过本地 schema 校验、用户确认和 SQLite 事务。前端从多个平行页面收敛为 Today/Agent、Plan、Records、Setup/Settings 四个用户区域，旧入口先重定向，再在所有引用清零后删除。

**Tech Stack:** Vue 3 + TypeScript + Pinia + Vue Router + Element Plus + Tauri 2 + Rust + SQLx/SQLite + Tokio + reqwest + OpenAI-compatible chat completions/tool calls + Vitest + Cargo tests。

---

## 0. 给执行助手的强制规则

本计划覆盖前端信息架构、TypeScript LLM 链路、Rust Agent、数据库兼容和 UI 重构。它们存在依赖关系，不能让多个助手同时修改同一个 src-tauri/src/agent 子树。推荐一个主助手逐任务执行；如果使用多个助手，必须按阶段顺序交接，并在每个阶段完成后运行全量测试。

执行前必须阅读：

- D:/智研/zhiyan/README.md
- D:/智研/zhiyan/PROJECT_STATUS.md
- D:/智研/zhiyan/docs/agent/feature-parity.md
- D:/智研/zhiyan/docs/agent/migration-runbook.md
- D:/智研/zhiyan/src/router/index.ts
- D:/智研/zhiyan/src/pages/AgentHome.vue
- D:/智研/zhiyan/src-tauri/src/agent/planner.rs
- D:/智研/zhiyan/src-tauri/src/agent/executor.rs

执行边界：

1. 不修改 src-tauri/src/db.rs 中已经发布的 migration 1–10。历史数据库必须可以继续启动。
2. 不把 API Key 放入 prompt、Agent message、日志、agent_context_audit 或普通 UI 状态之外的持久化字段。
3. 不让云端 LLM 直接生成 SQL、直接访问 SQLite 或绕过 Rust Tool Registry。
4. 不为了“看起来已删除”只隐藏按钮；每次删除前必须用 rg 检查 import、route、测试和动态引用。
5. 不新增向量数据库、RAG 服务、云端用户数据同步、账号系统或新的前端状态管理框架。
6. 每完成一个 Task 创建一个小提交；提交前运行该 Task 的最小测试，阶段结束再运行全量测试。
7. 如果现有用户数据或旧路由与计划冲突，优先保持数据可读和可回滚，然后把冲突记录在对应提交说明中。
8. 诊断、规划和删除必须与功能实现分开提交，方便其他助手逐个回滚。
9. 简化后的 Agent 不允许使用 `agent_r2_auto_execute` 自动写入。所有由模型提出的写操作都必须进入“可读预览 → 用户确认 → 本地执行”的 R3 审批路径；手动表单仍可按原有本地交互写入。
10. 删除 TypeScript 全周期计划生成器前，必须先完成 Rust 侧的行为等价测试；不得以当前仅覆盖一周、每天一个科目的 `plan.generate` v1 代替原计划能力。
11. 没有用户对当前 provider 配置的明确数据出境同意时，`agent_run_planner` 必须在 Rust 侧拒绝发送任何业务上下文。前端提示不能作为唯一防线。
12. 任何新 Tool 或已有 Tool 的调用都必须被限制在当前 run 绑定的 exam 范围内；不能因为模型提供了另一个 exam_id、subject_id 或 plan_id 而跨考试读取或写入。

## 1. 当前系统事实和改造边界

当前工程已经具备可用的 Agent Runtime，但产品上存在并行链路：

- /agent 是新的三栏壳层，但右侧 WorkbenchHost 直接嵌入 StudyPlan、StudyRecord、Analysis 和 Visualization 等旧页面：D:/智研/zhiyan/src/components/agent/WorkbenchHost.vue:1。
- 旧导航仍然暴露 Dashboard、考试配置、学习记录、学习计划、AI 分析、数据可视化、Agent、设置八个入口：D:/智研/zhiyan/src/components/layout/AppLayout.vue:30。
- 计划同时存在 Rust Planner、plan-generator.ts、plan-chat-agent.ts 和两个计划 AI 组件。
- 分析同时存在 TypeScript agent-engine.ts、analyzer.ts、Rust brief.rs 和 Rust Planner。
- LLM provider 同时存在 TypeScript src/services/llm-adapter.ts 和 Rust src-tauri/src/agent/llm/openai_compatible.rs。
- Rust 数据库包含基础业务表和 Agent session/run/step/event/approval/context/memory/job/message 表；migration 已到 v10：D:/智研/zhiyan/src-tauri/src/db.rs:20。
- Rust command 注册区约 27 个 command，其中大量 memory、job、debug、ownership command 仅服务于工程诊断或迁移兼容：D:/智研/zhiyan/src-tauri/src/lib.rs:60。
- 当前测试基线为 TypeScript 75 个测试、Rust 114 个测试通过；这说明代码内部自洽，不代表产品入口和职责已经收敛。

必须保留的本地事实：

- exams、subjects、knowledge_points
- study_plans、study_records、wrong_questions
- 设置、密钥存储、JSON 导入导出、数据库备份恢复
- 本地统计、完成率、逾期判断、错题状态、事务、幂等、撤销

允许由云端 LLM 接管的认知工作：

- 将自然语言转成查询意图；
- 根据本地统计解释学习状态；
- 生成计划草案或计划修改建议；
- 从自然语言整理学习记录和错题；
- 生成下一步建议和简报文案。

## 2. 目标产品契约

### 2.1 目标导航

普通用户最终只看到：

1. Today / Agent：今日状态、简报、对话、下一步动作、快速打卡。
2. Plan：单一时间线/列表和计划编辑；计划调整通过 Agent 完成。
3. Records：学习记录、错题、复盘。
4. Setup / Settings：考试配置、LLM 连接、主题、通知、数据备份。

以下页面不再作为普通导航入口：

- Dashboard：并入 Today；
- AI 分析：并入 Agent 消息和 Insight drawer；
- 数据可视化：缩成 Plan/Records 内的两个统计区块；
- PlanChatAssistant：删除，所有计划对话进入 Agent；
- AgentDebug：只在开发构建注册；
- 旧版 Agent OS 开关：迁移期保留，正式切换后只读或移除。

### 2.2 目标 Agent 交互

~~~text
用户输入目标
    ↓
Rust Planner 验证云端数据同意，并读取限定范围的本地上下文和当前会话历史
    ↓
云端 LLM 返回文本或调用一个已注册 Tool
    ↓
只读动作直接返回；计划类写动作先生成纯本地 Draft，其他写动作生成结构化预览
    ↓
用户确认
    ↓
Rust 再次校验 run/exam 范围、预条件、schema、幂等键，然后 SQLite 事务提交
    ↓
返回结果、影响范围和可撤销状态
~~~

第一阶段不新增另一套 action protocol，复用现有 agent_run_planner、ToolDescriptor、ToolCallResponse、agent_steps 和 agent_approvals。只有当现有协议无法表达计划草案时，才增加最小结构化字段；禁止为了替换旧 UI 重新设计完整 Agent OS 协议。

### 2.3 写入权限

| 操作 | LLM 能否直接完成 | 用户确认 | 本地处理 |
|---|---:|---:|---|
| 查询今日计划、记录、错题 | 可以调用只读 Tool | 否 | Rust 查询和输出 schema |
| 生成计划草案 | 只能选择生成参数 | 否，草案不写库 | Rust 纯函数生成 Draft、冲突和指纹 |
| 应用或调整计划 | 只能引用已生成的 Draft | 是 | run/exam/草案指纹/预条件校验 + SQLite transaction |
| 新建学习记录 | 只能生成结构化输入 | 是 | 字段校验、run 范围校验、幂等、写入 |
| 标记错题已掌握 | 只能提出动作 | 是 | 本地状态变更、范围校验；仅在 `undo_available=true` 时展示撤销 |
| 删除数据、覆盖数据库、修改凭据 | 不能由 Agent 自动完成 | 永远需要独立设置页确认 | 不暴露给 Agent Tool |

### 2.4 云端数据、上下文和运行状态契约

1. 用户首次将业务数据发送到某个 provider/base URL/model 组合前，必须看到数据范围说明并确认。设置值保存为 `cloud_llm_consent_fingerprint`，其值是 provider、base URL、model 和固定 policy version 的 SHA-256；其中任一配置变化都会使同意失效。
2. Provider 连通性测试可以在未同意前执行，但只能发送固定诊断 prompt 和静态 `diagnostics.ping` tool schema，绝不能读取考试、计划、记录、错题或会话内容。
3. 每次真实模型调用最多发送：当前用户请求、最近 12 条同一 session 的已持久化消息、当前考试摘要、当天最多 20 条计划、最近 14 天最多 20 条记录摘要、最多 10 个弱项/待复习错题摘要。超过上限的数据不发送，并在本地 trace 记录裁剪计数。
4. 业务数据和 Tool 输出均被视为不可信文本。system prompt 必须明确要求模型忽略其中的指令性内容；模型只能依据当前用户请求选择已注册工具。
5. run 的终态必须正确：纯文本回答和已完成 Tool loop 进入 `completed`；等待写入确认进入 `waiting_approval`；拒绝进入 `cancelled`；provider、schema 或持久化失败进入 `failed`。不得遗留长期 `running` 的 run。

### 2.5 必须先过的三道就绪门槛

下面三道门槛不是“后续优化”，而是切换旧产品入口和删除旧链路前的硬性前置条件。任何一个门槛未通过，都只能保留旧入口和旧实现，不能继续做删除工作。

1. **云端能力门槛：** Rust provider 能完成文本、stream、tool-call、超时、401/429 和空响应的稳定错误映射；API Key 只经 OS keyring/DPAPI；旧 fallback secret 不再被新代码读取或创建；未获得当前配置的数据出境同意时，业务上下文不会出境。
2. **Agent 安全门槛：** 上下文快照包含真正的会话历史和业务摘要；所有 Tool 在执行前校验 run/session/exam/资源归属；R3 审批的“确认”会真正执行已存储的动作，取消不会写库；run 不会停留在 `running`。
3. **产品等价门槛：** Rust 计划 Draft 能覆盖现有 TypeScript 计划生成器实际支持的日期范围、科目、知识点、阶段、容量和冲突行为；Draft 预览/应用通过 fixture 等价测试后，才允许删除 TypeScript 生成链。

### 2.6 强制执行顺序

执行助手必须按以下顺序推进，并在每个阶段结束时运行全量测试：

```text
基线与备份
  → Provider 能力测试、key 安全、数据出境同意
  → ContextSnapshot、会话历史、run 生命周期、exam scope guard
  → Plan Draft preview/apply、R3 审批执行闭环、行为等价 fixture
  → readiness E2E：无 LLM / provider 成功 / 只读 / 写入取消与确认 / 跨考试拒绝
  → 通过门槛后才收敛路由和隐藏旧入口
  → 通过零引用和等价测试后才删除 TypeScript AI 链路
  → 最后缩减 Scheduler、debug、依赖和 GUI 视觉层
```

禁止先删除 `plan-generator.ts`、`llm-adapter.ts`、Scheduler 或旧页面再“边改边补测试”。这会让失败时无法区分是产品行为变化、provider 问题还是清理造成的回归。

## 3. 文件责任地图

### 3.1 保留并作为唯一职责来源

- D:/智研/zhiyan/src/services/plan-service.ts：计划查询和本地 CRUD。
- D:/智研/zhiyan/src/services/record-service.ts：学习记录查询和写入。
- D:/智研/zhiyan/src/services/analyzer.ts：只保留确定性统计和聚合，不保留 AI 文案。
- D:/智研/zhiyan/src-tauri/src/agent/planner.rs：唯一 Agent/LLM 编排入口。
- D:/智研/zhiyan/src-tauri/src/agent/llm/：唯一 LLM provider 实现。
- D:/智研/zhiyan/src-tauri/src/agent/tools/：唯一 Tool schema 和数据权限边界。
- D:/智研/zhiyan/src-tauri/src/agent/executor.rs：保留事务、幂等、审批和撤销；后续只做收缩，不在第一阶段直接删除。
- D:/智研/zhiyan/src-tauri/src/credentials.rs：继续使用 OS keyring/DPAPI。
- D:/智研/zhiyan/src/services/export.ts：继续负责 JSON 导入导出和数据库备份恢复。

### 3.2 迁移后删除的 TypeScript 文件

只有在 rg 显示无生产 import、无测试 import、无动态 route 引用后，按以下顺序删除：

- D:/智研/zhiyan/src/components/plan/PlanChatAssistant.vue
- D:/智研/zhiyan/src/services/plan-chat-agent.ts
- D:/智研/zhiyan/src/components/plan/PlanGenerateDialog.vue（当前未被其他组件引用，删除前再次确认）
- D:/智研/zhiyan/src/services/plan-generator.ts
- D:/智研/zhiyan/src/services/plan-generator.test.ts
- D:/智研/zhiyan/src/pages/Analysis.vue
- D:/智研/zhiyan/src/stores/analysis.ts
- D:/智研/zhiyan/src/services/agent-engine.ts
- D:/智研/zhiyan/src/services/search.ts（仅在计划搜索链路删除且无其他调用后）
- D:/智研/zhiyan/src/services/llm-adapter.ts

### 3.3 迁移后删除或改为开发构建的 Rust 能力

第一轮不删历史 migration 和表，只删除运行时引用：

- memory command、MemoryRepository 和 agent_memories 的正常运行时依赖；
- job command、Scheduler 常驻 tick 和 agent_jobs 的正常运行时依赖；
- context audit UI 和 command；表可以暂时保留用于迁移兼容和隐私测试；
- ToolOwnership::Typescript/Shadow/Unavailable 和 agent_tool_owner.* 设置，只保留 Rust-owned registry；
- AgentDebug.vue 正式构建 route，开发构建仍可保留诊断入口。

不要直接从 db.rs 删除旧表定义，不要改写已经执行过的 migration。数据表清理属于单独的大版本任务，不属于本次首轮切换。

## 4. 阶段一：建立基线和回滚点

### Task 1: 创建切换分支并记录基线

**Files:**

- Create: D:/智研/zhiyan/docs/agent/cloud-llm-cutover-baseline.md
- Read: D:/智研/zhiyan/package.json
- Read: D:/智研/zhiyan/src-tauri/Cargo.toml

- [ ] Step 1: 创建分支并确认工作树

~~~powershell
git status --short
git switch -c codex/cloud-llm-agent-simplification
~~~

如果工作树存在用户未提交修改，不得清理、reset 或覆盖；先记录路径并在同一工作树上继续。

- [ ] Step 2: 运行 TypeScript 基线

~~~powershell
npm.cmd test -- --run
npm.cmd run typecheck
~~~

Expected：测试通过，当前基线约为 14 个测试文件、75 个测试；typecheck 退出码为 0。

- [ ] Step 3: 运行 Rust 基线

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib
~~~

Expected：退出码为 0，当前基线约为 114 个 Rust tests passed。

- [ ] Step 4: 记录调用图基线

~~~powershell
rg -n "PlanChatAssistant|plan-chat-agent|plan-generator|agent-engine|llm-adapter|agent_run_planner|agent_jobs|agent_memories|agent_context_audit" src src-tauri
~~~

将结果摘要写入 cloud-llm-cutover-baseline.md，并记录当前 git commit、Node、Rust、npm 版本。

- [ ] Step 5: 提交基线文档

~~~powershell
git add docs/agent/cloud-llm-cutover-baseline.md
git commit -m "docs: record cloud llm cutover baseline"
~~~

验收：任何后续助手都能从该文件知道改造前的测试结果和调用链数量。

### Mandatory Task A：打通云端 provider 能力、安全和同意契约

**依赖：** Task 1 完成；**阻塞：** Task 2–5、Task 9–10 不得进入删除步骤。

**Files：**

- Modify: D:/智研/zhiyan/src-tauri/src/credentials.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/llm/openai_compatible.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/commands.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/planner.rs
- Modify: D:/智研/zhiyan/src-tauri/src/lib.rs
- Modify: D:/智研/zhiyan/src/stores/settings.ts
- Modify: D:/智研/zhiyan/src/pages/Settings.vue
- Test: D:/智研/zhiyan/src-tauri/src/agent/llm/*
- Test: D:/智研/zhiyan/src-tauri/src/credentials.rs

- [ ] Step 1: 只允许 Rust 读取密钥

确认 provider 请求的 Authorization 只在 Rust provider 内存中组装；前端只提交非敏感配置和“测试/保存”意图。新代码不得创建或读取 `*_api_key_fallback`；旧 fallback 只能触发重新输入提示。

- [ ] Step 2: 完成 capability test

用固定诊断 prompt 验证 text stream 和 `diagnostics.ping` tool-call，覆盖 provider 缺失、keyring 缺失、401、403、429、timeout、空响应、stream 中断、协议不兼容。测试断言请求中没有 exam/plan/record/message 数据，错误和日志不包含 key、URL、body。

- [ ] Step 3: 完成配置指纹和数据同意

Rust 计算并校验 provider/base URL/model/policy version 的 fingerprint；未同意时允许固定 diagnostic 请求，禁止真实业务上下文；配置变化后必须重新确认。前端只显示状态，不承担安全拦截。

- [ ] Step 4: 运行门槛测试

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib agent::llm
cargo test --manifest-path src-tauri/Cargo.toml --lib credentials
npm.cmd test -- --run src/stores/settings.test.ts src/services/agent-client.test.ts
npm.cmd run typecheck
~~~

- [ ] Step 5: 提交能力门槛

~~~powershell
git add src-tauri/src/credentials.rs src-tauri/src/agent/llm/openai_compatible.rs src-tauri/src/agent/commands.rs src-tauri/src/agent/planner.rs src-tauri/src/lib.rs src/stores/settings.ts src/pages/Settings.vue
git commit -m "feat: harden cloud provider capability and consent"
~~~

验收：没有 keyring/配置/同意时，业务数据不会离开本地；provider 能力和错误码可被前端稳定消费。

### Mandatory Task B：补齐上下文、隔离和 run 生命周期

**依赖：** Mandatory Task A；**阻塞：** 任何云端 Agent Tool 和写入动作。

**Files：**

- Create: D:/智研/zhiyan/src-tauri/src/agent/context_snapshot.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/context.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/repository.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/executor.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/planner.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/state.rs
- Test: D:/智研/zhiyan/src-tauri/src/agent/context_snapshot.rs
- Test: D:/智研/zhiyan/src-tauri/src/agent/executor.rs
- Test: D:/智研/zhiyan/src-tauri/src/agent/state.rs

- [ ] Step 1: 建立 bounded ContextSnapshot

读取当前 session 最近 12 条持久化消息、当前考试、今日最多 20 条计划、最近 14 天最多 20 条记录、最多 10 条弱项/待复习错题；限制 history、每个 Tool output 和总 prompt 的字节数。`ContextScope/context_audit` 仍只做 provenance，不当作模型上下文。

- [ ] Step 2: 建立 run scope guard

从 `agent_runs.session_id` 取得绑定 exam；所有输入中的 exam/subject/knowledge point/plan/wrong question 都验证属于该 exam。跨考试读取和写入统一返回 `tool_scope_violation`，不执行 SQL mutation。为 exam A/exam B 建立双考试测试。

- [ ] Step 3: 补齐 run 状态迁移

确保纯文本、Tool loop、waiting approval、approve、reject、cancel、provider error、schema error、持久化 error、进程恢复都有明确终态；command 返回前不能留下长期 `running`。

- [ ] Step 4: 运行门槛测试

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib agent::context_snapshot
cargo test --manifest-path src-tauri/Cargo.toml --lib agent::executor
cargo test --manifest-path src-tauri/Cargo.toml --lib agent::state
cargo test --manifest-path src-tauri/Cargo.toml --lib agent
~~~

- [ ] Step 5: 提交安全门槛

~~~powershell
git add src-tauri/src/agent/context_snapshot.rs src-tauri/src/agent/context.rs src-tauri/src/agent/repository.rs src-tauri/src/agent/executor.rs src-tauri/src/agent/planner.rs src-tauri/src/agent/state.rs
git commit -m "feat: bound agent context scope and run lifecycle"
~~~

验收：provider mock 实际收到会话历史和受限业务摘要；跨考试请求无数据泄露、无数据库变化；所有 run 可观察且可恢复。

### Mandatory Task C：计划 Draft、审批执行和等价切换

**依赖：** Mandatory Task B；**阻塞：** Task 5、Task 10、Task 19。

**Files：**

- Modify: D:/智研/zhiyan/src-tauri/src/agent/tools/plan.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/executor.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/commands.rs
- Modify: D:/智研/zhiyan/src/services/agent-client.ts
- Modify: D:/智研/zhiyan/src/stores/agent.ts
- Create: D:/智研/zhiyan/src-tauri/src/agent/plan_draft.rs
- Test: D:/智研/zhiyan/src-tauri/src/agent/tools/plan.rs
- Test: D:/智研/zhiyan/src-tauri/src/agent/executor.rs

- [ ] Step 1: 定义 preview/apply 协议

`plan.preview_generate` 只返回本地生成的 Draft；`plan.apply_preview` 只接收已保存的 draft/preview step ID。优先复用现有 `agent_steps.output_json` / `agent_approvals` 的 JSON 字段保存 Draft，不新增表；只有现有 schema 无法表达时才提出最小兼容字段并补 migration 设计。Draft 包含真实行、冲突、摘要、版本、precondition hash 和 idempotency key；LLM 不能自由提供待写入行列表。

- [ ] Step 2: 修复审批闭环

新增 `agent_resolve_approval` 或等价 command。approve 从数据库加载原始 step/preview，重新做 scope、precondition、schema、idempotency 校验后执行；reject 只更新审批状态。前端 approve 不能只调用 state-only 的 `agent_decide_approval`。

- [ ] Step 3: 完成旧计划生成器 fixture 等价测试

覆盖旧生成器实际支持的日期范围、科目、知识点、阶段、考试日期、容量、覆盖和冲突；明确记录任何有意降级。只有 preview/cancel/apply/重复确认和 fixture 等价全部通过，才允许删除 TypeScript 计划链。

- [ ] Step 4: 运行门槛测试

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib agent::tools::plan
cargo test --manifest-path src-tauri/Cargo.toml --lib agent::executor
npm.cmd test -- --run src/pages/AgentHome.test.ts
npm.cmd run typecheck
~~~

- [ ] Step 5: 提交 Agent 协议门槛

~~~powershell
git add src-tauri/src/agent/tools/plan.rs src-tauri/src/agent/plan_draft.rs src-tauri/src/agent/executor.rs src-tauri/src/agent/commands.rs src/services/agent-client.ts src/stores/agent.ts src/pages/AgentHome.test.ts
git commit -m "feat: add draft apply and executable approvals"
~~~

验收：取消无写入，确认只写 Draft 中的内容，重复确认幂等，过期 Draft 安全失败；R3 写入确认确实完成动作。

## 5. 阶段二：先收敛产品入口，不删除底层代码

这一阶段只改变用户可见信息架构，保留旧代码和旧数据库，让产品可以随时回滚。

### Task 2: 收敛主导航和旧路由

**Files:**

- Modify: D:/智研/zhiyan/src/components/layout/AppLayout.vue:30-39
- Modify: D:/智研/zhiyan/src/router/index.ts:4-48
- Modify: D:/智研/zhiyan/src/pages/Settings.vue:27-38
- Test: D:/智研/zhiyan/src/router/index.test.ts（若不存在则创建）

- [ ] Step 1: 为目标导航写失败测试

测试必须验证普通导航只包含以下路径：

~~~ts
expect(menuItems.map((item) => item.index)).toEqual([
  '/agent',
  '/study-plan',
  '/study-record',
  '/exam-config',
  '/settings',
])
~~~

同时验证 /dashboard、/analysis、/visualization 不再出现在普通菜单。

- [ ] Step 2: 修改 AppLayout 菜单

保留 Agent、学习计划、学习记录、考试配置、设置；删除 Dashboard、AI 分析、数据可视化菜单项。菜单顺序固定为 Agent → Plan → Records → Setup → Settings。

- [ ] Step 3: 为旧入口增加可回滚重定向

路由在迁移期仍然可解析，但不再加载旧页面作为用户入口：

~~~ts
{ path: '/dashboard', redirect: '/agent' },
{ path: '/analysis', redirect: '/agent' },
{ path: '/visualization', redirect: '/agent' },
~~~

不要立即删除旧页面文件，因为旧测试和用户书签需要一个过渡期。

- [ ] Step 4: 收敛 Agent OS fallback 开关

通过就绪门槛后，根路由 `/` 必须始终进入 `/agent`。不要继续让旧用户数据库里的 `agent_os_enabled=0` 把用户送回 Dashboard；该设置键可以保留用于数据兼容和审计，但正式路由不得再用它决定默认入口。若确实需要紧急回滚，只能通过开发构建/版本回滚恢复，不在生产用户路径中保留第二套首页。

新增测试：已有 `agent_os_enabled=0` 的 settings 数据仍进入 `/agent`；`/dashboard`、`/analysis`、`/visualization`、不存在的旧路径统一重定向 `/agent`；开发诊断路由不进入生产构建。

- [ ] Step 5: 运行测试并提交

~~~powershell
npm.cmd test -- --run
npm.cmd run typecheck
git add src/components/layout/AppLayout.vue src/router/index.ts src/pages/Settings.vue src/router/index.test.ts
git commit -m "feat: make agent the single product entry"
~~~

验收：应用启动后默认进入 Agent；普通用户无法从菜单进入旧 Dashboard、AI 分析和可视化页面；直接访问旧 URL 不会 404。

### Task 3: 删除 Agent 侧的旧页面工作台入口

**Files:**

- Modify: D:/智研/zhiyan/src/components/agent/AgentSidebar.vue
- Modify: D:/智研/zhiyan/src/components/agent/workbench.ts
- Modify: D:/智研/zhiyan/src/components/agent/WorkbenchHost.vue
- Modify: D:/智研/zhiyan/src/pages/AgentHome.vue
- Test: D:/智研/zhiyan/src/pages/AgentHome.test.ts

- [ ] Step 1: 把 workbench 数量锁定为三个

目标 WORKBENCHES 只允许：

~~~ts
export const WORKBENCHES = [
  { key: 'checkin', label: '今日打卡' },
  { key: 'plan', label: '计划' },
  { key: 'record', label: '记录' },
] as const
~~~

删除 analysis 和 visualization workbench；分析结果不再作为一个页面嵌入 Agent。

- [ ] Step 2: 删除侧栏中的旧页面 deep links

AgentSidebar 只保留新建会话、会话列表和设置入口。删除 Dashboard、学习计划、学习记录、Agent Debug 的重复导航；Plan 和 Records 从右侧 workbench 进入。

- [ ] Step 3: 修改 AgentHome 的布局契约

保留左侧会话、中央对话/今日状态、右侧单一 workbench。右侧不再加载 Analysis.vue 和 Visualization.vue。切换 workbench 时不能重置会话和当前消息。

- [ ] Step 4: 写页面行为测试

~~~ts
expect(wrapper.find('[data-test="workbench-tab-analysis"]').exists()).toBe(false)
expect(wrapper.find('[data-test="workbench-tab-visualization"]').exists()).toBe(false)
expect(wrapper.find('[data-test="workbench-tab-plan"]').exists()).toBe(true)
expect(wrapper.find('[data-test="workbench-tab-record"]').exists()).toBe(true)
~~~

- [ ] Step 5: 运行测试并提交

~~~powershell
npm.cmd test -- --run src/pages/AgentHome.test.ts
npm.cmd run typecheck
git add src/components/agent/AgentSidebar.vue src/components/agent/WorkbenchHost.vue src/components/agent/workbench.ts src/pages/AgentHome.vue src/pages/AgentHome.test.ts
git commit -m "feat: reduce agent workbench to core study surfaces"
~~~

验收：Agent 不再是旧系统页面的容器；Agent 侧只表达 Today、Plan、Records 三种任务。

## 6. 阶段三：收敛计划产品

### Task 4: 让计划页面只负责查看和编辑

**Files:**

- Modify: D:/智研/zhiyan/src/pages/StudyPlan.vue
- Modify: D:/智研/zhiyan/src/stores/plan.ts
- Modify: D:/智研/zhiyan/src/components/plan/PlanCalendar.vue
- Modify: D:/智研/zhiyan/src/components/plan/PlanList.vue
- Test: D:/智研/zhiyan/src/pages/StudyPlan.test.ts（若不存在则创建）

- [ ] Step 1: 删除页面内 AI 对话入口

从 StudyPlan.vue 删除 PlanChatAssistant import、assistantVisible、assistantExam、openAssistant、onGenerated 和 AI 生成按钮。页面保留“新建计划任务”和“手动编辑”。

- [ ] Step 2: 只保留日历和列表视图

validViews 改为 ['calendar', 'list']；删除 Gantt、Compare 页签及其 URL 入口。页面默认 calendar，列表用于批量编辑和排序。

- [ ] Step 3: 清理 Plan Store 的 TypeScript 生成依赖

删除 src/stores/plan.ts 对 plan-generator 的 import、lastResult、generatePlan 和仅用于 AI 生成的类型。保留 loadPlansByDateRange、loadTodayTasks、updatePlanStatus、updatePlan、reorderPlans。

- [ ] Step 4: 确认考试配置页不再生成计划

从 ExamConfig.vue 删除 PlanChatAssistant。考试创建完成后只显示“进入今日 Agent”或“进入计划”，不自动启动云端请求。

- [ ] Step 5: 写页面边界测试

~~~ts
expect(wrapper.findComponent({ name: 'PlanChatAssistant' }).exists()).toBe(false)
expect(wrapper.text()).not.toContain('AI 生成 / 重新生成计划')
expect(wrapper.text()).toContain('日历')
expect(wrapper.text()).toContain('列表')
~~~

- [ ] Step 6: 运行测试并提交

~~~powershell
npm.cmd test -- --run
npm.cmd run typecheck
git add src/pages/StudyPlan.vue src/pages/ExamConfig.vue src/stores/plan.ts src/components/plan/PlanCalendar.vue src/components/plan/PlanList.vue
git commit -m "feat: make plan page deterministic and agent-driven"
~~~

验收：所有计划生成请求只能从 Agent 发起；Plan 页面不再包含第二个聊天产品。

### Task 5: 删除 TypeScript 计划生成链路

**Files:**

- Delete: D:/智研/zhiyan/src/services/plan-chat-agent.ts
- Delete: D:/智研/zhiyan/src/services/plan-generator.ts
- Delete: D:/智研/zhiyan/src/services/plan-generator.test.ts
- Delete: D:/智研/zhiyan/src/components/plan/PlanChatAssistant.vue
- Delete: D:/智研/zhiyan/src/components/plan/PlanGenerateDialog.vue

- [ ] Step 1: 删除前做零引用扫描

~~~powershell
rg -n "plan-chat-agent|PlanChatAssistant|plan-generator|PlanGenerateDialog|generateAIPlan|generatePlan\(" src
~~~

Expected：只有待删除文件自身和已经更新的历史文档引用；不得存在生产 import。

- [ ] Step 2: 先完成 Rust Draft 的行为等价门槛

~~~powershell
rg -n "plan\.generate|generate_descriptor|agent_run_planner|applyGeneratedPlan|generateAIPlan" src-tauri/src src
~~~

当前 Rust `plan.generate` v1 只接受 `exam_id/week_start/daily_capacity_min`，会生成七天、每天一个科目的本地草案，不能直接替代旧生成器。删除前必须完成 Mandatory Task C 的纯函数 Draft 管线；本 Task 只消费并验证该协议，不再另建一套计划写入协议：

1. `plan.preview_generate` 只读取本地数据并返回 `PlanDraft`，包含日期范围、科目/知识点分配、阶段、容量、覆盖策略、冲突、摘要、版本和 precondition hash；不写 `study_plans`。
2. Draft 生成逻辑必须覆盖旧 `plan-generator.ts` 的真实行为：非一周日期范围、科目与知识点轮换、阶段/考试日期、每日容量、已有计划覆盖与冲突。对旧实现建立固定 fixture，逐项比较结果；如果产品明确接受“滚动七天、每天一个科目”的降级能力，必须在产品契约中写明并由验收用例接受，不能默默降级。
3. `plan.apply_preview` 只接受本地已保存的 preview step/draft ID，不接受 LLM 自由提交的行列表；执行时重新校验 exam scope、precondition hash、schema 和 idempotency，再由 Rust transaction 写入。
4. 只有 Rust Draft fixture、preview/cancel/apply/重复确认测试全部通过后，才能把旧生成器标记为 deprecated 并删除。

- [ ] Step 3: 删除文件并清理 package 依赖候选

先删除源文件和测试，再运行 rg。`src/services/prompts.ts` 与 `.test.ts` 仍可能被 agent-engine 使用，留到 Task 14 在 agent-engine 清理时处理。不要在这个 Task 直接删除 `@tauri-apps/plugin-http`，因为 Settings 和 search 可能仍然引用它。

- [ ] Step 4: 运行测试并提交

~~~powershell
npm.cmd test -- --run
npm.cmd run typecheck
git add src/components/plan/PlanChatAssistant.vue src/components/plan/PlanGenerateDialog.vue src/services/plan-chat-agent.ts src/services/plan-generator.ts src/services/plan-generator.test.ts src/stores/plan.ts src/pages/StudyPlan.vue src/pages/ExamConfig.vue
git commit -m "refactor: remove duplicate typescript plan llm path"
~~~

验收：前端不再 import LLMConfig 来生成计划；所有计划生成都可在 Agent 对话中解释、预览和确认。

## 7. 阶段四：统一 LLM provider 到 Rust

### Task 6: 增加 Rust provider connection test command

**Files:**

- Modify: D:/智研/zhiyan/src-tauri/src/agent/commands.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/planner.rs
- Modify: D:/智研/zhiyan/src-tauri/src/lib.rs
- Modify: D:/智研/zhiyan/src/services/agent-client.ts
- Modify: D:/智研/zhiyan/src/pages/Settings.vue
- Test: D:/智研/zhiyan/src-tauri/src/agent/commands.rs
- Test: D:/智研/zhiyan/src/services/agent-client.test.ts

- [ ] Step 1: 先为 provider test 定义返回类型

Rust 和 TypeScript 使用同一结构：

~~~rust
#[derive(Debug, Clone, Serialize)]
pub struct ProviderTestResult {
    pub model: String,
    pub latency_ms: i64,
    pub text_stream: bool,
    pub tool_call: bool,
    pub error_code: Option<String>,
}
~~~

命令名固定为 `agent_test_provider`，无参数，从 settings 和 keyring 读取当前配置。返回值是能力测试结果，不是“收到 OK 就算成功”。

- [ ] Step 2: 在 Planner 增加测试方法

该方法必须：

1. 调用现有 build_provider()；
2. provider 缺失时返回 ProviderUnavailable；
3. 先发送固定诊断文本请求，验证 stream 能收到完整结束事件；
4. 再发送固定 `diagnostics.ping` schema 和固定 prompt，验证 provider 能返回合法 tool-call；不携带考试、计划、记录、错题或会话内容；
5. 将能力失败映射为稳定错误码：`provider_unavailable`、`provider_auth_failed`、`provider_rate_limited`、`provider_timeout`、`provider_protocol_error`、`provider_request_failed`；
6. 记录耗时和能力布尔值；
7. 不把 API Key、完整响应正文、请求 body 或 URL 写入错误信息和普通日志；
8. 使用现有 OpenAiCompatibleProvider，不新增前端 HTTP client。

- [ ] Step 3: 注册 Tauri command 和前端 client

在 lib.rs command 列表注册 agent_test_provider，在 agent-client.ts 添加：

~~~ts
export interface ProviderTestResult {
  model: string
  latency_ms: number
  text_stream: boolean
  tool_call: boolean
  error_code?: string
}

export function testAgentProvider(): Promise<ProviderTestResult> {
  return invoke<ProviderTestResult>('agent_test_provider')
}
~~~

- [ ] Step 4: 修改 Settings 的测试按钮

删除 Settings.vue 对 src/services/llm-adapter.ts 的 import 和 callLLM 调用，改为 testAgentProvider。测试按钮不能读取或传递 API Key 到前端 HTTP 请求。保存配置后测试“已保存配置”；若产品需要测试未保存草稿，必须由 Rust 接收短生命周期的非持久化配置并明确禁止把 key 返回给前端，而不能在前端自行拼 HTTP 请求。

- [ ] Step 5: 添加 provider 错误测试

至少测试：未配置 provider、缺失 key、401/403、429、网络失败、超时、空响应、stream 中断、tool-call schema 不兼容、成功文本 stream 和成功 `diagnostics.ping`。错误输出只能是稳定错误码；fixture 和日志断言不得包含 key、Authorization、完整 URL 或业务数据。

- [ ] Step 6: 运行 Rust/TypeScript 测试并提交

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib agent::commands
cargo test --manifest-path src-tauri/Cargo.toml --lib agent::llm
npm.cmd test -- --run src/services/agent-client.test.ts
npm.cmd run typecheck
git add src-tauri/src/agent/commands.rs src-tauri/src/agent/planner.rs src-tauri/src/agent/llm/openai_compatible.rs src-tauri/src/lib.rs src/services/agent-client.ts src/services/agent-client.test.ts src/pages/Settings.vue
git commit -m "refactor: route provider testing through rust agent"
~~~

验收：Settings、Agent、Brief 使用同一个 Rust provider；前端不再直接向 LLM endpoint 发请求。

### Task 7: 收缩 Settings 为单一 OpenAI-compatible 配置

**Files:**

- Modify: D:/智研/zhiyan/src/pages/Settings.vue
- Modify: D:/智研/zhiyan/src/stores/settings.ts
- Modify: D:/智研/zhiyan/src/types/index.ts:125-132
- Modify: D:/智研/zhiyan/src-tauri/src/credentials.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/commands.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/planner.rs
- Test: D:/智研/zhiyan/src/stores/settings.test.ts

- [ ] Step 1: 保留兼容字段，但统一产品语义

继续使用现有 settings keys：llm_provider、llm_base_url、llm_model、llm_temperature。不改历史 key 名，避免旧用户丢配置。

- [ ] Step 2: 简化 provider preset

UI 保留 OpenAI、DeepSeek、通义、Kimi、自定义五个 OpenAI-compatible preset；移除 Ollama 的可选项，因为当前 Rust Planner 对 Ollama 明确降级到本地模式，不符合“云端 Agent”目标。已有 Ollama 配置可以继续被读取，但设置页要显示“本地模型暂不支持 Agent Tool”，不能静默清空或覆盖旧配置。

- [ ] Step 3: 约束表单验证

连接保存前必须验证：baseUrl 为绝对 http/https URL、model 非空、云端 provider 的 API Key 非空、temperature 在 0..=2。API Key 继续由 settings.ts 调用 keyring command 保存。

保存顺序固定为“先写入 keyring，再写入非敏感 settings”；keyring 失败时不得保存新配置。新代码不得读取或创建现有 `*_api_key_fallback` SQLite secret；发现旧 fallback 时显示“请重新输入 API Key”，不静默解密、不发送，用户成功重新保存到 keyring 后删除旧 fallback。测试覆盖 keyring 不可用、旧 fallback 存在、重新保存和日志脱敏。

- [ ] Step 4: 实现 provider 同意指纹

首次发送真实业务上下文前，Settings/Agent 必须展示数据范围并由用户确认；Rust 保存 `cloud_llm_consent_fingerprint`，指纹由 provider、规范化 base URL、model 和固定 policy version 做 SHA-256。任一配置或 policy 变化都使同意失效。未匹配指纹时，Planner 只允许本地回答或返回明确的 consent_required，不得把业务数据放进 provider 请求。

- [ ] Step 5: 删除无关 AnySearch 配置

只有在 plan-chat-agent.ts 和 search.ts 零引用确认后，删除 Settings 中 AnySearch key 的读取、保存和 UI。不要误删 LLM provider keyring 逻辑。

- [ ] Step 6: 测试并提交

~~~powershell
npm.cmd test -- --run src/stores/settings.test.ts
npm.cmd run typecheck
git add src/pages/Settings.vue src/stores/settings.ts src/types/index.ts src-tauri/src/credentials.rs src-tauri/src/agent/commands.rs src-tauri/src/agent/planner.rs
git commit -m "feat: simplify llm settings to cloud provider only"
~~~

验收：用户只需要配置一个云端 provider；测试按钮使用 Rust command；旧用户的 provider/base URL/model/key 仍然能加载。

### Task 8: 删除 TypeScript LLM adapter

**Files:**

- Delete after reference scan: D:/智研/zhiyan/src/services/llm-adapter.ts
- Delete related tests after migration: D:/智研/zhiyan/src/services/llm-adapter.test.ts
- Modify: D:/智研/zhiyan/package.json
- Modify: D:/智研/zhiyan/package-lock.json 或实际使用的 lockfile

- [ ] Step 1: 执行零引用扫描

~~~powershell
rg -n "services/llm-adapter|callLLM|callLLMWithTools|parseJsonResponse|plugin-http" src
~~~

Expected：无生产调用。若仍有调用，先迁移调用方，不允许用 any 或动态 import 绕过扫描。

- [ ] Step 2: 删除 adapter 和测试

删除 TypeScript adapter 后，保留 Rust provider 的 mock/httpmock 测试。

- [ ] Step 3: 删除前端 HTTP 依赖

只有在 @tauri-apps/plugin-http 无任何 source/test import 后，才从 package.json 删除它并重新生成 lockfile。Rust reqwest 保留。

- [ ] Step 4: 验证并提交

~~~powershell
npm.cmd test -- --run
npm.cmd run typecheck
npm.cmd run build
git add package.json package-lock.json src/services/llm-adapter.ts src/services/llm-adapter.test.ts src/pages/Settings.vue src/stores/settings.ts src/types/index.ts
git commit -m "refactor: remove duplicate frontend llm adapter"
~~~

验收：整个前端代码库不存在 LLM endpoint、fetch、Bearer key 或 provider retry 实现。

## 8. 阶段五：让 Agent 成为唯一 AI 产品能力

### Task 9: 将 AI 分析并入 Agent，保留本地统计

**Files:**

- Modify: D:/智研/zhiyan/src-tauri/src/agent/planner.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/context.rs
- Create: D:/智研/zhiyan/src-tauri/src/agent/context_snapshot.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/repository.rs
- Modify: D:/智研/zhiyan/src/components/agent/ConversationPane.vue
- Modify: D:/智研/zhiyan/src/components/agent/DailyBrief.vue
- Modify: D:/智研/zhiyan/src/services/analyzer.ts
- Test: D:/智研/zhiyan/src/pages/AgentHome.test.ts
- Test: D:/智研/zhiyan/src-tauri/src/agent/planner.rs

- [ ] Step 1: 把本地统计定义成事实层

保留并测试以下数据：今日计划数、今日完成数、学习时长、逾期数、本周完成率、待复习错题数、弱项列表。它们必须来自 SQL/Analytics，不能由 LLM 计算。

- [ ] Step 2: 建立真正会发送给模型的 ContextSnapshot

`ContextScope/context_audit` 只记录来源 ID，不能冒充模型上下文。复用 Mandatory Task B 已建立的 `ContextSnapshot` builder，显式注入当前考试摘要、当前日期计划、近期记录、待复习错题/薄弱项和同一 session 的持久化历史消息；planner 必须把它放进发送给 provider 的 message，而不是只写审计表。不要在 Task 9 另建第二套上下文结构。

固定上限：同一 session 最近 12 条消息；当天最多 20 条计划；最近 14 天最多 20 条记录；最多 10 条薄弱项/待复习错题；history、每个 Tool output 和总 prompt 都有字符/字节上限，超限时本地截断并记录计数。业务文本和 Tool 输出一律当作不可信数据，system prompt 明确忽略其中的指令性内容。

新增测试：断言 provider mock 实际收到 session 历史和业务摘要；断言超过上限会截断；断言敏感字段不进入 snapshot；无 consent 时 builder 可以生成本地摘要但 planner 拒绝发送。

- [ ] Step 3: 移除 TypeScript AI 文案入口

当旧 /analysis 已重定向且 agent-engine.ts 无调用后，删除 runDailyAnalysis、runWeeklyAnalysis、runPhaseAnalysis 和 runPendingAnalyses。analyzer.ts 只保留确定性函数；删除仅用于生成 prompt、prediction narrative、Recommendation 文案的函数。

- [ ] Step 4: Daily Brief 改为本地即时摘要

agent_brief_preview 默认只做本地统计，不再自动调用一次 LLM。打开 Today 时显示事实摘要；用户点击“解释一下”或在 Agent 中询问时，才让 Planner 使用同一份上下文生成语言说明。

- [ ] Step 5: 无云端配置时给出明确状态

禁止显示“本地模式”但让用户误以为已经使用模型。前端显示：

~~~text
尚未连接云端模型。你仍可查看和编辑本地数据；连接模型后可使用对话、计划调整和智能复盘。
~~~

- [ ] Step 6: 更新 Planner 测试

测试必须覆盖：

1. provider 成功时，模型可以调用只读 Tool 后回答；
2. 写 Tool 返回 approval 时，前端显示预览而不是直接变更；
3. provider 缺失时，不写入任何业务表；
4. provider 错误时，不伪造“模型已分析”的内容；
5. 统计数字来自本地 context，不允许由 prompt 外部拼接未经校验的数字。
6. session 历史会进入 provider message，且只包含当前 session 的最近 12 条；跨 session、跨 exam 的消息不可见。
7. run 绑定的 exam 与所有 Tool 输入不一致时，返回稳定的 scope violation，不执行数据库写入。
8. 文本回答结束、Tool loop 结束、等待审批、取消、provider/schema/持久化失败分别落到 `completed`、`waiting_approval`、`cancelled` 或 `failed`，不存在遗留 `running`。

- [ ] Step 6: 运行测试并提交

~~~powershell
npm.cmd test -- --run
cargo test --manifest-path src-tauri/Cargo.toml --lib
npm.cmd run typecheck
git add src-tauri/src/agent/planner.rs src-tauri/src/agent/context.rs src-tauri/src/agent/context_snapshot.rs src-tauri/src/agent/repository.rs src-tauri/src/brief.rs src/components/agent/ConversationPane.vue src/components/agent/DailyBrief.vue src/services/analyzer.ts src/pages/AgentHome.test.ts
git commit -m "feat: consolidate analysis into agent conversation"
~~~

验收：用户不再需要选择“日报/周报/阶段分析”；一句“我这周学习情况怎么样”即可触发同一套 Agent 能力。

### Task 10: 统一计划生成、计划调整和记录整理动作

**Files:**

- Modify: D:/智研/zhiyan/src-tauri/src/agent/tools/plan.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/tools/record.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/tools/wrong_question.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/executor.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/commands.rs
- Modify: D:/智研/zhiyan/src/services/agent-client.ts
- Modify: D:/智研/zhiyan/src/stores/agent.ts
- Modify: D:/智研/zhiyan/src/types/index.ts
- Modify: D:/智研/zhiyan/src/components/agent/ApprovalCard.vue
- Create: D:/智研/zhiyan/src/components/agent/AgentActionPreview.vue
- Modify: D:/智研/zhiyan/src/components/agent/ConversationPane.vue
- Test: 对应 Rust tool tests 和 D:/智研/zhiyan/src/pages/AgentHome.test.ts

- [ ] Step 1: 固定 Agent 支持的最小动作集合

第一版只支持以下现有工具：

~~~text
exam.get_active
plan.get_range
plan.get_today
plan.preview_generate
plan.apply_preview
record.get_history
record.checkin_plan
record.create_free
wrong_question.create
wrong_question.mark_mastered
~~~

旧 `plan.generate` v1 在行为等价切换完成前保留为 deprecated compatibility path；不新增搜索、可视化、报表、记忆、任意数据库操作 Tool。

- [ ] Step 2: 为每个写动作定义预览字段

预览必须至少包含：动作名称、影响对象数量、日期范围、原值/新值摘要、结构化字段、冲突、风险、确认按钮、取消按钮、`undo_available`。预览由 Rust 生成并脱敏，前端不得通过 `Object.entries` 猜测模型返回对象，也不得展示或记录 API Key。计划预览必须包含实际 Draft 行和 precondition hash，而不是只显示“调用 plan.generate”。

所有由模型触发的 `plan.apply_preview`、`record.checkin_plan`、`record.create_free`、`wrong_question.create`、`wrong_question.mark_mastered` 都必须声明 `confirmation=Required`、R3；不得依赖 `agent_r2_auto_execute`。手动表单可以走独立本地 service，但不能复用模型 Tool 的免确认路径。

- [ ] Step 3: 写动作只经过 Rust executor

确认后的调用必须继续通过 Rust executor，不能由 Vue 直接调用 plan-service 进行 Agent 写入。新增 `agent_resolve_approval`（或等价的单一 Rust command）：approve 时加载数据库中已保存的 approval/preview step，重新校验 run/session/exam scope、precondition hash、schema 和 idempotency，再在同一执行边界中运行原 Tool；reject 时只把 approval 标记为 rejected，不写业务表。前端 store 必须暴露 resolve/undo 并在完成后刷新 messages、Today brief、Plan/Records workbench。普通手动编辑仍可使用既有本地 service。

- [ ] Step 4: 保留撤销，但不展示 Agent 内部状态

用户只看到“待确认 / 已应用 / 已取消 / 执行失败”和可选的“撤销”。`record.checkin_plan` 当前有实际 undo 能力时才显示撤销；`plan.apply_preview`、`record.create_free`、`wrong_question.create`、`wrong_question.mark_mastered` 没有 undo 实现前不得宣称可撤销。run_id、step index、policy JSON、receipt JSON 等保留在后端用于诊断，不暴露为产品概念。

- [ ] Step 5: 锁定审批和 scope 的回归测试

测试必须证明：R2 不会只返回摘要后把动作丢掉；用户点击确认后确实发生一次业务写入；取消不发生写入；重复确认不会重复写入；审批过期、数据已被手动修改、precondition 不匹配会安全失败；session 绑定 exam A 时不能读取或写入 exam B 的 plan/record/wrong question；所有失败都让 run 进入可观察的终态。

- [ ] Step 6: 运行写入安全测试并提交

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib agent::tools
cargo test --manifest-path src-tauri/Cargo.toml --lib agent::executor
npm.cmd test -- --run src/pages/AgentHome.test.ts
git add src-tauri/src/agent/tools/plan.rs src-tauri/src/agent/tools/record.rs src-tauri/src/agent/tools/wrong_question.rs src-tauri/src/agent/executor.rs src-tauri/src/agent/commands.rs src/services/agent-client.ts src/stores/agent.ts src/components/agent/ApprovalCard.vue src/components/agent/AgentActionPreview.vue src/components/agent/ConversationPane.vue src/pages/AgentHome.test.ts
git commit -m "feat: expose a small confirmed agent action set"
~~~

验收：云端 LLM 只能提出动作；业务数据的最终变化始终由 Rust 事务完成。

## 9. 阶段六：收缩后台基础设施

### Task 11: 将长期记忆降级为明确用户偏好

**Files:**

- Modify: D:/智研/zhiyan/src-tauri/src/agent/planner.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/context.rs
- Modify: D:/智研/zhiyan/src-tauri/src/brief.rs
- Modify: D:/智研/zhiyan/src-tauri/src/lib.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/commands.rs
- Modify: D:/智研/zhiyan/src/services/agent-client.ts
- Modify: D:/智研/zhiyan/src/types/index.ts
- Test: D:/智研/zhiyan/src-tauri/src/agent/planner.rs

- [ ] Step 1: 先移除 UI 和 client API

从 agent-client.ts、TypeScript types 和 AgentDebug.vue 的正式入口删除 memory list/create/confirm/update/deactivate/delete 调用。AgentDebug 仅开发构建保留时，这些调用可以暂时留在 debug-only module，但不能进入正常 bundle。

- [ ] Step 2: 停止 Planner 自动读取推断记忆

移除 Planner::run_inner 中对 MemoryRepository::relevant 和 touch 的调用。上下文只包含当前考试、近期消息、本日计划、近期记录、错题和确定性统计。

- [ ] Step 3: 保留显式设置，不做自动记忆

如果用户需要学习时长、提醒时间或考试策略，将其存入既有 settings key，并由 Settings 页面明确编辑。不要新增 candidate/confirmed 状态。

- [ ] Step 4: 暂不删除历史表

agent_memories 表和 migration 7 保留不动；新增代码不得读取它。待连续一个正式版本确认无读取后，再单独做数据库清理计划。

- [ ] Step 5: 测试并提交

~~~powershell
rg -n "MemoryRepository|agent_memory_|agent_memories|memory\.relevant|memory\.touch" src src-tauri
cargo test --manifest-path src-tauri/Cargo.toml --lib
npm.cmd test -- --run
git add src-tauri/src/agent/planner.rs src-tauri/src/agent/context.rs src-tauri/src/brief.rs src-tauri/src/lib.rs src-tauri/src/agent/commands.rs src/services/agent-client.ts src/types/index.ts
git commit -m "refactor: replace inferred agent memory with explicit settings"
~~~

验收：Agent 不再自动推断和管理七类长期记忆；用户的明确偏好仍然可用。

### Task 12: 收缩 Daily Brief 和 durable job system，保留提醒依赖

**Files:**

- Modify: D:/智研/zhiyan/src-tauri/src/brief.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/commands.rs
- Modify: D:/智研/zhiyan/src-tauri/src/lib.rs
- Modify: D:/智研/zhiyan/src-tauri/src/scheduler.rs
- Modify: D:/智研/zhiyan/src-tauri/src/tray.rs
- Modify: D:/智研/zhiyan/src-tauri/src/notify.rs
- Modify: D:/智研/zhiyan/src/App.vue
- Modify: D:/智研/zhiyan/src/components/agent/DailyBrief.vue
- Modify: D:/智研/zhiyan/src/services/agent-client.ts
- Test: D:/智研/zhiyan/src-tauri/src/brief.rs

- [ ] Step 1: 把 brief 定义为按需读取

保留 `agent_brief_preview(exam_id)`，让 command 直接使用受托管的 `BriefBuilder`/`BriefService` 和本地 Analytics，不再从 `State<Scheduler>` 间接取得 brief。Today 页面进入或用户点击刷新时调用一次；不需要为 brief 创建 `agent_jobs`。

- [ ] Step 2: 删除自动 LLM 简报调用

从 BriefBuilder::build 移除 provider 参数和 explain_with_llm；Brief.mode 固定为 local，explanation 为 None。解释由用户在 Agent 中主动请求。

- [ ] Step 3: 保留最小提醒循环，移除非必要 job

不能直接停止创建 Scheduler：`tray.rs` 当前依赖 Scheduler 做暂停/恢复和提醒，`notify.rs` 也由它发送原生通知。第一版只保留 `task_reminder`、`overdue_check` 和托盘所需的最小本地循环；删除 daily_brief、weekly_report、retry_failed、cleanup_failed 的自动调度和对应 job command/debug UI。`agent_brief_preview` 不再依赖 Scheduler state。

只有产品明确决定放弃原生提醒时，才能另开任务同时移除 Scheduler、notify、tray pause menu 和对应 Tauri plugin；本 Task 不得让“删除后台 Agent job”误伤托盘提醒。

- [ ] Step 4: 保留用户明确提醒

如果已有通知设置仍然被用户使用，只保留本地 task reminder/overdue 路径；不要让提醒依赖云端 LLM，不要为一次提醒创建 durable Agent run。若 brief 变为纯按需读取，移除 `agent-daily-brief` 的自动事件监听，或明确证明保留的事件只来自本地 brief 而不是模型。

- [ ] Step 5: 暂不修改 migration 8

历史 agent_jobs 表保留，避免旧数据库迁移失败。新代码不得向该表插入新任务。

- [ ] Step 6: 测试并提交

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib brief
cargo test --manifest-path src-tauri/Cargo.toml --lib
npm.cmd test -- --run
npm.cmd run typecheck
git add src-tauri/src/brief.rs src-tauri/src/agent/commands.rs src-tauri/src/lib.rs src-tauri/src/scheduler.rs src-tauri/src/tray.rs src-tauri/src/notify.rs src/App.vue src/components/agent/DailyBrief.vue src/services/agent-client.ts
git commit -m "refactor: make brief on-demand and shrink agent jobs"
~~~

验收：应用打开不会启动后台 LLM/报告 Agent job loop；今日摘要仍然正确显示；Scheduler 只执行明确保留的本地提醒；托盘暂停/恢复仍然可用；没有遗留 command 试图创建已删除的 job 类型。

### Task 13: 简化 Tool ownership 和调试面

**Files:**

- Modify: D:/智研/zhiyan/src-tauri/src/agent/tools/mod.rs
- Modify: D:/智研/zhiyan/src-tauri/src/agent/commands.rs
- Modify: D:/智研/zhiyan/src-tauri/src/lib.rs
- Modify: D:/智研/zhiyan/src/router/index.ts
- Modify: D:/智研/zhiyan/src/pages/AgentDebug.vue
- Modify: D:/智研/zhiyan/src/types/index.ts

- [ ] Step 1: 删除运行时 ownership 分支并验证旧数据库

在确认没有 TypeScript executor、shadow tool 或 ownership setting 使用后，删除 ToolOwnership::Typescript、Shadow、Unavailable，ListedTool 只返回 Rust registry descriptor。保留 ToolDescriptor 的 input/output schema、risk、confirmation、undo、idempotency 和 data permissions。特别检查 migration v5/v10 留下的 `record.checkin_plan=typescript` 等旧值：新 runtime 必须仍按 Rust owner 执行，不得因为旧 settings 把动作变成 unavailable。

- [ ] Step 2: 从 settings 清除 ownership 读取

保留 migration 5/10 不动；新代码不得查询 agent_tool_owner.*。不要在启动时重写历史 setting。

新增 Rust regression test：预置 legacy ownership settings，列举每个内置 Tool，确认 descriptor 和执行路径都来自 Rust；确认不会修改 legacy setting，也不会影响 `record.checkin_plan` 的 undo/receipt。

- [ ] Step 3: Debug route 仅在开发构建注册

路由采用明确条件：

~~~ts
const devRoutes = import.meta.env.DEV
  ? [{ path: '/agent-debug', name: 'agent-debug', component: () => import('@/pages/AgentDebug.vue') }]
  : []
~~~

生产构建访问 /agent-debug 时重定向到 /agent。

- [ ] Step 4: 删除 debug 里的业务操作按钮

即便开发构建保留 Debug，也只保留 provider health、当前 run、tool schema 和日志查看；删除 memory CRUD、job schedule、真实写入和 undo 操作按钮，防止调试页成为第二个产品入口。

- [ ] Step 5: 测试并提交

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib
npm.cmd test -- --run
npm.cmd run build
git add src-tauri/src/agent/tools/mod.rs src-tauri/src/agent/commands.rs src-tauri/src/lib.rs src/router/index.ts src/pages/AgentDebug.vue src/types/index.ts
git commit -m "refactor: reduce agent diagnostics to development surface"
~~~

验收：正式构建没有 Debug 入口；Agent Tool 只有一个 Rust owner；调试能力不再改变用户业务数据。

## 10. 阶段七：删除旧 AI 代码和可选依赖

### Task 14: 删除旧分析链路

**Files:**

- Delete after reference scan: D:/智研/zhiyan/src/pages/Analysis.vue
- Delete: D:/智研/zhiyan/src/stores/analysis.ts
- Delete: D:/智研/zhiyan/src/services/agent-engine.ts
- Delete after zero-reference scan: D:/智研/zhiyan/src/services/prompts.ts
- Delete after zero-reference scan: D:/智研/zhiyan/src/services/prompts.test.ts
- Modify before deleting analyzer types: D:/智研/zhiyan/src/pages/Visualization.vue
- Modify before deleting analyzer types: D:/智研/zhiyan/src/components/analysis/SuggestionCard.vue
- Modify before deleting analyzer types: D:/智研/zhiyan/src/components/analysis/ScoreGauge.vue
- Modify: D:/智研/zhiyan/src/services/export.ts
- Modify: D:/智研/zhiyan/src/services/db.ts
- Modify: D:/智研/zhiyan/src/components/dashboard/AiDailySummary.vue
- Modify: D:/智研/zhiyan/src/App.vue
- Modify: D:/智研/zhiyan/src/router/index.ts

- [ ] Step 1: 先解除 analyzer 类型和页面依赖

先扫描 `Visualization.vue`、`Analysis.vue`、`SuggestionCard.vue`、`ScoreGauge.vue` 对 `PredictionResult`、`Recommendation` 等类型的引用；把仍然需要的确定性统计类型移动到稳定的本地类型文件，或先删除无入口组件，再删除 analyzer 的 AI 文案类型。不能先删 analyzer 再让 Visualization/Analysis 通过 `any` 编译。

- [ ] Step 2: 删除旧启动补偿调用

从 App.vue 删除 runPendingAnalyses import 和 agent_os_enabled === '0' 分支。应用启动只加载 settings、exam 和 Today 所需的本地数据。

- [ ] Step 3: 修改 Dashboard 残留组件

AiDailySummary.vue 不再跳转 /analysis；如果该组件仍被使用，改为调用 agentBriefPreview 或跳转 /agent。若无任何 import，删除该组件。

- [ ] Step 4: 删除文件并做引用扫描

~~~powershell
rg -n "Analysis|useAnalysisStore|agent-engine|runPendingAnalyses|/analysis|prompts|ai_analyses|AiDailySummary|PredictionResult|Recommendation" src
~~~

生产源码不得存在旧分析 import；数据库 `ai_analyses` 表以及 `export.ts`/`db.ts` 中的导入、导出和 allowed-table 兼容逻辑暂时保留，避免历史 bundle 和历史记录丢失。新 Agent 不得写入该表。

- [ ] Step 5: 清理 Markdown 依赖候选

如果 marked 只被删除的 Analysis.vue 使用，从 package.json 删除 marked。highlight.js 当前没有源代码 import，确认后删除。

- [ ] Step 6: 测试并提交

~~~powershell
npm.cmd test -- --run
npm.cmd run typecheck
npm.cmd run build
git add src/pages/Analysis.vue src/stores/analysis.ts src/services/agent-engine.ts src/services/prompts.ts src/services/prompts.test.ts src/pages/Visualization.vue src/components/analysis/SuggestionCard.vue src/components/analysis/ScoreGauge.vue src/services/export.ts src/services/db.ts src/components/dashboard/AiDailySummary.vue src/App.vue src/router/index.ts package.json package-lock.json
git commit -m "refactor: remove legacy ai analysis product"
~~~

验收：项目不存在第二个 AI 分析产品；历史 ai_analyses 数据仍然保留在数据库中但不再被新流程写入。

## 11. 阶段八：GUI 重构和视觉收敛

GUI 必须在入口和职责完成收敛后实施，否则会继续美化重复页面。

### Task 15: 建立稳定的 Agent UI 结构

**Files:**

- Modify: D:/智研/zhiyan/src/pages/AgentHome.vue
- Modify: D:/智研/zhiyan/src/components/agent/AgentSidebar.vue
- Modify: D:/智研/zhiyan/src/components/agent/ConversationPane.vue
- Modify: D:/智研/zhiyan/src/components/agent/DailyBrief.vue
- Modify: D:/智研/zhiyan/src/components/agent/ApprovalCard.vue
- Create: D:/智研/zhiyan/src/components/agent/AgentActionPreview.vue
- Create: D:/智研/zhiyan/src/components/agent/AgentEmptyState.vue
- Modify: D:/智研/zhiyan/src/assets/main.css
- Test: D:/智研/zhiyan/src/pages/AgentHome.test.ts

- [ ] Step 1: 固定页面层级

页面只允许以下层级：

~~~text
AgentHome
├── AgentSidebar
├── AgentHeader
├── DailyBrief / TodaySummary
├── ConversationPane
│   ├── MessageList
│   ├── AgentActionPreview
│   └── Composer
└── WorkbenchHost
    ├── PlanCheckinBoard
    ├── StudyPlan
    └── StudyRecord
~~~

不要再在 Agent 页面内挂载完整 Dashboard、Analysis 或 Visualization 页面。

- [ ] Step 2: 设计单一视觉语言

使用现有 CSS variables，不再为每个页面单独定义颜色。视觉规则固定为：

1. 页面背景一个主色，内容面板使用相邻层级表面色；
2. 主要操作每个区域最多一个 primary button；
3. 消息、计划和记录使用细分隔线，不使用大量悬浮卡片；
4. Agent tool 执行过程默认折叠，只有错误、等待确认和最终结果展开；
5. 取消渐变、彩色 badge 堆叠和重复标题；
6. 中央对话列控制最大阅读宽度，右侧 workbench 不超过整体宽度的 36%；
7. 所有中文文本、空状态、错误状态和未连接状态都由组件明确展示，禁止只显示 loading spinner。

- [ ] Step 3: 创建统一 Action Preview

AgentActionPreview.vue 接收现有 AgentApproval/ToolCallResponse 数据，显示动作摘要、影响范围、确认/取消/撤销按钮。组件不执行数据库操作，只 emit approve、reject、undo。

- [ ] Step 4: 补齐状态设计

实现并测试四类状态：无考试、未连接 LLM、云端请求中、写入等待确认。每类状态都提供一个明确下一步，不用空白页面。

- [ ] Step 5: 更新页面测试

测试消息发送、等待审批、审批后刷新、provider 缺失提示、切换 workbench 后会话消息仍存在。

- [ ] Step 6: 运行测试并提交

~~~powershell
npm.cmd test -- --run
npm.cmd run typecheck
npm.cmd run build
git add src/pages/AgentHome.vue src/components/agent/AgentSidebar.vue src/components/agent/ConversationPane.vue src/components/agent/DailyBrief.vue src/components/agent/ApprovalCard.vue src/components/agent/AgentActionPreview.vue src/components/agent/AgentEmptyState.vue src/assets/main.css
git commit -m "feat: establish focused agent workspace ui"
~~~

验收：用户打开应用后无需理解“Agent OS、Workbench、AI 分析、数据可视化”等工程名词，可以直接看到今天应该做什么以及如何完成。

### Task 16: 收缩计划和图表视觉复杂度

**Files:**

- Modify: D:/智研/zhiyan/src/pages/StudyPlan.vue
- Modify: D:/智研/zhiyan/src/components/plan/PlanCalendar.vue
- Modify: D:/智研/zhiyan/src/components/plan/PlanList.vue
- Modify: D:/智研/zhiyan/src/pages/Visualization.vue（迁移期间可保留，不再导航）
- Modify: D:/智研/zhiyan/src/components/viz/*
- Modify: D:/智研/zhiyan/src/components/charts/*
- Modify: D:/智研/zhiyan/src/main.ts
- Modify: D:/智研/zhiyan/src/services/echarts-theme.ts

- [ ] Step 1: 计划只保留两个视图

保留 Calendar 和 List；删除 Gantt、Compare 的组件 import 和 route。List 负责拖拽排序、状态修改和单项编辑。

- [ ] Step 2: 只保留两个核心统计

保留“学习时长趋势”和“薄弱科目/知识点”两个视图。其余图表数据函数可以暂时保留到没有引用后再删除；不得为了显示所有已实现图表继续增加页面入口。

- [ ] Step 3: 图表改为按需加载

Today/Agent 默认不加载 ECharts；用户点击“查看统计”时才加载图表组件。必须同时修改 `src/main.ts` 的全局注册和 `echarts-theme.ts` 的初始化策略，否则只改 Vue 组件仍会在启动时加载整套 ECharts。LLM 只解释图表数据，不生成图表数据。

- [ ] Step 4: 依赖扫描

~~~powershell
rg -n "PlanGantt|PlanCompare|Visualization|VChart|echarts|vue-echarts|vuedraggable|register\w*Chart" src
~~~

`vuedraggable` 仍被 PlanList 使用时必须保留；在仍保留两个核心图表期间不能删除 `echarts/vue-echarts`。只有实现了真实的按需加载并完成 bundle 检查后，才能声称首屏不加载图表；不要只修改文案。

- [ ] Step 5: 测试并提交

~~~powershell
npm.cmd test -- --run
npm.cmd run typecheck
npm.cmd run build
git add src/pages/StudyPlan.vue src/pages/Visualization.vue src/components/plan/PlanCalendar.vue src/components/plan/PlanList.vue src/components/viz/CorrectRateChart.vue src/components/viz/DurationTrendChart.vue src/components/viz/KpHeatmapChart.vue src/components/viz/SubjectPieChart.vue src/components/viz/SubjectRadarChart.vue src/components/charts/WeeklyTrendChart.vue src/components/charts/SubjectRatioChart.vue src/main.ts src/services/echarts-theme.ts package.json package-lock.json
git commit -m "feat: reduce plan and analytics visual surface"
~~~

验收：计划和统计页面不再给用户四种计划视图和六种图表选择；核心信息更快可读。

## 12. 阶段九：数据库、兼容性和清理策略

### Task 17: 建立非破坏性弃用检查

**Files:**

- Create: D:/智研/zhiyan/docs/agent/cloud-llm-cutover-migration.md
- Modify: D:/智研/zhiyan/src-tauri/src/db.rs（只允许新增验证测试，不修改 migration 1–10）
- Modify: D:/智研/zhiyan/src/services/export.ts
- Modify: D:/智研/zhiyan/src/services/db.ts
- Test: D:/智研/zhiyan/src-tauri/src/db.rs

- [ ] Step 1: 写明历史表策略

文档必须列出以下表目前只读/弃用状态：ai_analyses、agent_context_audit、agent_memories、agent_jobs、ownership settings。说明它们不会在本次发布中被 drop。`ai_analyses` 仍然保留在 export/import 和 `ALLOWED_TABLES` 兼容集合中，旧 JSON/SQLite backup 必须能恢复；不能因为 UI 删除就移除历史数据通道。

- [ ] Step 2: 增加迁移回归测试

测试使用内存 SQLite 执行完整 migrations，并验证基础表和当前运行时必要表存在：

~~~text
exams
subjects
knowledge_points
study_plans
study_records
wrong_questions
settings
agent_sessions
agent_runs
agent_steps
agent_approvals
agent_messages
~~~

- [ ] Step 3: 增加旧数据读取测试

插入一条旧 ai_analyses、旧 agent_memories 和旧 agent_jobs 记录，启动 runtime 后确认：迁移成功、基础业务可读、新 Agent 不会自动修改这些旧记录；同时覆盖旧 `agent_tool_owner.*`、旧 `cloud_llm_consent_fingerprint` 和旧 API-key fallback 的升级行为。

- [ ] Step 4: 增加备份和恢复门槛

发布前先生成可恢复的 SQLite/JSON 备份；在临时数据库中执行升级、导入、导出和恢复，确认 exams、plans、records、wrong questions、settings、历史 Agent 表都能读回。任何数据迁移失败必须阻止发布，不得用“用户重新配置”掩盖数据丢失。

- [ ] Step 5: 提交兼容性文档和测试

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib db
git add docs/agent/cloud-llm-cutover-migration.md src-tauri/src/db.rs src/services/export.ts src/services/db.ts
git commit -m "test: preserve legacy database compatibility during cutover"
~~~

验收：旧用户升级不会丢数据；代码删减不会依赖破坏性数据库迁移。

### Task 18: 依赖和死代码最终清理

**Files:**

- Modify: D:/智研/zhiyan/package.json
- Modify: D:/智研/zhiyan/package-lock.json
- Modify: D:/智研/zhiyan/src-tauri/Cargo.toml
- Modify: D:/智研/zhiyan/src-tauri/src/lib.rs

- [ ] Step 1: 执行全仓死引用扫描

~~~powershell
rg -n "highlight\.js|marked|@tauri-apps/plugin-http|tauri-plugin-http|agent-engine|plan-generator|plan-chat-agent|scheduler|MemoryRepository|ContextAudit|agent_job_|agent_memory_" src src-tauri package.json src-tauri/Cargo.toml
~~~

- `scheduler.rs`、`tray.rs`、`notify.rs` 若仍承担 task reminder/overdue 和托盘暂停，属于有意保留的运行时能力；只有无引用且产品明确放弃提醒时才可删除。

- [ ] Step 2: 只删除已无引用的依赖

候选包括：highlight.js、marked、前端 @tauri-apps/plugin-http。echarts、vue-echarts、vuedraggable 只有在对应组件完全删除后才可删除。Rust reqwest 必须保留为唯一 provider client。

- [ ] Step 3: 重新安装和构建

~~~powershell
npm install
npm.cmd test -- --run
npm.cmd run typecheck
npm.cmd run build
cargo test --manifest-path src-tauri/Cargo.toml --lib
~~~

- [ ] Step 4: 提交清理结果

~~~powershell
git status --short
git add package.json package-lock.json src-tauri/Cargo.toml src-tauri/src/lib.rs src-tauri/src/brief.rs src-tauri/src/scheduler.rs src-tauri/src/notify.rs
# 再按本 Task 实际修改的文件逐个追加；禁止使用 git add src-tauri/src/agent 或 git add -A
git commit -m "chore: remove unused cloud agent dependencies"
~~~

验收：生产源码和依赖列表只保留目标产品实际使用的能力。

## 13. 阶段十：端到端验收和发布门槛

### Task 19: 编写 Agent 产品验收矩阵

**Files:**

- Create: D:/智研/zhiyan/docs/agent/cloud-llm-cutover-test-matrix.md
- Modify: D:/智研/zhiyan/MANUAL_TEST.md
- Test: D:/智研/zhiyan/src/pages/AgentHome.test.ts

- [ ] Step 1: 配置测试 provider

使用 OpenAI-compatible mock server 或测试 provider，不在仓库、截图、日志和测试 fixture 中写真实 API Key。

- [ ] Step 2: 验证首次启动

覆盖：无考试 → Setup；有考试但无 LLM → Today 显示本地数据和连接提示；有 LLM → Agent 可以回答今日计划。

- [ ] Step 3: 验证只读 Agent

输入“今天安排了什么”，确认返回内容中的计划来自 SQLite 当前记录；修改数据库中的计划后重新询问，结果随数据变化。

- [ ] Step 4: 验证计划生成

输入“根据剩余天数调整计划”，确认出现包含真实 Draft 行、冲突和 precondition 的计划预览；点击取消，数据库不变化；点击确认，数据库只发生预览中的变化；重复确认不会产生重复任务；手动修改原计划后再确认会因 precondition 变化而安全失败。

- [ ] Step 5: 验证学习记录和错题动作

测试自然语言整理记录、创建错题、标记掌握、撤销；任何失败都不能留下半条记录。

- [ ] Step 6: 验证 provider 异常

测试 401、429、超时、断网和空响应；UI 必须展示可理解的错误和重试/设置入口，不能伪造成功消息。

- [ ] Step 7: 验证数据同意、隔离和 run 生命周期

覆盖：未同意业务数据时 provider 请求只包含固定诊断内容或被 `consent_required` 拒绝；修改 provider/base URL/model 后旧同意失效；session 绑定 exam A 时读取/写入 exam B 被 `tool_scope_violation` 拒绝且无数据库变化；文本完成、审批等待、审批取消、审批确认、provider 错误和取消分别进入正确终态。

- [ ] Step 8: 验证旧路由和数据

直接访问 /dashboard、/analysis、/visualization、/agent-debug，确认生产构建全部重定向或不可见；已有 `agent_os_enabled=0` 仍进入 Agent；确认旧 JSON、SQLite backup、已有学习记录和 `ai_analyses` 历史数据仍可导入/恢复。

- [ ] Step 8: 提交验收矩阵

~~~powershell
npm.cmd test -- --run
npm.cmd run typecheck
npm.cmd run build
cargo test --manifest-path src-tauri/Cargo.toml --lib
git add docs/agent/cloud-llm-cutover-test-matrix.md MANUAL_TEST.md src/pages/AgentHome.test.ts
git commit -m "test: add cloud llm agent acceptance matrix"
~~~

### Task 20: 最终发布检查

**Files:**

- Modify: D:/智研/zhiyan/PROJECT_STATUS.md
- Modify: D:/智研/zhiyan/docs/agent/feature-parity.md
- Modify: D:/智研/zhiyan/docs/agent/migration-runbook.md

- [ ] Step 1: 更新状态文档

记录：唯一 LLM provider、已删除的 TypeScript AI 链路、保留的本地事实层、弃用但未删除的历史表、当前导航和生产 Debug 策略。

- [ ] Step 2: 运行发布前完整命令

~~~powershell
git status --short
npm.cmd test -- --run
npm.cmd run typecheck
npm.cmd run build
cargo test --manifest-path src-tauri/Cargo.toml --lib
~~~

- [ ] Step 3: 检查敏感信息

~~~powershell
rg -n "sk-[A-Za-z0-9]|api[_-]?key\s*[:=]\s*['\"]|Bearer\s+" src src-tauri docs tests
~~~

Expected：只出现类型名、脱敏示例或测试占位，不出现真实凭据。

- [ ] Step 4: 更新最终文档并提交

~~~powershell
git add PROJECT_STATUS.md docs/agent/feature-parity.md docs/agent/migration-runbook.md
git commit -m "docs: finalize cloud llm agent simplification"
~~~

最终验收：

1. 正常用户只有 Today、Plan、Records、Setup/Settings 四个心智入口。
2. 前端没有独立 AI 分析、计划 AI 对话和 LLM HTTP 实现。
3. Rust 是唯一 LLM provider 和 Agent 编排入口。
4. LLM 无法绕过用户确认和本地事务写入数据。
5. 没有云端配置时，本地数据仍然可用且状态明确。
6. 旧数据库、导入导出、备份恢复仍然可用。
7. 全部 TypeScript、Rust、构建和手工验收通过。

## 14. 推荐交接给其他 vibe coding 助手的执行提示词

将下面内容连同本计划文件一起交给执行助手：

~~~text
你正在改造 D:/智研/zhiyan。请先完整阅读
docs/superpowers/plans/2026-08-04-cloud-llm-agent-simplification.md，
再读取该计划当前 Task 所列的全部文件。

执行规则：
1. 一次只完成一个 Task，不跨阶段偷删代码。
2. 先写/更新测试，再修改实现；每个 Step 完成后运行计划中的命令。
3. 不修改 src-tauri/src/db.rs 已发布的 migration 1–10。
4. 不向云端 LLM 发送 API Key，不让 LLM 直接写 SQLite。
5. 删除任何文件前运行 rg 做零引用扫描。
6. 完成一个 Task 后报告：修改文件、测试命令、测试结果、剩余风险、commit hash。
7. 如果测试失败，先修复当前 Task，不要继续删除下一条链路。
8. 如果发现计划与现有代码冲突，保留数据兼容和可回滚能力，并在报告中说明，不要擅自扩大范围。
~~~

## 15. 自审清单

- [ ] 产品入口已经从旧系统 + Agent 双入口收敛为 Agent 单入口。
- [ ] 计划、分析、LLM provider 没有并行的第二实现。
- [ ] 本地统计和本地写入边界没有交给 LLM。
- [ ] 记忆、调度、context audit、ownership 复杂度已经从普通用户路径移除。
- [ ] 历史数据库和 migration 仍然兼容。
- [ ] 生产构建不包含 Agent Debug 和旧分析入口。
- [ ] 依赖删除经过 rg、typecheck、test、build 四重验证。
- [ ] 每一阶段都有独立可运行状态和回滚点。
