# Agent Tools and Policy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the first policy-enforced Rust Agent tool vertical slice: read today's plan with `plan.get_today`, then perform an exactly-once, undoable plan check-in with `record.checkin_plan`, while the existing Vue workflows remain the production owner until an explicit ownership gate is switched.

**Architecture:** A typed Rust registry publishes immutable tool descriptors and JSON Schemas. `AgentRuntime` is the only model-facing execution boundary: it validates the call, consults the policy engine, and routes approved work through an executor that commits business data and Agent Step/Event receipts in the same SQLite transaction. Migration v5 extends `agent_steps` for policy and undo receipts and seeds per-tool ownership flags; no second receipt table is introduced.

**Tech Stack:** Rust 2021, Tauri 2, SQLx SQLite/WAL, Serde/serde_json, jsonschema, Tokio, Vue 3, TypeScript, Vitest.

**Design source:** `docs/superpowers/specs/2026-07-17-rust-agent-os-redesign.md`

**Milestone dependency:** Milestone 1 commit `2075c1f` or a descendant containing migration v4, `AgentRepository`, `AgentRuntime`, typed Tauri commands, and `/agent-debug`.

---

## File map

### Rust protocol, policy, and execution

- Create `src-tauri/src/agent/tools/mod.rs`: tool protocol types, descriptor registry, schema validation, and stable lookup.
- Create `src-tauri/src/agent/tools/plan.rs`: `plan.get_today@1` descriptor, DTOs, and read query.
- Create `src-tauri/src/agent/tools/record.rs`: `record.checkin_plan@1` descriptor, validated input/output, transactional check-in, idempotent replay, and compensation.
- Create `src-tauri/src/agent/policy.rs`: R0-R4 policy decisions and approval validation.
- Create `src-tauri/src/agent/executor.rs`: the only tool execution path; ownership, policy, Step/Event, approval, timeout, idempotency, and undo orchestration.
- Modify `src-tauri/src/agent/mod.rs`: export the new modules.
- Modify `src-tauri/src/agent/model.rs`: add tool call, result, approval, and ownership DTOs without changing existing Run DTO fields.
- Modify `src-tauri/src/agent/repository.rs`: expose transaction-scoped Agent audit helpers as `pub(crate)`; do not expose raw SQL to model-facing code.
- Modify `src-tauri/src/agent/runtime.rs`: own `AgentExecutor` and expose `execute_tool`, `decide_approval`, and `undo_tool`.
- Modify `src-tauri/src/agent/commands.rs`: typed Tauri command boundary and redacted errors.
- Modify `src-tauri/src/db.rs`: migration v5 and migration tests.
- Modify `src-tauri/src/lib.rs`: construct the executor from the canonical Rust pool and register commands.
- Create `src-tauri/tests/agent_tools.rs`: real SQLite parity, policy, audit, idempotency, crash-window, approval, and undo integration tests.

### Parity fixtures and frontend contract

- Create `src/services/agent-tool-parity.test.ts`: TypeScript characterization tests that emit/verify the shared fixture contract before Rust behavior is written.
- Create `tests/fixtures/agent-tools/plan-get-today.json`: plan query input and canonical expected rows.
- Create `tests/fixtures/agent-tools/record-checkin-plan.json`: existing plan/record fixtures, accepted inputs, validation errors, and expected aggregates.
- Modify `src/types/index.ts`: TypeScript DTOs matching Rust `snake_case` serialized fields.
- Modify `src/services/agent-client.ts`: typed tool list/execute/approval/undo invokes.
- Modify `src/services/agent-client.test.ts`: exact command-name and camelCase Tauri argument contract.
- Modify `src/pages/AgentDebug.vue`: hidden tool descriptor/read/receipt controls only; no production navigation change.
- Modify `src/pages/AgentDebug.test.ts`: component contract for the hidden slice.

### Operations and milestone evidence

- Modify `docs/agent/feature-parity.md`: mark each tool `shadow` or `rust-owned` only after its gate passes.
- Modify `docs/agent/migration-runbook.md`: migration v5 backup, ownership switch, crash recovery, and rollback sequence.
- Modify `MANUAL_TEST.md`: packaged-path R0/R1, duplicate invocation, undo, approval, and old-flow regression checks.

## Locked behavioral contract

- Business day is local date, with `00:00:00` through `03:59:59` attributed to the previous day. `plan.get_today` uses that business day. A plan check-in always copies `study_plans.date`, `subject_id`, and `knowledge_point_id`; the caller cannot supply them.
- `plan.get_today` returns plans ordered by `date`, `sort_order`, then `created_at`. Its projected `record_count`, `actual_duration`, `actual_tasks`, and `status` match `getPlansByDateRange`: a pending plan with records is presented as `in_progress`; a completed or skipped plan is not downgraded. The latest content keeps the TypeScript service's exact `ORDER BY created_at DESC` tie behavior; Rust must not add a secondary ID sort.
- `record.checkin_plan` rejects missing, skipped, and future plans; rejects `duration_min <= 0`, negative question counts, `correct_count > questions_count`, ratings outside `1..=5`, and invalid session values.
- A successful check-in writes one `study_records` row, optional `wrong_questions`, plan aggregates, one completed `agent_steps` receipt, and events in one SQL transaction. Rust intentionally replaces the legacy partial-success warning path with atomic success/failure.
- `actual_duration` is the sum of every record linked to the plan. `actual_tasks` is the newest non-empty record content by `created_at`, falling back to `planned_tasks`. `finish=true` sets `completed`; otherwise an existing `completed` plan remains completed and any other non-skipped plan becomes `in_progress`.
- R1 requires a non-empty idempotency key. Repeating the key returns the stored output and cannot create a second record, wrong question, aggregate increment, Step, or completion event.
- Undo is a compensating transaction. It deletes only the record and wrong questions created by that tool receipt, recalculates plan aggregates from remaining facts, marks the Step undone, and emits one undo event. Repeated undo returns the first undo result without changing data.
- The model never receives a repository or SQL handle. Every call enters `AgentRuntime::execute_tool`, and `AgentExecutor` is the only component allowed to dispatch a registered tool.
- Initial ownership is `shadow` for `plan.get_today` and `typescript` for `record.checkin_plan`. Shadow reads may be compared in hidden diagnostics. A Rust write returns `ownership_not_rust` until the flag is explicitly `rust-owned`; the Vue production check-in continues calling `createPlanCheckin` during this milestone.

## Task 1: Freeze TypeScript parity with shared fixtures

**Files:**

- Create: `tests/fixtures/agent-tools/plan-get-today.json`
- Create: `tests/fixtures/agent-tools/record-checkin-plan.json`
- Create: `src/services/agent-tool-parity.test.ts`

- [ ] **Step 1: Add the plan fixture**

Create `tests/fixtures/agent-tools/plan-get-today.json` with this exact shape:

```json
{
  "now_local": "2026-07-18T02:30:00+08:00",
  "business_date": "2026-07-17",
  "input": { "exam_id": "exam-1" },
  "expected_output": {
    "business_date": "2026-07-17",
    "plans": [
    {
      "id": "plan-1",
      "exam_id": "exam-1",
      "subject_id": "subject-math",
      "knowledge_point_id": "kp-function",
      "date": "2026-07-17",
      "planned_tasks": "复习函数",
      "planned_duration": 60,
      "actual_duration": 30,
      "actual_tasks": "完成第一节",
      "status": "in_progress",
      "generated_by": "local",
      "ai_suggestion": null,
      "user_modified": 0,
      "sort_order": 0,
      "created_at": "2026-07-16 09:00:00",
      "updated_at": "2026-07-17 21:00:00",
      "subject_name": "数学",
      "knowledge_point_name": "函数",
      "record_count": 1
    }
    ]
  }
}
```

