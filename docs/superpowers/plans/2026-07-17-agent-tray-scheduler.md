# Tray Lifecycle, Background Jobs, and Daily Brief — M4

**Design source:** `docs/superpowers/specs/2026-07-17-rust-agent-os-redesign.md` (§10.3 cost accounting, §12 local fallback, §13 tray & scheduling, §15 daily loop)
**Roadmap:** Milestone 4 of `docs/superpowers/plans/2026-07-17-rust-agent-os-program-roadmap.md`

## Why this slice

M3 shipped the Rust model loop, context audit, and structured memory. M4 makes the
agent **resident**: closing the window hides it instead of quitting, a tray menu
controls the app, background jobs (daily brief, task reminder, overdue check,
weekly report) run on a deduped schedule with catch-up after sleep/wake/restart,
and reminders surface through the OS notification plugin. The local aggregation
piece (overdue detection, day/week stats, rule-based weakness spotting) is the
Fallback Engine core that also powers the daily brief skeleton. Cost accounting
(§10.3) is folded in as a small additive change to the Planner turn.

**Excluded from M4 (deferred to M5/M6):** the Agent OS three-column UI, workbench
host, `agent_artifacts` table, notification preferences UI, and packaged 24-hour
soak run (manual, recorded in `MANUAL_TEST.md`).

## Decisions

- Tray uses Tauri v2 built-in `TrayIconBuilder` + `Menu`; no new crate.
- Notifications use the already-installed `tauri-plugin-notification`.
- `agent_jobs` is a single new table (v8 migration): dedup via `UNIQUE(dedup_key)`,
  status machine `scheduled -> running -> completed | failed (retry_at)`, `paused`
  for reminder suppression.
- Scheduler ticks every 60s from a Tokio task; `tick(now)` takes the clock as an
  argument so tests control time. Startup `bootstrap()` re-schedules meaningful
  missed jobs (daily brief for today, overdue check) — never replays failed
  user-visible writes.
- Job handlers are synchronous functions over the existing pool where possible;
  the brief is the only handler that may call the LLM (through `Planner`), falling
  back to a pure local brief when no provider is configured.
- Reminder pause is a `settings` key `agent_reminders_paused` toggled from the tray.
- Cost accounting: `PlannerTurn` gains `estimated_cost_usd`; per-1k-token rates are
  settings (`agent_cost_per_1k_prompt_tokens`, `agent_cost_per_1k_completion_tokens`)
  with conservative defaults, so pricing stays a product decision.
- "今日任务" tray item opens/focuses the main window; navigation to a task view is
  M5 work (no Agent OS page exists yet).

## File map

```
src-tauri/src/
├── tray.rs              # NEW: tray icon, menu, close-to-hide, exit flag
├── scheduler.rs         # NEW: agent_jobs repo + tick loop + job dispatch
├── analytics.rs         # NEW: overdue / day+week stats / weak areas (pure SQL)
├── brief.rs             # NEW: daily brief (local skeleton + optional LLM)
├── lib.rs               # register tray, manage Scheduler, run tick task, new commands
├── db.rs                # v8 migration: agent_jobs
└── agent/
    ├── planner.rs       # PlannerTurn.estimated_cost_usd + rate lookup
    └── commands.rs      # agent_job_* and agent_brief_preview commands (hidden)
src/ (frontend)
├── types/index.ts       # AgentJob + estimated_cost_usd contract
├── services/agent-client.ts  # listJobs, briefPreview
└── pages/AgentDebug.vue # Jobs + brief preview section
docs/
├── agent/feature-parity.md      # new rows
├── agent/migration-runbook.md   # M4 section + rollback
└── superpowers/plans/… (this file)
```

## Locked behavioral contract

1. Closing the main window hides it; the process stays alive with the tray icon.
2. Tray menu: 打开智研 (show+focus), 暂停提醒 (toggle), 今日任务 (show window),
   彻底退出 (stop new jobs, finish in-flight transaction, `app.exit(0)`).
3. Every job instance carries a `dedup_key`; the same key schedules at most once.
4. `tick(now)` runs jobs whose `scheduled_at <= now` (or `retry_at <= now`) exactly
   once; a running job is never double-started.
5. On restart, `bootstrap()` re-creates jobs for today's brief and overdue check
   if their daily dedup key is absent; nothing user-visible is replayed.
6. Reminder-type jobs (`task_reminder`, `overdue_check`) are skipped while
   `agent_reminders_paused = '1'`.
7. Daily brief: local skeleton (today plans, overdue count, week completion, due
   wrong questions, confirmed memory hints) always produced; LLM adds an
   explanation paragraph only when a provider is configured and healthy.
8. Notifications carry title+body only, never raw plan/record text; duplicates are
   prevented by the job dedup key (not by notification API).
9. `PlannerTurn.estimated_cost_usd = prompt_tokens/1000*rate_p + completion_tokens/1000*rate_c`,
   with rates from settings (defaults: 0.002 / 0.006 USD per 1k — conservative
   public-cloud ballpark; product may adjust).

## Task 1: Tray lifecycle (close-to-hide, menu, exit)

- `tray.rs`: `pub fn build_tray(app: &AppHandle) -> tauri::Result<()>` creating the
  icon from the default app icon, menu items above, and handlers.
- `lib.rs`: intercept `WindowEvent::CloseRequested` on the main window — if the
  global `EXITING` flag is unset, prevent close and hide the window; the tray
  "彻底退出" sets `EXITING` then `app.exit(0)`.
- Pause toggle reads/writes `settings.agent_reminders_paused` and updates the menu
  check state.
