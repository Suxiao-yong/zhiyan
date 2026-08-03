# 手动测试清单

> Phase 5 E2E 用手动清单替代（Tauri webview 的 Playwright 驱动需 tauri-driver，搭建复杂；本地应用手动测更实际）。

## 一、Windows 兼容性

- [ ] Windows 11 上 `npm run tauri dev` 正常启动（窗口、WebView2）
- [ ] `npm run tauri build` 产出安装包并可安装运行
- [ ] 应用关闭再开，数据持久（考试/记录/计划仍在）
- [ ] 数据库文件位于 `%APPDATA%\com.zhiyan.app\zhiyan.db`
- [ ] 中文字符在 UI、数据、导出 JSON 中均正常

## 二、LLM Provider 兼容性

- [ ] **DeepSeek**：配置 `https://api.deepseek.com` + `deepseek-chat` + Key → 连接测试成功 → AI 计划生成 / 每日分析为 AI
- [ ] **OpenAI**：`https://api.openai.com` + `gpt-4o` + Key → 连接成功
- [ ] **通义千问**：`https://dashscope.aliyuncs.com/compatible-mode` + `qwen-plus` → 连接成功
- [ ] **Kimi**：`https://api.moonshot.cn` + `moonshot-v1-8k` → 连接成功
- [ ] **Ollama**：本机运行 Ollama → `http://localhost:11434` + 模型名（无需 Key）→ 连接成功
- [ ] **未配置 LLM**：计划生成 / 分析走本地降级，标注"本地分析（非 AI）"
- [ ] **错误处理**：错误 Key → "API Key 无效"；超时 → "AI 请求超时"；频率 → "请求过于频繁"

## 三、首次使用完整流程

- [ ] 首次启动 → Welcome 引导（5 步）
- [ ] 选考试类型 → 建考试（名称/日期须晚于今天/总分）→ 添加科目（目标分/水平/权重）→ 知识点 1-5 星自评 → 完成 → 跳转学习计划
- [ ] 重启应用 → 落 Dashboard（非 Welcome）
- [ ] 中途退出引导 → 重开仍进 Welcome

## 四、每日使用流程

- [ ] 打开 → Dashboard 今日时长 / 连续打卡 / 本周时长 / 完成率卡片正确
- [ ] 本周时长趋势柱状图、各科占比环形图渲染；点柱钻取当日记录、点切片过滤趋势
- [ ] 今日待办显示今日计划，点“开始打卡”→ 保存进度后状态为进行中，完成任务后完成率更新
- [ ] 学习记录默认进入“计划打卡”；计划日期/科目/知识点锁定，填写时长/做题/掌握度/时段/心情后保存
- [ ] 同一计划分两次打卡 → 历史记录出现两条，计划实际时长等于两次时长之和
- [ ] 已完成计划可“补充记录”，新增记录后仍保持已完成
- [ ] 跳过任务 → 不产生学习记录；恢复后根据已有记录回到未开始或进行中
- [ ] 选择未来日期 → 任务可查看但不能提前打卡
- [ ] 自由记录（科目/知识点/时长/做题/掌握度/时段/心情）→ 保存 → 来源显示“自由记录”
- [ ] 做题联动：做题数 > 正确数 → 自动弹错题录入区；错题库出现
- [ ] 历史记录筛选（日期范围/科目）、分页、编辑、删除；计划记录显示“计划打卡”来源
- [ ] 编辑计划记录时不能修改计划日期/科目/知识点，修改时长后计划实际时长重新累计
- [ ] 删除计划的最后一条记录 → 计划实际时长归零并回到未开始；关联错题保留
- [ ] 错题库：标记已掌握、看详情、复习+1、筛选
- [ ] 跨天 04:00 归一化：凌晨 00:00-03:59 自由记录（不选日期）→ 归属前一天；计划打卡始终使用计划日期
- [ ] 学习计划：4 视图切换流畅；URL `/study-plan/:view` 同步；甘特图、列表拖拽排序、对比图表
- [ ] AI 分析：每日/每周/阶段 → 报告列表 + Markdown 渲染 + 建议卡片 + 仪表盘；确认/拒绝/应用（AI 建议不自动应用）

