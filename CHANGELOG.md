# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