- [ ] **Step 2: Add the check-in fixture**

Create `tests/fixtures/agent-tools/record-checkin-plan.json` with the exact values below:

```json
{
  "business_date": "2026-07-17",
  "plan": {
    "id": "plan-1",
    "exam_id": "exam-1",
    "subject_id": "subject-math",
    "knowledge_point_id": "kp-function",
    "date": "2026-07-17",
    "planned_tasks": "复习函数",
    "planned_duration": 60,
    "status": "pending"
  },
  "existing_records": [
    { "id": "record-old", "duration_min": 20, "content": "热身" }
  ],
  "input": {
    "plan_id": "plan-1",
    "duration_min": 30,
    "content": "完成第一节",
    "questions_count": 10,
    "correct_count": 8,
    "mastery_rating": 4,
    "difficulty_notes": "复合函数仍需练习",
    "mood": 5,
    "session_time": "evening",
    "finish": false,
    "wrong_questions": [
      {
        "question_source": "练习册 1-3",
        "question_desc": "定义域判断",
        "correct_answer": "[-1, 1]",
        "my_answer": "(-1, 1)",
        "error_type": "概念不清",
        "error_reason": "忽略端点"
      }
    ]
  },
  "expected": {
    "date": "2026-07-17",
    "subject_id": "subject-math",
    "knowledge_point_id": "kp-function",
    "actual_duration": 50,
    "actual_tasks": "完成第一节",
    "status": "in_progress",
    "questions_count": 10,
    "correct_count": 8,
    "mastery_rating": 4
  }
}
```

- [ ] **Step 3: Write characterization tests before Rust implementation**

Create `src/services/agent-tool-parity.test.ts`. Import both JSON fixtures, mock `./db` using the established service-test pattern, and include these assertions:

```typescript
import { beforeEach, describe, expect, it, vi } from 'vitest'
import planFixture from '../../tests/fixtures/agent-tools/plan-get-today.json'
import checkinFixture from '../../tests/fixtures/agent-tools/record-checkin-plan.json'

vi.mock('./db', () => ({
  count: vi.fn(), execute: vi.fn(), getAll: vi.fn(async () => []), getById: vi.fn(),
  insert: vi.fn(async () => 'record-new'), query: vi.fn(), remove: vi.fn(),
  setSetting: vi.fn(), update: vi.fn(),
}))

import * as db from './db'
import { getPlansByDateRange } from './plan-service'
import { createPlanCheckin } from './record-service'

describe('Agent tool TypeScript parity contract', () => {
  beforeEach(() => vi.clearAllMocks())

  it('projects today plan fields and repairs the same derived values', async () => {
    const expected = planFixture.expected_output.plans[0]
    vi.mocked(db.query).mockResolvedValueOnce([{ ...expected, recorded_duration: 30,
      latest_record_content: '完成第一节', actual_duration: 0, actual_tasks: null,
      status: 'pending' }] as never)
    const rows = await getPlansByDateRange(planFixture.input.exam_id,
      planFixture.business_date, planFixture.business_date)
    expect(rows).toEqual(planFixture.expected_output.plans)
    expect(db.update).toHaveBeenCalledWith('study_plans', 'plan-1', {
      actual_duration: 30, actual_tasks: '完成第一节', status: 'in_progress',
    })
  })

  it('copies locked plan fields and computes accepted check-in values', async () => {
    vi.mocked(db.getById).mockImplementation(async (table) => table === 'study_plans'
      ? ({ ...checkinFixture.plan, actual_duration: 20, actual_tasks: '热身',
          generated_by: 'local', ai_suggestion: null, user_modified: 0, sort_order: 0,
          created_at: '', updated_at: '' } as never)
      : ({ id: 'record-new', plan_id: 'plan-1', ...checkinFixture.expected,
          content: checkinFixture.input.content, difficulty_notes: checkinFixture.input.difficulty_notes,
          mood: 5, session_time: 'evening', created_at: '', updated_at: '' } as never))
    vi.mocked(db.query).mockResolvedValueOnce([{ total: 50, count: 2,
      latest_content: '完成第一节' }] as never)
    const legacyWrong = {
      ...checkinFixture.input.wrong_questions[0],
      subject_id: checkinFixture.plan.subject_id,
      knowledge_point_id: checkinFixture.plan.knowledge_point_id,
    }
    await createPlanCheckin('plan-1', checkinFixture.input, false, [legacyWrong])
    expect(db.insert).toHaveBeenCalledWith('study_records', expect.objectContaining({
      plan_id: 'plan-1', date: '2026-07-17', subject_id: 'subject-math',
      knowledge_point_id: 'kp-function', duration_min: 30, questions_count: 10,
      correct_count: 8, mastery_rating: 4,
    }))
    expect(db.update).toHaveBeenCalledWith('study_plans', 'plan-1', {
      actual_duration: 50, actual_tasks: '完成第一节', status: 'in_progress',
    })
    expect(db.insert).toHaveBeenCalledWith('wrong_questions', expect.objectContaining({
      record_id: 'record-new', subject_id: 'subject-math', knowledge_point_id: 'kp-function',
      question_desc: '定义域判断', question_source: '练习册 1-3',
      correct_answer: '[-1, 1]', my_answer: '(-1, 1)', error_type: '概念不清',
      error_reason: '忽略端点',
    }))
  })
})
```

- [ ] **Step 4: Run and freeze the TypeScript contract**

Run:

```powershell
npm.cmd test -- src/services/agent-tool-parity.test.ts src/services/record-service.test.ts src/services/plan-service.test.ts
```

Expected: all three files pass. If a fixture differs from current service output, change the fixture only after identifying the exact current rule; do not change production services in this task.

- [ ] **Step 5: Commit the parity baseline**

```powershell
git add tests/fixtures/agent-tools src/services/agent-tool-parity.test.ts
git commit -m "test: freeze first agent tool parity contract"
```

## Task 2: Add migration v5 tool receipts and ownership flags

**Files:**

- Modify: `src-tauri/src/db.rs`

- [ ] **Step 1: Write failing migration tests**

Add tests that apply `SCHEMA_SQL`, migrations 2-5 in order to an in-memory SQLite database and assert:

```rust
let columns: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('agent_steps')")
    .fetch_all(&pool).await.unwrap();
for name in ["policy_json", "receipt_json", "undo_json", "undone_at"] {
    assert!(columns.iter().any(|column| column == name));
}
let owners: Vec<(String, String)> = sqlx::query_as(
    "SELECT key, value FROM settings WHERE key LIKE 'agent_tool_owner.%' ORDER BY key"
).fetch_all(&pool).await.unwrap();
assert_eq!(owners, vec![
    ("agent_tool_owner.plan.get_today".into(), "shadow".into()),
    ("agent_tool_owner.record.checkin_plan".into(), "typescript".into()),
]);
assert_eq!(migrations().last().unwrap().version, 5);
```

Also apply migration v5 twice to separate databases: one upgraded from v4 and one initialized from all migrations. Both must preserve a seeded `study_plans` row and the v4 Agent rows.