## 五、数据导入导出流程

- [ ] 导出全部 / 指定考试 / 日期范围 → JSON 文件，结构含 7 表
- [ ] 导入：skip / overwrite / merge 三种冲突模式；非法 JSON 整批拒绝并提示
- [ ] 升级已有数据库 → migration v3 成功，旧学习记录显示为自由记录，旧计划数据仍可查看
- [ ] 导入不含 `plan_id` 的 v1 JSON 备份 → 校验通过，记录按自由记录导入
- [ ] 删除或重新生成计划 → 已有打卡记录保留并解除计划关联
- [ ] 大数据量（数百条记录）导入导出无 UI 卡死（全局 Loading）
- [ ] 数据库备份 → `.db` 文件（VACUUM INTO 一致性快照）
- [ ] 恢复 → 覆盖 + 应用重启 → 数据恢复

## 六、其它

- [ ] 暗色主题切换即时生效（Element Plus + Tailwind + 背景文字）
- [ ] 数据可视化页 6 图表渲染 + 时间范围/科目筛选 + 导出 PNG
- [ ] 侧边栏各页切换正常
- [ ] 删除考试/科目 → 弹级联确认框（告知数量）→ 子数据清空（应用层级联兜底）
- [ ] `npx vitest run` 单测全过

## Agent Runtime Foundation

- [ ] 运行 `npm.cmd run tauri dev`，直接打开 `/agent-debug`（该隐藏路由不出现在侧边栏）。
- [ ] 页面健康状态显示“可用”，且启动日志没有 panic、migration 或数据库路径错误。
- [ ] 创建测试会话；创建并启动 Run；确认页面状态为 `running`。
- [ ] 保留该 Run 为 `running` 并重启应用；只读检查数据库确认该 Run 变为 `interrupted`，同时存在 `run.interrupted` 审计事件。
- [ ] 重启前处于 `waiting_approval` 的 Run 保持 `waiting_approval`，不会被误改为 `interrupted`。
- [ ] 回归 Dashboard、学习计划、计划打卡、学习记录、分析、可视化和设置，确认原有数据与操作不受 Agent 表影响。
- [ ] 在设置中创建数据库备份；恢复时确认执行顺序为 `closeDb()` -> `agent_prepare_database_restore` -> 替换数据库 -> 重启应用。
- [ ] 恢复后再次打开 `/agent-debug`，确认 Runtime 健康、原有业务数据可读，且 Agent 五表仍存在。

## Agent Tools Policy Vertical Slice

> 以下检查需在打包后的 Windows 应用上执行。未完成的项保持未勾选并如实记录。

- [ ] 备份 `%APPDATA%\com.zhiyan.app\zhiyan.db`，构建并安装 Windows 包。
- [ ] 回归 Dashboard、学习计划、计划打卡、学习记录、错题、分析、可视化、设置、导出、备份和恢复流程。
- [ ] `/agent-debug` 列出两个版本 1 工具描述符；`plan.get_today` 输出与 TypeScript 视图匹配。
- [ ] 在一次性数据库中设置 `record.checkin_plan` 为 `rust-owned`，提交一次打卡并用相同幂等键重复调用；确认只有一条记录和一次时长增量。
- [ ] 执行一次和两次 undo；第一次补偿、第二次为幂等回放。
- [ ] 通过调试构建的 R3 合成测试：无审批零写入、有效审批一次写入、过期/陈旧/拒绝审批零写入。
- [ ] 使用 `closeDb -> agent_prepare_database_restore -> 替换 -> 重启` 恢复备份；验证旧业务和 Agent 行。

## M4 Tray & Scheduler（打包后手测）

> 以下检查需在打包后的 Windows 应用上执行。未完成的项保持未勾选并如实记录。

