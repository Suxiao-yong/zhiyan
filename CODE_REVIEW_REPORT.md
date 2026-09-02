# 智研 zhiyan 全量代码详细检查报告

> **版本**: 0.2.0 | **栈**: Vue 3 + TypeScript + Tauri 2 (Rust + SQLite via `tauri-plugin-sql`)  
> **检查时间**: 2026-08-28 | **检查人**: pi (全量扫描 `src/` + `src-tauri/`)  
> **静态基线**: `vue-tsc --noEmit` **0 error**；`eslint .` **1 warning** (`vue/attributes-order`)；`vitest` 受 `rollup` 可选依赖缺失阻塞（环境问题，重装 `node_modules` 可解，非代码问题）；`cargo test` 迁移单测在 `src-tauri/src/db.rs` 内覆盖 10 项（含 FK/触发器/幂等）  
> **规则**: 前后端不匹配 > 逻辑错误 > 编码/健壮性；每项给出 **文件:行号 / 复现 / 影响 / 修复**

---

## 1. 执行摘要

| 级别                                      | 数量   | 定义                            |
| ----------------------------------------- | ------ | ------------------------------- |
| 🔴 严重（数据丢失/泄露/功能不可用）       | 5      | 必须随 P0 修复                  |
| 🟠 高（功能错误/口径不一致/边界破坏体验） | 6      | P1 修复                         |
| 🟡 中（健壮性/可维护性/覆盖率缺口）       | 9      | P2 修复                         |
| **合计**                                  | **20** | 含 2 项既有测试已锁定的显式契约 |

> 整体工程质量较高：全库参数化、`COLUMNS` 白名单、触发器统一 `localtime`、级联策略显式兜底、外键实测、密钥永不落地前端。问题集中在 **v12 内容侧两表未打通导出链路**、**SM-2 三列未进前端类型**、**`last_review_at` UTC 混写** 三处历史债务，其余为边界与一致性收敛点。

---

## 2. 前后端不匹配（Frontend ↔ Backend Contract）

### 🔴 M-01 导出/导入静默丢失 `materials` / `flashcards`（数据丢失）

- **文件**
  - `src/services/export.ts:47-92 (TABLES, exportData)`
  - `src/services/db.ts:14-18 (ALLOWED_TABLES 含两表)` / `src-tauri/src/db.rs:迁移 v12`
  - `src/types/index.ts` 无 `Material`/`Flashcard` 类型
- **现状**  
  `db.rs` v12 已创建 `materials (id, exam_id, subject_id, title, content, created_at)` 与 `flashcards (id, subject_id, knowledge_point_id, material_id, source_ref, front/back, review_*, created_at, last_review_at)`，`db.ts` `ALLOWED_TABLES`/`COLUMNS` 亦已补齐，且 `exam-service.ts:getExamCascadeCounts/deleteSubjectCascade` 正确按两表计数与清理。  
  但 `export.ts:47` `TABLES = ['exams','subjects','knowledge_points','study_plans','study_records','wrong_questions','ai_analyses']` **遗漏两表**；`exportData()` 4 段分片查询均未涉及两表；`importData()` 同 `TABLES` 循环；`types/index.ts` 亦无对应接口，页面只能 `any` 透传。
- **复现**
  1. 新建考试 → 导入材料（`ImportMaterialDialog.vue` 调用 `materials` 白名单直写 + 触发闪卡生成）
  2. 全量导出 JSON → 检查 `bundle` 只有 7 键
  3. 清库导入 → 材料原文与闪卡丢失，`knowledge_points.material_id/source_ref` 悬空
- **影响**  
  全量备份实际不全量；跨设备迁移/恢复后溯源链路断裂；`flashcards` SM-2 进度丢失。
