# Agent OS Interface (three-column shell) — M5

**Design source:** `docs/superpowers/specs/2026-07-17-rust-agent-os-redesign.md` (§7.1 messages, §9.2 approval card, §14 Vue Agent OS, §15 daily loop, §16 error recovery)
**Roadmap:** Milestone 5 of `docs/superpowers/plans/2026-07-17-rust-agent-os-program-roadmap.md`

## Why this slice

M3/M4 shipped the Rust runtime, memory, scheduler, brief, and notifications, all
reachable today only through the hidden `/agent-debug` page. M5 builds the
product-facing **Agent OS**: a three-column shell (`/agent`) — session
sidebar, conversation + brief + approvals center, and a business-workbench
right pane — wired to the existing commands, plus the missing message layer
(spec §7.1 `agent_messages`) and the three read commands the UI needs
(session list, session messages, approval list). The daily brief becomes a
first-class card that folds into an artifact after handling, and the first
workbench (plan check-in) is embedded by reusing the existing check-in UI.

**Excluded from M5 (deferred to M6):** full TypeScript planner/LLM adapter
cutover, `agent_artifacts` persistence table, workbench set beyond plan
check-in, notification preferences UI.

## Decisions

- `agent_messages` is a new additive table (v9 migration): session-scoped,
  role (user/assistant/system), text, structured JSON content, token usage,
  model, and a nullable run reference. The planner appends a user message
  (goal) and an assistant message (final text) per turn; local fallbacks are
  recorded with zero tokens.
- Read commands are additive and read-only: `agent_session_list(limit)`,
  `agent_session_messages(session_id)`, `agent_approval_list(limit)`.
- The UI runs a single Tauri command per user action — no new backend event
  channels in this slice. Planner streaming stays on `/agent-debug`.
- `DailyBrief.vue` renders `agent_brief_preview`; the brief pushes an
  `agent-daily-brief` event when a window is listening. The push is emitted
  **from the command layer** (the command receives `app: tauri::AppHandle`
  via Tauri injection and emits there) — never from a managed state holding
  an AppHandle (see M4 commit note about the broken test exe).
- The workbench right pane hosts exactly one workbench in M5: plan check-in
  (`PlanCheckinBoard`), reused from the existing components. The original
  routes stay live; the sidebar deep-links to them (spec §14.3).
- Existing debug page stays untouched as the engineering surface.

## File map

```
src-tauri/src/
├── db.rs                  # v9 migration: agent_messages
├── agent/
│   ├── model.rs           # AgentMessage DTO (+ tests)
│   ├── repository.rs      # session list / messages / approval list queries
│   ├── commands.rs        # agent_session_list / _messages / agent_approval_list
│   ├── planner.rs         # persist user+assistant messages per turn
│   └── brief.rs           # (unchanged; command emits push event)
src/
├── router/index.ts        # /agent route
├── pages/AgentHome.vue    # three-column shell
├── components/agent/
│   ├── AgentSidebar.vue   # new session, recent sessions, workbench links
│   ├── ConversationPane.vue  # message stream + input + planner turn
│   ├── DailyBrief.vue     # brief card, folds to artifact after handling
│   ├── ApprovalCard.vue   # pending approvals + decide controls
│   ├── AgentStatus.vue    # run status pill + cancel/retry
│   └── WorkbenchHost.vue  # right pane; hosts PlanCheckinBoard
├── stores/agent.ts        # sessions/messages/runs/brief/approvals state
├── services/agent-client.ts  # sessionList, sessionMessages, approvalList
└── pages/AgentHome.test.ts
docs/
├── agent/feature-parity.md      # rows + notes
├── agent/migration-runbook.md   # M5 section
└── superpowers/plans/… (this file)
```

## Locked behavioral contract

1. Creating a session via the sidebar starts a run-less session; sending a
   message runs one `agent_run_planner` turn and appends `user` + `assistant`
   messages to `agent_messages` (local turns record zero tokens).
2. `agent_session_list` returns sessions newest-first; selecting one loads its
   messages; the stream is a simple append on each turn (no resend).
3. The daily brief card appears on first open of `/agent` for the active exam;
   after the user acknowledges it, the card collapses into the conversation as
   a collapsible artifact (in-memory; `agent_artifacts` is M6).
4. Pending approvals for the active exam render as cards with 批准/拒绝 and
   expiry; deciding calls `agent_decide_approval`; decided cards disappear.
5. The workbench pane hosts the plan check-in workbench; route links in the
   sidebar deep-link to the existing pages (study plan, dashboard, etc.).
6. No managed state holds an owned `AppHandle`; the brief push event is
   emitted from the `agent_brief_preview` command via its injected `app`
   parameter.

## Task 1: Message layer (v9) + read commands