- [ ] **Step 2: Run the focused test and observe RED**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml db::tests -- --nocapture
```

Expected: failure because migration 5 and the four columns do not exist.

- [ ] **Step 3: Add migration v5**

Append this migration after v4 in `migrations()`:

```rust
Migration {
    version: 5,
    description: "add agent tool policy receipts and ownership flags",
    sql: r#"
        ALTER TABLE agent_steps ADD COLUMN policy_json TEXT;
        ALTER TABLE agent_steps ADD COLUMN receipt_json TEXT;
        ALTER TABLE agent_steps ADD COLUMN undo_json TEXT;
        ALTER TABLE agent_steps ADD COLUMN undone_at TEXT;
        CREATE INDEX IF NOT EXISTS idx_agent_steps_tool_status
            ON agent_steps(tool_name, status);
        INSERT OR IGNORE INTO settings (key, value, description) VALUES
          ('agent_tool_owner.plan.get_today', 'shadow',
           'typescript|shadow|rust-owned; controls plan.get_today delivery'),
          ('agent_tool_owner.record.checkin_plan', 'typescript',
           'typescript|shadow|rust-owned; controls record.checkin_plan writes');
    "#,
    kind: MigrationKind::Up,
},
```

Do not add a receipt table. `agent_steps.idempotency_key`, `input_json`, `output_json`, and the four new fields are the receipt; `agent_events` is its append-only audit trail.

- [ ] **Step 4: Run schema verification**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml db::tests -- --nocapture
cargo fmt --manifest-path src-tauri\Cargo.toml -- --check
```

Expected: migration tests pass and format check exits `0`.

- [ ] **Step 5: Commit migration v5**

```powershell
git add src-tauri/src/db.rs
git commit -m "feat: add agent tool receipt migration"
```

## Task 3: Implement the stable tool protocol and registry

**Files:**

- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/agent/mod.rs`
- Create: `src-tauri/src/agent/tools/mod.rs`
- Create: `src-tauri/src/agent/tools/plan.rs`
- Create: `src-tauri/src/agent/tools/record.rs`

- [ ] **Step 1: Add protocol tests first**

Test registry uniqueness, immutable versions, metadata completeness, input validation, and output validation:

```rust
#[test]
fn registry_has_unique_stable_names_and_complete_metadata() {
    let registry = ToolRegistry::built_in();
    assert_eq!(registry.names(), vec!["plan.get_today", "record.checkin_plan"]);
    for descriptor in registry.descriptors() {
        assert_eq!(descriptor.version, "1");
        assert!(descriptor.timeout_ms > 0);
        assert!(!descriptor.data_permissions.is_empty());
        assert!(descriptor.input_schema.is_object());
        assert!(descriptor.output_schema.is_object());
    }
}

#[test]
fn checkin_schema_rejects_unlocked_and_invalid_fields() {
    let registry = ToolRegistry::built_in();
    let error = registry.validate_input("record.checkin_plan", "1", &serde_json::json!({
        "plan_id": "plan-1", "date": "2026-07-18", "subject_id": "subject-math",
        "duration_min": 0, "finish": false
    })).unwrap_err();
    assert_eq!(error.code(), "tool_schema_invalid");

    for forbidden in ["subject_id", "knowledge_point_id"] {
        let mut input = serde_json::json!({
            "plan_id": "plan-1", "duration_min": 30, "finish": false
        });
        input[forbidden] = serde_json::json!("caller-supplied");
        let error = registry.validate_input("record.checkin_plan", "1", &input).unwrap_err();
        assert_eq!(error.code(), "tool_schema_invalid", "{forbidden} must stay plan-locked");

        let mut nested = serde_json::json!({
            "plan_id": "plan-1", "duration_min": 30, "finish": false,
            "wrong_questions": [{ "question_desc": "定义域判断" }]
        });
        nested["wrong_questions"][0][forbidden] = serde_json::json!("caller-supplied");
        let nested_error = registry.validate_input("record.checkin_plan", "1", &nested).unwrap_err();
        assert_eq!(nested_error.code(), "tool_schema_invalid", "nested {forbidden} must be rejected");
        assert!(serde_json::from_value::<RecordCheckinPlanInput>(nested).is_err(),
            "deny_unknown_fields must prevent Serde from silently ignoring {forbidden}");
    }
}
```

- [ ] **Step 2: Run protocol tests and observe RED**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml agent::tools::tests -- --nocapture
```

Expected: compile failure because `agent::tools` is absent.

- [ ] **Step 3: Add the JSON Schema validator dependency**

Add one direct dependency:

```toml
jsonschema = { version = "0.33", default-features = false }
```

Run `cargo check --manifest-path src-tauri\Cargo.toml` immediately. The resolver records the compatible `0.33.x` patch in `Cargo.lock`; do not replace it with a git dependency.

- [ ] **Step 4: Define the complete protocol**

Create `tools/mod.rs` with these public types and invariant-preserving constructors:

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use crate::agent::error::AgentError;

