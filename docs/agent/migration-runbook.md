# Agent OS Migration Runbook

## Safety rules

1. Never remove an old write path before its parity tests pass.
2. Never let TypeScript and Rust own the same write operation at the same time.
3. Back up `%APPDATA%\com.zhiyan.app\zhiyan.db` before a packaged migration test.
4. Treat migration failure as a release blocker; keep the original database untouched.

## Milestone 1 rollback

- Disable access to `/agent-debug`.
- Revert migration version 4 only on disposable test databases. Production rollback keeps the additive Agent tables.
- Remove Rust Agent state registration and commands.
- Existing business tables and TypeScript services remain unchanged.
