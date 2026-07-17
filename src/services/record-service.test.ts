import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { StudyPlan, StudyRecord } from '@/types'

vi.mock('./db', () => ({
  count: vi.fn(),
  execute: vi.fn(async () => ({ lastInsertId: 0, rowsAffected: 0 })),
  getAll: vi.fn(async () => []),
  getById: vi.fn(),
  insert: vi.fn(async () => 'r1'),
  query: vi.fn(),
  remove: vi.fn(),
  setSetting: vi.fn(),
  update: vi.fn(),
}))

import * as db from './db'
import { createPlanCheckin, createRecord, deleteRecord, updateRecord } from './record-service'

function plan(overrides: Partial<StudyPlan> = {}): StudyPlan {
  return {
    id: 'p1',
    exam_id: 'e1',
    subject_id: 's1',
    knowledge_point_id: 'k1',
    date: '2020-01-01',
    planned_tasks: '复习函数',
    planned_duration: 60,
    actual_duration: null,
    actual_tasks: null,
    status: 'pending',
    generated_by: 'local',
    ai_suggestion: null,
    user_modified: 0,
    sort_order: 0,
    created_at: '',
    updated_at: '',
    ...overrides,
  }
}

function record(overrides: Partial<StudyRecord> = {}): StudyRecord {
  return {
    id: 'r1',
    plan_id: 'p1',
    date: '2020-01-01',
    subject_id: 's1',
    knowledge_point_id: 'k1',
    duration_min: 30,
    content: '完成第一节',
    questions_count: 0,
    correct_count: 0,
    mastery_rating: null,
    difficulty_notes: null,
    mood: null,
    session_time: 'evening',
    created_at: '',
    updated_at: '',
    ...overrides,
  }
}

describe('计划任务打卡', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(db.getById).mockImplementation(async (table, id) => {
      if (table === 'study_plans' && id === 'p1') return plan() as any
      if (table === 'study_records' && id === 'r1') return record() as any
      return null
    })
    vi.mocked(db.query).mockResolvedValue([
      { total: 30, count: 1, latest_content: '完成第一节' },
    ] as any)
  })

  it('使用计划的日期、科目和知识点创建记录并标记进行中', async () => {
    await createPlanCheckin('p1', { duration_min: 30, content: '完成第一节' }, false)

    expect(db.insert).toHaveBeenCalledWith(
      'study_records',
      expect.objectContaining({
        plan_id: 'p1',
        date: '2020-01-01',
        subject_id: 's1',
        knowledge_point_id: 'k1',
        duration_min: 30,
      }),
    )
    expect(db.update).toHaveBeenCalledWith(
      'study_plans',
      'p1',
      expect.objectContaining({
        actual_duration: 30,
        actual_tasks: '完成第一节',
        status: 'in_progress',
      }),
    )
  })

  it('完成提交后把计划标记为已完成', async () => {
    await createPlanCheckin('p1', { duration_min: 30, content: '完成第一节' }, true)

    expect(db.update).toHaveBeenCalledWith(
      'study_plans',
      'p1',
      expect.objectContaining({ status: 'completed' }),
    )
  })

  it('错题保存失败时仍先同步已经写入的学习进度', async () => {
    vi.mocked(db.insert)
      .mockResolvedValueOnce('r1')
      .mockRejectedValueOnce(new Error('错题保存失败'))

    await expect(
      createPlanCheckin('p1', { duration_min: 30 }, false, [
        { subject_id: 's1', question_desc: '错题' },
      ]),
    ).rejects.toMatchObject({
      name: 'PlanCheckinSavedWithWarning',
      message: expect.stringContaining('学习记录已保存'),
    })

    expect(db.update).toHaveBeenCalledWith(
      'study_plans',
      'p1',
      expect.objectContaining({ actual_duration: 30, status: 'in_progress' }),
    )
  })

  it('计划汇总失败时返回已保存警告并携带记录', async () => {
    vi.mocked(db.update).mockRejectedValueOnce(new Error('database busy'))

    await expect(createPlanCheckin('p1', { duration_min: 30 }, false)).rejects.toMatchObject({
      name: 'PlanCheckinSavedWithWarning',
      message: expect.stringContaining('学习记录已保存'),
      record: expect.objectContaining({ id: 'r1' }),
    })
  })

  it('拒绝不存在的计划', async () => {
    vi.mocked(db.getById).mockResolvedValueOnce(null)

    await expect(createPlanCheckin('missing', { duration_min: 15 }, false)).rejects.toThrow(
      '计划已被删除或重新生成',
    )
    expect(db.insert).not.toHaveBeenCalled()
  })

  it('拒绝为已跳过任务打卡', async () => {
    vi.mocked(db.getById).mockResolvedValueOnce(plan({ status: 'skipped' }) as any)

    await expect(createPlanCheckin('p1', { duration_min: 15 }, false)).rejects.toThrow(
      '请先恢复任务',
    )
    expect(db.insert).not.toHaveBeenCalled()
  })

  it('拒绝为未来计划提前打卡', async () => {
    vi.mocked(db.getById).mockResolvedValueOnce(plan({ date: '2999-01-01' }) as any)

    await expect(createPlanCheckin('p1', { duration_min: 15 }, false)).rejects.toThrow(
      '未来计划不能提前打卡',
    )
    expect(db.insert).not.toHaveBeenCalled()
  })

  it('自由记录明确写入空计划关联', async () => {
    await createRecord({ subject_id: 's1', duration_min: 20 })

    expect(db.insert).toHaveBeenCalledWith(
      'study_records',
      expect.objectContaining({ plan_id: null }),
    )
  })

  it('编辑计划打卡后重新汇总关联计划', async () => {
    vi.mocked(db.query).mockResolvedValue([
      { total: 45, count: 1, latest_content: '完成两节' },
    ] as any)

    await updateRecord('r1', { duration_min: 45, content: '完成两节' })

    expect(db.update).toHaveBeenCalledWith('study_records', 'r1', {
      duration_min: 45,
      content: '完成两节',
    })
    expect(db.update).toHaveBeenCalledWith(
      'study_plans',
      'p1',
      expect.objectContaining({ actual_duration: 45, actual_tasks: '完成两节' }),
    )
  })

  it('删除最后一条计划打卡后把任务恢复为未开始', async () => {
    vi.mocked(db.query).mockResolvedValue([{ total: 0, count: 0, latest_content: null }] as any)

    await deleteRecord('r1')

    expect(db.remove).toHaveBeenCalledWith('study_records', 'r1')
    expect(db.update).toHaveBeenCalledWith(
      'study_plans',
      'p1',
      expect.objectContaining({ actual_duration: 0, status: 'pending' }),
    )
    expect(vi.mocked(db.update).mock.invocationCallOrder.at(-1)).toBeLessThan(
      vi.mocked(db.remove).mock.invocationCallOrder[0],
    )
  })
})