pub mod plan;
pub mod record;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskLevel {
    #[serde(rename = "R0")] R0,
    #[serde(rename = "R1")] R1,
    #[serde(rename = "R2")] R2,
    #[serde(rename = "R3")] R3,
    #[serde(rename = "R4")] R4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confirmation { Automatic, SummaryOrSetting, Required, NavigationOnly }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Idempotency { RetrySafe, RequiredExactlyOnce, NoAutomaticRetry }

#[derive(Debug, Clone, Serialize)]
pub struct ToolDescriptor {
    pub name: &'static str,
    pub version: &'static str,
    pub input_schema: Value,
    pub output_schema: Value,
    pub risk: RiskLevel,
    pub confirmation: Confirmation,
    pub supports_undo: bool,
    pub timeout_ms: u64,
    pub idempotency: Idempotency,
    pub data_permissions: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolOwnership { Typescript, Shadow, RustOwned, Unavailable }

#[derive(Debug, Clone, Serialize)]
pub struct ListedTool {
    pub descriptor: ToolDescriptor,
    pub ownership: ToolOwnership,
}

#[derive(Clone)]
pub struct ToolRegistry { descriptors: BTreeMap<&'static str, ToolDescriptor> }

impl ToolRegistry {
    pub fn built_in() -> Self {
        let descriptors = [plan::descriptor(), record::descriptor()]
            .into_iter().map(|d| (d.name, d)).collect();
        Self { descriptors }
    }
    pub fn descriptors(&self) -> Vec<&ToolDescriptor> { self.descriptors.values().collect() }
    pub fn names(&self) -> Vec<&'static str> { self.descriptors.keys().copied().collect() }
    pub fn get(&self, name: &str, version: &str) -> Result<&ToolDescriptor, AgentError> {
        let descriptor = self.descriptors.get(name)
            .ok_or_else(|| AgentError::ToolNotFound(name.to_owned()))?;
        if descriptor.version != version {
            return Err(AgentError::ToolVersionMismatch {
                name: name.to_owned(), expected: descriptor.version.to_owned(), got: version.to_owned()
            });
        }
        Ok(descriptor)
    }
    pub fn validate_input(&self, name: &str, version: &str, value: &Value)
        -> Result<(), AgentError> {
        let descriptor = self.get(name, version)?;
        jsonschema::validator_for(&descriptor.input_schema)
            .map_err(|e| AgentError::ToolSchema(e.to_string()))?
            .validate(value).map_err(|e| AgentError::ToolSchema(e.to_string()))
    }
    pub fn validate_output(&self, descriptor: &ToolDescriptor, value: &Value)
        -> Result<(), AgentError> {
        jsonschema::validator_for(&descriptor.output_schema)
            .map_err(|e| AgentError::ToolSchema(e.to_string()))?
            .validate(value).map_err(|e| AgentError::ToolSchema(e.to_string()))
    }
}
```

Add the referenced stable `AgentError` variants in `agent/error.rs`, each with a safe static code: `tool_not_found`, `tool_version_mismatch`, `tool_schema_invalid`, `ownership_not_rust`, `approval_required`, `approval_invalid`, `tool_timeout`, `idempotency_required`, `idempotency_conflict`, and `ownership_unavailable`. The idempotency race variant and command mapping are fixed:

```rust
#[error("idempotency key is already being resolved; retry")]
IdempotencyConflict,
```

Add this exact arm to the existing `AgentError::code` match without changing its other arms:

```rust
Self::IdempotencyConflict => "idempotency_conflict",
```

At the Tauri boundary, map `IdempotencyConflict` to the safe message `idempotency key is already being resolved; retry`; do not include SQLite constraint text, SQL, paths, or input values. The conversion test must assert both the exact code and the redacted retry message.

- [ ] **Step 5: Define exact descriptor schemas**

`plan::descriptor()` is R0, automatic, retry-safe, no undo, 2-second timeout, permissions `study_plans:read`, `subjects:read`, `knowledge_points:read`, `study_records:aggregate`. Its input requires only `exam_id`; its output requires `business_date` and `plans` with every `StudyPlan` plus `subject_name`, `knowledge_point_name`, and `record_count` field listed in the shared fixture.

`record::descriptor()` is R1, automatic, exactly-once, undoable, 5-second timeout, permissions `study_plans:read_write`, `study_records:write`, `wrong_questions:write`, `agent_audit:write`. Its schema sets `additionalProperties: false`, requires `plan_id`, `duration_min`, and `finish`, and constrains:

```json
{
  "type": "object",
  "additionalProperties": false,
  "required": ["plan_id", "duration_min", "finish"],
  "properties": {
    "plan_id": { "type": "string", "minLength": 1 },
    "duration_min": { "type": "integer", "minimum": 1 },
    "content": { "type": ["string", "null"] },
    "questions_count": { "type": "integer", "minimum": 0, "default": 0 },
    "correct_count": { "type": "integer", "minimum": 0, "default": 0 },
    "mastery_rating": { "type": ["integer", "null"], "minimum": 1, "maximum": 5 },
    "difficulty_notes": { "type": ["string", "null"] },
    "mood": { "type": ["integer", "null"], "minimum": 1, "maximum": 5 },
    "session_time": { "type": ["string", "null"], "enum": ["morning", "afternoon", "evening", null] },
    "finish": { "type": "boolean" },
    "wrong_questions": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "question_source": { "type": ["string", "null"] },
          "question_desc": { "type": ["string", "null"] },
          "correct_answer": { "type": ["string", "null"] },
          "my_answer": { "type": ["string", "null"] },
          "error_type": { "type": ["string", "null"] },
          "error_reason": { "type": ["string", "null"] }
        }
      },
      "default": []
    }
  }
}
```

The Rust DTOs must use the database field names:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct PlanGetTodayInput { pub exam_id: String }

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckinWrongQuestionInput {
    pub question_source: Option<String>,
    pub question_desc: Option<String>,
    pub correct_answer: Option<String>,
    pub my_answer: Option<String>,
    pub error_type: Option<String>,
    pub error_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordCheckinPlanInput {
    pub plan_id: String,
    pub duration_min: i64,
    pub content: Option<String>,
    #[serde(default)] pub questions_count: i64,
    #[serde(default)] pub correct_count: i64,
    pub mastery_rating: Option<i64>,
    pub difficulty_notes: Option<String>,
    pub mood: Option<i64>,
    pub session_time: Option<String>,
    #[serde(default)] pub finish: bool,
    #[serde(default)] pub wrong_questions: Vec<CheckinWrongQuestionInput>,
}
```

- [ ] **Step 6: Run protocol verification and commit**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml agent::tools::tests -- --nocapture
cargo clippy --manifest-path src-tauri\Cargo.toml --all-targets --all-features -- -D warnings
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/agent
git commit -m "feat: define agent tool protocol registry"
```

Expected: tests and Clippy exit `0`.

## Task 4: Implement the R0-R4 policy engine

**Files:**

- Create: `src-tauri/src/agent/policy.rs`
- Modify: `src-tauri/src/agent/mod.rs`

- [ ] **Step 1: Write the decision-table tests**

```rust
#[test]
fn policy_matches_supervised_risk_table() {
    assert_eq!(decide(ctx(RiskLevel::R0, false, None)).unwrap(), PolicyDecision::Execute);
    assert_eq!(decide(ctx(RiskLevel::R1, false, None)).unwrap(), PolicyDecision::ExecuteWithUndo);
    assert_eq!(decide(ctx(RiskLevel::R2, false, None)).unwrap(), PolicyDecision::PresentSummary);
    assert_eq!(decide(ctx(RiskLevel::R2, true, None)).unwrap(), PolicyDecision::Execute);
    assert_eq!(decide(ctx(RiskLevel::R3, false, None)).unwrap(), PolicyDecision::AwaitApproval);
    assert_eq!(decide(ctx(RiskLevel::R4, true, None)).unwrap(), PolicyDecision::NavigateOnly);
}

#[test]
fn no_unapproved_r3_can_execute() {
    for grant in [None, Some(expired_grant()), Some(wrong_step_grant()),
                  Some(wrong_precondition_grant()), Some(rejected_grant())] {
        assert_ne!(decide(ctx(RiskLevel::R3, true, grant)).unwrap(), PolicyDecision::Execute);
    }
    assert_eq!(decide(ctx(RiskLevel::R3, false, Some(valid_grant()))).unwrap(),
               PolicyDecision::Execute);
}
```

- [ ] **Step 2: Run policy tests and observe RED**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml agent::policy::tests -- --nocapture
```

Expected: compile failure because `policy.rs` is absent.

- [ ] **Step 3: Implement the complete decision API**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision { Execute, ExecuteWithUndo, PresentSummary, AwaitApproval, NavigateOnly }

pub struct ApprovalGrant<'a> {
    pub approval_id: &'a str,
    pub step_id: &'a str,
    pub expected_step_id: &'a str,
    pub status: &'a str,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub now: chrono::DateTime<chrono::Utc>,
    pub precondition_hash: &'a str,
    pub current_precondition_hash: &'a str,
}

pub struct PolicyContext<'a> {
    pub risk: RiskLevel,
    pub user_allows_r2: bool,
    pub approval: Option<ApprovalGrant<'a>>,
}

pub fn decide(context: PolicyContext<'_>) -> Result<PolicyDecision, AgentError> {
    use PolicyDecision::*;
    use RiskLevel::*;
    Ok(match context.risk {
        R0 => Execute,
        R1 => ExecuteWithUndo,
        R2 if context.user_allows_r2 => Execute,
        R2 => PresentSummary,
        R3 => match context.approval {
            Some(grant) if grant.status == "approved"
                && grant.step_id == grant.expected_step_id
                && grant.expires_at > grant.now
                && grant.precondition_hash == grant.current_precondition_hash => Execute,
            Some(_) => return Err(AgentError::ApprovalInvalid),
            None => AwaitApproval,
        },
        R4 => NavigateOnly,
    })
}
```

The executor, not the model, supplies `risk`, current time, Step ID, and current precondition hash. Ignore any same-named fields in model input because schemas disallow them.

- [ ] **Step 4: Verify and commit policy**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml agent::policy::tests -- --nocapture
git add src-tauri/src/agent/policy.rs src-tauri/src/agent/mod.rs src-tauri/src/agent/error.rs
git commit -m "feat: enforce supervised agent tool policy"
```

