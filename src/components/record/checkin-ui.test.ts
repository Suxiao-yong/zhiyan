import { describe, expect, it } from 'vitest'
import { checkinAction, defaultCheckinDuration } from './checkin-ui'

describe('计划打卡界面规则', () => {
  it('按计划状态返回明确操作', () => {
    expect(checkinAction('pending')).toEqual({ label: '开始打卡', type: 'primary' })
    expect(checkinAction('in_progress')).toEqual({ label: '继续记录', type: 'primary' })
    expect(checkinAction('completed')).toEqual({ label: '补充记录', type: 'success' })
    expect(checkinAction('skipped')).toEqual({ label: '恢复任务', type: 'warning' })
  })

  it('优先使用计划剩余时长，完成或超时时默认三十分钟', () => {
    expect(defaultCheckinDuration({ planned_duration: 60, actual_duration: 45 })).toBe(15)
    expect(defaultCheckinDuration({ planned_duration: 60, actual_duration: 70 })).toBe(30)
    expect(defaultCheckinDuration({ planned_duration: null, actual_duration: null })).toBe(30)
    expect(
      defaultCheckinDuration({
        status: 'completed',
        planned_duration: 120,
        actual_duration: 30,
      }),
    ).toBe(30)
  })

  it('带日期钻取时默认展示历史记录', async () => {
    const { initialStudyRecordTab } = await import('./checkin-ui')
    expect(initialStudyRecordTab('2026-07-17')).toBe('records')
    expect(initialStudyRecordTab(undefined)).toBe('checkin')
  })

  it('按计划关联显示记录来源', async () => {
    const { recordSourceLabel } = await import('./checkin-ui')
    expect(recordSourceLabel('p1')).toBe('计划打卡')
    expect(recordSourceLabel(null)).toBe('自由记录')
  })
})