- **修复（约 40 行）**

  ```ts
  const TABLES = ['exams','subjects','knowledge_points','materials','flashcards','study_plans','study_records','wrong_questions','ai_analyses'] as const
  // exportData: 在 kp 之后插入
  const materials = await queryChunked<any>(`SELECT * FROM materials ${kpWhere}`, kpParams) // 或按 exam_id 直查
  const flashcards = await queryChunked<any>(`SELECT * FROM flashcards ${...}`, ...)
  // importData: 按依赖顺序插入 materials → flashcards → knowledge_points → 其余
  ```

  新增 `src/types/index.ts`：

  ```ts
  export interface Material {
    id: string
    exam_id: string
    subject_id: string
    title: string
    content: string
    created_at: string
  }
  export interface Flashcard {
    id: string
    subject_id: string
    knowledge_point_id: string | null
    material_id: string | null
    source_ref: string | null
    front: string
    back: string
    review_count: number
    mastered: number
    next_review_at: string | null
    review_interval_days: number
    ease_factor: number
    created_at: string
    last_review_at: string | null
  }
  ```

### 🔴 M-02 `WrongQuestion` 前端类型缺 SM-2 三列（功能不可用）

- **文件**
  - `src/types/index.ts:92-106` `interface WrongQuestion`
  - `src-tauri/src/db.rs:迁移 v11 (next_review_at/review_interval_days/ease_factor)`
  - `src/services/db.ts:117-120` `COLUMNS['wrong_questions']` 已含三列
  - `src-tauri/src/agent/tools/review.rs` / `src-tauri/src/review.rs` 完整 SM-2 调度
- **现状**  
  Rust 侧 `wrong_questions` 已含 `next_review_at TEXT, review_interval_days REAL DEFAULT 0, ease_factor REAL DEFAULT 2.5` 且有部分索引 `idx_wrong_questions_due WHERE mastered=0`；`db.ts` 白名单正确；但 `types/index.ts` 仅有 `review_count/mastered/last_review_at`，缺 3 列。
- **影响**  
  前端 `WrongQuestionList.vue` / `ReviewWorkbench.vue` 无法读/显间隔与下次日期；`record-service.ts:setWrongMastered/incrementWrongReview` 等需 `as any`；与 Rust `review.complete` 返回的 `interval_days/ease_factor/next_review_at` 无法落库回显。
- **修复**

  ```ts
  export interface WrongQuestion {
    // ...
    review_count: number
    mastered: number
    next_review_at: string | null
    review_interval_days: number
    ease_factor: number
    created_at: string
    last_review_at: string | null
  }
  ```

### 🔴 M-03 考试级导出在空科目时回退为全量（数据泄露/范围错误）

- **文件**  
  `src/services/export.ts:60-92`

  ```ts
  const kpWhere = range.scope === 'exam' && subjectIds.length ? `WHERE subject_id IN (...)` : ''
  // recSql / wqSql 同理
  ```

- **复现**  
  创建新考试未建科目（`subjects=[]`）→ 选择“按考试导出”→ `subjectIds.length===0`→ `kpWhere=''`、`recSql='SELECT * FROM study_records'`（无 WHERE）→ 导出全库记录/错题/KP。
- **影响**  
  选定一考试却拿到别考试数据；分享 JSON 即泄露；`knowledge_points` 同理。
- **修复**

  ```ts
  if (range.scope==='exam' && !subjectIds.length) {
    // 该考试下无科目：直接空结果，不扫描全表
    return { ... , knowledge_points:[], study_records:[], wrong_questions:[], materials:[], flashcards:[] }
  }
  // 或将无条件分支改为 WHERE 1=0
  ```

### 🟠 M-04 `suggest_knowledge_points` 的 `exam_type` 命名口径

- **文件**
  - 后端 `src-tauri/src/suggest.rs:18-22` `struct SuggestKpInput { exam_type, exam_name, subjects }`
  - 前端 `src/pages/Welcome.vue:259-268` `invoke('suggest_knowledge_points', { input:{ exam_type, exam_name, subjects }})`
  - `src-tauri/src/lib.rs:87` 注册命令
