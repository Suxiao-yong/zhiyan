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
- [ ] 今日待办显示今日计划，勾选 → 完成率更新
- [ ] 学习记录：快速记录（科目/知识点/时长/做题/掌握度/时段/心情）→ 保存 → 日历打绿点、列表出现
- [ ] 做题联动：做题数 > 正确数 → 自动弹错题录入区；错题库出现
- [ ] 历史记录筛选（日期范围/科目）、分页、编辑、删除
- [ ] 错题库：标记已掌握、看详情、复习+1、筛选
- [ ] 跨天 04:00 归一化：凌晨 00:00-03:59 快速记录（不选日期）→ 归属前一天；补记（选日期）不归一化
- [ ] 学习计划：4 视图切换流畅；URL `/study-plan/:view` 同步；甘特图、列表拖拽排序、对比图表
- [ ] AI 分析：每日/每周/阶段 → 报告列表 + Markdown 渲染 + 建议卡片 + 仪表盘；确认/拒绝/应用（AI 建议不自动应用）

## 五、数据导入导出流程

- [ ] 导出全部 / 指定考试 / 日期范围 → JSON 文件，结构含 7 表
- [ ] 导入：skip / overwrite / merge 三种冲突模式；非法 JSON 整批拒绝并提示
- [ ] 大数据量（数百条记录）导入导出无 UI 卡死（全局 Loading）
- [ ] 数据库备份 → `.db` 文件（VACUUM INTO 一致性快照）
- [ ] 恢复 → 覆盖 + 应用重启 → 数据恢复

## 六、其它

- [ ] 暗色主题切换即时生效（Element Plus + Tailwind + 背景文字）
- [ ] 数据可视化页 6 图表渲染 + 时间范围/科目筛选 + 导出 PNG
- [ ] 侧边栏各页切换正常
- [ ] 删除考试/科目 → 弹级联确认框（告知数量）→ 子数据清空（应用层级联兜底）
- [ ] `npx vitest run` 单测全过
