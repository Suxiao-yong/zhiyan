# Plan-Based Study Check-Ins Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make generated study-plan tasks the primary source of study check-ins while retaining optional free-form records.

**Architecture:** Add an optional `plan_id` relation from study records to plans. Keep records as the source of truth for real study behavior, centralize check-in orchestration in `record-service.ts`, and reuse one plan check-in dialog from the record page, dashboard, and plan views.

**Tech Stack:** Vue 3, TypeScript, Pinia, Element Plus, Vitest, Tauri 2, SQLite

---

## File map

- `src-tauri/src/db.rs`: schema migration for `study_records.plan_id` and its index.
- `src/types/index.ts`: expose the nullable plan relation on `StudyRecord`.
- `src/services/db.ts`: permit `plan_id` in generic study-record inserts and updates.
- `src/services/record-service.ts`: implement plan check-in validation, persistence, aggregation, restore, update, and delete synchronization.
- `src/services/record-service.test.ts`: service-level red/green coverage with a mocked database.
- `src/services/export.test.ts`: old-backup compatibility coverage.
- `src/stores/record.ts`: expose plan check-in actions to Vue components.
- `src/services/plan-service.ts`: load tasks for a selected date, restore skipped tasks, and prevent recordless completion.
- `src/stores/plan.ts`: refresh plan collections after status changes.
- `src/components/record/PlanCheckinDialog.vue`: dedicated check-in form for a plan task.
- `src/components/record/PlanCheckinBoard.vue`: date selector and task-card actions.
- `src/components/record/checkin-ui.ts`: pure UI-state helpers used by components and tests.
- `src/components/record/checkin-ui.test.ts`: button-label and default-duration tests.
- `src/components/record/QuickRecordDialog.vue`: keep free recording but label it explicitly.
- `src/components/record/RecordList.vue`: show plan/free source and synchronize plan progress after edits and deletes.
- `src/pages/StudyRecord.vue`: make plan check-in the default tab and free recording secondary.
- `src/components/dashboard/TodayTodoList.vue`: replace direct completion checkboxes with check-in actions.
- `src/pages/Dashboard.vue`: host the shared check-in dialog and refresh dashboard data.
- `src/components/plan/TaskDetailDialog.vue`: remove direct completed selection and open the shared check-in dialog.
- `src/components/plan/PlanList.vue`: route completion through check-in and retain skip/restore.
- `MANUAL_TEST.md`: document plan-based check-in, partial progress, repeated check-ins, and free recording.

### Task 1: Lock the plan check-in service contract with failing tests

**Files:**
- Create: `src/services/record-service.test.ts`
- Modify: `src/services/record-service.ts`
- Modify: `src/stores/record.ts`

- [ ] **Step 1: Write failing service tests**

Mock `./db` and add these behaviors. Use a `plan(overrides)` fixture whose date is `2026-07-17`, subject is `s1`, and knowledge point is `k1`; use a `record(overrides)` fixture with the complete `StudyRecord` shape.

