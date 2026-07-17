# Rust Agent OS Program Roadmap

**Design source:** `docs/superpowers/specs/2026-07-17-rust-agent-os-redesign.md`

**Delivery rule:** Every milestone ships a runnable application, preserves existing user data, and has an independent rollback point. A milestone starts only after the previous milestone passes its exit gate.

## Milestone sequence

| Order | Plan | Deliverable | Exit gate |
|---|---|---|---|
| 1 | `2026-07-17-agent-runtime-foundation.md` | Agent schema, Rust database pool, run state machine, repository, Tauri contract, hidden debug page | Run creation, transitions, restart recovery and existing app regression suite pass |
| 2 | `2026-07-17-agent-tools-policy.md` | Read tools, check-in write tool, policy engine, approval and idempotency | Rust and TypeScript parity tests pass; no unapproved R3 execution |
| 3 | `2026-07-17-agent-model-context-memory.md` | Rust model adapters, streaming tool loop, context audit, structured memory and local fallback | Provider contract tests, privacy tests and offline workflow pass |
| 4 | `2026-07-17-agent-tray-scheduler.md` | Tray lifecycle, background jobs, daily brief, reminders and retry policy | Sleep/wake, restart, duplicate-job and 24-hour soak tests pass |
| 5 | `2026-07-17-agent-os-interface.md` | Three-column Agent OS, daily brief, approval cards, run timeline and workbench host | Desktop E2E daily learning loop passes with accessibility checks |
| 6 | `2026-07-17-agent-parity-production.md` | Remaining tools, full workbench migration, old-path removal, migration rollback and release hardening | 100% feature parity, upgrade matrix and packaged-app acceptance pass |

## Cross-milestone controls

- Use `agent_os_enabled` and per-tool ownership flags until Milestone 6.
- Back up the SQLite database before each schema migration in packaged-app tests.
- Keep TypeScript and Rust writes mutually exclusive for each feature.
- Record baseline and new behavior in `docs/agent/feature-parity.md`.
- Add a rollback note to `docs/agent/migration-runbook.md` in every milestone.
- Commit after every independently passing task.
- Run `npm.cmd test`, `npm.cmd run typecheck`, `npm.cmd run build`, `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`, and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` at every milestone exit.

## Model routing for implementation

- Use a high-capability model for schema design, concurrency, lifecycle, security review and difficult failures.
- Use a balanced coding model for Rust/Vue implementation and most tests.
- Use a low-cost model for mechanical documentation and formatting only when it can run the same verification commands.
- Start a fresh context for each task and load only the design, current plan and touched files.
- Keep code review independent from implementation prompts.
