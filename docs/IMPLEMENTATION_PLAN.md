# 智研 v0.4 改造实施计划

> 状态：**待审核**（审核通过后按序执行，每步完成后运行对应验证）
> 目标版本：`0.4.0` — 缓存命中优化 / 对话内可操作 / 文件材料导入

---

## 0. 背景与根因（均已代码确认）

用户实测三个问题：

| # | 现象 | 根因（代码级） |
|---|---|---|
| 1 | LLM 输入缓存命中率 <10% | (a) 动态快照（今日计划/日期/记录）拼接进 **system prompt 前缀**，每次请求字节全变 → 前缀缓存全 miss，与 Pi issue #1873 同病；(b) 会话历史完全不回放，请求间前缀零重叠；(c) `ProviderUsage` 未解析缓存字段，应用内无法观测 |
| 2 | 只能对话，无法完成任何操作 | (a) 一轮以 `WaitingApproval` 结束后前端**从不刷新审批列表**（`refreshApprovals()` 仅 DailyBrief 挂载时调用）→ 审批卡不出现，写计划流程视觉中断；(b) 历史仅作 12 条数据块塞在快照里，模型无真实多轮上下文 |
| 3 | 材料只能手动输入 | `ImportMaterialDialog.vue` 仅 textarea，无文件解析 |

## 1. 设计原则（抄现成，不自研）

参考对象与抄录点：

- **Claude Code**（官方博客 "prompt caching is everything"）：缓存 key = 请求字节前缀，任一字节变化使后续全失效 → 静态在前、动态在后。
- **Hermes Agent（Nous Research）**：严格分离「可缓存 system prompt 状态」与「调用时临时动态内容」；消息布局 = 静态 system 前缀 → 会话历史逐字增长 → 动态内容尾部；超限保最新 N 条。
- **Pi**：自称 *"a machine for keeping token prefixes stable"*；issue #1873 教训：时间戳/动态数据进 system prompt = 每次全 miss。

抄录结论 → 智研消息布局（OpenAI-compatible 自动前缀缓存，无 cache_control，布局理念等价）：

```
[0]  system   SYSTEM_PROMPT（字节级静态，现有常量不动）
[1..]        会话历史 ≤12 条，逐字回放为真实 user/assistant 消息
[-2] user     本次 goal
[-1] system   动态快照（唯一变化部分，尾部，永不打断前缀）
```

超限裁剪优先级（抄 Hermes 尾部保护）：先丢尾部动态快照 → 再丢最老历史 → 最后裁剪 goal。文件导入部分独立（Tauri 插件组合），与缓存设计无关。

## 2. 改动清单（按执行顺序）

### A. Rust 核心：缓存友好消息布局 — `src-tauri/src/agent/planner.rs`

1. `run_inner()` 消息构建重写：
   - system 消息固定为 `SYSTEM_PROMPT`（不再拼接快照）
   - 新增 `session_history(run_id)`：查 run 所属 session 的 `agent_messages`（JOIN agent_runs），取 user/assistant 角色、按时间正序、limit `HISTORY_LIMIT=12`、每条 `MAX_FIELD_BYTES=800` 截断，回放为真实消息
   - goal 之后追加尾部 system 消息 = `snapshot.to_system_text()`
2. `enforce_message_budget()` 适配新布局：
   - Phase 1 裁剪顺序：① 尾部快照（`len-1`）② 最老历史/工具结果 ③ goal（`len-2`）
2. 缓存停顿暴露（每轮结束等待审批时 run 状态 `waiting_approval`，模型无感知，无需改动 run loop）

### A2. 快照去重 — `src-tauri/src/agent/context_snapshot.rs`

- `to_system_text()` 不再渲染 `session_history` 块（已作为真实消息回放，避免双份 token 且不浪费缓存）
- `truncation` 行文案去掉「跳过 N 条更早消息」的 history 部分（字段保留，避免结构大改）
- 检查 `context_snapshot` 测试对这两处的断言并同步更新

### B. Rust：缓存字段解析与落库

1. `src-tauri/src/agent/llm/mod.rs`（**已落地**）：`ProviderUsage` 增加 `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`（serde rename，缺省 0）
2. `src-tauri/src/agent/llm/openai_compatible.rs`：无改动（字段经统一 ProviderUsage 透传，parse_usage 无需改）
3. `src-tauri/src/agent/planner.rs`（**已落地**）：`PlannerTurn` + `LoopAccumulator` 增加两字段，`into_turn` 透传；`record_messages()` 写入两列
4. `src-tauri/src/db.rs`：迁移 v13
   ```sql
   ALTER TABLE agent_messages ADD COLUMN prompt_cache_hit_tokens INTEGER NOT NULL DEFAULT 0;
   ALTER TABLE agent_messages ADD COLUMN prompt_cache_miss_tokens INTEGER NOT NULL DEFAULT 0;
   ```
5. `src-tauri/src/agent/model.rs`：`AgentMessage` 增加两字段（i64）
6. `src-tauri/src/agent/repository.rs`：`session_messages` SELECT 增加两列

