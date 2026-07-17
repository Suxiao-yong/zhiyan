# 智研 Rust Agent OS 重构设计

## 1. 目标

智研将从“传统功能页中嵌入 AI”重构为一个由监督式 Agent 驱动的桌面学习应用。用户进入应用后先看到 Agent 每日简报，再通过对话、行动卡和动态工作台完成计划、学习打卡、错题复盘、数据分析与计划调整。

本次重构采用 Rust 常驻 Agent 内核。Vue 负责交互与可视化，Rust 负责 Agent 运行、数据访问、工具执行、权限审批、后台调度和模型调用。现有功能必须全部保留。

### 成功标准

- Agent 成为首次使用引导完成后的默认入口。
- 每日简报、任务执行、学习打卡、复盘和计划调整形成闭环。
- 关闭主窗口后，Agent 可以在系统托盘继续执行提醒和分析任务。
- LLM 不可用时，计划查询、打卡、统计、提醒和基础调整仍可使用。
- 所有高风险写操作都需要用户确认。
- 用户可以查看、修改和删除 Agent 长期记忆。
- 用户可以查看每次模型调用使用了哪些本地数据。
- 现有考试、计划、记录、错题、分析、可视化和数据管理功能保持可用。

## 2. 已确认的产品决策

| 决策 | 选择 |
|---|---|
| Agent 自主程度 | 监督式。低风险操作自动执行，高风险操作等待确认 |
| 产品入口 | Agent 主界面加功能工作台 |
| 主界面 | Agent OS 三栏布局，加入主动式每日简报 |
| 长期记忆 | 结构化、可查看、可编辑、可删除 |
| LLM 故障 | 保持 Agent 界面，使用本地规则和统计能力降级 |
| 后台运行 | 主窗口关闭后托盘常驻，用户可以彻底退出 |
| 云端上下文 | 只发送当前任务需要的最小数据集 |
| 第一成功场景 | 每日学习闭环 |
| 技术方案 | Rust 常驻 Agent 内核，Vue 作为交互终端 |

## 3. 范围

### 3.1 本次包含

- Rust Agent Runtime 与运行状态机
- Rust 业务数据访问层
- Agent 工具注册、参数校验、执行与审计
- 写操作风险分类、审批和撤销支持
- OpenAI 兼容模型与 Ollama 的 Rust 适配器
- 最小必要上下文构建与模型调用记录
- 结构化长期记忆
- 本地意图路由与规则降级
- 托盘、后台调度、通知和失败重试
- Agent OS 三栏主界面与动态工作台
- 现有业务功能迁移和旧链路清理
- 数据迁移、回滚、测试与发布保护

### 3.2 本次不包含

- 多 Agent 协作界面
- 语音对话
- 云账号、跨设备同步或团队功能
- 插件市场
- 移动端
- 向量数据库
- Agent 自动执行删除考试、覆盖数据库或修改凭据

这些能力可以在统一 Agent Runtime 稳定后单独设计。

## 4. 总体架构

```text
Vue Agent OS
  会话 / 每日简报 / 审批卡 / 执行轨迹 / 动态工作台
                         │
                 Tauri commands + events
                         │
Rust Agent Runtime
  Planner / Executor / Policy / Context / Memory / Scheduler
                         │
    Rust repositories / LLM providers / Search / Notifications
                         │
           SQLite / Keyring / HTTP / Operating System
```

### 4.1 进程边界

智研继续使用一个 Tauri 应用进程。用户关闭主窗口时，Tauri 隐藏窗口并保留托盘进程。用户选择“彻底退出”后，Runtime 停止接收任务，等待正在提交的数据库事务完成，再关闭进程。

不增加 Windows Service。系统服务会扩大安装权限、升级、日志和卸载范围，当前目标不需要它。

### 4.2 单写者原则

最终状态下，Vue 不直接写 SQLite。Vue 通过类型化 Tauri command 调用 Rust 工具。Rust repository 层负责事务、外键、时间归一化和错误转换。

