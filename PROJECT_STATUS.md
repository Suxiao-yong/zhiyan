# 项目进度状态

更新时间：2026-07-18（Asia/Shanghai）  
工作区：`D:\智研\zhiyan\.worktrees\agent-tools-policy-phase2`  
分支：`codex/agent-tools-policy-phase2`

## 当前结论

第二阶段（Agent tools/policy vertical slice）已开始执行。里程碑 1 基线已验证通过；Task 1–Task 3 已完成并通过规格与质量双审查；Task 4 即将开始。当前没有已知阻塞。

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

## 正在处理的问题

### Task 4：R0–R4 策略引擎

- 下一项工作将先写完整决策表 RED 测试，再实现纯函数策略判定和 R3 approval precondition 校验。
- 策略层只能使用 descriptor 的不可变 risk/confirmation/data permissions，不接受请求覆盖这些元数据。

## 下一步计划

1. 等待 Task 2 实现代理提交，独立进行规格审查和代码质量审查；若有 Critical/Important 问题，先修复并复审。
2. 依次执行 Task 3–Task 6：工具协议/注册表、R0–R4 policy、`plan.get_today`、原子 exactly-once `record.checkin_plan` 与 undo。
3. 执行 Task 7–Task 8：连接 executor/runtime/approval/ownership，并增加类型化 Tauri 命令和隐藏 `/agent-debug` 调试切片。
4. 执行 Task 9：并发幂等、隐私稳定错误、迁移升级/恢复测试及所有权切换/回滚文档。
5. 执行 Task 10：跑完整 Vitest、typecheck、build、Rust test/fmt/clippy/diff 门禁；无法在当前环境完成的打包手测项将保持未勾选并如实记录。
6. 最终使用 `finishing-a-development-branch` 进行新鲜验证，并保留分支供用户选择后续合并、PR 或继续保留。

## 额度监控

当前线程没有设置可查询的 token 预算（`remainingTokens: null`）。我会在每个任务提交和验证节点更新状态；若上下文或执行额度接近上限，将优先保存本文件、记录实际测试证据和未完成任务，再停止扩展工作。