Expected: policy tests pass, including `no_unapproved_r3_can_execute`.

## Task 5: Implement `plan.get_today` with Rust/TypeScript parity

**Files:**

- Modify: `src-tauri/src/agent/tools/plan.rs`
- Modify: `src-tauri/tests/agent_tools.rs`

- [ ] **Step 1: Write the real-SQL parity test**

Seed the plan fixture into an in-memory database created from migrations 1-5. Insert the old 20/30-minute records with deterministic `created_at`. Assert the serialized Rust output equals the shared fixture's `business_date` and `plans`, including nulls and numeric fields. Also test `business_date_at` at `03:59:59` and `04:00:00` local time.

```rust
assert_eq!(business_date_at(local_datetime(2026, 7, 18, 3, 59, 59)), "2026-07-17");
assert_eq!(business_date_at(local_datetime(2026, 7, 18, 4, 0, 0)), "2026-07-18");
let output = plan::get_today(&pool, PlanGetTodayInput { exam_id: "exam-1".into() },
                             "2026-07-17").await.unwrap();
assert_eq!(serde_json::to_value(output).unwrap(), fixture["expected_output"]);
```

The fixture already nests `business_date` and `plans` under `expected_output`, so Rust and TypeScript consume the same node.

- [ ] **Step 2: Run the focused test and observe RED**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --test agent_tools plan_get_today -- --nocapture
```

Expected: compile failure because `plan::get_today` and DTO rows are absent.

- [ ] **Step 3: Implement the business clock**

Use local fixed-offset time supplied by Runtime for deterministic tests:

```rust
pub fn business_date_at(now: chrono::DateTime<chrono::FixedOffset>) -> String {
    let date = if now.hour() < 4 { now.date_naive() - chrono::Days::new(1) }
               else { now.date_naive() };
    date.format("%Y-%m-%d").to_string()
}
```

Runtime obtains `chrono::Local::now().fixed_offset()`; tool input has no clock field.

- [ ] **Step 4: Implement the exact read query**

Use `sqlx::query_as` with a dedicated `PlanWithNames` row whose fields match `src/types/index.ts::StudyPlan` plus `subject_name`, `knowledge_point_name`, and `record_count`:

```sql
SELECT p.id, p.exam_id, p.subject_id, p.knowledge_point_id, p.date,
       p.planned_tasks, p.planned_duration,
       CASE WHEN COUNT(r.id) > 0 THEN COALESCE(SUM(r.duration_min), 0)
            ELSE p.actual_duration END AS actual_duration,
       CASE WHEN COUNT(r.id) > 0 THEN COALESCE(
          (SELECT r2.content FROM study_records r2
           WHERE r2.plan_id = p.id AND r2.content IS NOT NULL AND r2.content <> ''
           ORDER BY r2.created_at DESC LIMIT 1), p.planned_tasks)
            ELSE p.actual_tasks END AS actual_tasks,
       CASE WHEN COUNT(r.id) > 0 AND p.status = 'pending' THEN 'in_progress'
            ELSE p.status END AS status,
       p.generated_by, p.ai_suggestion, p.user_modified, p.sort_order,
       p.created_at, p.updated_at, s.name AS subject_name,
       k.name AS knowledge_point_name, COUNT(r.id) AS record_count
FROM study_plans p
LEFT JOIN subjects s ON s.id = p.subject_id
LEFT JOIN knowledge_points k ON k.id = p.knowledge_point_id
LEFT JOIN study_records r ON r.plan_id = p.id
WHERE p.exam_id = ? AND p.date = ?
GROUP BY p.id
ORDER BY p.date, p.sort_order, p.created_at
```

This R0 function must not update `study_plans`; it computes the same presented values without introducing a read-side write.

- [ ] **Step 5: Run Rust and TypeScript parity gates**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --test agent_tools plan_get_today -- --nocapture
npm.cmd test -- src/services/agent-tool-parity.test.ts src/services/plan-service.test.ts
```

Expected: both implementations produce the shared fixture values.

- [ ] **Step 6: Commit the read tool**

```powershell
git add src-tauri/src/agent/tools/plan.rs src-tauri/tests/agent_tools.rs tests/fixtures/agent-tools/plan-get-today.json src/services/agent-tool-parity.test.ts
git commit -m "feat: add today plan Rust tool"
```

## Task 6: Implement atomic exactly-once `record.checkin_plan` and undo

**Files:**

- Modify: `src-tauri/src/agent/tools/record.rs`
- Create: `src-tauri/src/agent/executor.rs`
- Modify: `src-tauri/src/agent/mod.rs`
- Modify: `src-tauri/tests/agent_tools.rs`

- [ ] **Step 1: Write failing success and validation tests**

Load the shared fixture and assert the stored record copies the plan's locked fields, all input learning metrics, one wrong question, and aggregate values. Add table-driven rejects for a missing/skipped/future plan, zero duration, negative questions, correct count above total, out-of-range mastery/mood, and invalid session time. Every reject must assert zero new records and no plan change.

```rust
let result = record::checkin_plan(&mut tx, input, "2026-07-17", "record-new").await.unwrap();
assert_eq!(result.record_id, "record-new");
assert_eq!(scalar_i64(&mut tx, "SELECT actual_duration FROM study_plans WHERE id='plan-1'").await, 50);
assert_eq!(scalar_string(&mut tx, "SELECT status FROM study_plans WHERE id='plan-1'").await, "in_progress");
assert_eq!(scalar_i64(&mut tx, "SELECT COUNT(*) FROM wrong_questions WHERE record_id='record-new'").await, 1);
```

- [ ] **Step 2: Write failing idempotency, crash, and undo tests**

Through `AgentExecutor`, call the same R1 input twice with `idempotency_key="checkin/device-a/42"`. Assert both outputs are identical and counts for record, wrong question, Step, and `tool.completed` event remain one. Install a SQLite trigger that rejects `tool.completed`; assert the business row, plan aggregate, and Step all roll back. Then undo twice and assert one undo event and restored aggregate from the pre-existing record.

```rust
assert_eq!(first.output, second.output);
assert_eq!(count("study_records", "id = ?", &first.record_id).await, 1);
assert_eq!(count("agent_steps", "idempotency_key = ?", key).await, 1);
assert_eq!(count_events("tool.completed", first.step_id()).await, 1);

sqlx::raw_sql("CREATE TRIGGER reject_tool_complete BEFORE INSERT ON agent_events
  WHEN NEW.event_type='tool.completed' BEGIN SELECT RAISE(ABORT,'crash window'); END;")
  .execute(&pool).await.unwrap();
assert!(executor.execute(call_with_key("checkin/device-a/43")).await.is_err());
assert_eq!(count("study_records", "id = 'record-crash'", ()).await, 0);
```

