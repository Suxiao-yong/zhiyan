# 项目进度状态

更新时间：2026-07-18（M6 完成后更新）
工作区：`D:\智研\zhiyan`（main 分支）
分支：`main`

## 当前结论

纯 agent 改造的 M3（模型、上下文与记忆）、M4（托盘、调度与简报）、M5（Agent OS 界面）、M6（全面迁移与生产化）全部完成并提交：

- **M3 Part 1**：OpenAI 兼容 Provider、模型↔工具循环、本地降级与软 token 预算（`4528a45` 记录里程碑）。
- **M3 Part 2**：SSE 流式对话与工具循环，`agent-planner-chunk` 事件实时渲染（`323902f` 标记 shipped）。
- **M3 Part 3**：Context Inspector（v6 `agent_context_audit`）、结构化长期记忆（v7 `agent_memories`）、记忆接入 Context Builder（`e6c5550`）。
- **M4**：托盘生命周期、`agent_jobs` v8 后台调度、本地聚合、每日简报、任务/逾期通知、费用估算。
- **M5**：`/agent` Agent OS 三栏界面——会话侧栏、对话中心（`agent_messages` v9 持久化、审批卡、状态、简报卡）、计划打卡工作台嵌入右栏；简报命令层推送与 `weekly_report` job。
- **M6**：§8.2 剩余工具集（三个 R0 查询、三个 R1 写、`plan.generate` R2 周草案）、右栏五工作台注册表、`agent_os_enabled` 回退开关（默认 `/agent`，关闭回旧仪表盘并禁用 TS 分析补跑）、生产化加固（forward-only 迁移测试、安全审查闭环、M6 打包验收清单）。

仍搁置：完整 Fallback Engine 产品面、`data.*` agent 工具、`exam.update`/`plan.reorder`/`record.update|delete`/`visualization.get_dataset`、TypeScript planner 文件移除（`plan-chat-agent.ts` 保留为回退聊天 UI）、通知偏好 UI。Ollama 工具支持按产品决策排除。

## M6 提交记录

提交：`53b7365`（计划）、`1f0d16d`（查询工具）、`03ca580`（写工具）、`517b6ed`（plan.generate）、`e1704db`（工作台注册表）、`0001ead`（TS 分析降级）、`33b1c23`（回退开关+路由）、`ed465bb`（加固）、`cd33140`（slotting 死循环修复）、`c6569ad`（周内日期分布）、`e25f5a6`/`84378fa`（KP 校验测试）

- 九个工具：`exam.get_active`/`plan.get_range`/`record.get_history`（R0）、`record.create_free`/`wrong_question.create`/`wrong_question.mark_mastered`（R1 幂等写）、`plan.generate`（R2 周草案，`agent_r2_auto_execute` 门控，按权重 floor+最大小数分配、每周幂等、日期严格在周内）。新工具默认 rust-owned（v5 seed）；未配置工具保持 fail-closed。
- 工作台注册表：右栏五个工作台（打卡/学习计划/记录与错题/AI 分析/数据可视化），原生切换器，切换不丢对话；`StudyRecord` 支持 `initial-tab`。
- 回退开关：`agent_os_enabled`（默认开）决定 `/` → `/agent` 或 `/dashboard`；关闭时 `App.vue` 恢复 TS 分析补跑路径。4 个路由测试。
- 加固：forward-only 迁移测试（无 DROP/RENAME/DELETE）、日期解析、KP 归属校验（两工具+正负测试）、slotting 死循环修复（8-14 科目回归）、MANUAL_TEST M6 打包清单。
- 安全审查：两轮 security_review 后 verdict 无 blocking；review 最终 ship as-is。
- 验证：Rust 全量 113 lib + 12 repository + 42 tools = 167 通过；前端 75 用例通过；typecheck、build、Clippy `-D warnings`、fmt、diff-check 全部通过。

## 历史：M5 提交记录

提交：`2c30e55`（计划）、`41586c7`（消息层 v9 + 读命令）、`9cf9214`（三栏壳 + store + 路由）、`41ec696`（简报推送 + 周报）

- v9 迁移新增 `agent_messages`（角色 CHECK、会话级联、token 列），planner 每 turn 持久化 user/assistant 消息（本地 turn 零 token）。
- 新读命令：`agent_session_list` / `agent_session_messages` / `agent_approval_list`。
- `/agent`（全屏）三栏：AgentSidebar（新会话/最近会话/工作台深链）、ConversationPane（消息流+输入）、DailyBrief（acknowledge 折叠）、ApprovalCard、AgentStatus、WorkbenchHost（嵌入 PlanCheckinBoard）。
- 简报推送走命令层（`agent_brief_preview` 注入 AppHandle 发射事件），managed state 不持 AppHandle（M4 教训）。
- `weekly_report` handler：周统计 + 薄弱点 → 文本摘要。
- 验证：Rust 全量 107 lib + 12 repository + 42 tools = 161 通过；前端 70 用例通过；typecheck、build、Clippy `-D warnings`、fmt、diff-check 全部通过。