- v9 migration `agent_messages`: `id`, `session_id` (FK, cascade), `run_id`
  (nullable), `role` CHECK (`user|assistant|system`), `text`, `content_json`,
  `prompt_tokens`, `completion_tokens`, `model`, `created_at`; index
  `(session_id, created_at)`.
- `model.rs`: `AgentMessage` DTO.
- `repository.rs`: `session_list(limit)`, `session_messages(session_id)`,
  `approval_list(limit)` (pending first, then recent).
- Commands: `agent_session_list`, `agent_session_messages`,
  `agent_approval_list`.
- `planner.rs`: after each turn (model or local), insert the user goal and the
  assistant final text into `agent_messages` (single transaction with the run
  record).
- Tests: migration v9 (additive, CHECKs, cascade); repository queries with
  seeded sessions/messages/approvals; planner persists messages for model and
  local turns; read commands compile.

## Task 2: Agent store + client + route

- `stores/agent.ts`: sessions, activeSessionId, messages, run, brief, approvals,
  loading/error; actions `refreshSessions`, `createSession`, `selectSession`,
  `sendMessage` (planner turn + message append), `loadBrief`, `refreshApprovals`,
  `decideApproval`, `cancelRun`.
- `agent-client.ts`: `agentSessionList`, `agentSessionMessages`,
  `agentApprovalList` (+ existing).
- Router: `/agent` → `AgentHome.vue` (hidden from the main nav; reached from
  sidebar "Agent" item in AppLayout, plus direct URL).
- Tests: store actions against mocked client (create/select/send/brief/
  approval flows).

## Task 3: Three-column shell

- `AgentHome.vue` + `AgentSidebar.vue`: left column with 新会话, recent
  sessions (click to load), workbench links (Dashboard / 学习计划 / 打卡 /
  Agent 调试), and the Agent 任务 entry.
- `ConversationPane.vue`: message stream (user/assistant bubbles, tokens),
  input box, send → `sendMessage`, run status pill, cancel.
- `WorkbenchHost.vue`: right pane; empty state + PlanCheckinBoard (Task 6).
- Tests: sidebar renders sessions and switches; sending appends messages;
  workbench pane mounts the check-in workbench.

## Task 4: Daily brief card

- `DailyBrief.vue`: on mount loads the brief for the active exam; renders
  summary + numbers + optional explanation; an acknowledge action collapses it
  into a collapsible artifact strip in the conversation; listens for
  `agent-daily-brief` events to refresh.
- Command layer push: `agent_brief_preview` gains an injected `app` parameter
  and emits `agent-daily-brief` with the payload (frontend listens).
- Tests: brief renders and folds; event listener refreshes; no AppHandle is
  held by managed state (regression: test exe still links).

## Task 5: Approval card + run status

- `ApprovalCard.vue`: pending approvals (tool, preview summary, risk, expiry,
  option summary) with 批准/拒绝; empty state.
- `AgentStatus.vue`: run status pill (idle/running/waiting_approval/completed),
  cancel button.
- Tests: approvals render and decide; status pill reflects the run.

## Task 6: Plan check-in workbench

- `WorkbenchHost.vue` embeds `PlanCheckinBoard` (reused from
  `src/components/record/`), wired to the existing `checkin_plan` tool path
  (shadow/typescript ownership from the tool list).
- Sidebar deep links to existing routes remain.
- Tests: workbench mounts with the tool descriptor; check-in flow still works
  through the existing tool client.

## Task 7: Brief push + weekly report handler

- `agent_brief_preview` emits the brief event (Task 4). Scheduler's
  `weekly_report` job handler: builds a text report from `week_stats` + weak
  areas and stores it in `last_result` (no notification).
- Tests: brief preview emits; weekly_report job produces a report payload.

## Task 8: Verification + docs

- Rust all-targets, Vitest, typecheck, build, fmt, clippy, `git diff --check`.
- feature-parity rows (agent os shell, messages); runbook M5 section (v9
  additive, rollback); PROJECT_STATUS milestone record; MANUAL_TEST packaged
  checklist (shell, brief card, approval card, workbench).

## Exit criteria

- `/agent` renders the three-column shell; sessions create/select and messages
  persist and reload across restarts.
- Sending a message runs one planner turn and appends user/assistant messages
  (local turns included, zero tokens).
- The daily brief card shows on first open, folds after acknowledge, and
  refreshes from the command-layer push event.
- Pending approvals render and decide; run status reflects the run.
- Plan check-in workbench works inside the right pane; original routes still
  work.
- No managed state holds an AppHandle; the test exe links and runs.

## Deferred to M6

- `agent_artifacts` persistence; full workbench set (plan generate, records,
  analysis) and workbench registry; notification preferences UI; TypeScript
  planner/LLM cutover.

## Rollback

- Remove the `/agent` route and components; remove `agent_session_list` /
  `agent_session_messages` / `agent_approval_list` commands and the planner
  message writes. v9 `agent_messages` is additive; leave it or drop on
  disposable test databases.