- Tests: settings toggle round-trip; `EXITING` flag logic (unit, no window);
  menu construction is manual-tested (`MANUAL_TEST.md` entry).

## Task 2: agent_jobs v8 + Scheduler

- v8 migration `agent_jobs` (schema in Decisions above) + indexes
  `(status, scheduled_at)` and `(job_type, dedup_key)`.
- `scheduler.rs`:
  - `JobType` enum (`daily_brief`, `task_reminder`, `overdue_check`, `weekly_report`,
    `retry_failed`, `cleanup_failed`) with `as_str`/`parse`.
  - `Scheduler { pool, app: Option<AppHandle> }`: `schedule()`, `tick(now)`,
    `bootstrap()`, `pause_state()`; handlers for each JobType (thin: call
    analytics/brief/notification helpers).
  - `tick` dispatch: `INSERT ... WHERE status='scheduled'` claim via atomic
    `UPDATE status='running' WHERE id=? AND status='scheduled'` (rows_affected
    gate), run handler, record `completed`/`failed` + `last_result` +
    `runs+1`/`retry_at`.
- Commands (hidden, for /agent-debug): `agent_job_list`,
  `agent_job_schedule(job_type, dedup_key, scheduled_at)`.
- Tests (in-memory SQLite + injected clock): dedup key uniqueness; tick runs due
  jobs once; atomic claim prevents double-run; failure sets retry_at and retries;
  paused reminder skipped; bootstrap re-creates only today's meaningful jobs.

## Task 3: Local aggregation (Fallback Engine core)

- `analytics.rs` (pure SQL over the pool, no LLM):
  - `overdue_plans(exam_id, today) -> Vec<PlanRow>`: `date < today AND status='pending'`
    using the existing 04:00 business-day rule (`agent::tools::plan::business_date_at`).
  - `day_stats(exam_id, date)`: planned count, completed count, completion rate,
    total duration (from `study_records` joined on `study_plans`).
  - `week_stats(exam_id, monday)`: same shape for a 7-day window.
  - `weak_areas(exam_id, limit)`: wrong-question groups whose correctness is below
    a threshold (default 60%), ordered worst-first.
- Tests: seeded real-SQLite rows for each aggregation; empty-exam edges; business
  date boundary.

## Task 4: Daily brief

- `brief.rs`: `build_local(scope) -> Value` (today plans, overdue, week completion,
  due wrong questions, confirmed memories via `MemoryRepository::relevant`).
  `build_with_llm(...)` calls the planner provider with no tools and merges the
  explanation; any provider error falls back to the local brief, never failing.
- The `daily_brief` job stores `last_result` (JSON) and emits
  `agent-daily-brief` when a window is listening.
- Command `agent_brief_preview` renders today's brief on demand (hidden debug).
- Tests: local brief content with seeded data; LLM branch with `SyntheticProvider`;
  provider failure falls back cleanly.

## Task 5: Reminders and notifications

- `notify.rs` (or in `scheduler.rs`): `send(app, title, body)` wrapping
  `tauri_plugin_notification::NotificationExt`, permission-checked.
- `task_reminder` job: reads `agent_reminder_time` (default `19:00`), schedules
  today's run at that local time; body lists unfinished today tasks by count, never
  their text.
- `overdue_check` job: runs once daily (dedup key = business date), notifies when
  overdue count > 0; body is the count + earliest date only.
- Tests: job scheduling at reminder time; pause suppression; notification body
  contains no plan text (assert on the message builder payload via a thin wrapper).

## Task 6: Cost accounting + debug page + docs

- `planner.rs`: `PlannerTurn.estimated_cost_usd: f64` (0.0 in local mode unless
  tokens accrued); rate lookup from settings with defaults.
- Frontend: `AgentPlannerTurn.estimated_cost_usd`, `AgentJob` type,
  `listAgentJobs`, `agentBriefPreview`; `/agent-debug` Jobs + brief sections.
- Docs: `feature-parity.md` rows (tray/jobs/brief; planner cost); `migration-runbook.md`
  M4 section (v8 additive, rollback = disable tick task + remove commands);
  `PROJECT_STATUS.md` milestone record.
- Full exit gate: Rust all-targets, Vitest, typecheck, build, fmt, clippy,
  `git diff --check`.

## Exit criteria

- Close hides; tray shows all four items; pause toggles persist across restart;
  彻底退出 exits with in-flight transaction completed.
- Same dedup key never double-schedules; tick under injected clock runs each due
  job once; restart bootstrap catches up today's brief/overdue only.
- Local brief and all three aggregations pass seeded-data tests; LLM brief merges
  and degrades on provider failure.
- Notifications never contain raw plan/record/wrong-question text.
- `estimated_cost_usd` present on every model turn; settings rates honored.
- Feature parity, runbook, and project status updated.

## Deferred to M5/M6

- Agent OS three-column UI, workbench, brief card, approval card UI.
- `agent_artifacts` table and artifact persistence.
- Notification/reminder preference UI (tray pause only in M4).
- `weekly_report` full handler (week stats exist; the composed report body is M5
  when a home UI can show it).
- Packaged 24-hour soak + native tray interaction (manual, `MANUAL_TEST.md`).

## Rollback

- Disable the tick task (`setup` no longer spawns `scheduler` loop), remove the
  `agent_job_*`/`agent_brief_preview` commands and tray creation; the window
  close handler reverts to default close.
- v8 `agent_jobs` is additive; leave the table (older binaries ignore it) or drop
  on disposable test databases. No business-table changes exist in M4.
