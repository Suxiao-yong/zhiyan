# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
