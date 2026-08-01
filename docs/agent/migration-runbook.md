# Agent OS Migration Runbook

## Safety rules

1. Never remove an old write path before its parity tests pass.
2. Never let TypeScript and Rust own the same write operation at the same time.
3. Back up `%APPDATA%\com.zhiyan.app\zhiyan.db` before a packaged migration test.
4. Treat migration failure as a release blocker; keep the original database untouched.

## Canonical database and startup order

- Both `tauri-plugin-sql` and the Rust Agent pool use `app_config_dir()/zhiyan.db`.
- On Windows with the current application identifier, the canonical file is `%APPDATA%\com.zhiyan.app\zhiyan.db`.
- `tauri.conf.json` must preload `sqlite:zhiyan.db`. This lets the SQL plugin apply migrations, including migration v4, before Tauri `setup` opens the Rust pool.
- Startup then opens the Rust pool and atomically changes only residual `running` Runs to `interrupted`; `waiting_approval` Runs remain unchanged.

## Milestone 1 verified state

- Migration v4 adds the five additive Agent Runtime tables without changing existing business tables.
- Rust owns Agent sessions, Runs, lifecycle transitions, audit events, startup recovery, and the hidden `/agent-debug` contract.
- Existing exam, plan, check-in, record, analysis, visualization, settings, import, export, backup, and restore paths remain TypeScript-owned.
- Automated milestone verification is recorded by the milestone commit. Native WebView interaction and restart recovery remain checked items in `MANUAL_TEST.md` until performed on the target Windows desktop.

## Milestone 1 Verification Evidence (2026-07-18)

The following results were produced in the milestone worktree on 2026-07-18:

- Vitest: 11 test files and 43 tests passed.
- Rust: 21 unit tests and 12 integration tests passed in the default parallel run; the same 21 unit tests and 12 integration tests passed with `--test-threads=1`.
- `npm.cmd run typecheck`: exit code 0.
- `npm.cmd run build`: exit code 0. The build emitted only existing Rollup annotation/chunk-size warnings; no build error occurred.
- `cargo fmt --manifest-path src-tauri\Cargo.toml -- --check`: exit code 0.
- `cargo clippy --manifest-path src-tauri\Cargo.toml --all-targets --all-features -- -D warnings`: exit code 0.
- `git diff --check`: exit code 0.
- Controlled `npm.cmd run tauri dev`: Vite became ready, Rust compiled, and `zhiyan.exe` started with `Responding=True`. The process tree started for this check was then stopped explicitly.
- Canonical database checked read-only at `%APPDATA%\com.zhiyan.app\zhiyan.db`; SQLite `quick_check` returned `ok`.
- The read-only database inspection found all five Agent tables (`agent_sessions`, `agent_runs`, `agent_steps`, `agent_events`, `agent_approvals`) and successful `_sqlx_migrations` versions 1 through 4.
- Pre-check backup, including existing `-shm` and `-wal` companions, was copied to `%TEMP%\zhiyan-m1-pre-tauri-20260718-133526` without overwriting the canonical database.

The following checks remain manual pending and are not claimed as passed: native WebView clicks, creating a Run through the UI, restart recovery of a running Run, regression of the dashboard/plan/check-in/records/analysis/settings UI, and a real backup-restore replacement sequence.

## Database backup and restore sequence

Use this order so neither SQLite pool retains the main database or WAL file during replacement:

1. Create a consistent backup and verify the selected backup path before restore.
2. Call frontend `closeDb()` to close the cached `tauri-plugin-sql` handle.
3. Invoke `agent_prepare_database_restore`; it checkpoints WAL and closes/removes both the Rust pool and any remaining SQL-plugin pool entry.
4. Replace `%APPDATA%\com.zhiyan.app\zhiyan.db` with the chosen backup.
5. Relaunch the application so migrations run before Runtime setup and both pools are recreated.

Do not reopen either pool between steps 2 and 5.

## Agent tool ownership cutover

Cut over only after parity and packaged checks pass.

```sql
-- Switch record.checkin_plan to Rust-owned (irreversible until rollback).
UPDATE settings SET value='rust-owned'
WHERE key='agent_tool_owner.record.checkin_plan' AND value='typescript';
```

Roll back before reopening the TypeScript writer.

```sql
-- Revert record.checkin_plan to TypeScript ownership.
UPDATE settings SET value='typescript'
WHERE key='agent_tool_owner.record.checkin_plan' AND value='rust-owned';
```

Rules:

- The application must close/relaunch after either ownership change.
- Never leave TypeScript and Rust writers enabled concurrently.
- `plan.get_today` may move `shadow -> rust-owned` independently because it is read-only.
- `record.checkin_plan` stays `typescript` or `shadow` until the packaged manual vertical slice in `MANUAL_TEST.md` is signed off.

## Milestone 2 Verification Evidence (2026-07-18)

The following results were produced in the milestone worktree on 2026-07-18:

