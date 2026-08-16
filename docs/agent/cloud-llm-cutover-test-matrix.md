# Agent 产品验收矩阵

> 目标：以 Agent 为唯一 AI 入口、云端 LLM 负责理解与生成、本地 Rust/SQLite 负责事实与提交。
> 手工验收时使用 OpenAI-compatible mock server 或测试 provider；仓库、截图、日志和测试
> fixture 中不得出现真实 API Key。每项通过后勾选并记录日期/构建号。
> 更新：2026-08-16。A–G 为打包手测项（未在本环境执行，保持未勾选，不得用静态检查代替）；
> H 为已实际执行的自动化命令，勾选项均附命令输出证据。

## A. 首次启动（Task 19 Step 2）

- [ ] 无考试：进入 Today/Agent，显示"请先配置考试"空状态，引导到 Setup；无空白面板。
- [ ] 有考试、无 LLM：Today 显示本地统计简报与"尚未连接云端模型"提示；本地数据可查看编辑。
- [ ] 有考试 + 已配置 LLM（含数据出境同意）：Agent 能回答"今天安排了什么"，答案中的计划来自 SQLite 当前记录。
- [ ] 旧 `agent_os_enabled=0` 仍进入 /agent（不再回退旧 Dashboard）。

## B. 只读 Agent（Step 3）

- [ ] 输入"今天安排了什么"，返回内容中的计划与 SQLite 一致。
- [ ] 修改数据库中的计划后重新询问，结果随数据变化（无缓存/伪造）。
- [ ] provider 缺失时本地降级回答明确标注非模型内容，不伪造"模型已分析"。

## C. 计划生成与调整（Step 4）

- [ ] 输入"根据剩余天数调整计划"，出现包含真实 Draft 行、冲突和 precondition 的计划预览。
- [ ] 点击取消：数据库零变化。
- [ ] 点击确认：数据库只发生预览中的变化；重复确认不产生重复任务（幂等键）。
- [ ] 手动修改原计划后再确认：因 precondition 变化安全失败（`precondition_changed`）。
- [ ] 跨考试：session 绑定 exam A 时，输入引用 exam B 的计划/记录 → `tool_scope_violation`，无数据库变化。

## D. 学习记录与错题动作（Step 5）

- [ ] 自然语言整理学习记录：预览 → 确认 → 写入；字段校验失败不写入。
- [ ] 创建错题：确认后写入；任一失败不留半条记录（事务）。
- [ ] 标记错题已掌握：本地状态变更；`undo_available=true` 时展示撤销。
- [ ] 撤销（undo）打卡/记录：补偿生效；重复撤销为幂等回放。
- [ ] `record.checkin_plan` 即使存在旧 `agent_tool_owner.*=typescript` 设置仍按 Rust-owned 执行。

## E. Provider 异常（Step 6）

- [ ] 401 → "API Key 无效"类错误；UI 展示可理解的错误与设置入口。
- [ ] 429 / 超时 / 断网 / 空响应 → 对应错误码；不伪造成功消息。
- [ ] 恢复配置后重试成功。

## F. 数据同意、隔离与 run 生命周期（Step 7）

- [ ] 未同意数据出境：provider 请求只包含固定诊断内容或被 `consent_required` 拒绝，业务数据不出本地。
- [ ] 修改 provider/base URL/model 后旧同意失效，需重新确认。
- [ ] run 终态：文本完成 → `completed`；等待审批 → `waiting_approval`；取消 → `cancelled`；provider/schema/持久化错误 → `failed`；无遗留 `running`。
- [ ] 进程恢复：中断的 run 变为 `interrupted`；`waiting_approval` 的 run 保持不动。

## G. 旧路由与历史数据（Step 8）

- [ ] 生产构建直接访问 `/dashboard`、`/analysis`、`/visualization` → 重定向到 /agent。
- [ ] 生产构建 `/agent-debug` → 重定向或不可见（dev-only 路由）。
- [ ] 旧 JSON 备份可导入；旧 SQLite backup 可恢复；`ai_analyses` 历史数据保留可读。
- [ ] 新流程不写入 `ai_analyses`、`agent_memories`、`agent_tool_owner.*`。

## H. 自动化回归（2026-08-16 已执行）

- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib`：**164 passed, 0 failed**
      （含 db/executor/planner/runtime/scheduler/brief/llm/context_snapshot）。
- [x] `npm.cmd test -- --run`：**13 个测试文件、79 个用例全部通过**（含 AgentHome/AgentDebug/router/export/analyzer/settings）。
- [x] `npm.cmd run typecheck`：exit 0。
- [x] `npm.cmd run build`：exit 0（仅已有 chunk size 与动态导入 warning）。
- [x] `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`：通过。
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings`：通过。
- [ ] `npm.cmd run tauri build`（MSI/WiX）：release 编译通过，但 WiX `light.exe` 打包
      失败（本环境 WiX 工具链问题）；改用 `tauri build --bundles nsis` 成功产出
      `智研_0.1.0_x64-setup.exe`（NSIS 安装包，6MB）。打包安装/手测项仍未执行，等待用户验收。