### C. 前端：缓存可见 + 审批卡即时刷新

1. `src/types/index.ts`：`AgentMessage`、`AgentPlannerTurn` 增加缓存字段
2. `src/components/agent/ConversationPane.vue`：assistant 消息 meta 追加 `缓存 {hit}+{miss}`（保留 `tokens X+Y` 原格式，兼容现有测试 `toContain('tokens 40+5')`）
3. `src/stores/agent.ts`：`sendMessage()` 尾部追加 `await refreshApprovals()` → WaitingApproval 轮次后审批卡立即出现

### D. 文件导入（PDF/PPTX/DOCX/TXT）

1. `src-tauri/Cargo.toml`：新增依赖 `zip = "2"`、`pdf-extract = "0.7"`
2. 新建 `src-tauri/src/import.rs`：
   - 命令 `parse_material_file(path: String) -> Result<ParsedMaterial, CommandError>`，`ParsedMaterial { title, kind, content }`
   - `txt/md` → `std::fs::read_to_string`
   - `docx` → zip 读 `word/document.xml`，提取 `<w:t>` 文本（含 `<w:tab/>`→空格），段落用 `</w:p>` 换行
   - `pptx` → zip 读 `ppt/slides/slide*.xml`，提取 `<a:t>` 文本，按 slide 分段
   - `pdf` → `pdf_extract::extract_text_from_mem`
   - 其他扩展名 → `InvalidFormat` 错误（稳定错误码）
3. `src-tauri/src/lib.rs`：注册 `import::parse_material_file`
4. `src/components/plan/ImportMaterialDialog.vue`：
   - 新增「从文件导入」按钮（dialog 区域）：`@tauri-apps/plugin-dialog` open()（filter: pptx/pdf/docx/txt/md）→ `invoke('parse_material_file', { path })` → 自动填充标题（文件名）+ 正文 → 用户核对后走现有 submit 入库与闪卡流程
   - 正文超 50000 字符时截断并提示（复用现有校验）

### E. 测试

| 位置 | 内容 |
|---|---|
| planner.rs 更新 | `provider_receives_the_bounded_context_snapshot_in_the_system_prompt` 改为断言：`request[0]` 为静态 SYSTEM_PROMPT（无快照）、最后一条为快照、中间含历史回放 |
| planner.rs 更新 | `long_term_memories_are_not_offered_in_system_prompt` 改为遍历全部消息断言不含记忆词 |
| planner.rs 新增 | 历史回放：先在 session 写入一条历史 user/assistant 消息，断言请求中回放且 goal 顺位其后 |
| planner.rs 新增 | 缓存字段：SyntheticProvider 脚本含 hit/miss → 断言 `PlannerTurn.prompt_cache_hit_tokens` 累计 |
| import.rs 新增 | 内存构建 docx/pptx zip → 断言文本抽取；txt 直读；不支持扩展名报错 |
| context_snapshot.rs 更新 | 涉及 session_history 渲染的断言 |
| 前端 | types 字段补全后 `vue-tsc` 通过；ConversationPane/AgentHome 测试不回归 |

### F. 验证命令（全部必须通过）

```bash
cd src-tauri && cargo test          # 全量 Rust 测试
cd .. && npm run test               # vitest 前端测试
npm run typecheck                   # vue-tsc --noEmit
npx eslint src src-tauri/src        # lint
cargo build                         # 确认可编译
```

手动验证清单（Tauri dev 运行）：
1. 连续多轮对话 → 消息 meta 显示 `缓存 A+B`，A/B 随轮次增长（命中率自证）
2. 「帮我制定本周计划」→ 审批卡立即出现 → 确认 → 计划页可见 → 可撤销
3. 导入 .docx/.pptx/.pdf → 标题正文自动填充 → 入库 → Agent 拆解概念
4. 不支持的扩展名（.exe）→ 提示错误且不崩溃

## 3. 风险与边界（YAGNI 标注）

- **不做**：Hermes 式双模型对话压缩（ContextCompressor）—— 12 条截断已满足规模；`ponytail:` 标注为将来历史变长时再抄
- **不做**：cache_control 显式断点 —— 本应用 OpenAI-compatible（DeepSeek 自动前缀缓存），无此字段
- **不做**：旧 .doc 二进制格式解析（需 antiword/COM），仅支持 .docx；如确需再加
- **风险**：迁移 v13 对已有数据库执行 ALTER TABLE —— 项目已有 v1..v12 迁移机制，此迁移不可逆但仅加列（默认 0），安全
- **风险**：pdf-extract 依赖链较大（lopdf/flate2），仅影响构建体积与首次编译时长，不影响运行时

## 4. 工作量预估

| 步骤 | 预计改动行数 |
|---|---|
| A 消息布局 + A2 快照去重 | ~120 行（planner.rs / context_snapshot.rs） |
| B 缓存落库 | ~40 行（db.rs / model.rs / repository.rs / record_messages） |
| C 前端 | ~30 行（types / ConversationPane / stores） |
| D 文件导入 | ~200 行（import.rs 新文件 + Dialog 改造） |
| E 测试 | ~150 行 |