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

## Milestone 1 rollback

- Disable access to `/agent-debug`.
- Restore the pre-milestone application binary while keeping the additive Agent tables in production; the old binary ignores them.
- Revert migration version 4 only on disposable test databases. Never drop Agent tables from a user database merely to roll back the application.
- If startup migration or canonical-path validation fails, stop the upgrade, preserve the original database and its `-wal`/`-shm` companions, and restore the timestamped pre-upgrade backup.
- Remove Rust Agent state registration and commands only in the rollback code branch; retain TypeScript ownership of every business write.
- Relaunch the rollback build and verify dashboard, plan check-in, records, analysis, settings, backup, and restore before declaring rollback complete.