## 历史：M4 提交记录

提交：`ed89962`（计划）、`e48a0fe`（托盘）、`82f0133`（调度器 + v8）、`71d8315`（本地聚合）、`c74f65a`（每日简报）、`53a27ab`（通知出站）、`6e4c7d5`（费用 + 调试页 Jobs/Brief）

- 托盘：主窗口关闭改为隐藏（`EXITING` 标志区分真实退出），托盘四菜单项，暂停状态持久化到 settings。
- 调度：60s tick 循环 + `ensure_today_jobs` 每日排程（简报 08:00 / 逾期 09:00 / 提醒默认 19:00），全局去重键，失败重试 +5min。
- 通知：经 tokio channel 出站，`Scheduler` 不持有 `AppHandle`（managed state 持有会产出损坏测试 exe，见提交说明），正文仅计数与日期。
- 验证：Rust 全量 100 lib + 12 repository + 42 tools = 154 通过；前端 64 用例通过；typecheck、build、Clippy `-D warnings`、fmt、diff-check 全部通过。

## 历史：M3 Part 3 提交记录

提交：`bc6ccc8`（audit 数据层）、`22052cb`（Inspector 读取命令 + UI）、`6353bc0`（结构化记忆 + UI）、`eeed904`（CI 增强）、`e6c5550`（记忆接入 Context Builder）

- v6 迁移新增 `agent_context_audit`，替换 `model.invoked` 事件：每次模型调用记录 tools_offered、数据类别、记录 ID、字段集合、token 与 local 标志，**不存任何原文**。
- v7 迁移新增 `agent_memories`：7 种类型（schedule_preference / daily_capacity / subject_preference / learning_constraint / reminder_preference / strategy_preference / confirmed_weakness），source 决定自动确认（user_statement 直接 confirmed，其余 candidate 待确认），支持编辑/停用/删除，`relevant()` 供 Context Builder 按考试、最近使用取用；Planner 每次 run 将确认记忆拼入 system prompt。

## 历史：第二阶段（Agent tools/policy vertical slice，Task 1–10）

（工作区 `.worktrees/agent-tools-policy-phase2`，分支 `codex/agent-tools-policy-phase2`）

## 已完成的修改

### 基线

- 从 `main` 的里程碑 1 后代 `ab58f94` 创建隔离分支和 worktree。
- 前端基线测试：11 个测试文件、43 个用例全部通过。
- Rust 基线测试：21 个单元测试和 12 个集成测试全部通过。
- Rust 基线构建成功。

### Task 1：冻结 TypeScript parity 契约

提交：`f75d7c96bad5fa22f503362b57a16f85c78cb975`  
提交信息：`test: freeze first agent tool parity contract`

- 新增 `tests/fixtures/agent-tools/plan-get-today.json`。
- 新增 `tests/fixtures/agent-tools/record-checkin-plan.json`。
- 新增 `src/services/agent-tool-parity.test.ts`。
- Characterization tests 覆盖计划派生字段修复、check-in 锁定字段复制、聚合值和错题关联。
- Task 1 指定测试：3 个文件、15 个用例通过。
- 未修改生产 TypeScript service。
- 规格审查通过；代码质量审查无 Critical/Important 问题，仅提出非阻塞的 fixture 驱动和类型收紧建议。

### Task 2：迁移 v5、工具收据字段与所有权设置

提交：`e26f75a`、`254ccef`  
提交信息：`feat: add agent tool receipt migration`、`test: verify agent tool status index`

- `agent_steps` 新增 `policy_json`、`receipt_json`、`undo_json`、`undone_at` 四个 nullable 收据字段。
- 新增 `idx_agent_steps_tool_status(tool_name, status)`，并用真实 SQLite 检查索引存在及列顺序。
- 写入 `plan.get_today=shadow`、`record.checkin_plan=typescript` 两个默认所有权设置。
- 未新增第二张收据表。
- 迁移测试覆盖 v4→v5 升级和全量初始化，并确认既有 `study_plans` 与 v4 Agent 行保留。
- focused db tests：8/8 通过；`cargo fmt -- --check`、`git diff --check` 通过。

### Task 3：稳定工具协议、注册表与 JSON Schema

提交：`aa71034`、`30f4322`、`7e20fec`  
主提交信息：`feat: define agent tool protocol registry`

