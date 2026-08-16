# Agent Feature Parity（2026-08-16 Cloud LLM 简化修复后）

| Capability                           | Current owner                                         | Status     | Notes                                                                                                           |
| ------------------------------------ | ----------------------------------------------------- | ---------- | --------------------------------------------------------------------------------------------------------------- |
| Exam/subject configuration           | TypeScript services + Setup UI                        | local      | 未进入 Agent 工具面（保持本地确定性）                                                                           |
| Plan generation & adjustment         | Agent (Rust Planner) + R3 approval                    | rust-owned | 对话内预览（真实 Draft 行/冲突/precondition）→ 确认提交；`plan.generate`/`plan.get_today` 等工具全部 Rust-owned |
| Plan check-in & free record          | Agent (Rust tool)                                     | rust-owned | `record.checkin_plan` 即使存在旧 `agent_tool_owner.*=typescript` 设置仍按 Rust-owned 执行（Task 13）            |
| Wrong questions                      | Agent (Rust tool)                                     | rust-owned | `wrong_question.create` / `wrong_question.mark_mastered`                                                        |
| Daily brief                          | Rust `brief.rs`                                       | local      | 纯按需本地摘要（`mode=local`，无 LLM 调用）；模型解释只在 Agent 对话中显式请求                                  |
| Analysis & prediction                | 并入 Agent 对话                                       | retired    | TS 链路（agent-engine/analysis store/prompts）已删；`ai_analyses` 历史表只读弃用                                |
| Visualization datasets               | 仅存两个核心统计（时长趋势/知识点热力图）             | retired    | `/visualization` 重定向；ECharts 惰性加载                                                                       |
| Import/export/backup/restore         | TypeScript services + Rust fs plugin                  | local      | `ai_analyses` 仍保留在 ALLOWED_TABLES 与导出中（兼容历史）                                                      |
| Agent session/run/messages/approvals | Rust Runtime + Planner                                | rust-owned | v9 `agent_messages` 持久化；run 终态机                                                                          |
| Structured long-term memory          | 无（降级为 Settings 键）                              | retired    | `agent_memories` 表保留、新代码不读（Task 11）                                                                  |
| Background jobs                      | Rust Scheduler                                        | rust-owned | 仅 `task_reminder`/`overdue_check`；其余 job 类型 deprecated（Task 12）                                         |
| Tray lifecycle / reminders           | Rust Tray/Scheduler/notify                            | rust-owned | 暂停/恢复、任务与逾期通知（正文仅计数/日期）                                                                    |
| LLM provider                         | Rust `OpenAiCompatibleProvider` (reqwest)             | rust-owned | 唯一 provider；TS adapter 与 plugin-http 已删                                                                   |
| Agent OS shell + conversation        | Vue AgentHome（Sidebar/Brief/Conversation/Workbench） | rust-owned | 工作台仅 check-in/plan/record 三栏                                                                              |
| Debug surface                        | `/agent-debug`（dev-only 路由）                       | rust-owned | 仅 health/run/tool schema/审计/planner loop                                                                     |

States: `rust-owned`、`local`、`retired`（不再有 `legacy`/`shadow`/`typescript` 所有权状态；
ToolOwnership 收敛为单一 `RustOwned`，`agent_tool_owner.*` settings 只读弃用）。

## 回归命令（2026-08-16 实际输出）

- `cargo test --manifest-path src-tauri/Cargo.toml --lib`：**164 passed, 0 failed**
  （db/executor/planner/runtime/scheduler/brief/llm 等；测试总数以本次输出为准）。
- `npm.cmd test -- --run`：**13 个测试文件、79 个用例通过**（AgentHome/AgentDebug/router/export/analyzer/settings 等）。
- `npm.cmd run typecheck`：exit 0。
- `npm.cmd run build`：exit 0（仅已有 chunk size 与动态导入 warning）。
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`：通过。
- `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings`：通过。
- 手工验收：`docs/agent/cloud-llm-cutover-test-matrix.md`