迁移期间允许旧 TypeScript 服务读取和写入现有业务表，但新旧链路不能同时处理同一种写操作。每项功能使用 feature flag 切换写入所有权。Rust 版本通过功能对等测试后，项目删除对应的 TypeScript 写路径。

### 4.3 数据访问

Rust 数据层采用 `sqlx` SQLite 连接池，启用：

- `foreign_keys = ON`
- WAL 日志模式
- `busy_timeout`
- 受控连接数
- 显式事务

迁移代码沿用现有版本序列，从当前版本 3 继续增加。项目先加入 Agent 表，不重建现有业务表。前端 `tauri-plugin-sql` 在业务迁移完成后移除。

Agent 事件用于审计和运行恢复，不替代业务表。考试、计划、记录和错题表仍是业务事实来源。

## 5. Rust 模块设计

```text
src-tauri/src/
├── agent/
│   ├── mod.rs
│   ├── runtime.rs
│   ├── state.rs
│   ├── planner.rs
│   ├── executor.rs
│   ├── policy.rs
│   ├── context.rs
│   ├── memory.rs
│   ├── scheduler.rs
│   ├── event_bus.rs
│   ├── fallback.rs
│   └── tools/
│       ├── mod.rs
│       ├── exam.rs
│       ├── plan.rs
│       ├── record.rs
│       ├── wrong_question.rs
│       ├── analysis.rs
│       ├── visualization.rs
│       ├── search.rs
│       └── data_management.rs
├── db/
│   ├── mod.rs
│   ├── migrations.rs
│   └── repositories/
├── llm/
│   ├── mod.rs
│   ├── provider.rs
│   ├── openai_compatible.rs
│   ├── ollama.rs
│   └── stream.rs
├── tray.rs
└── commands.rs
```

每个模块只有一个职责。Runtime 不拼接 SQL；工具不直接发送 UI 事件；模型适配器不知道业务表结构。

## 6. Agent 运行状态

### 6.1 Run 状态

`queued -> running -> waiting_approval -> running -> completed`

其他终态为 `cancelled` 和 `failed`。应用崩溃后，Runtime 将残留的 `running` 任务恢复为 `interrupted`，然后按步骤的幂等性决定重试或等待用户处理。

### 6.2 Step 状态

每个步骤记录：

- 输入参数快照
- 工具名称和版本
- 风险等级
- 幂等键
- 前置条件版本
- 执行结果或错误
- 开始和结束时间

查询步骤可以安全重试。写步骤执行前生成幂等键，提交事务后写入结果事件。Runtime 不自动重试结果不确定的写步骤。

### 6.3 取消与恢复

用户可以取消尚未提交的 Run。数据库事务一旦提交，Runtime 将结果展示给用户，并在工具支持时提供补偿操作。系统不使用跨工具的长事务。

## 7. Agent 数据表

### 7.1 会话与消息

- `agent_sessions`：标题、考试上下文、状态、最近活动时间
- `agent_messages`：角色、文本、结构化内容、token 用量和模型信息

### 7.2 运行与审计

- `agent_runs`：目标、状态、触发来源、当前步骤和预算
- `agent_steps`：工具步骤、参数、状态、幂等键和结果
- `agent_events`：状态变化、工具调用、错误和 UI 事件
- `agent_approvals`：操作预览、风险、前置条件、选择和过期时间
- `agent_artifacts`：每日简报、计划预览、图表配置和报告引用

### 7.3 记忆与后台任务

- `agent_memories`：类型、内容、来源、置信度、确认状态和最后使用时间
- `agent_jobs`：任务类型、调度时间、去重键、最近结果和重试时间
- `agent_context_audit`：模型调用使用的数据类别、记录 ID、字段集合和用途

`agent_context_audit` 默认不保存完整 Prompt。调试模式可以临时保存脱敏 Prompt，用户可以关闭或清除该功能。

## 8. 工具系统

### 8.1 工具协议

每个工具声明：

- 稳定名称与版本
- JSON Schema 参数
- JSON Schema 返回值
- 风险等级
- 是否需要用户确认
- 是否支持撤销
- 执行超时
- 幂等策略
- 所需数据权限