- 新增 `jsonschema 0.33.0` 直接依赖及稳定的 `ToolRegistry`、risk/confirmation/idempotency/ownership 类型。
- 新增 `plan.get_today@1`、`record.checkin_plan@1` descriptor 与输入 DTO。
- plan 输出 schema 精确要求共享 fixture 的 19 个字段；check-in schema 拒绝顶层/嵌套锁定字段和未知字段。
- registry 按工具名保持唯一版本，输入校验必须先通过 name/version lookup，不能由调用方传入 descriptor 绕过。
- 新增安全稳定错误码及 `IdempotencyConflict` 命令边界精确 code/message 回归测试。
- 修复审查发现的非法 `properties: null` schema，并恢复后续 `list_tools` 所需的 `ListedTool` 公共 DTO。
- focused tools tests：5/5；commands tests：4/4；Clippy `-D warnings`、fmt、diff-check 通过。

### Task 4：R0–R4 策略引擎

提交：`133d310`  
提交信息：`feat: enforce supervised agent tool policy`

- 新增 `src-tauri/src/agent/policy.rs` 并从 `agent/mod.rs` 导出。
- 实现 `PolicyDecision` 五态：Execute、ExecuteWithUndo、PresentSummary、AwaitApproval、NavigateOnly。
- 实现 R0/R1 自动执行、R2 摘要/设置闸门、R3 有效审批（状态、步骤、过期时间、前置条件 hash）校验、R4 仅导航。
- TDD RED 已确认缺少策略 API；GREEN focused policy tests：7/7。
- `cargo fmt --check`、Clippy `-D warnings`、`git diff --check` 通过。
- 规格审查与恢复后的独立代码质量审查均通过；质量审查 Critical/Important/Minor 均为 None。

### Task 5：`plan.get_today` Rust 只读工具

提交：`dbbdf85`、`666e74d`  
提交信息：`feat: add today plan Rust tool`、`test: harden today plan Rust tool`

- 实现本地 04:00 业务日边界和真实 SQLite `plan.get_today` 查询。
- 输出与共享 TypeScript fixture 精确一致，同时保持 Rust R0 查询只读，不回写 `study_plans`。
- 覆盖多记录 SUM、最新非空内容、空内容 fallback、无记录、completed/skipped、三键排序和同时间戳无 ID 次级规则。
- 测试从 fixture 的时间、业务日期和 exam 输入驱动；执行全部当前迁移并用 `PRAGMA query_only=ON` 强制只读。
- Rust focused tests：6/6；TypeScript parity/plan tests：5/5；Clippy、fmt、diff-check 通过。
- 规格与质量复审通过，Critical/Important 均为 None。

### Task 6：原子 exactly-once check-in 与 undo

提交：`7dff6f2`、`7a2c048`、`9978340`、`5022f56`  
主提交信息：`feat: add exactly-once plan check-in tool`

- 实现 `record.checkin_plan` 的锁定计划字段复制、全学习指标、错题写入和精确聚合更新。
- 实现 Task 6 专用最小 `AgentExecutor`：canonical input、幂等 Step reservation/replay/conflict、业务写入、Step receipt 与 `tool.completed` 单事务提交。
- 审计事件 trigger 失败时，业务记录、计划聚合和 Step 全部回滚；Persistence 错误固定脱敏。
- 实现精确四字段 `record.checkin_plan.v1` undo payload、定向补偿、重复 undo 回放及 `tool.undone` 原子回滚。
- 修复审查发现的撤销后 replay 标志、orphan/reassigned wrong question、影响行数校验和完成状态 provenance 问题。
- `receipt_json.compensation` 记录 finish/baseline provenance；有效 finish receipt 必须未撤销且联结仍存在、仍属于同一计划的 record。
- 覆盖 pending/precompleted 的多步逆序撤销链，确保不错误复活或丢失 completed 状态。
- 最终 agent_tools：24/24；Rust 全量：72/72；TypeScript parity/record：12/12；Clippy、fmt、diff-check 通过。
- 最终规格与质量复审通过，Critical/Important 均为 None。

### Task 7：Runtime / Executor / policy / approval / ownership

提交：`cfec345`、`20d1e44`  
提交信息：`feat: connect agent policy executor runtime`、`fix: harden agent executor orchestration`

- 增加精确 ToolCall DTO 与单一 production/test orchestration core，R0–R4 共用 registry、schema、ownership、policy、approval、timeout、output schema 和 audit 顺序。
- ownership 在业务/补偿事务内重读并 fail closed；legacy R1 API 与 undo 也不能绕过所有权。
- 增加 Run status/current_step gate 与完成 CAS；终态、错误 step、取消/中断 Run 均不能产生业务写入。
- R3 approval 支持 UTC 10 分钟到期、决定条件更新、resume 前置条件重检，以及 rejected/expired/stale 的 Step/Run 原子终态。
- Runtime 暴露 list/execute/decide/undo 四个工具方法；lib 使用同一 canonical pool 构造 repository 与 executor。
- failure audit 仅记录安全 code/IDs；output schema 在 completed Step/Event 前校验。
- 最终默认与串行 Rust tests：各 93/93；Clippy、fmt、diff-check 通过。
- 规格与质量复审通过，Critical/Important 均为 None；非阻塞项是真实 R3 工具上线前将 test-stage precondition hash 升级为带版本 SHA-256。

