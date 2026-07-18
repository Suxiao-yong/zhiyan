import { beforeEach, describe, expect, it, vi } from 'vitest'

import planGetTodayFixture from '../../tests/fixtures/agent-tools/plan-get-today.json'
import recordCheckinFixture from '../../tests/fixtures/agent-tools/record-checkin-plan.json'

vi.mock('./db', () => ({
  count: vi.fn(),
  execute: vi.fn(),
  getAll: vi.fn(),
  getById: vi.fn(),
  insert: vi.fn(),
  query: vi.fn(),
  remove: vi.fn(),
  setSetting: vi.fn(),
  update: vi.fn(),
}))

import * as db from './db'
import { getPlansByDateRange } from './plan-service'
import { createPlanCheckin } from './record-service'

describe('agent tool TypeScript parity contract', () => {
  beforeEach(() => vi.clearAllMocks())

  it('repairs and returns today plans from recorded study facts', async () => {
    const expectedPlan = planGetTodayFixture.expected_output.plans[0]
    vi.mocked(db.query).mockResolvedValue([
      {
        ...expectedPlan,
        actual_duration: 0,
        actual_tasks: null,
        status: 'pending',
        recorded_duration: 30,
        latest_record_content: '完成第一节',
      },
    ] as any)

    const plans = await getPlansByDateRange(
      planGetTodayFixture.input.exam_id,
      planGetTodayFixture.business_date,
      planGetTodayFixture.business_date,
    )

    expect(plans).toEqual(planGetTodayFixture.expected_output.plans)
    expect(db.update).toHaveBeenCalledWith('study_plans', 'plan-1', {
      actual_duration: 30,
      actual_tasks: '完成第一节',
      status: 'in_progress',
    })
  })

  it('creates a locked plan checkin, aggregates progress, and copies wrong-question links', async () => {
    const plan = {
      ...recordCheckinFixture.plan,
      actual_duration: 0,
      actual_tasks: null,
      generated_by: 'local',
      ai_suggestion: null,
      user_modified: 0,
      sort_order: 0,
      created_at: '2026-07-16 09:00:00',
      updated_at: '2026-07-17 21:00:00',
    }
    const recordNew = {
      id: 'record-new',
      plan_id: plan.id,
      date: plan.date,
      subject_id: plan.subject_id,
      knowledge_point_id: plan.knowledge_point_id,
      duration_min: recordCheckinFixture.input.duration_min,
      content: recordCheckinFixture.input.content,
      questions_count: recordCheckinFixture.input.questions_count,
      correct_count: recordCheckinFixture.input.correct_count,
      mastery_rating: recordCheckinFixture.input.mastery_rating,
      difficulty_notes: recordCheckinFixture.input.difficulty_notes,
      mood: recordCheckinFixture.input.mood,
      session_time: recordCheckinFixture.input.session_time,
      created_at: '2026-07-17 21:30:00',
      updated_at: '2026-07-17 21:30:00',
    }
    vi.mocked(db.getById).mockImplementation(async (table) => {
      if (table === 'study_plans') return plan as any
      return recordNew as any
    })
    vi.mocked(db.insert).mockResolvedValue('record-new')
    vi.mocked(db.query).mockResolvedValue([
      { total: 50, count: 2, latest_content: '完成第一节' },
    ] as any)
    const legacyWrong = recordCheckinFixture.input.wrong_questions.map((wrong) => ({
      ...wrong,
      subject_id: 'legacy-subject',
      knowledge_point_id: 'legacy-knowledge-point',
    }))

    await createPlanCheckin('plan-1', recordCheckinFixture.input, false, legacyWrong)

    expect(db.insert).toHaveBeenCalledWith('study_records', {
      plan_id: 'plan-1',
      date: recordCheckinFixture.expected.date,
      subject_id: recordCheckinFixture.expected.subject_id,
      knowledge_point_id: recordCheckinFixture.expected.knowledge_point_id,
      duration_min: recordCheckinFixture.input.duration_min,
      content: recordCheckinFixture.input.content,
      questions_count: recordCheckinFixture.expected.questions_count,
      correct_count: recordCheckinFixture.expected.correct_count,
      mastery_rating: recordCheckinFixture.expected.mastery_rating,
      difficulty_notes: recordCheckinFixture.input.difficulty_notes,
      mood: recordCheckinFixture.input.mood,
      session_time: recordCheckinFixture.input.session_time,
    })
    expect(db.update).toHaveBeenCalledWith('study_plans', 'plan-1', {
      actual_duration: recordCheckinFixture.expected.actual_duration,
      actual_tasks: recordCheckinFixture.expected.actual_tasks,
      status: recordCheckinFixture.expected.status,
    })
    expect(db.insert).toHaveBeenCalledWith('wrong_questions', {
      record_id: 'record-new',
      subject_id: recordCheckinFixture.expected.subject_id,
      knowledge_point_id: recordCheckinFixture.expected.knowledge_point_id,
      ...recordCheckinFixture.input.wrong_questions[0],
    })
  })
})