Runtime 在模型生成工具调用后执行 Schema 校验。模型不能跳过 Policy Engine 或直接调用 repository。

### 8.2 首批工具

查询工具：

- `exam.get_active`
- `plan.get_today`
- `plan.get_range`
- `record.get_history`
- `wrong_question.get_due`
- `analysis.get_summary`
- `visualization.get_dataset`

写工具：

- `record.checkin_plan`
- `record.create_free`
- `record.update`
- `record.delete`
- `plan.update_task`
- `plan.reorder`
- `plan.skip`
- `plan.restore`
- `plan.generate`
- `wrong_question.create`
- `wrong_question.mark_mastered`
- `exam.update`
- `data.export`
- `data.import`
- `data.backup`
- `data.restore`

### 8.3 功能对等映射

| 现有能力 | Agent 入口 | 工作台 |
|---|---|---|
| 考试与科目配置 | 对话创建或修改，关键删除手动完成 | 考试配置工作台 |
| AI/本地计划生成 | Agent 研究、讨论、预览、确认 | 计划工作台 |
| 计划日历、甘特图、列表、对比 | Agent 打开指定视图 | 计划工作台 |
| 计划打卡与自由记录 | 今日行动卡和对话命令 | 打卡工作台 |
| 错题维护 | Agent 提醒复盘和归类 | 错题工作台 |
| 每日、每周、阶段分析 | 每日简报和主动建议 | 洞察工作台 |
| 六类图表 | Agent 根据问题选择图表 | 可视化工作台 |
| 导入、导出、备份、恢复 | Agent 解释并准备操作 | 设置工作台 |

## 9. 权限与审批

### 9.1 风险等级

| 等级 | 示例 | 策略 |
|---|---|---|
| R0 只读 | 查询计划、生成统计、打开图表 | 自动执行 |
| R1 可撤销写入 | 新增一次打卡、创建记忆 | 自动执行，提供撤销 |
| R2 局部修改 | 修改单个任务、更新记忆 | 显示摘要，可按用户设置自动执行 |
| R3 批量或破坏性操作 | 批量重排、删除记录、导入 | 必须确认 |
| R4 安全敏感 | 删除考试、恢复数据库、修改凭据 | Agent 不能执行，只能导航到手动界面 |

### 9.2 审批卡

审批卡显示：

- Agent 的原因
- 修改前后差异
- 受影响记录数
- 是否可以撤销
- 数据版本和过期时间

用户确认后，Executor 重新检查前置条件。数据已经变化时，原审批失效，Agent 生成新预览。

## 10. 模型与上下文

### 10.1 Provider 兼容

Rust 模型层保留现有 Provider：DeepSeek、OpenAI、通义、Kimi、Ollama 和自定义 OpenAI 兼容地址。API Key 继续保存在系统凭据管理器。

OpenAI 兼容 Provider 支持流式文本、工具调用、结构化输出、超时、退避重试和用量记录。Ollama 能力由运行时探测；不支持工具调用的模型使用本地 Planner 加文本生成模式。

### 10.2 最小必要上下文

Context Builder 根据任务读取相关数据：

- 当前考试和目标
- 当前日期附近的计划
- 与问题有关的记录和错题摘要
- 已确认的相关记忆
- 最近会话摘要

用户可以在 Context Inspector 中查看本次使用的数据类型、范围和字段。错题原文、自由文本记录等内容默认不发送，Agent 需要时请求单次授权。

### 10.3 成本控制

- 短意图使用本地路由，不调用 LLM。
- 每日简报先由本地聚合生成，模型只负责解释和建议。
- 对话达到阈值后生成摘要，不重复发送完整历史。
- 工具结果只返回完成任务需要的字段。
- 每个 Run 记录 token 和费用估算，并设置软预算。
- 模型不可用或预算耗尽时切换到本地模式。

## 11. 结构化长期记忆

首版记忆类型：

- `schedule_preference`
- `daily_capacity`
- `subject_preference`
- `learning_constraint`
- `reminder_preference`
- `strategy_preference`
- `confirmed_weakness`