- **现状**  
  前端已正确用 `snake_case` 且包一层 `{ input: {...}}`（Tauri 单参结构体需此包装）。与 `agent-client.ts` 全仓 `snake_case` 策略一致，且测试未覆盖该命令。
- **判定**  
  **当前一致，非缺陷**。保留为契约备忘：若未来将 `SuggestKpInput` 改 `#[serde(rename_all="camelCase")]`，前端需同步为 `examType/examName`。建议加一条 `suggest` 契约单测锁死 `exam_type` 键名（见 M-09）。

### 🟠 M-05 Agent 18 工具仅 2 个 parity 用例（契约覆盖率缺口）

- **文件**  
  `src/services/agent-tool-parity.test.ts` 仅覆盖 `plan.get_today` / `record.checkin_plan`  
  `src-tauri/src/agent/tools/mod.rs:ToolRegistry::built_in()` 注册 18 个：`exam.get_active, flashcard.{complete,create_batch,get_due}, knowledge_point.create_batch, material.create, plan.{apply_preview,generate,get_range,get_today,preview_generate}, record.{checkin_plan,create_free,get_history}, review.{complete,get_due}, wrong_question.{create,mark_mastered}`
- **影响**  
  其余 16 工具的 `input` 形态错配要到联调/线上才暴露（如 `plan.generate` 需 `exam_id/week_start/daily_capacity_min`，`review.complete` 需 `wrong_question_id/quality` 等）。
- **修复**  
  为每工具补 `tests/fixtures/agent-tools/<name>.json` + 一例 `invoke` 参数断言（至少校验 `jsonschema` 通过且 `output` 含必选字段）。

### 🟡 M-06 `ALLOWED_TABLES` 含 `settings` 但 `COLUMNS['settings']` 主键为 `key` 非 `id`

- **现状**  
  `db.ts:getById(table,id)` 对 `settings` 误用会查 `id=?` 永远空；但全仓仅 `getSetting/hasSetting/setSetting` 专用访问，无调用方误用 `getById('settings',…)`。
- **判定**  
  设计有意：`settings` 例外表由专用函数封装。保留现状，建议在 `getById` 内 `if(table==='settings') throw` 防误用。

---

## 3. 逻辑错误 / 功能 Bug

### 🔴 B-01 `incrementWrongReview` 写入 UTC，触发器与 Rust 侧写入 `localtime`（时区错位）

- **文件**
  - `src/services/record-service.ts:424-427`

    ```ts
    await execute(
      'UPDATE wrong_questions SET review_count = review_count + 1, last_review_at = ? WHERE id = ?',
      [new Date().toISOString(), id],
    )
    ```

  - `src-tauri/src/db.rs` 全库 `DEFAULT (datetime('now','localtime'))`、触发器 `datetime('now','localtime')`
  - `src-tauri/src/agent/tools/review.rs:complete` `last_review_at = datetime('now','localtime')`、`review_interval_days/ease_factor/next_review_at` 按本地日计算
- **问题**  
  前端写 `2026-08-28T02:30:00.000Z`（UTC），Rust 读侧 `WHERE last_review_at < date(?,'-7 days')` 与 `next_review_at <= date('now','localtime')` 按本地日比较，跨东八区误差 8h；`next_review_at`（本地日 `YYYY-MM-DD`）与 `last_review_at`（UTC ISO）混时区。
- **修复**  
  前端改为本地或交由 SQL 生成，二选一：

  ```ts
  // 方案 A：SQL 生成（推荐，无参，绝对一致）
  await execute(
    "UPDATE wrong_questions SET review_count = review_count + 1, last_review_at = datetime('now','localtime') WHERE id = ?",
    [id],
  )
  // 方案 B：JS 本地字符串
  const local = fmtLocal(new Date()) // YYYY-MM-DD HH:MM:SS
  await execute('UPDATE ... last_review_at = ? ...', [local, id])
  ```