- [ ] **Step 3: Run the focused tests and observe RED**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --test agent_tools checkin -- --nocapture
```

Expected: failure because the check-in transaction is not implemented.

- [ ] **Step 4: Implement validation and transaction SQL**

Inside the transaction:

1. Read the plan by ID and validate status/date.
2. Generate UUIDs for the record and wrong questions in Rust before insertion.
3. Insert the record using plan-owned date/subject/knowledge point.
4. Insert each wrong question using the same locked subject/knowledge point and new record ID.
5. Recalculate the plan with this exact update:

```sql
UPDATE study_plans
SET actual_duration = COALESCE((
      SELECT SUM(duration_min) FROM study_records WHERE plan_id = study_plans.id
    ), 0),
    actual_tasks = COALESCE((
      SELECT content FROM study_records
      WHERE plan_id = study_plans.id AND content IS NOT NULL AND content <> ''
      ORDER BY created_at DESC, id DESC LIMIT 1
    ), planned_tasks),
    status = CASE
      WHEN status = 'skipped' THEN status
      WHEN ? = 1 OR status = 'completed' THEN 'completed'
      ELSE 'in_progress'
    END
WHERE id = ?
```

Return:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecordCheckinPlanOutput {
    pub record_id: String,
    pub plan_id: String,
    pub date: String,
    pub subject_id: String,
    pub knowledge_point_id: Option<String>,
    pub actual_duration: i64,
    pub actual_tasks: Option<String>,
    pub status: String,
    pub wrong_question_ids: Vec<String>,
}
```

The executor writes this compensation payload to `undo_json` before commit:

```json
{"kind":"record.checkin_plan.v1","record_id":"record-new","plan_id":"plan-1","wrong_question_ids":["wrong-new"]}
```

- [ ] **Step 5: Implement idempotent reservation and atomic receipt**

For R1 reject an empty key before opening a transaction. Within one transaction, first query a completed Step by key. If found, verify `tool_name`, `tool_version`, and canonical input JSON match; return its stored output. If the key belongs to another input, return `idempotency_conflict`. Otherwise insert the Step and perform the business operation, then update it and append the event before committing:

```sql
UPDATE agent_steps
SET status='completed', output_json=?, policy_json=?, receipt_json=?, undo_json=?, completed_at=datetime('now','localtime')
WHERE id=? AND status='running';

INSERT INTO agent_events(run_id, step_id, event_type, payload_json)
VALUES(?, ?, 'tool.completed', ?);
```

Canonical input JSON must recursively sort object keys before hashing/comparison. Do not include volatile time or generated IDs in the input fingerprint.

- [ ] **Step 6: Implement idempotent compensation**

The undo transaction verifies the Step is completed, `supports_undo=true`, and `undo_json.kind` equals `record.checkin_plan.v1`; deletes only listed wrong-question IDs and the listed record ID; recalculates plan aggregates; sets `undone_at`; and appends `tool.undone`. If `undone_at` is already set, return the stored undo result without another event.

When no records remain, the aggregate update is:

```sql
UPDATE study_plans
SET actual_duration=0,
    actual_tasks=planned_tasks,
    status=CASE WHEN status='skipped' THEN 'skipped' ELSE 'pending' END
WHERE id=?;
```

- [ ] **Step 7: Verify parity and transactional safety**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --test agent_tools checkin -- --nocapture
cargo test --manifest-path src-tauri\Cargo.toml --test agent_tools idempotency -- --nocapture
cargo test --manifest-path src-tauri\Cargo.toml --test agent_tools undo -- --nocapture
npm.cmd test -- src/services/agent-tool-parity.test.ts src/services/record-service.test.ts
```

Expected: accepted business values match; duplicate write count is one; injected audit failure leaves no business write; undo is idempotent.

- [ ] **Step 8: Commit the write tool**

```powershell
git add src-tauri/src/agent/tools/record.rs src-tauri/tests/agent_tools.rs
git commit -m "feat: add exactly-once plan check-in tool"
```

## Task 7: Connect Runtime, Executor, policy, approvals, and ownership

**Files:**

- Modify: `src-tauri/src/agent/model.rs`
- Modify: `src-tauri/src/agent/repository.rs`
- Modify: `src-tauri/src/agent/runtime.rs`
- Modify: `src-tauri/src/agent/executor.rs`
- Modify: `src-tauri/src/agent/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/tests/agent_tools.rs`

- [ ] **Step 1: Define and test the execution DTO contract**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    pub run_id: String,
    pub step_index: i64,
    pub tool_name: String,
    pub tool_version: String,
    pub input: serde_json::Value,
    pub idempotency_key: Option<String>,
    pub approval_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ToolCallResponse {
    Completed { step_id: String, output: serde_json::Value, replayed: bool, undo_available: bool },
    WaitingApproval { step_id: String, approval_id: String, preview: serde_json::Value, expires_at: String },
    SummaryRequired { step_id: String, preview: serde_json::Value },
    NavigationRequired { route: String, reason: String },
}
```

Tests must prove request risk/permissions fields cannot override descriptor metadata because those fields are rejected by each tool's `additionalProperties: false` input schema.

- [ ] **Step 2: Write executor policy and atomic-audit tests**

Create synthetic test descriptors for R2, R3, and R4 with a test dispatch counter. Assert R2 without setting produces `SummaryRequired`, R3 without approval produces one pending approval and zero dispatches, valid approval dispatches once, expired/stale/rejected approval dispatches zero times, and R4 returns a settings route with zero dispatches. Inject a Step/Event failure and assert business writes roll back.

- [ ] **Step 3: Run executor tests and observe RED**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --test agent_tools executor -- --nocapture
```

Expected: compile failure because `AgentExecutor` is absent.

- [ ] **Step 4: Implement ownership and executor ordering**

The executor must perform this order:

```text
registry lookup -> input schema -> ownership -> Step reservation/replay -> policy
-> approval/precondition -> timeout-wrapped dispatch -> output schema
-> business data + Step receipt + Event commit
```

Read ownership with a parameterized query:

```sql
SELECT value FROM settings WHERE key = 'agent_tool_owner.' || ?
```

`ToolDescriptor` remains immutable static metadata. `AgentExecutor::list_tools` returns dynamic `ListedTool` values by reading `agent_tool_owner.<name>` for every descriptor:

```rust
pub async fn list_tools(&self) -> Result<Vec<ListedTool>, AgentError> {
    let mut listed = Vec::new();
    for descriptor in self.registry.descriptors() {
        let key = format!("agent_tool_owner.{}", descriptor.name);
        let value: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
            .bind(&key).fetch_optional(&self.pool).await.map_err(map_sqlx)?;
        let ownership = match value.as_deref() {
            Some("typescript") => ToolOwnership::Typescript,
            Some("shadow") => ToolOwnership::Shadow,
            Some("rust-owned") => ToolOwnership::RustOwned,
            _ => ToolOwnership::Unavailable,
        };
        listed.push(ListedTool { descriptor: descriptor.clone(), ownership });
    }
    Ok(listed)
}
```

Missing or invalid settings fail closed as `Unavailable`; a settings/database error returns `AgentError::Persistence` rather than silently marking the tool available. `Unavailable` is display-only and never executable. Permit `plan.get_today` in `shadow` for diagnostic comparison, but tag its receipt `delivery="shadow"`. Reject every write when ownership is not `rust-owned`. Do not auto-promote ownership. Add a RED/GREEN test deleting each ownership setting and asserting `Unavailable`, then add a database-error test asserting `Persistence`.

- [ ] **Step 5: Persist and recheck R3 approvals**

When R3 first enters policy, insert `agent_approvals` with risk `3`, preview, precondition hash, `pending` status, and a 10-minute UTC expiration, then atomically transition the Run to `waiting_approval`. `decide_approval` updates only a pending, unexpired approval. On resumed execution, reread source rows, recompute the precondition hash, and require exact equality before dispatch.

Use these event types: `tool.requested`, `tool.waiting_approval`, `approval.approved`, `approval.rejected`, `tool.completed`, `tool.failed`, `tool.undone`. Payloads contain IDs, tool name/version, risk, result status, and redacted error code; never raw SQL, API keys, full free-text notes, or database paths.

- [ ] **Step 6: Expose only Runtime methods**

```rust
impl AgentRuntime {
    pub async fn list_tools(&self) -> Result<Vec<ListedTool>, AgentError> {
        self.executor.list_tools().await
    }
    pub async fn execute_tool(&self, request: ToolCallRequest)
        -> Result<ToolCallResponse, AgentError> { self.executor.execute(request).await }
    pub async fn decide_approval(&self, approval_id: &str, approve: bool)
        -> Result<ApprovalRecord, AgentError> { self.executor.decide_approval(approval_id, approve).await }
    pub async fn undo_tool(&self, step_id: &str)
        -> Result<ToolUndoResponse, AgentError> { self.executor.undo(step_id).await }
}
```

Make dispatcher and raw transaction helpers `pub(crate)`. No Tauri command may receive a pool, repository, risk value, policy decision, or data permission list.

- [ ] **Step 7: Verify Runtime integration and commit**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --test agent_tools executor -- --nocapture
cargo test --manifest-path src-tauri\Cargo.toml agent::runtime::tests -- --nocapture
cargo clippy --manifest-path src-tauri\Cargo.toml --all-targets --all-features -- -D warnings
git add src-tauri/src/agent src-tauri/src/lib.rs src-tauri/tests/agent_tools.rs
git commit -m "feat: connect agent policy executor runtime"
```

