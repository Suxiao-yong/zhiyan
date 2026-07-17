import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('./db', () => ({ query: vi.fn(), update: vi.fn() }))
vi.mock('./record-service', () => ({ businessToday: () => '2026-07-17' }))

import * as db from './db'
import { getPlansByDateRange, updatePlan, updatePlanStatus } from './plan-service'

describe('计划状态写入约束', () => {
  beforeEach(() => vi.clearAllMocks())

  it('拒绝绕过学习记录直接完成任务', async () => {
    await expect(updatePlanStatus('p1', 'completed')).rejects.toThrow('请通过计划打卡完成任务')
    expect(db.update).not.toHaveBeenCalled()
  })

  it('拒绝从计划编辑表单直接写入完成状态', async () => {
    await expect(updatePlan('p1', { status: 'completed' })).rejects.toThrow(
      '请通过计划打卡完成任务',
    )
    expect(db.update).not.toHaveBeenCalled()
  })

  it('加载任务时用已保存记录修复中断的计划汇总', async () => {
    vi.mocked(db.query).mockResolvedValue([
      {
        id: 'p1',
        exam_id: 'e1',
        subject_id: 's1',
        knowledge_point_id: null,
        date: '2026-07-17',
        planned_tasks: '复习函数',
        planned_duration: 60,
        actual_duration: 0,
        actual_tasks: null,
        status: 'pending',
        generated_by: 'local',
        ai_suggestion: null,
        user_modified: 0,
        sort_order: 0,
        created_at: '',
        updated_at: '',
        subject_name: '数学',
        knowledge_point_name: null,
        record_count: 1,
        recorded_duration: 30,
        latest_record_content: '完成第一节',
      },
    ] as any)

    const plans = await getPlansByDateRange('e1', '2026-07-17', '2026-07-17')

    expect(db.update).toHaveBeenCalledWith('study_plans', 'p1', {
      actual_duration: 30,
      actual_tasks: '完成第一节',
      status: 'in_progress',
    })
    expect(plans[0]).toEqual(
      expect.objectContaining({ actual_duration: 30, status: 'in_progress' }),
    )
  })
})