### 🟠 B-02 `getPlansByDateRange` 在读路径逐条写回（并发/性能）

- **文件**  
  `src/services/plan-service.ts:18-55`

  ```ts
  for (const row of rows) {
    if (plan.record_count>0) {
      if (plan.actual_duration !== recorded_duration || ...) await update('study_plans', plan.id, {...})
    }
  }
  ```

- **问题**  
  日历/列表/今日任务每次拉取都会对每条 `record_count>0` 的计划执行一次 `UPDATE`。日历切换、列表与日历并发请求会并发写同一行；500 条计划即 500 次串行 `UPDATE`，无事务，`user_modified` 等字段可能被覆盖；离线中断留半更新。
- **修复**  
  将修复下沉为单条条件更新或去写：

  ```ts
  await execute(
    `UPDATE study_plans SET actual_duration = ?, actual_tasks = ?, status = ?
                 WHERE id = ? AND (actual_duration IS NOT ? OR actual_tasks IS NOT ? OR status IS NOT ?)`,
    [actualDuration, actualTasks, status, plan.id, actualDuration, actualTasks, status],
  )
  ```

  或改内存投影（读时不写，后台定时校正）。

### 🟠 B-03 业务日 04:00 边界使当日计划在 00:00-03:59 无法打卡（体验）

- **文件**  
  `src/services/record-service.ts:17-34 (businessToday/normalizeDate)`, `:170 (plan.date > businessToday())`  
  `src/services/plan-service.ts:70 (getTodayPlans → businessToday())`  
  `src-tauri/src/agent/tools/plan.rs:business_date_at` 同为 `<4:00 回退一天`
- **复现**  
  02:30 时 `businessToday() === 昨天`，日历与今日任务展示昨天计划；对真实日历今天（`plan.date === 今天`）执行 `createPlanCheckin` 被判 `未来计划不能提前打卡`。
- **修复**  
  若为有意复用“跨天算昨天”规则，应在打卡校验与空状态文案显式说明（“00:00-04:00 仍属昨日”）；否则将守卫改为日历日：

  ```ts
  if (plan.date > fmtDate(new Date())) throw ...
  // 记录的 date 归一仍由 normalizeDate 负责
  ```

### 🟠 B-04 简报与工位卡待复习口径不一致（展示不一致）

- **文件**
  - 简报 `src-tauri/src/brief.rs:55-66` `due_wrong_questions = SELECT COUNT(*) ... WHERE last_review_at IS NULL OR last_review_at < date(?,'-7 days')`
  - 工位卡/工具 `src-tauri/src/agent/tools/review.rs:46-62` `WHERE mastered=0 AND (next_review_at IS NULL OR next_review_at <= date('now','localtime'))`
  - 指令 `src-tauri/src/agent/commands.rs:review_list_due / flashcard_list_due`
- **影响**  
  同一考试两处数字不一致（简报按 7 天粗筛，卡片按 SM-2 精排）。
- **修复**  
  简报改复用 `review::count_due`（`next_review_at` 口径），仅 `next_review_at IS NULL` 时回退 7 天，并在 UI 注明口径。

### 🟡 B-05 级联删除/重排无事务（半删风险）

- **文件**  
  `src/services/exam-service.ts:deleteExam/deleteSubjectCascade`（6-7 次 `execute`）、`src/services/record-service.ts:deleteRecord`、`src/services/plan-service.ts:reorderPlans/reorderKnowledgePoints`
- **问题**  
  `tauri-plugin-sql` 为连接池，`PRAGMA foreign_keys` 不可靠（`db.ts` 已注释），应用层逐条删；中途失败留半删考试/半排序。
