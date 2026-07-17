import type { PlanStatus } from '@/types'

export interface CheckinAction {
  label: string
  type: 'primary' | 'success' | 'warning'
}

export function checkinAction(status: PlanStatus): CheckinAction {
  if (status === 'completed') return { label: '补充记录', type: 'success' }
  if (status === 'skipped') return { label: '恢复任务', type: 'warning' }
  if (status === 'in_progress') return { label: '继续记录', type: 'primary' }
  return { label: '开始打卡', type: 'primary' }
}

export function defaultCheckinDuration(plan: {
  status?: PlanStatus
  planned_duration: number | null
  actual_duration: number | null
}): number {
  if (plan.status === 'completed') return 30
  const remaining = (plan.planned_duration ?? 0) - (plan.actual_duration ?? 0)
  return remaining > 0 ? remaining : 30
}

export function initialStudyRecordTab(dateQuery: unknown): 'checkin' | 'records' {
  return typeof dateQuery === 'string' && dateQuery ? 'records' : 'checkin'
}

export function recordSourceLabel(planId: string | null | undefined): '计划打卡' | '自由记录' {
  return planId ? '计划打卡' : '自由记录'
}
