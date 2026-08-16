# 手动测试清单

> 产品已收敛为：Today/Agent（唯一 AI 入口）、Plan、Records、Setup/Settings 四个用户区域。
> Agent 产品验收请以 `docs/agent/cloud-llm-cutover-test-matrix.md` 为准；本节为平台与周边回归清单。
> 打包后手测项未完成保持未勾选并如实记录。

## 一、Windows 兼容性

- [ ] Windows 11 上 `npm run tauri dev` 正常启动（窗口、WebView2）
- [ ] `npm run tauri build` 产出安装包并可安装运行
- [ ] 应用关闭再开，数据持久（考试/记录/计划/会话仍在）
- [ ] 数据库文件位于 `%APPDATA%\com.zhiyan.app\zhiyan.db`
- [ ] 中文字符在 UI、数据、导出 JSON 中均正常

## 二、首次使用完整流程

- [ ] 首次启动 → Welcome 引导（选考试类型 → 建考试 → 添加科目 → 知识点自评 → 完成）
- [ ] 重启应用 → 进入 /agent（Today 三栏界面），非 Welcome
- [ ] 中途退出引导 → 重开仍进 Welcome

## 三、四个用户区域

- [ ] **Today/Agent**：简报卡显示本地统计；对话可发送消息；待审批卡可确认/取消；右侧今日打卡/计划/记录工作台可切换且不丢会话。
- [ ] **Plan**：日历/列表两个视图；列表拖拽排序、状态修改、单项编辑；计划生成与调整通过 Agent 完成（预览 → 确认）。
- [ ] **Records**：学习记录、错题、复盘；自由记录与计划打卡；错题标记掌握与撤销。
- [ ] **Setup/Settings**：考试配置、LLM 连接（单一 OpenAI-compatible provider，测试按钮走 Rust command）、主题、通知、数据备份恢复。

## 四、数据导入导出与备份

- [ ] 导出全部 / 指定考试 / 日期范围 → JSON 文件，结构含业务表与 `ai_analyses`（历史兼容）
- [ ] 导入：skip / overwrite / merge 三种冲突模式；非法 JSON 整批拒绝并提示
- [ ] 升级已有数据库 → migration v1–v10 成功，旧学习记录、计划、`ai_analyses` 历史数据仍可查看
- [ ] 数据库备份 → `.db` 文件（VACUUM INTO 一致性快照）；恢复 → 覆盖 + 重启 → 数据恢复
- [ ] 大数据量（数百条记录）导入导出无 UI 卡死（全局 Loading）

## 五、托盘与提醒（打包后手测）

- [ ] 关闭主窗口后进程常驻托盘，再次打开窗口可恢复
- [ ] 托盘菜单：打开智研、暂停提醒（勾选状态切换）、今日任务、彻底退出
- [ ] 暂停提醒后到提醒时间不再弹通知；恢复后重新提醒
- [ ] 到配置的提醒时间且当日有未完成任务时出现"今日任务提醒"通知，正文只含计数
- [ ] 有逾期计划时出现"逾期计划提醒"通知，正文含计数与最早日期
- [ ] 通知正文不包含计划任务、记录或错题原文

## 六、旧路由与调试面

- [ ] `/dashboard`、`/analysis`、`/visualization` 重定向到 /agent
- [ ] 生产构建 `/agent-debug` 不可见（dev-only）；开发构建可打开且只含诊断（health/run/tool schema/审计/planner loop），无业务写入按钮

## 七、自动化回归（2026-08-16 已执行）

- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib`：164 passed, 0 failed
- [x] `npm.cmd test -- --run`：13 个测试文件、79 个用例全部通过
- [x] `npm.cmd run typecheck`：exit 0
- [x] `npm.cmd run build`：exit 0（仅已有 chunk size 与动态导入 warning）
- [x] `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`：通过
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings`：通过
- [ ] `npm.cmd run tauri build`（MSI/WiX）：release 编译通过，WiX `light.exe` 在本环境
      打包失败；`tauri build --bundles nsis` 成功产出 `智研_0.1.0_x64-setup.exe`（NSIS 安装包）。
      安装运行与手测项等待用户验收环境执行。