- [ ] 关闭主窗口后进程常驻托盘，再次打开窗口可恢复。
- [ ] 托盘菜单：打开智研、暂停提醒（勾选状态切换）、今日任务、彻底退出。
- [ ] 暂停提醒后，到提醒时间不再弹通知；恢复后重新提醒。
- [ ] 设置 `agent_reminder_time` 后，`agent_jobs` 中 `task_reminder` 按该时间排程。
- [ ] 到 19:00（或配置时间）且当日有未完成任务时，出现"今日任务提醒"通知，正文只含计数。
- [ ] 有逾期计划时出现"逾期计划提醒"通知，正文含计数与最早日期。
- [ ] `/agent-debug` 的 Background Jobs 区块显示三类每日任务及其状态；Daily Brief 区块可预览今日简报。
- [ ] 应用运行跨天（或休眠唤醒）后，`agent_jobs` 自动出现新一天的三个任务（去重键防重复）。
- [ ] 通知正文不包含计划任务、记录或错题原文。

## M5 Agent OS（打包后手测）

- [ ] 侧栏"Agent"入口或直接访问 `/agent` 打开三栏界面。
- [ ] 左栏"新会话"创建会话；最近会话列表出现并可切换。
- [ ] 中栏发送消息：user/assistant 气泡出现（assistant 显示 token）；重启应用后消息仍在（`agent_messages` 持久化）。
- [ ] 每日简报卡首次打开显示摘要与统计；点"知道了"后折叠为 artifact；`agent-daily-brief` 推送可刷新。
- [ ] 有待审批操作时审批卡显示 R2-R4 与预览；批准/拒绝后卡片消失。
- [ ] 运行状态 pill 反映 queued/running/waiting_approval/completed；运行中可取消。
- [ ] 右栏计划打卡工作台可加载当日计划并完成打卡（与 /study-record 行为一致）。
- [ ] 侧栏工作台深链（仪表盘/学习计划/学习记录/Agent 调试）可跳转，原页面正常。

## M6 Parity & Production（打包后手测）

### 2026-08-04 真机验证（release exe + NSIS setup + CDP 驱动）

- [x] 干净库启动：全新数据目录初始化 v1→v10 无 panic，WebView 加载引导页。
- [x] 首页路由：`agent_os_enabled` 默认(未设/1) → `/` 进 `/agent`（三栏壳：侧栏/对话/简报/审批/五工作台）。
- [x] 回退开关：设置 `agent_os_enabled=0` 重启 → 进 `/dashboard`（旧界面 + 恢复旧分析路径）；恢复 `1` 后回 `/agent`。
- [x] 工作台切换：计划打卡/学习计划/记录与错题/AI 分析/数据可视化 五 tab 点击挂载正确，切换不丢对话。
- [x] Agent Debug 工具列表：9 个工具卡片齐全（`exam.get_active`/`plan.get_range`/`record.get_history` R0、`record.create_free`/`wrong_question.*` R1、`plan.generate` R2，均 rust-owned），健康状态可用。
- [x] 对话链路：发送消息 → 创建会话 + run → planner 本地降级回复（"no llm provider configured"）→ 消息持久化（`agent_sessions` 1 / `agent_messages` 2 / `agent_runs` 1）。
- [x] Windows 安装：`智研_0.1.0_x64-setup.exe` 静默安装成功（zhiyan.exe 26MB + uninstall.exe），安装版启动正常（窗口标题正确），卸载清理无注册表残留。
- [x] 升级演练：自动化覆盖 v1→v10 文件库升级数据保留（`file_databases_from_v1_through_v4` / `prepared_restore_replaces_with_v4_backup` 测试）。**手测发现并修复真实升级 bug**：v4/v5 迁移曾被打补丁（新增表/seed）导致老库启动 panic（101），已恢复不可变迁移并把 M6 seed 移至 v10（`d61defe`）。
- [ ] 通过 Agent Debug 执行 `plan.generate`：首次返回摘要；开启 `agent_r2_auto_execute` 后执行写入 7 天计划；同周重跑不重复生成。（debug 页暂无 generate 执行控件，链路由 Rust 测试 `plan_generate_is_approval_gated_and_idempotent_per_week` 覆盖；待补 debug 控件后真机复验。）
- [ ] 24h 常驻：托盘提醒按时、简报卡正常、无重复每日简报；重启后代理计划/记忆/会话完整。（待长时间运行复验。）
- [ ] 回滚演练：备份数据目录后降级到上一版本安装包，数据可打开。（上一版本无发布安装包，待首个发布后执行。）
