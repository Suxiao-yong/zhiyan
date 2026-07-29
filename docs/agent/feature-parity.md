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

Notes:

- `plan.get_today` may be promoted `shadow -> rust-owned` after packaged read parity sign-off (read-only; no write-owner conflict).
- `record.checkin_plan` stays `typescript` until the packaged manual vertical slice in `MANUAL_TEST.md` is signed off.
- The model adapter and tool loop (M3 Part 1) is Rust-owned runtime code reachable only through the hidden `agent_run_planner` command. It does not yet replace the TypeScript planner (`plan-chat-agent.ts`) or the TypeScript LLM adapter; that cutover is M6. Deferred M3 parts: streaming, Ollama tool support, the dedicated `agent_context_audit` table + Context Inspector UI, structured long-term memory, and the full Fallback Engine.

States: `legacy`, `shadow`, `rust-owned`, `retired`.