### Task 8：类型化 Tauri 命令与隐藏调试页

提交：`a9ec318`、`a403af5`、`09a7c61`  
主提交信息：`feat: expose first agent tool vertical slice`

- 新增 list/execute/decide/undo 四个类型化 Tauri 命令及精确 camelCase invoke 边界。
- TypeScript DTO 镜像 Rust snake_case/tagged union；descriptor 静态 metadata 与动态 ownership 分离。
- 隐藏 `/agent-debug` 支持 shadow plan read、rust-owned check-in receipt/undo，以及 list persistence error fail-closed。
- 生产 check-in flow、router、sidebar、`PlanCheckinBoard.vue` 和 `record-service.ts` 均未改动。
- 修复同一 Run 读→写 current_step 同步、queued/cancelled running gate、整数 duration 与 outbound JsonValue。
- Completed fresh/replay 使用 `max(local, submittedStep + 1)` 单调对账，既恢复丢失响应后的滞后，也不重复推进或回退。
- 最终 frontend focused：19/19；全 Vitest：55/55；typecheck/build、Rust commands/full suite、Clippy、fmt、diff-check 通过。
- 最终规格与质量复审通过，Critical/Important 均为 None。

### Task 9：并发幂等、隐私稳定错误、迁移升级/恢复与所有权切换文档

提交：`f1fcdf6`  
提交信息：`test: harden agent tool ownership and recovery`

- 新增 `sha2` 依赖，用于 canonical input SHA-256 fingerprint；input snapshot 只保留结构化字段和 fingerprint，不保留 free text。
- WAL 双连接同 key 并发 race：Barrier 同步启动两个 Tokio task，结果为一个 normal completion + 一个 replay，study_records/agent_steps/tool.completed 各恰好为 1。
- 首次事务 rollback 后第二次调用自动重试成为唯一完成者；未解析 key 三次 bounded read 后返回 `idempotency_conflict`（code 和 message 精确匹配），零业务/审计写入。
- 事件 payload 和 command error 不暴露 SECRET_MARKER、SQL 文本、`%APPDATA%` 或绝对路径；`IdempotencyConflict` 命令边界返回精确脱敏 message。
- receipt 的 `permissions` 列表与 descriptor 的 `data_permissions` 一致。
- Pool reopen 后 fingerprint 仍能 replay；free text 变化触发 `idempotency_conflict`。
- v1–v4 文件数据库各自升级到 v5 一次，通过 plugin pool 和 runtime pool 双路径 reopen 验证五表 + 四收据列 + 所有权默认值。
- `prepare_database_restore` 后替换 v4 备份，重启升级一次并保持所有权默认。
- 42 个集成测试、51 个单元测试、12 个 db 测试全部通过；Clippy、fmt、diff-check 通过。
- `migration-runbook.md` 新增精确的 cutover/rollback SQL 和操作规则。
- `feature-parity.md` 新增 `plan.get_today@1` (shadow) 和 `record.checkin_plan@1` (typescript) 两行。

### Task 10：里程碑 2 完整验证与打包出口门

提交：`c210f42`  
提交信息：`docs: complete agent tools policy milestone`

- Vitest：12 个测试文件、55 个用例通过。
- TypeScript typecheck：exit 0。
- Frontend build：exit 0（仅已有 Rollup chunk size 警告）。
- Rust fmt：exit 0。
- Rust 全量测试（parallel）：51 + 12 + 42 = 105 通过。
- Rust 全量测试（serial `--test-threads=1`）：105 通过。
- Clippy `-D warnings`：exit 0。
- `git diff --check`：exit 0。
- `migration-runbook.md` 新增 Milestone 2 Verification Evidence 小节，如实记录并发、隐私、升级和恢复验证结果。
- `feature-parity.md` 补充诚实所有权说明：`plan.get_today` 可在打包读 parity 签字后升为 `rust-owned`；`record.checkin_plan` 保持 `typescript` 直到打包手测签字。
- `MANUAL_TEST.md` 新增 Agent Tools Policy Vertical Slice 手测清单；未完成的打包项保持未勾选。
- 未能在当前环境完成打包手测项（Windows 安装包构建/WebView 交互/R3 UI 审批/备份恢复替换），已如实记录。

## 下一步计划

分支 `codex/agent-tools-policy-phase2` 保留供用户选择：

1. 合并到 `main`。
2. 创建 Pull Request。
3. 继续保留分支进行打包手测验证。
4. 使用 `finishing-a-development-branch` 进行新鲜验证。

## 额度监控

Task 9 和 Task 10 均已完成并提交。
