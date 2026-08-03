# Agent OS Feature Parity

| Capability | Current owner | Target owner | Migration state | Regression command |
|---|---|---|---|---|
| Exam and subject configuration | TypeScript services | Rust tools | legacy | `npm.cmd test` |
| Plan generation and editing | TypeScript services | Rust tools | legacy | `npm.cmd test -- src/services/plan-generator.test.ts src/services/plan-service.test.ts` |
| Plan check-in and free record | TypeScript services | Rust tools | legacy | `npm.cmd test -- src/services/record-service.test.ts` |
| Wrong questions | TypeScript services | Rust tools | legacy | `npm.cmd test` |
| Analysis and prediction | TypeScript services | Rust tools | legacy | `npm.cmd test -- src/services/analyzer.test.ts` |
| Visualization datasets | TypeScript services | Rust tools | legacy | `npm.cmd test` |
| Import, export, backup and restore | TypeScript services plus Tauri plugins | Rust tools | legacy | `npm.cmd test -- src/services/export.test.ts` |
| Agent session and run state | Rust Runtime | Rust Runtime | rust-owned | `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1` |
| `plan.get_today@1` (R0 read) | Rust tool | Rust tool | shadow | `cargo test --manifest-path src-tauri/Cargo.toml --test agent_tools plan_get_today -- --test-threads=1` |
| `record.checkin_plan@1` (R1 write + undo) | Rust tool | Rust tool | typescript | `cargo test --manifest-path src-tauri/Cargo.toml --test agent_tools checkin -- --test-threads=1` |
| Model adapter and tool loop (non-streaming) | Rust Runtime | Rust Runtime | rust-owned | `cargo test --manifest-path src-tauri/Cargo.toml --lib agent::planner -- --test-threads=1` |
| Context audit + inspector (M3 Part 3) | Rust Context Builder | Rust Context Builder | rust-owned | `cargo test --manifest-path src-tauri/Cargo.toml --lib agent::context -- --test-threads=1` |
| Structured long-term memory (M3 Part 3) | Rust MemoryRepository | Rust MemoryRepository | rust-owned | `cargo test --manifest-path src-tauri/Cargo.toml --lib agent::memory -- --test-threads=1` |
| Tray lifecycle + background jobs + daily brief (M4) | Rust Scheduler/Tray | Rust Scheduler/Tray | rust-owned | `cargo test --manifest-path src-tauri/Cargo.toml --lib scheduler -- --test-threads=1` |
| Agent OS shell + conversation (M5) | Rust messages + Vue shell | Rust message layer, Vue UI | rust-owned | `npx vitest run src/pages/AgentHome.test.ts` |

Notes:

- `plan.get_today` may be promoted `shadow -> rust-owned` after packaged read parity sign-off (read-only; no write-owner conflict).
- `record.checkin_plan` stays `typescript` until the packaged manual vertical slice in `MANUAL_TEST.md` is signed off.
- The model adapter and tool loop (M3 Part 1) is Rust-owned runtime code reachable only through the hidden `agent_run_planner` command. Streaming is shipped (M3 Part 2): the provider streams `chat/completions` SSE, the Planner forwards content deltas through an `on_chunk` callback, and `agent_run_planner` emits `agent-planner-chunk` events rendered live on `/agent-debug`. It does not yet replace the TypeScript planner (`plan-chat-agent.ts`) or the TypeScript LLM adapter; that cutover is M6. M3 Part 3 ships the Context Inspector (dedicated `agent_context_audit` table + `agent_context_audit_list` command + `/agent-debug` view) and structured long-term memory (`agent_memories`, seven spec §11 types, candidate→confirmed flow, management UI). M4 ships the tray lifecycle (close-to-hide, pause toggle, quit), the background scheduler (`agent_jobs` v8, deduped daily brief/overdue/reminder jobs with restart/day-rollover self-heal), local aggregation, the daily brief (local skeleton + optional LLM explanation), task/overdue notifications (counts and dates only), and cost accounting (`PlannerTurn.estimated_cost_usd`). Still deferred: the full Fallback Engine (overdue/stats/weakness/notifications) as a composed product surface and the weekly report (M5), and push-style brief events (M5 command layer). **Ollama tool support is excluded by product decision (cloud LLMs only for the agent loop)**; Ollama stays on the TypeScript plain-chat path and degrades to local mode in the Rust planner. M5 ships the Agent OS three-column shell at `/agent`: session sidebar (new/recent sessions, workbench deep links), conversation center (persisted `agent_messages` per planner turn, composer, run status, approval cards, daily brief card with command-layer push), and the plan check-in workbench embedded in the right pane. The TypeScript planner/LLM adapter and the full workbench set remain M6.

States: `legacy`, `shadow`, `rust-owned`, `retired`.
