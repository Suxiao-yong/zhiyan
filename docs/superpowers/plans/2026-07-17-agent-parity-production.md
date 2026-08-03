# Parity, Full Migration, and Production Hardening — M6

**Design source:** `docs/superpowers/specs/2026-07-17-rust-agent-os-redesign.md` (§8.2 tool set, §17 phases 7–8, §18–19 acceptance)
**Roadmap:** Milestone 6 of `2026-07-17-rust-agent-os-program-roadmap.md` (`2026-07-17-agent-parity-production.md`)

## Why this slice

M1–M5 shipped the runtime, memory, scheduler, brief, notifications, and the
Agent OS shell, but the tool set is still thin (`plan.get_today`,
`record.checkin_plan`) and the frontend still writes the database directly and
calls the LLM directly. M6 completes the §8.2 tool set (query tools, free
records, wrong questions, plan generation draft), migrates the workbench set
into the shell, removes the direct write/LLM paths, merges the planning and
analysis agents onto the Rust planner, adds the `agent_os_enabled` fallback,
and hardens for release (migration rollback matrix, security review, packaged
acceptance). Each task commits independently and keeps `main` runnable.

**Excluded from M6 (post-release backlog):** `data.import`/`data.export`/
`data.backup`/`data.restore` as model-callable tools (the UI backup/restore
flow stays as-is; agent-facing import is deferred), `exam.update` /
`plan.reorder` / `record.update` / `record.delete` as model-callable tools
(manual UI keeps ownership; the agent may request them in M6+), and the
`visualization.get_dataset` tool.

## Decisions

- New tools follow the existing pattern: pure `async fn(tx, input, …)` in
  `tools/`, descriptors registered in `tools/mod.rs`, risk-gated dispatch in
  the executor. Query tools are R0 (no approval), free/wrong-question writes
  are R1 (automatic within a run, idempotency `retry_safe`), plan generation
  is R2 (draft + approval).
- `plan.generate` produces a **local rule-based draft** in Rust (week
  distribution + subject weight + daily capacity from settings/memory), never
  calls the LLM itself; the planner conversation interprets/explains the draft.
- Workbench migration reuses existing page components behind a small registry
  in `WorkbenchHost` (check-in, plan, record, wrong-question, analysis,
  visualization); each workbench remains reachable via its original route.
- Old-path removal is **feature-scoped and reversible**: for each frontend
  direct DB write that has a Rust tool, the store action is switched to the
  tool (keeping the tool ownership flag visible on the debug page); the TS
  LLM analysis path (`agent-engine.runPendingAnalyses`) is disabled behind the
  same `agent_os_enabled` flag the UI uses.
- `agent_os_enabled` setting (default `1`) controls the Agent OS home route
  and the TS analysis path; flipping it to `0` restores the old dashboard
  first screen and legacy analysis — the emergency fallback switch.
- Database migrations stay additive through v9; the production hardening task
  adds an upgrade-matrix test (v1–v9 in-memory sequences) and documents
  packaged-app rollback.

## File map

```
src-tauri/src/agent/tools/
├── mod.rs            # register new descriptors (exam.get_active, plan.get_range,
│                     #   record.get_history, record.create_free,
│                     #   wrong_question.create/mark_mastered, plan.generate)
├── exam.rs           # NEW: get_active
├── plan.rs           # + get_range, generate (draft)
├── record.rs         # + get_history, create_free
└── wrong_question.rs # NEW: create, mark_mastered
src-tauri/src/agent/executor.rs   # dispatch wiring + R1/R2 gates
src/ (frontend)
├── stores/agent.ts               # workbench selection state
├── components/agent/WorkbenchHost.vue  # workbench registry
├── pages/AgentHome.vue           # workbench switcher
├── router/index.ts               # AgentHome default when agent_os_enabled
└── services/agent-engine.ts      # analysis path behind the flag
docs/agent/feature-parity.md, migration-runbook.md, PROJECT_STATUS.md, MANUAL_TEST.md
```

## Locked behavioral contract

1. Query tools are R0 and never write; writes go through the existing
   transaction/executor path with idempotency keys.
2. `record.create_free` inserts a `study_records` row (+ optional wrong
   questions) exactly once per idempotency key; `wrong_question.create` /
   `mark_mastered` likewise.
