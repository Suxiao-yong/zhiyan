# 项目进度状态

更新时间：2026-07-18（Asia/Shanghai）  
工作区：`D:\智研\zhiyan\.worktrees\agent-tools-policy-phase2`  
分支：`codex/agent-tools-policy-phase2`

## 当前结论

第二阶段（Agent tools/policy vertical slice）已开始执行。里程碑 1 基线已验证通过；Task 1 已完成并提交；Task 2 正在由独立实现代理按 TDD 实施。当前没有已知阻塞。

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

## 正在处理的问题

### Task 2：迁移 v5、工具收据字段与所有权设置

- 实现代理正在 `src-tauri/src/db.rs` 中先写迁移测试并观察 RED，再追加 migration v5。
- 目标是增加 `agent_steps.policy_json`、`receipt_json`、`undo_json`、`undone_at`，工具状态索引，以及两个 `agent_tool_owner.*` 默认设置。
- 必须验证 v4 升级和全量初始化都保留既有业务行与 Agent 行；不得新增 receipt 表。
- 代理尚未提交 Task 2；因此本状态文件不将其标记为完成。

## 下一步计划

1. 等待 Task 2 实现代理提交，独立进行规格审查和代码质量审查；若有 Critical/Important 问题，先修复并复审。
2. 依次执行 Task 3–Task 6：工具协议/注册表、R0–R4 policy、`plan.get_today`、原子 exactly-once `record.checkin_plan` 与 undo。
3. 执行 Task 7–Task 8：连接 executor/runtime/approval/ownership，并增加类型化 Tauri 命令和隐藏 `/agent-debug` 调试切片。
4. 执行 Task 9：并发幂等、隐私稳定错误、迁移升级/恢复测试及所有权切换/回滚文档。
5. 执行 Task 10：跑完整 Vitest、typecheck、build、Rust test/fmt/clippy/diff 门禁；无法在当前环境完成的打包手测项将保持未勾选并如实记录。
6. 最终使用 `finishing-a-development-branch` 进行新鲜验证，并保留分支供用户选择后续合并、PR 或继续保留。

## 额度监控

当前线程没有设置可查询的 token 预算（`remainingTokens: null`）。我会在每个任务提交和验证节点更新状态；若上下文或执行额度接近上限，将优先保存本文件、记录实际测试证据和未完成任务，再停止扩展工作。