- **修复**  
  包事务：`await execute('BEGIN'); try { ...; await execute('COMMIT')} catch { await execute('ROLLBACK'); throw }`  
  重排改单条 `CASE`：`UPDATE knowledge_points SET sort_order = CASE id WHEN ? THEN 0 ... END WHERE id IN (...)`

### 🟡 B-06 `updateRecord` 对 `plan_id` 绑定记录的字段守卫遗漏 `plan_id` 本身

- **文件**  
  `src/services/record-service.ts:247-258`

  ```ts
  if (existing?.plan_id) {
    delete data.date
    delete data.subject_id
    delete data.knowledge_point_id
  }
  await update('study_records', id, data)
  ```

- **问题**  
  白名单 `COLUMNS['study_records']` 含 `plan_id`，故 `input` 传入 `plan_id` 可改绑计划，守卫未删 `plan_id`。
- **修复**  
  增加 `delete data.plan_id`；`RecordInput` 改 `Omit<..., 'plan_id'>` 防类型层误传。

### 🟡 B-07 `recalculatePlanProgress` 参数组装易错位

- **文件**  
  `src/services/record-service.ts:121-145`

  ```ts
  const exclusion = excludeRecordId ? ' AND id <> ?' : ''
  const params = excludeRecordId
    ? [planId, excludeRecordId, planId, excludeRecordId]
    : [planId, planId]
  ```

  对应 SQL 含子查询与主查询各 2 占位，共 4 参。虽当前正确，但后续改列极易错位且可读性差。

- **修复**  
  拆两步查询或命名参，或改单聚合 `SELECT ... WHERE plan_id=? AND (? IS NULL OR id<>?)`。

### 🟡 B-08 `createPlanCheckin` 覆盖错题归属无提示

- **文件**  
  `src/services/record-service.ts:196-204`

  ```ts
  await createWrongQuestion({
    ...wrong,
    record_id: created.id,
    subject_id: plan.subject_id,
    knowledge_point_id: plan.knowledge_point_id,
  })
  ```

- **问题**  
  强制以计划归属覆写错题 `subject_id/knowledge_point_id`，忽略调用方 `wrong.subject_id`，未在 UI 提示“已按计划归属重定向”。
- **修复**  
  文档化行为并在 UI 提示，或当 `wrong.subject_id` 与计划不一致时抛显式错误让用户确认。

### 🟡 B-09 分页/计数 `cond.replace(/r\./g,'')` 脆弱

- **文件**  
  `src/services/record-service.ts:293,403`
- **问题**  
  `count('study_records', cond.replace(/r\./g,''), params)` 依赖正则剥离别名；若未来条件含字符串字面量 `r.` 会误伤。`viz-service.ts` 在 `cond` 后追加 `AND r.xxx IS NOT NULL` 依赖 `buildCond` 始终返回 `WHERE`。
- **修复**  
  `count` 侧显式构造无别名条件；`viz` 侧将追加条件并入 `buildCond` 的 `where[]` 再统一拼接。

### 🟡 B-10 `analyzer.ts` 趋势口径为“时长”而非“正确率/掌握度”（易误导）

- **文件**  
  `src/services/analyzer.ts:42-61`

  ```ts
  const firstMin = rs.filter(r=>r.date<midDate).reduce((a,r)=>a+r.duration_min,0)
  const lastMin  = rs.filter(r=>r.date>=midDate).reduce((a,r)=>a+r.duration_min,0)
  trend = lastMin > firstMin*1.15 ? '上升' : ...
  ```

- **问题**  
  同一科目前后半段“趋势”仅看时长增减，时长上升但正确率下降仍判“上升”。
- **修复**  
  改双指标：时长 + 正确率加权，或趋势改名为“投入趋势”并在 UI 标注口径。

---

## 4. 编码问题 / 技术债

### C-01 `settings` store 死代码

- **文件**  
  `src/stores/settings.ts:101-126 refreshKeyState`

  ```ts
  try { invoke('has_api_key'); return } catch(e){ if (invoke错) return; throw e }
  try { invoke('has_api_key') } catch ... // ← 永远不可达
  ```