3. `plan.generate` returns a draft (planned rows, not yet persisted); applying
   the draft is an R2 approval-gated write of the generated rows.
4. The WorkbenchHost registry hosts: plan check-in, study plan, study record,
   wrong question, analysis, visualization. Switching workbenches never loses
   the conversation.
5. With `agent_os_enabled = 1` (default), `/` redirects to `/agent`; with `0`,
   `/` redirects to `/dashboard` and the TS analysis path is skipped.
6. No managed state holds an owned `AppHandle`; no new event push is added in
   M6 beyond the existing command-layer brief push.
7. Every task keeps Rust all-targets, Vitest, typecheck, build, fmt, clippy,
   and `git diff --check` green.

## Task 1: Query tools (R0)

- `exam.get_active` (active exam + subjects), `plan.get_range` (date range),
  `record.get_history` (recent records with subject names). Descriptors +
  executor wiring + tests (schema validation, empty results, date bounds).
- Commit.

## Task 2: Write tools (R1)

- `record.create_free` (duration, content, optional wrong questions),
  `wrong_question.create`, `wrong_question.mark_mastered`. All idempotent,
  transaction-scoped, R1 automatic. Tests: idempotency key replay creates one
  row; conflict on bad plan/record references.
- Commit.

## Task 3: Plan generation draft (R2)

- `plan.generate` local rule-based draft in Rust (mirror the TS generator's
  week shape): inputs exam_id + week start (+ optional capacity from settings
  `agent_daily_capacity_min`); output draft rows with subject weights; apply
  step inserts them. R2 approval gate. Tests: draft shape, apply idempotency.
- Commit.

## Task 4: Workbench registry

- `WorkbenchHost` hosts the six workbenches (reusing existing page components);
  `AgentHome` gains a workbench switcher in the right pane header. Tests:
  switching mounts the right workbench, conversation survives.
- Commit.

## Task 5: Old-path removal (feature-scoped)

- Switch frontend direct DB writes that have Rust tools to the tool path
  (free record + wrong-question creation at minimum; check-in already does).
- Disable the TS LLM analysis path (`runPendingAnalyses`) behind
  `agent_os_enabled`; planner conversations already run on the Rust planner.
  Tests: store actions call the tool client; analysis path skipped when flag
  off.
- Commit.

## Task 6: Fallback switch + default route

- `agent_os_enabled` setting; router default `/` → `/agent` when on, →
  `/dashboard` when off. Tests for both redirects.
- Commit.

## Task 7: Production hardening

- Upgrade-matrix test (v1→v9 and v9→v1 rollback sequences in-memory), security
  review of the new tools (approval gates, idempotency, input validation),
  packaged-app checklist in MANUAL_TEST (Windows install/upgrade/db rollback,
  24h soak, no duplicate notifications).
- Commit.

## Task 8: Verification + docs

- Full gates; feature-parity rows for the new tools; runbook M6 section
  (additive, rollback); PROJECT_STATUS milestone record; MANUAL_TEST
  packaged checklist.
- Commit.

## Exit criteria

- The §8.2 tool subset in scope works through the executor with idempotency
  and correct risk gates; parity tests cover query/write/plan-generate.
- The six workbenches mount inside the shell; original routes still work.
- Direct frontend DB writes for free records and wrong questions go through
  tools; TS LLM analysis is flag-controlled.
- `agent_os_enabled` toggles the home route and the analysis path.
- Upgrade matrix passes; security review has no unresolved findings; packaged
  checklist is recorded.
- All automated gates green.

## Deferred

- `data.*` agent tools, `exam.update`, `plan.reorder`, `record.update/delete`,
  `visualization.get_dataset` (manual UI ownership; may become model-callable
  later).
- Full TypeScript planner file removal (`plan-chat-agent.ts` stays as the
  fallback chat UI until a later release).

## Rollback

- Tools: remove the descriptors/executor wiring; the frontend store falls back
  to the previous direct path (per-feature ownership flag still works).
- `agent_os_enabled = 0` restores dashboard-first and legacy analysis.
- Workbench registry: revert `WorkbenchHost` to the single check-in host.
- All M6 schema work is additive (no new migration unless one task needs it —
  none is planned).