```ts
it('creates a check-in from immutable plan fields and marks progress', async () => {
  vi.mocked(db.getById).mockResolvedValue(plan({ status: 'pending' }) as any)
  vi.mocked(db.insert).mockResolvedValue('r1')
  vi.mocked(db.query)
    .mockResolvedValueOnce([{ total: 30, latest_content: '完成第一节' }] as any)

  await createPlanCheckin('p1', { duration_min: 30, content: '完成第一节' }, false)

  expect(db.insert).toHaveBeenCalledWith(
    'study_records',
    expect.objectContaining({
      plan_id: 'p1',
      date: '2026-07-17',
      subject_id: 's1',
      knowledge_point_id: 'k1',
    }),
  )
  expect(db.update).toHaveBeenCalledWith(
    'study_plans',
    'p1',
    expect.objectContaining({ actual_duration: 30, status: 'in_progress' }),
  )
})

it('marks a plan completed only through a finishing check-in', async () => {
  vi.mocked(db.getById).mockImplementation(async (table, id) => {
    if (table === 'study_plans' && id === 'p1') return plan({ status: 'pending' }) as any
    if (table === 'study_records' && id === 'r1') return record({ id: 'r1', plan_id: 'p1' }) as any
    return null
  })
  vi.mocked(db.insert).mockResolvedValue('r1')
  vi.mocked(db.query).mockResolvedValue([{ total: 30, count: 1, latest_content: '完成' }] as any)

  await createPlanCheckin('p1', { duration_min: 30, content: '完成' }, true)

  expect(db.update).toHaveBeenCalledWith(
    'study_plans',
    'p1',
    expect.objectContaining({ actual_duration: 30, status: 'completed' }),
  )
})

it('rejects missing, skipped, and future plans before inserting', async () => {
  vi.mocked(db.getById).mockResolvedValueOnce(null)
  await expect(createPlanCheckin('missing', { duration_min: 15 }, false)).rejects.toThrow(
    '计划已被删除或重新生成',
  )

  vi.mocked(db.getById).mockResolvedValueOnce(plan({ status: 'skipped' }) as any)
  await expect(createPlanCheckin('p1', { duration_min: 15 }, false)).rejects.toThrow(
    '请先恢复任务',
  )

  vi.mocked(db.getById).mockResolvedValueOnce(plan({ date: '2999-01-01' }) as any)
  await expect(createPlanCheckin('p1', { duration_min: 15 }, false)).rejects.toThrow(
    '未来计划不能提前打卡',
  )
  expect(db.insert).not.toHaveBeenCalled()
})

it('keeps free records unlinked', async () => {
  await createRecord({ subject_id: 's1', duration_min: 20 })
  expect(db.insert).toHaveBeenCalledWith(
    'study_records',
    expect.objectContaining({ plan_id: null }),
  )
})
```

- [ ] **Step 2: Run the focused test and verify RED**

Run: `npx vitest run src/services/record-service.test.ts`

Expected: FAIL because `createPlanCheckin` and plan aggregation do not exist.

- [ ] **Step 3: Implement the minimal service API**

Add these contracts and behavior:

```ts
export type PlanCheckinInput = Omit<RecordInput, 'date' | 'subject_id' | 'knowledge_point_id'>

export async function recalculatePlanProgress(
  planId: string,
  requestedStatus?: 'in_progress' | 'completed',
): Promise<void>

export async function createPlanCheckin(
  planId: string,
  input: PlanCheckinInput,
  finish: boolean,
  wrongs: WrongQuestionInput[] = [],
): Promise<StudyRecord>

export async function getPlanCheckins(planId: string): Promise<RecordWithNames[]>

export async function restorePlan(planId: string): Promise<void>
```

`createPlanCheckin` must re-read the plan, reject missing/skipped/future tasks, copy plan date/subject/knowledge point into the record, insert wrong questions with the new record ID, and recalculate the plan after the fact record exists.

- [ ] **Step 4: Expose the API through the record store**

Add `createPlanCheckin`, `getPlanCheckins`, and `restorePlan` actions that delegate to the service and return created records when relevant.

- [ ] **Step 5: Run the focused test and verify GREEN**

Run: `npx vitest run src/services/record-service.test.ts`

Expected: all tests in the file pass.

### Task 2: Keep plan aggregates correct when records change

**Files:**
- Modify: `src/services/record-service.test.ts`
- Modify: `src/services/record-service.ts`

- [ ] **Step 1: Add failing update/delete tests**

```ts
it('recalculates the linked plan after editing a check-in', async () => {
  vi.mocked(db.getById).mockResolvedValueOnce(record({ plan_id: 'p1' }) as any)
  await updateRecord('r1', { duration_min: 45 })
  expect(db.update).toHaveBeenCalledWith('study_records', 'r1', { duration_min: 45 })
  expectPlanRecalculated('p1')
})

it('returns a plan to pending after its last check-in is deleted', async () => {
  vi.mocked(db.getById).mockResolvedValueOnce(record({ plan_id: 'p1' }) as any)
  vi.mocked(db.query).mockResolvedValueOnce([{ total: 0, count: 0, latest_content: null }] as any)
  await deleteRecord('r1')
  expect(db.update).toHaveBeenCalledWith(
    'study_plans',
    'p1',
    expect.objectContaining({ actual_duration: 0, status: 'pending' }),
  )
})
```