Expected: no unapproved R3 dispatch, all atomic-audit tests pass, and Clippy exits `0`.

## Task 8: Add typed Tauri commands and hidden debug slice

**Files:**

- Modify: `src-tauri/src/agent/commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/types/index.ts`
- Modify: `src/services/agent-client.ts`
- Modify: `src/services/agent-client.test.ts`
- Modify: `src/pages/AgentDebug.vue`
- Modify: `src/pages/AgentDebug.test.ts`

- [ ] **Step 1: Write failing frontend command-contract tests**

```typescript
it('invokes typed tool commands with camelCase boundary arguments', async () => {
  await listAgentTools()
  expect(invoke).toHaveBeenLastCalledWith('agent_list_tools')
  await executeAgentTool(request)
  expect(invoke).toHaveBeenLastCalledWith('agent_execute_tool', { request })
  await decideAgentApproval('approval-1', true)
  expect(invoke).toHaveBeenLastCalledWith('agent_decide_approval', { approvalId: 'approval-1', approve: true })
  await undoAgentTool('step-1')
  expect(invoke).toHaveBeenLastCalledWith('agent_undo_tool', { stepId: 'step-1' })
})
```

- [ ] **Step 2: Run frontend tests and observe RED**

```powershell
npm.cmd test -- src/services/agent-client.test.ts src/pages/AgentDebug.test.ts
```

Expected: failure because new client exports and controls are absent.

- [ ] **Step 3: Add the Tauri commands**

```rust
#[tauri::command]
pub async fn agent_list_tools(runtime: State<'_, AgentRuntime>) -> Result<Vec<ListedTool>, CommandError> {
    runtime.list_tools().await.map_err(Into::into)
}

#[tauri::command]
pub async fn agent_execute_tool(runtime: State<'_, AgentRuntime>, request: ToolCallRequest)
    -> Result<ToolCallResponse, CommandError> {
    runtime.execute_tool(request).await.map_err(Into::into)
}

#[tauri::command]
pub async fn agent_decide_approval(runtime: State<'_, AgentRuntime>, approval_id: String, approve: bool)
    -> Result<ApprovalRecord, CommandError> {
    let approval_id = trimmed_required(approval_id, "approval_id")?;
    runtime.decide_approval(&approval_id, approve).await.map_err(Into::into)
}

#[tauri::command]
pub async fn agent_undo_tool(runtime: State<'_, AgentRuntime>, step_id: String)
    -> Result<ToolUndoResponse, CommandError> {
    let step_id = trimmed_required(step_id, "step_id")?;
    runtime.undo_tool(&step_id).await.map_err(Into::into)
}
```

Register all four in `generate_handler!`. Keep existing Agent commands.

- [ ] **Step 4: Add exact TypeScript DTOs and client functions**

Mirror the Rust serialized fields and discriminated `state` union in `src/types/index.ts`. Keep static descriptor metadata separate from the dynamic ownership response:

```typescript
export type AgentToolOwnership = 'typescript' | 'shadow' | 'rust-owned' | 'unavailable'
export interface AgentToolDescriptor { name: string; version: string; risk: 'R0'|'R1'|'R2'|'R3'|'R4'; confirmation: string; supports_undo: boolean; timeout_ms: number; idempotency: string; data_permissions: string[]; input_schema: unknown; output_schema: unknown }
export interface ListedAgentTool { descriptor: AgentToolDescriptor; ownership: AgentToolOwnership }

export const listAgentTools = (): Promise<ListedAgentTool[]> =>
  invoke('agent_list_tools')
export const executeAgentTool = (request: AgentToolCallRequest): Promise<AgentToolCallResponse> =>
  invoke('agent_execute_tool', { request })
export const decideAgentApproval = (approvalId: string, approve: boolean): Promise<AgentApproval> =>
  invoke('agent_decide_approval', { approvalId, approve })
export const undoAgentTool = (stepId: string): Promise<AgentToolUndoResponse> =>
  invoke('agent_undo_tool', { stepId })
```

- [ ] **Step 5: Extend only the hidden page**

On `/agent-debug`, list each `ListedAgentTool` name/version/risk and its dynamic ownership separately from the descriptor, provide an exam ID input for `plan.get_today`, display JSON output, and display Step ID/replayed/undo state for a check-in receipt. Do not add a sidebar link and do not replace calls in `PlanCheckinBoard.vue` or `record-service.ts`. Disable the check-in execute button when `ownership !== 'rust-owned'`; show `shadow`, `typescript`, or `unavailable` as the current gate. If `agent_list_tools` returns a persistence error, show the redacted command error and keep every write disabled.

The component test must assert a shadow `plan.get_today` call renders its output and a TypeScript-owned check-in is disabled. Add a separate mocked `rust-owned` case that calls execute exactly once and makes undo available.

- [ ] **Step 6: Verify typed boundary and commit**

```powershell
npm.cmd test -- src/services/agent-client.test.ts src/pages/AgentDebug.test.ts
npm.cmd run typecheck
cargo test --manifest-path src-tauri\Cargo.toml agent::commands::tests -- --nocapture
git add src-tauri/src/agent/commands.rs src-tauri/src/lib.rs src/types/index.ts src/services/agent-client.ts src/services/agent-client.test.ts src/pages/AgentDebug.vue src/pages/AgentDebug.test.ts
git commit -m "feat: expose first agent tool vertical slice"
```

Expected: command contract, component tests, typecheck, and Rust command tests pass; the production check-in flow is unchanged.

## Task 9: Prove ownership cutover, privacy, concurrency, and rollback

**Files:**

- Modify: `src-tauri/tests/agent_tools.rs`
- Modify: `docs/agent/feature-parity.md`
- Modify: `docs/agent/migration-runbook.md`
- Modify: `MANUAL_TEST.md`