- **修复**  
  删除不可达块，保留首段 `try/catch`（已覆盖 `has_api_key + hasSetting` 全路径）。`vue-tsc` 不报，需手动删；删除后 `typecheck` 仍 0 错。

### C-02 诊断键常驻 `settings` 表

- **文件**  
  `src/services/record-service.ts:69-79` `setSetting('last_record_attempt'/'last_record_error')`
- **影响**  
  每次建记录写两行 `settings`，永不清理；`exportData` 全量导出一并带出。
- **修复**  
  改 `sessionStorage`/内存日志，或写后 `setTimeout` 清理；`exportData` 排除 `last_record_*`。

### C-03 导入 `merge` 无法显式清空 `''`

- **文件**  
  `src/services/export.ts:118-140` `if(merged[k]==='' && v!=='') merged[k]=v` 视 `''` 为缺失
- **影响**  
  `description=''` 等意图清空的 `merge` 无法落地，旧值保留。
- **修复**  
  文档注明 `merge` 不支持显式清空；或约定 `__clear: true` 标记。

### C-04 导出文件名中文与 `:` 兼容

- **现状**  
  `export.ts:225,255` `智研导出_...${date}.json` / `VACUUM INTO '${safe}'` 已 `replace(/'/g,"''")` 转义单引号，`;` 在单引号内安全；`dateStr` 已将 `:` 换 `-`。
- **建议**  
  保留中文名，`examName` 过滤 `[/\\:*?"<>|]`；`safe` 路径追加校验长度 < 260。

### C-05 `any` 使用 38 处

- **文件**  
  `grep : any|as any|<any>` 全仓 38 处，集中在 `export.ts:queryChunked<any>`, `importData(b:any)`, `getById<any>`，`types` 映射处。
- **建议**  
  为 `Material/Flashcard` 等补类型后，`export.ts` 可收敛为 `ExportBundle = { materials: Material[]; flashcards: Flashcard[]; ... }`。

### C-06 仅 1 处 `eslint` 警告

- **文件**  
  `src/pages/StudyPlan.vue:43` `vue/attributes-order: data-test 应在 @click 前` → `npx eslint . --fix` 即解。

### C-07 `viz-service` `subjectId` 别名与 `s.name` 隐式依赖

- **文件**  
  `src/services/viz-service.ts:24-30` `SELECT r.subject_id AS subjectId, s.name AS subjectName`
- **现状**  
  与 `types` 无强类型校验，但 `query<T>` 泛型约束正确。
- **建议**  
  保持现状，补 `viz-service.test.ts` 锁列别名。

### C-08 `apiKey` 输入清空后 lingering `keyConfigured`

- **文件**  
  `src/stores/settings.ts:saveLlmConfig` 仅当 `apiKeyInput` 非空才 `keyConfigured=true`，清空输入不重置；`clearLlmConfig` 需显式点“清除”。
- **建议**  
  文档化行为或在保存空输入且 `has_api_key` 已 false 时同步 `keyConfigured=false`。

---

## 5. 编码/字符集专项

- `file -i src/**/*.ts` 均为 `charset=utf-8`，无 BOM（`head -c 3` 为 `2f 2f 20` 即 `//`），中文注释/字符串均 `utf-8` 正常显示，无 mojibake。
- `zh-cn` 文案集中在 `export.ts` 文件名 `智研导出/备份`、`settings.ts`、`Welcome.vue` 三处，NTFS 实测可写；`VACUUM INTO` 已做单引号转义，无注入。
- 时间统一口径：除 **B-01** 的一处 `toISOString()` 外，全仓 `YYYY-MM-DD` 字符串词法比较与 `datetime('now','localtime')` 一致；`businessToday()` 与 `plan.rs:business_date_at` 均 `<4:00 回退` 一致。

---