模型提出的记忆先进入候选状态。明确的用户陈述可以自动确认；根据行为推断的记忆必须由用户确认。用户可以编辑、停用或删除每条记忆。

记忆内容不使用向量检索。Runtime 按考试、类型、状态和最近使用时间选择少量相关记录。

## 12. 本地降级

Fallback Engine 处理：

- 今日计划查询
- 计划打卡
- 自由记录
- 逾期检测
- 基础计划调整建议
- 每日和每周统计
- 规则化薄弱点识别
- 通知与提醒
- 工作台导航

离线时，界面继续使用 Agent 形态。消息明确标记“本地模式”，不伪装成模型推理。需要开放式解释、联网研究或复杂计划生成的操作进入待重试状态。

## 13. 后台运行

### 13.1 托盘行为

- 关闭主窗口：隐藏窗口，Agent 保持运行
- 托盘菜单：打开智研、暂停提醒、今日任务、彻底退出
- 彻底退出：停止新任务，完成正在提交的事务，然后退出

### 13.2 后台 Job

- 每日简报生成
- 今日任务提醒
- 逾期检查
- 周报和阶段分析
- 网络恢复重试
- 失败任务清理

Job 使用唯一去重键。系统时间变化、休眠唤醒或应用重启后，Scheduler 只补跑仍有意义的任务。

## 14. Vue Agent OS

### 14.1 文件边界

```text
src/
├── pages/AgentHome.vue
├── components/agent/
│   ├── AgentShell.vue
│   ├── AgentSidebar.vue
│   ├── ConversationPane.vue
│   ├── DailyBrief.vue
│   ├── ApprovalCard.vue
│   ├── RunTimeline.vue
│   ├── ContextInspector.vue
│   ├── AgentStatus.vue
│   └── WorkbenchHost.vue
├── stores/agent.ts
├── services/agent-client.ts
└── workbenches/
```

### 14.2 三栏布局

- 左栏：新会话、最近会话、Agent 任务、工作台入口
- 中栏：每日简报、对话、工具进度、审批和错误恢复
- 右栏：当前 Artifact 或业务工作台

每日简报出现在当天首次打开时。用户处理后，简报折叠为会话中的 Artifact。

### 14.3 工作台迁移

现有 Vue 组件优先复用。页面级组件拆出可嵌入的工作台外壳，不重写图表和表单。原 URL 在迁移期保留，最终作为 Agent 深链接和设置中的备用导航。

## 15. 每日学习闭环

1. Scheduler 聚合今日计划、近期执行率、待复习错题和提醒偏好。
2. 本地生成简报骨架；LLM 可用时补充解释和建议。
3. 用户打开应用后查看简报并选择下一项任务。
4. Agent 打开计划打卡工作台。
5. 用户保存本次学习结果。
6. Rust 在一个事务中写入记录并重算计划进度。
7. Agent 检查偏差；偏差达到阈值时生成调整草案。
8. 用户确认后，Agent 修改后续计划并记录审计事件。

## 16. 错误处理

- LLM 超时：保留会话和 Run，显示重试或本地模式。
- 工具参数错误：不执行工具，将 Schema 错误反馈给 Planner。
- 数据库错误：回滚当前事务，记录不包含敏感字段的错误事件。
- 网络中断：只重试幂等请求；写步骤等待用户处理。
- 应用崩溃：启动时恢复 Run 状态，结果不确定的写步骤不自动重放。
- 审批过期：重新读取数据并生成新差异。
- 数据迁移失败：保留原数据库，恢复启动前备份并停止升级。

## 17. 迁移路线

### 阶段 0：基线与保护

- 建立功能对等矩阵
- 备份真实测试数据库
- 补齐现有服务契约测试
- 记录性能和安装包基线
- 增加 Agent 总开关

### 阶段 1：Runtime 骨架

- 增加 Agent 表和 Rust 数据层
- 实现 Run、Step、Event 和 Approval 状态机
- 实现 Tauri command 和事件通道
- 增加隐藏的 Agent 调试页

### 阶段 2：只读工具

- 迁移考试、计划、记录、错题、分析和图表查询
- 对比 Rust 与 TypeScript 结果
- 保持旧界面为默认入口