- [ ] **Step 1: Add ownership and concurrent duplicate tests (RED first)**

Start two Tokio tasks against a WAL file database, each invoking the same R1 key. Use a barrier so both start together. The RED test must assert that the intended result is one normal completion plus one replay, not a terminal unique-constraint error:

```rust
let (left, right) = tokio::join!(executor.execute(call_with_key("same-key")),
                                 executor.execute(call_with_key("same-key")));
let responses = [left.unwrap(), right.unwrap()];
assert_eq!(responses.iter().filter(|response| response.replayed()).count(), 1);
assert_eq!(count_rows(&pool, "study_records", "plan_id='plan-1'").await, 1);
assert_eq!(count_rows(&pool, "agent_steps", "idempotency_key='same-key'").await, 1);
assert_eq!(count_events(&pool, "tool.completed").await, 1);
```

Implement the unique-key race algorithm before turning this test GREEN: when Step reservation encounters `UNIQUE(idempotency_key)`, rollback and close the current transaction; in a new transaction query the completed Step by key; verify `tool_name`, `tool_version`, and the canonical input fingerprint match; then replay its stored output with `replayed=true`. If the second read finds no completed Step because the first transaction rolled back or the database remained busy, retry the read with the configured `busy_timeout` a finite three times; after that return `AgentError::IdempotencyConflict`, whose `code()` is exactly `idempotency_conflict`, without dispatching the tool. Never return the raw unique-constraint error and never dispatch twice.

The GREEN test must cover both outcomes: a committed first transaction yields one completion plus one replay, while an injected first-transaction rollback lets the second caller retry and become the sole completion. Add a third race case that holds the key unresolved through all three bounded reads and assert `error.code() == "idempotency_conflict"`, `error.to_string() == "idempotency key is already being resolved; retry"`, and zero business/audit writes. With settings `typescript` and `shadow`, assert the write dispatch counter remains zero. With `rust-owned`, assert it becomes one.

- [ ] **Step 2: Add privacy and stable-error tests**

Pass notes containing `SECRET_MARKER` and trigger a SQL error. Query Agent events and command errors and assert neither contains the marker, SQL text, `%APPDATA%`, nor an absolute path. Convert `AgentError::IdempotencyConflict` at the command boundary and assert `{ code: "idempotency_conflict", message: "idempotency key is already being resolved; retry" }`; assert no SQLite constraint detail is present. Assert input snapshots retain only permitted structured check-in fields and receipt metadata lists the descriptor's permission categories.

- [ ] **Step 3: Add upgrade and restore tests**

Create file databases from migrations v1, v2, v3, and v4, seed representative business rows, apply through v5, reopen through both configured pools, and assert all rows plus five Agent tables and four receipt columns. Exercise `prepare_database_restore`, replace with a v4 backup, relaunch migration setup, and assert the backup upgrades once with ownership defaults.

- [ ] **Step 4: Run focused hardening tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --test agent_tools -- --test-threads=1
cargo test --manifest-path src-tauri\Cargo.toml db::tests -- --test-threads=1
```

Expected: concurrency, privacy, migration upgrade, restore, R3, idempotency, and undo tests pass serially.

- [ ] **Step 5: Document the exact cutover and rollback**

Document these commands/operations in `migration-runbook.md`:

```sql
-- Cut over only after parity and packaged checks pass.
UPDATE settings SET value='rust-owned'
WHERE key='agent_tool_owner.record.checkin_plan' AND value='typescript';

-- Roll back before reopening the TypeScript writer.
UPDATE settings SET value='typescript'
WHERE key='agent_tool_owner.record.checkin_plan' AND value='rust-owned';
```

The application must close/relaunch after either ownership change. Never leave TypeScript and Rust writers enabled concurrently. `plan.get_today` may move `shadow -> rust-owned` independently because it is read-only.

- [ ] **Step 6: Commit hardening evidence**

```powershell
git add src-tauri/tests/agent_tools.rs docs/agent/feature-parity.md docs/agent/migration-runbook.md MANUAL_TEST.md
git commit -m "test: harden agent tool ownership and recovery"
```

## Task 10: Milestone 2 full verification and packaged exit gate

**Files:**

- Modify: `docs/agent/feature-parity.md`
- Modify: `MANUAL_TEST.md`

- [ ] **Step 1: Run every automated gate from a clean worktree**

```powershell
npm.cmd test
npm.cmd run typecheck
npm.cmd run build
cargo fmt --manifest-path src-tauri\Cargo.toml -- --check
cargo test --manifest-path src-tauri\Cargo.toml
cargo test --manifest-path src-tauri\Cargo.toml -- --test-threads=1
cargo clippy --manifest-path src-tauri\Cargo.toml --all-targets --all-features -- -D warnings
git diff --check
```

Required evidence: zero Vitest failures, zero Rust unit/integration failures in parallel and serial modes, no type/build errors, no formatter/Clippy/diff warnings.

- [ ] **Step 2: Run the packaged manual vertical slice on a backed-up database**

Back up `%APPDATA%\com.zhiyan.app\zhiyan.db`, build/install the Windows package, and perform these checks:

1. Existing dashboard, plan, check-in, record, wrong-question, analysis, visualization, settings, export, backup, and restore flows remain usable.
2. `/agent-debug` lists both version-1 descriptors and `plan.get_today` matches today's existing TypeScript view.
3. In a disposable database with the R1 ownership flag explicitly set `rust-owned`, submit one check-in and invoke the same idempotency key twice; exactly one record and one duration increment exist.
4. Undo once and twice; the first compensates and the second is a no-op replay.
5. Exercise a synthetic R3 test command through the debug build: no approval means zero business writes, valid approval permits one, and stale/expired/rejected approval permits zero.
6. Restore the pre-test backup using `closeDb -> agent_prepare_database_restore -> replace -> relaunch`; verify old business and Agent rows.

- [ ] **Step 3: Record ownership honestly**

Keep `record.checkin_plan` as `shadow` or `typescript` if packaged parity is not manually signed off. Mark it `rust-owned` only after Step 2 succeeds and the production UI is deliberately switched in a later reviewed commit. `plan.get_today` may be marked `rust-owned` after read parity because no write-owner conflict exists. Leave unchecked manual items unchecked.

- [ ] **Step 4: Commit milestone evidence**

```powershell
git add docs/agent/feature-parity.md MANUAL_TEST.md
git commit -m "docs: complete agent tools policy milestone"
git status --short
```

Expected: commit succeeds and final status is clean.

## Exit criteria

- Shared fixtures prove Rust/TypeScript parity for the R0 projection and successful R1 business outcome.
- `plan.get_today@1` is schema-validated, read-only, and honors the local 04:00 business-day boundary.
- `record.checkin_plan@1` copies locked plan fields, persists all learning metrics, updates aggregate duration/tasks/status, and never writes twice for one idempotency key.
- R1 undo compensates exactly once and retains an Agent audit receipt.
- R0 executes automatically; R1 executes automatically with undo; R2 summarizes unless enabled; R3 never executes without a live matching approval; R4 only navigates.
- Tool execution, business mutation, Step receipt, and Event receipt share a transaction, closing the write/audit crash window.
- Ownership gates prevent TypeScript/Rust double writes and have an explicit rollback procedure.
- Existing user-facing Vue workflows are unchanged in this milestone; tool diagnostics remain hidden at `/agent-debug`.
- Full automated verification and packaged manual checks are recorded with actual results before any ownership claim.
