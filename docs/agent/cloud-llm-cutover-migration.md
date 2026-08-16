# Cloud LLM Cutover：非破坏性弃用与兼容性检查

> 本文档记录 2026-08-04 Cloud LLM Agent 产品简化改造中，**只读 / 弃用**的数据表与
> 配置键的处理策略。原则：历史数据永不丢失、可读、可回滚；新流程不写入弃用表；
> 已发布 migration 1–10 永不修改。

## 1. 历史表策略

以下表保留在 schema 中（**不会在本次发布中 drop**），但新代码不得写入：

| 表 / 配置                       | 状态         | 说明                                                                                                                                                                                                  |
| ------------------------------- | ------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ai_analyses`                   | 只读 / 弃用  | 旧 TypeScript AI 分析产物（daily/weekly/phase/prediction）。仍保留在 `export.ts` / `db.ts` 的导入导出与 `ALLOWED_TABLES` 兼容集合中；旧 JSON/SQLite backup 必须能恢复。新 Agent 不写该表（Task 14）。 |
| `agent_context_audit`           | 保留（诊断） | 每轮模型调用的来源 ID 审计。仅 `agent_context_audit_list` 调试命令读取；无新写入路径变化。                                                                                                            |
| `agent_memories`                | 只读 / 弃用  | 七类结构化长期记忆。`agent_memory_*` 命令、前端 API 与 UI 已删除（Task 11）；表保留以兼容历史数据库，新代码不得读取。                                                                                 |
| `agent_jobs`                    | 部分只读     | migration 8 表。Task 12 后只调度/执行 `task_reminder`、`overdue_check`；`daily_brief`、`weekly_report`、`retry_failed`、`cleanup_failed` 为 deprecated 类型，历史行可列出但 dispatch 一律 skip。      |
| `agent_tool_owner.*` settings   | 只读 / 弃用  | migration 5/10 seed 的 ownership 开关。Task 13 后运行时不再读取；legacy 值（`typescript`/`shadow`/`unavailable`）inert，所有工具按 Rust-owned 执行，启动时不改写这些键。                              |
| `*_api_key_fallback` settings   | 只读 / 弃用  | 旧密钥 fallback。前端只检测存在并提示“请重新输入 API Key”，不解密、不发送、不迁移；用户重新保存到 keyring 后可手动删除（Settings 不自动删除）。                                                       |
| `cloud_llm_consent_fingerprint` | 保留         | 数据出境同意指纹。配置变化后由 Planner 校验并失效，不视为弃用。                                                                                                                                       |

## 2. 迁移与升级保证

- migration 1–10 已发布，**禁止修改**；历史数据库必须继续启动（`db.rs` 回归测试
  `full_migrations_create_every_required_runtime_table`、`legacy_deprecated_rows_survive_migrations_untouched`）。
- 版本策略（2026-08-11 复核）：v11（plan preview/apply 的 ownership 种子）在交付前已被移除——
  检查 `_sqlx_migrations` 与所有可分发数据库后确认 **没有任何数据库应用过 v11**，因此本次发布
  **不新增 migration，v10 为当前最新**；v1–v10 的 SQL 逐字未改。若未来任一已发布数据库已应用
  v11，则 v11 不可改写或重编号，后续版本必须从 v12 继续递增。
- 工具 ownership 始终由 Rust 静态 registry 决定，不依赖这些 inert 的 `agent_tool_owner.*` settings。
- 旧库升级路径：`db::migrations()` 顺序执行（幂等，`CREATE TABLE IF NOT EXISTS` /
  `INSERT OR IGNORE` seed），不删列、不删表、不改既有行。
- 新 Agent 写入边界：只写 `study_plans`、`study_records`、`wrong_questions`、
  `agent_sessions/runs/steps/approvals/messages/events` 及必要的 `settings` 键；
  永不写 `ai_analyses`、`agent_memories`、`agent_jobs`（除 reminder 调度）、
  `agent_tool_owner.*`。

## 3. 备份与恢复门槛（发布前手工步骤）

1. 在发布构建上生成 SQLite 备份与 JSON 导出（设置 → 数据备份）。
2. 在临时数据库中依次执行：完整升级 → JSON 导入 → SQLite 恢复 → 导出。
3. 校验 `exams`、`subjects`、`knowledge_points`、`study_plans`、`study_records`、
   `wrong_questions`、`settings`、`ai_analyses`、`agent_sessions/runs/steps/approvals/messages`
   都能读回，且旧记录内容不变。
4. 任一迁移/导入/恢复失败即阻塞发布，不得用“用户重新配置”掩盖数据丢失。

## 4. 相关测试

- `src-tauri/src/db.rs`：
  - `full_migrations_create_every_required_runtime_table`
  - `legacy_deprecated_rows_survive_migrations_untouched`
  - 既有 migration v5/v6/v7/v8/v9/v10 与完整初始化测试。
- `src-tauri/src/agent/executor.rs` / `runtime.rs`：legacy ownership settings 回归测试。