### 阶段 3：写工具与 Policy Engine

- 迁移计划打卡和自由记录
- 迁移计划修改、跳过、恢复和排序
- 迁移错题与考试配置写入
- 实现审批、幂等和补偿操作

### 阶段 4：模型、上下文与记忆

- 迁移 LLM Provider
- 实现流式对话和工具循环
- 实现 Context Builder、Context Inspector 和费用记录
- 实现结构化记忆与本地降级

### 阶段 5：托盘与调度

- 实现窗口隐藏、托盘菜单和彻底退出
- 实现 Job 调度、去重、补跑和通知
- 接入每日简报、逾期检查和周报

### 阶段 6：Agent OS 界面

- 将 AgentHome 设为默认路由
- 实现三栏布局、每日简报、审批卡和动态工作台
- 将现有页面组件嵌入 WorkbenchHost

### 阶段 7：全面迁移

- 迁移计划生成、搜索、分析、导入导出和恢复
- 完成功能对等验收
- 移除前端直接写数据库和直接调用 LLM 的路径
- 合并原规划 Agent 与分析 Agent

### 阶段 8：生产化

- 安全测试、崩溃恢复和长时间运行测试
- Windows 安装、升级、卸载和数据库回滚测试
- 性能、内存和安装包优化
- 保留一个版本的旧界面紧急回退开关

每个阶段独立提交并保持主分支可运行。下一阶段不能依赖未通过验收的功能。

## 18. 测试策略

### Rust

- 状态机单元测试
- Policy Engine 决策表测试
- 工具 Schema 与参数属性测试
- Repository SQLite 集成测试
- 事务、幂等和崩溃恢复测试
- Scheduler 时间跳变和补跑测试
- LLM Provider 模拟服务器测试

### Vue

- Agent store 单元测试
- 每日简报、审批卡和运行轨迹组件测试
- WorkbenchHost 路由测试
- 流式事件和断线恢复测试

### 跨层

- Tauri command 契约测试
- 旧服务与 Rust 工具功能对等测试
- 真实桌面 E2E 测试
- 数据库从版本 1、2、3 升级测试
- 备份恢复后 Agent 状态一致性测试

## 19. 发布验收

- 原有功能对等率达到 100%。
- R3 和 R4 操作未经确认的执行次数为 0。
- 相同幂等键不会创建重复打卡或重复计划修改。
- Run 中断后可以恢复、取消或明确要求人工处理。
- LLM 不可用时，核心学习功能保持可用。
- 用户可以管理全部长期记忆。
- Context Inspector 能展示每次模型调用的数据范围。
- 数据库迁移失败后原数据保持完整。
- 应用在托盘运行 24 小时后无重复通知和失控重试。
- 自动化测试、类型检查、Rust lint、前端构建和 Tauri 构建全部通过。

## 20. 开发模型与成本策略

开发过程不固定使用单一旗舰模型：

- 架构、迁移、并发、安全和疑难故障使用高能力模型。
- 常规 Rust、Vue、测试和修复使用均衡模型。
- 文档、格式化和机械迁移使用低成本模型。

每个任务只加载相关文件与本设计文档。单次上下文不得包含整个仓库。阶段结束后将结果写入代码、测试和进度文档，再开始新上下文。

如果直接使用 OpenAI API，GPT-5.5 适合高风险设计和审查，但其输入和输出单价高于成本型模型。开发代理应将长输入控制在 272K tokens 以下，并复用稳定上下文以利用缓存。Codex 套餐的实际额度消耗以产品计费界面为准。

## 21. 完成定义

当以下条件同时满足时，本次重构完成：

- Agent OS 成为默认产品界面。
- Rust Runtime 接管 Agent 编排、模型、后台任务和全部业务写入。
- 所有现有功能可以通过 Agent 或工作台访问。
- 旧的前端 Agent 编排和直接数据库写入代码已经删除。
- 安全、迁移、离线、托盘和桌面 E2E 验收通过。
- README、用户指南、隐私说明和手工测试文档已经更新。