- [ ] **Step 2: Verify RED**

Run: `npx vitest run src/services/record-service.test.ts`

Expected: FAIL because update and delete do not reload the old record or recalculate its plan.

- [ ] **Step 3: Implement synchronization**

Before update/delete, load the existing record. Recalculate its linked plan after the write. Do not allow `plan_id`, `date`, `subject_id`, or `knowledge_point_id` changes through normal edits of a linked record. Preserve `completed` when linked records remain; preserve `skipped`; use `pending` when no record remains.

- [ ] **Step 4: Verify GREEN**

Run: `npx vitest run src/services/record-service.test.ts`

Expected: all service tests pass.

### Task 3: Add the persistent relation and import compatibility

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src/types/index.ts`
- Modify: `src/services/db.ts`
- Create: `src/services/export.test.ts`

- [ ] **Step 1: Write the old-backup compatibility test**

```ts
it('accepts version-one bundles whose study records have no plan_id', () => {
  const bundle = validBundle({
    study_records: [{ id: 'r1', date: '2026-07-16', subject_id: 's1', duration_min: 30 }],
  })
  expect(validateBundle(bundle)).toEqual({ ok: true, errors: [] })
})
```

- [ ] **Step 2: Run the focused test**

Run: `npx vitest run src/services/export.test.ts`

Expected: PASS for the existing permissive validator; this is a characterization test that protects backward compatibility before schema changes.

- [ ] **Step 3: Add migration version 3**

Append a migration containing:

```sql
ALTER TABLE study_records ADD COLUMN plan_id TEXT REFERENCES study_plans(id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS idx_records_plan ON study_records(plan_id);
```

- [ ] **Step 4: Update frontend types and column whitelist**

Add `plan_id: string | null` to `StudyRecord`, add optional `plan_id` to `RecordInput`, and add the column to `COLUMNS.study_records`.

- [ ] **Step 5: Verify type and compatibility tests**

Run: `npx vitest run src/services/record-service.test.ts src/services/export.test.ts && npm run typecheck`

Expected: both test files pass and TypeScript exits 0.

### Task 4: Build reusable plan check-in UI helpers and dialog

**Files:**
- Create: `src/components/record/checkin-ui.ts`
- Create: `src/components/record/checkin-ui.test.ts`
- Create: `src/components/record/PlanCheckinDialog.vue`

- [ ] **Step 1: Write failing pure UI tests**

```ts
expect(checkinAction('pending')).toEqual({ label: '开始打卡', kind: 'primary' })
expect(checkinAction('in_progress').label).toBe('继续记录')
expect(checkinAction('completed').label).toBe('补充记录')
expect(defaultCheckinDuration({ planned_duration: 60, actual_duration: 45 })).toBe(15)
expect(defaultCheckinDuration({ planned_duration: 60, actual_duration: 70 })).toBe(30)
```

- [ ] **Step 2: Verify RED**

Run: `npx vitest run src/components/record/checkin-ui.test.ts`

Expected: FAIL because the helper module does not exist.

- [ ] **Step 3: Implement the helpers**

Export `checkinAction(status)` and `defaultCheckinDuration(plan)` with the labels and duration rules from the design.

- [ ] **Step 4: Implement the dialog using the tested helpers**

Accept `modelValue` and `plan: PlanWithNames | null`; lock plan metadata; reuse the current question, mastery, session, mood, and `WrongQuestionInline` controls. Add a `submitting` guard and emit `saved` only after `recordStore.createPlanCheckin` resolves. Footer actions call the same submit function with `finish=false` and `finish=true`.

- [ ] **Step 5: Verify helpers and types**

Run: `npx vitest run src/components/record/checkin-ui.test.ts && npm run typecheck`

Expected: tests pass and TypeScript exits 0.

### Task 5: Make plan check-in the default study-record workflow

**Files:**
- Create: `src/components/record/PlanCheckinBoard.vue`
- Modify: `src/pages/StudyRecord.vue`
- Modify: `src/components/record/QuickRecordDialog.vue`
- Modify: `src/components/record/RecordList.vue`
- Modify: `src/services/record-service.ts`
- Modify: `src/services/plan-service.ts`

- [ ] **Step 1: Add selected-date plan loading**

Add `getPlansByDate(examId, date)` as a named wrapper around `getPlansByDateRange(examId, date, date)`. Extend `PlanWithNames` with `record_count: number` and add a correlated `COUNT(*)` projection to plan queries so every task card can render its check-in count.

- [ ] **Step 2: Build the board**

Default to `businessToday()`, allow past dates, show future tasks read-only, and emit/open `PlanCheckinDialog` for pending/in-progress/completed tasks. Restore skipped tasks through the record store.

- [ ] **Step 3: Recompose the page**

Use tabs named `checkin`, `records`, and `wrong`; make `checkin` active by default. Change the header subtitle to describe plan-based check-ins. Keep an outlined “自由记录” action that opens `QuickRecordDialog`.

- [ ] **Step 4: Add record source display**

Extend `RecordWithNames` queries with `plan_tasks`. Add a “来源” column showing “计划打卡” when `plan_id` exists and “自由记录” otherwise. Change empty-state copy so it does not direct users to quick recording as the primary path.

- [ ] **Step 5: Verify types and production compilation**

Run: `npm run typecheck && npm run build`

Expected: both commands exit 0.

### Task 6: Route dashboard and plan completion through check-in

**Files:**
- Modify: `src/components/dashboard/TodayTodoList.vue`
- Modify: `src/pages/Dashboard.vue`
- Modify: `src/components/plan/TaskDetailDialog.vue`
- Modify: `src/components/plan/PlanList.vue`
- Modify: `src/stores/plan.ts`
- Modify: `src/services/plan-service.ts`

- [ ] **Step 1: Replace dashboard checkboxes with task actions**

Have `TodayTodoList` emit `checkin(plan)` and `restore(plan)`. Render tested labels from `checkinAction`. Keep completed styling without allowing direct uncheck.

- [ ] **Step 2: Host and refresh the dialog on Dashboard**

Open `PlanCheckinDialog` for the emitted plan. After save, reload dashboard stats and today's tasks so time, streak, completion rate, and task cards update together.

- [ ] **Step 3: Remove recordless completion from plan views**

Replace completed status selections with a check-in action. Retain only skip/restore direct transitions. `updatePlanStatus(id, 'completed')` must reject or be inaccessible so no component can recreate the old inconsistent state.

- [ ] **Step 4: Refresh local collections after status changes**

Update `planStore.todayTasks` and `planStore.plans` entries after service changes, or reload the relevant query after dialog save.

- [ ] **Step 5: Run focused and full tests**

Run: `npm test`

Expected: all Vitest files pass with zero failures.

### Task 7: Documentation and final verification

**Files:**
- Modify: `MANUAL_TEST.md`
- Modify: `README.md`

- [ ] **Step 1: Update user-facing workflow documentation**

Replace quick-record-first instructions with plan task check-in. Document save-progress, finish-task, multiple check-ins, skipped-task restore, historical plan check-ins, and the secondary free-record flow.

- [ ] **Step 2: Update manual regression scenarios**

Add checks for migration from an existing database, old JSON imports, repeated task check-ins, edit/delete recalculation, future-task blocking, and record persistence when plans are removed.

- [ ] **Step 3: Run formatting checks without rewriting unrelated files**

Run: `npx prettier --check "src/**/*.{ts,vue,json,css}" "README.md" "MANUAL_TEST.md"`

Expected: no formatting errors in modified source and documentation. If failures are limited to edited files, format only those files.

- [ ] **Step 4: Run the full verification suite**

Run: `npm test && npm run typecheck && npx eslint . && npm run build && git diff --check`

Expected: all commands exit 0, all tests pass, and `git diff --check` prints no errors.

- [ ] **Step 5: Review requirement coverage**

Confirm in the diff that plan check-in is the default record route, free recording remains available, one plan supports multiple records, actual duration is recalculated, direct recordless completion is gone, old data remains readable, and docs describe the new workflow.