- Vitest: 12 test files and 55 tests passed.
- Rust: 51 unit tests, 12 db tests, and 42 integration tests passed in the default parallel run; the same tests passed with `--test-threads=1`.
- `npm.cmd run typecheck`: exit code 0.
- `npm.cmd run build`: exit code 0. The build emitted only existing Rollup annotation/chunk-size warnings; no build error occurred.
- `cargo fmt --manifest-path src-tauri\Cargo.toml -- --check`: exit code 0.
- `cargo clippy --manifest-path src-tauri\Cargo.toml --all-targets --all-features -- -D warnings`: exit code 0.
- `git diff --check`: exit code 0.
- Concurrency: WAL dual-connection race produces one completion and one replay with exactly one business row, one Step, and one event.
- Privacy: event payloads and command errors never expose free-text secrets, SQL text, `%APPDATA%`, or absolute paths.
- Migration upgrade: file databases seeded at v1–v4 each upgrade to v5 exactly once and reopen through both configured pools.
- Restore: `prepare_database_restore` followed by a v4 backup replacement re-upgrades once with ownership defaults.

The following checks remain manual pending and are not claimed as passed: packaged Windows build, native WebView agent-debug interaction, R3 approval UI flow, and backup-restore replacement in the running app.

## Milestone 1 rollback

- Disable access to `/agent-debug`.
- Restore the pre-milestone application binary while keeping the additive Agent tables in production; the old binary ignores them.
- Revert migration version 4 only on disposable test databases. Never drop Agent tables from a user database merely to roll back the application.
- If startup migration or canonical-path validation fails, stop the upgrade, preserve the original database and its `-wal`/`-shm` companions, and restore the timestamped pre-upgrade backup.
- Remove Rust Agent state registration and commands only in the rollback code branch; retain TypeScript ownership of every business write.
- Relaunch the rollback build and verify dashboard, plan check-in, records, analysis, settings, backup, and restore before declaring rollback complete.

## M3 Part 1 (model adapter + tool loop + local fallback)

- Adds **no migration**. Model usage audit lives in the existing `agent_events` table as `model.invoked` events (payload: `local`, `prompt_tokens`, `completion_tokens`, `tools_offered`, `data_permissions`); no prompt text is stored. A dedicated `agent_context_audit` table is deferred to a later M3 part.
- The `Planner` is constructed from the same Rust pool as `AgentRuntime` and managed as `tauri::State`; it reads `llm_provider`/`llm_base_url`/`llm_model`/`llm_temperature` from settings and the API key from the OS keyring via the re-exported `credentials::api_key_for`. When no key is configured, the provider is Ollama, or the provider fails terminally, the Planner returns a deterministic local-mode turn that performs no successful model call and is marked `local` (never claims model output).
- Provider errors (`provider_unavailable`, `provider_request_failed`, `budget_exhausted`, `max_iterations`) are redacted at the command boundary; the message never contains the base URL, API key, request body, or response body.
- Rollback: remove the `agent_run_planner` command registration and the `Planner` state from `lib.rs`. The existing application is unchanged — production Vue flows, tool ownership, and the TypeScript write path are untouched. No database rollback is needed.

## M3 Part 2 (streaming)

- Adds **no migration**. The OpenAI-compatible provider streams `chat/completions` SSE (`OpenAiCompatibleProvider::chat_stream`), the Planner forwards content deltas through an `on_chunk` callback, and `agent_run_planner` emits `agent-planner-chunk` events rendered live on `/agent-debug`.
- Rollback: revert to the non-streaming command registration. No database rollback is needed.

## M3 Part 3 (context inspector + structured memory)

- Migration **v6** adds the additive `agent_context_audit` table (per model call: `call_seq`, `purpose`, `local`, `prompt_tokens`, `completion_tokens`, `tools_offered_json`, `categories_json`, `record_ids_json`, `field_sets_json`, `created_at`). It replaces the Part 1/2 `model.invoked` `agent_events` rows: the Planner records one audit row per provider call and one `local=1` row per fallback turn. The audit stores data categories, record IDs, and field names only — **never raw content** (no plan tasks, record text, or wrong-question text) per the privacy rule.
- Migration **v7** adds the additive `agent_memories` table (`exam_id`, `memory_type`, `content`, `source`, `confidence`, `status`, timestamps, `last_used_at`) with CHECK constraints on the seven spec §11 types, the three sources, and the candidate/confirmed/inactive status. `user_statement` memories auto-confirm; `behavior_inferred` and `model_candidate` memories start as `candidate` and require user confirmation. `MemoryRepository::relevant` offers confirmed memories to a future context builder, exam-scoped first and ordered by last use.
- New commands: `agent_context_audit_list`, `agent_memory_list`, `agent_memory_create`, `agent_memory_confirm`, `agent_memory_update`, `agent_memory_deactivate`, `agent_memory_delete`; `ContextAudit` and `MemoryRepository` are managed as Tauri state. `/agent-debug` renders the Context Inspector (audit rows) and the Memory management section.
- Rollback: remove the v6/v7 commands and the two managed states from `lib.rs`; the tables stay in user databases harmlessly (add-on, ignored by older binaries) or can be dropped on disposable test databases. Disable the `/agent-debug` memory/inspector sections for the rollback UI.
