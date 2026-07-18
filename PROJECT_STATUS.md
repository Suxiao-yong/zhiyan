# 项目进度状态

更新时间：2026-07-18（Asia/Shanghai，用户要求状态总结后更新）  
工作区：`D:\智研\zhiyan\.worktrees\agent-tools-policy-phase2`  
分支：`codex/agent-tools-policy-phase2`

## 当前结论

第二阶段（Agent tools/policy vertical slice）已开始执行。里程碑 1 基线已验证通过；Task 1–Task 8 已完成并通过规格与质量双审查；下一项为 Task 9。当前没有代码实现阻塞。

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

## 正在处理的问题

### 当前待处理：Task 9 并发、隐私、升级恢复与回滚

- 增加 WAL 双连接同 key 并发 race、三次 bounded replay read、稳定 idempotency_conflict。
- 加固事件/命令隐私、v1–v4 升级与 restore、ownership cutover/rollback 文档。

## 下一步计划

1. 执行 Task 9：并发幂等、隐私稳定错误、迁移升级/恢复测试及所有权切换/回滚文档。
2. 执行 Task 10：跑完整 Vitest、typecheck、build、Rust test/fmt/clippy/diff 门禁；无法在当前环境完成的打包手测项将保持未勾选并如实记录。
3. 最终使用 `finishing-a-development-branch` 进行新鲜验证，并保留分支供用户选择后续合并、PR 或继续保留。

## 额度监控

当前线程没有设置可查询的 token 预算（`remainingTokens: null`）。我会在每个任务提交和验证节点更新状态；若上下文或执行额度接近上限，将优先保存本文件、记录实际测试证据和未完成任务，再停止扩展工作。