## 6. 前后端一致性校验清单（本次已逐项核对）

| 项                                                                                               | 前端                                                                                                      | 后端                                                             | 结论                                                                                                                  |
| ------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| `exams/subjects/knowledge_points/study_records/study_plans/wrong_questions/ai_analyses/settings` | `db.ts:COLUMNS + types/index.ts`                                                                          | `db.rs:SCHEMA_SQL`                                               | **一致**（含触发器/索引/CHECK）                                                                                       |
| `materials/flashcards`                                                                           | `db.ts:ALLOWED/COLUMNS` 完整                                                                              | `db.rs:迁移 v12` 完整                                            | **一致**，但 `export/types` 未打通（M-01/M-02）                                                                       |
| `study_plans.sort_order`                                                                         | `db.ts`/`types` 含                                                                                        | `db.rs:迁移 v2`                                                  | **一致**                                                                                                              |
| `study_records.plan_id`                                                                          | 含                                                                                                        | `迁移 v3`                                                        | **一致**                                                                                                              |
| `wrong_questions.subject_id + SM-2 三列`                                                         | `db.ts` 含，`types` 缺                                                                                    | `迁移 v11` 含                                                    | **M-02**                                                                                                              |
| `knowledge_points.{material_id,source_ref}`                                                      | `db.ts`/`types` 含                                                                                        | `迁移 v12`                                                       | **一致**                                                                                                              |
| Tauri 命令 27 个                                                                                 | `agent-client.ts` 21 + `Welcome.vue` 1 + `export.ts` 1 + `settings.ts` 3（含 `store/has/delete_api_key`） | `lib.rs:generate_handler!` 27                                    | **一一对应**，参数均为 `snake_case`（测试锁死 `agent-client.test.ts:75` `camelCase boundary` 实为 `snake_case` 断言） |
| `suggest_knowledge_points`                                                                       | `Welcome.vue:{ input:{ exam_type, exam_name, subjects }}`                                                 | `suggest.rs:SuggestKpInput` 同名                                 | **一致**                                                                                                              |
| 时间基准                                                                                         | `record-service:businessToday/normalizeDate`                                                              | `plan.rs:business_date_at` + `db.rs:localtime`                   | **一致**（除 B-01）                                                                                                   |
| 级联语义                                                                                         | `exam-service:deleteExam/deleteSubjectCascade` 应用层兜底                                                 | `db.rs: ON DELETE CASCADE/SET NULL` + 连接池 `PRAGMA` 不可靠注释 | **一致**，但无事务（B-05）                                                                                            |

---

## 7. 修复优先级与验证

### P0（随首版修复，约 1 人日）

1. M-01 + M-02 + M-03 + B-01 — 数据链路与时区
2. `npm run typecheck` 0 错 + `eslint --fix` + `cargo test` 迁移单测

### P1（体验与一致性）

1. B-02 + B-03 + B-04 + B-05 — 读时写/边界文案/口径统一/事务
2. M-05 Ag parity 用例补齐至 18 工具

### P2（健壮性）

1. B-06..B-10 + C-01..C-08 — 守卫/参数/趋势口径/`any` 收敛

### 验证脚本

```bash
# 前端
npm run typecheck          # 0 error
npm run lint -- --max-warnings 0
rm -rf node_modules package-lock.json && npm i && npm test  # 全绿（含新增 parity）

# 后端
cargo test -- --nocapture  # 迁移/FK/调度单测全绿

# 手工
# 1) 新建考试→导入材料→闪卡入库→按考试导出 JSON 含 materials/flashcards→新库导入后溯源可回跳
# 2) 空科目考试按考试导出：kps/records/wrong 为 [] 而非全量
# 3) 23:50 与 02:30 各建一条记录/错题复盘，待复习计数与 last_review_at 一致
```

---

_本报告为只读扫描，未改动任何源码；P0 四项可一键拆为子任务并行实施。_
