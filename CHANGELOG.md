# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **学习效果闭环（AI 辅助学习者）**：打通“打卡 → 检查题 → 掌握度 → 计划 → 复习调度”的效果闭环：
  - **错题间隔重复调度**：migration v11 为 `wrong_questions` 增加 `next_review_at` / `review_interval_days` / `ease_factor` 列与到期部分索引（v1–v10 SQL 不变）；新增 `review.rs` SM-2 lite 纯函数模块（失败归零次日重见，成功按 1→6→×ease 爬梯，ease 地板 1.3、间隔上限 365 天）。
  - **Agent 新工具**：`review.get_due`（R0 只读，查询今日应复习错题）与 `review.complete`（R1 自动执行 + 事务级撤销，SM-2 调度写入并自增 `review_count`）；工具总数 11→13。
  - **掌握度闭环修复**：打卡/自由记录后按知识点最近 3 条 `mastery_rating` 均值重算 `knowledge_points.current_mastery`（此前该字段无任何更新路径，为死数据）；聚合在 SAVEPOINT 内执行，保持“failed tool 无已提交业务写入”不变式。
  - **计划证据链**：周计划草案任务在审批卡携带确定性依据——掌握度、未掌握错题数、距考试天数与中文理由（仅进审批预览投影，不改变落库契约）。
  - **教学法提示词**：Agent 打卡先出检查题、答错时苏格拉底式反问引导、基于历史错题出变式题、根据检查结果如实评定 `mastery_rating`。
  - **提醒集成**：`task_reminder` 文案追加今日待复习错题数。
  - **前端**：右栏工作台新增“今日复习”卡片（由新增只读命令 `review_list_due` 驱动，preview-safe 回退空结果）。
- **Agent Runtime 基础（里程碑 1）**：通过 migration v4 新增五张持久化表（`agent_sessions`、`agent_runs`、`agent_steps`、`agent_events`、`agent_approvals`）；实现 Run 状态机、审计事件、启动恢复（`running` → `interrupted`，保留 `waiting_approval`）；暴露 `agent_health`、`agent_prepare_database_restore` 命令；新增隐藏路由 `/agent-debug`。
- **Agent 工具与策略垂直切片（里程碑 2）**：
  - Migration v5：为 `agent_steps` 增加 `policy_json`、`receipt_json`、`undo_json`、`undone_at` 收据列与 `idx_agent_steps_tool_status` 索引，并写入 `plan.get_today=shadow`、`record.checkin_plan=typescript` 所有权默认值。
  - 稳定工具协议：`ToolRegistry` + JSON Schema 校验，注册 `plan.get_today@1`（R0 只读）与 `record.checkin_plan@1`（R1 exactly-once + undo）。
  - R0–R4 策略引擎：R0 自动、R1 自动+撤销、R2 摘要/设置闸门、R3 有效审批校验、R4 仅导航。
  - `plan.get_today`：本地 04:00 业务日边界、真实 SQLite 只读查询，输出与 TypeScript fixture 精确一致。
  - `record.checkin_plan`：锁定计划字段复制、全学习指标、错题写入、聚合更新、原子 exactly-once 与幂等 replay；`record.checkin_plan.v1` undo 补偿事务定向回滚并重算聚合。
  - 并发与幂等：WAL 双连接同 key race 解析（三次 bounded 重读，未解析返回 `idempotency_conflict`，零重复写入）。
  - 所有权闸门：`shadow`/`typescript`/`rust-owned` 防止 TS/Rust 双写，`record.checkin_plan` 在显式切换前保持 TypeScript 所有。
  - 隐私：输入只存结构化 snapshot + SHA-256 fingerprint，事件与命令错误脱敏，不含 free-text、SQL、路径。
  - 类型化 Tauri 命令 `agent_list_tools`/`agent_execute_tool`/`agent_decide_approval`/`agent_undo_tool` 与隐藏调试页；生产打卡流程未改动。

### Changed

- `record.checkin_plan` 对 skipped/future 计划的拒绝从 `tool_schema_invalid` 改为 `conflict`（业务状态冲突而非输入畸形），不再因此将整个 Run 标记为失败。
- **Cloud LLM Agent 简化改造（2026-08-16）**：唯一 AI 入口收敛为 Agent 对话（Rust `OpenAiCompatibleProvider`，reqwest）；删除 TypeScript LLM 适配、AnySearch 联网搜索与本地降级链路；计划生成/调整、学习记录、错题、复盘全部通过 Agent 工具（R3 审批 + 脱敏预览 + 事务级 undo）；`plan.preview_generate`/`plan.apply_preview` 落地为生产工具。
  - 导航收敛为四个心智入口（Today `/agent`、Plan、Records、Settings），`/dashboard`、`/analysis`、`/visualization` 重定向到 `/agent`；`/agent-debug` 仅开发构建注册。
  - 调度器只保留 `task_reminder` 与 `overdue_check`；每日简报改为本地确定性按需读取；长期记忆表保留但新代码不再读取。
  - 跨层设置契约修复：提醒时间统一读写 `reminder_time`（原 Rust 侧误读 `agent_reminder_time`，设置永不生效）；调度提醒尊重 `notification_enabled` 开关；切换 provider 时刷新 keyring 状态；活跃考试同步 `agent_active_exam_id` 供提醒使用。
  - 引导完成后跳转 `/agent`（单一入口契约）；`vitest` 默认超时提高至 15s 消除冷启动偶发超时；`@types/node` 纳入 devDependencies。

## [0.1.0] - 2026-07-02

### Added

- **考试配置**：支持考研、考公、考证、自定义考试四种类型；科目管理 + 树形知识点结构
- **学习计划**：AI / 本地算法双模式生成计划；日历、甘特图、列表、计划 vs 实际对比四视图；拖拽排序
- **学习记录**：日历打卡、快速记录、做题 + 错题自动联动、跨天 04:00 归一化
- **数据可视化**：时长趋势、各科占比、正确率曲线、进度雷达、知识点热力图、分数预测仪表（可导出 PNG）
- **AI 分析**：半 Agent 模式 — 每日 / 每周 / 阶段诊断与分数预测；建议需用户确认后应用；无 LLM 时降级为本地统计
- **AI 规划助手**：联网搜索 + 多轮讨论 + 自动展开为逐日计划
- **数据管理**：JSON 导入导出（分批 + schema 校验 + 冲突处理）；数据库备份恢复
- **安全**：API Key 经 OS 凭据管理器（DPAPI）加密 + SQLite 混淆降级
- **主题**：亮色 / 暗色主题切换
- **通知**：桌面提醒（每日学习提醒 + 启动补发）
- **LLM 兼容**：DeepSeek / OpenAI / 通义千问 / Kimi / Ollama / 自定义（OpenAI 兼容接口）
- **联网搜索**：AnySearch API 集成（匿名可用，Key 可选更高限额）
