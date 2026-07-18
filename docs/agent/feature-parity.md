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

States: `legacy`, `shadow`, `rust-owned`, `retired`.
