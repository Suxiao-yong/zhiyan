# Cloud LLM Cutover Baseline

> 记录 2026-08-04 Cloud LLM Agent 产品简化改造开始前的基线状态,供任何后续助手判断改造前后行为差异。

## 环境与仓库

- 分支: `main`
- 基线 commit: `e3dead1` (chore: ignore .reasonix local metadata; add agent design concepts)
- Node: v22.16.0
- npm: 10.9.2
- rustc: 1.97.1 (8bab26f4f 2026-07-14)
- cargo: 1.97.1 (c980f4866 2026-06-30)

## 测试基线

- TypeScript: 75 个测试通过(vitest run)
- Rust: 114 个 lib tests 通过(cargo test --lib)
- typecheck / build: 见下方验证记录

## 调用图基线(改造前)

### TypeScript 旧 AI 链路引用

- `src/App.vue:8` → `runPendingAnalyses` from `@/services/agent-engine`
- `src/components/plan/PlanChatAssistant.vue:5-6` → `runChatTurn`/`finalizePlan` from `plan-chat-agent`, `generateLocalPlan` from `plan-generator`
- `src/pages/ExamConfig.vue:11,130` → `PlanChatAssistant` 组件
- `src/pages/Settings.vue:7` → `callLLM` from `@/services/llm-adapter`
- `src/pages/StudyPlan.vue:10,75` → `PlanChatAssistant` 组件
- `src/services/agent-engine.ts:6` → `callLLM`, `parseJsonResponse` from `llm-adapter`
- `src/services/llm-adapter.test.ts:6` → `parseJsonResponse`
- `src/services/plan-chat-agent.ts:4,6` → `callLLM`/`callLLMWithTools`/`parseJsonResponse` from `llm-adapter`, `applyGeneratedPlan` from `plan-generator`
- `src/services/plan-generator.ts:6` → `callLLM`, `parseJsonResponse`
- `src/services/prompts.ts` → 被 agent-engine / plan-generator 使用
- `src/stores/analysis.ts:6` → `@/services/agent-engine`
- `src/stores/plan.ts:7` → `@/services/plan-generator`
- `src/types/index.ts:314` → `agent_jobs` 类型

### Rust Agent 运行时引用

- `agent_run_planner` command: `src-tauri/src/agent/commands.rs:374`
- `agent_context_audit_list` command: `src-tauri/src/agent/commands.rs:207`
- `agent_memory_*` commands + `MemoryRepository`: `src-tauri/src/agent/memory.rs`
- `agent_job_*` commands + Scheduler/`agent_jobs`: `src-tauri/src/scheduler.rs`
- Context audit 表(不存原文,只存 ID/类别): `src-tauri/src/agent/context.rs`
- 27 个 Tauri command 注册于 `src-tauri/src/lib.rs:59-87`

### 路由与导航

- `src/router/index.ts`: `/dashboard` `/exam-config` `/study-record` `/study-plan` `/study-plan/:view?` `/analysis` `/visualization` `/settings` `/agent-debug` `/agent` `/` `/welcome`;`agent_os_enabled=0` 时 `/` 回退 `/dashboard`
- `src/components/layout/AppLayout.vue:30-39`: 八个菜单项(仪表盘/考试配置/学习记录/学习计划/AI 分析/数据可视化/Agent/设置)

### 工作台注册表

- `src/components/agent/workbench.ts`: `checkin`/`plan`/`record`/`analysis`/`visualization` 五个 workbench

## 改造边界(本次不可违反)

1. 不修改 `src-tauri/src/db.rs` 已发布 migration 1–10。
2. API Key 不进 prompt、Agent message、日志、审计或持久化 UI 状态。
3. 云端 LLM 不直接生成 SQL、不直接访问 SQLite、不绕过 Rust Tool Registry。
4. 删除任何文件前先做零引用扫描。
5. 不新增向量库/RAG/云同步/账号系统/新状态管理框架。
6. 简化后 Agent 不允许 `agent_r2_auto_execute` 自动写入;所有模型写操作走 R3 审批。
7. 未获数据出境同意时 `agent_run_planner` 在 Rust 侧拒绝发送业务上下文。
8. 任何 Tool 调用被限制在当前 run 绑定的 exam 范围内。
