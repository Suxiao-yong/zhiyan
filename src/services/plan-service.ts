// 学习计划 CRUD/查询 + 计划vs实际对比统计。
// 与 exam-service/record-service 同模式：业务规则在此，通用 db.ts 不污染。

import { query, update } from './db'
import { businessToday } from './record-service'
import type { PlanStatus, StudyPlan } from '@/types'

export type PlanWithNames = StudyPlan & {
  subject_name: string | null
  knowledge_point_name: string | null
}

export async function getPlansByDateRange(
  examId: string,
  from: string,
  to: string,
): Promise<PlanWithNames[]> {
  return query<PlanWithNames>(
    `SELECT p.*, s.name AS subject_name, k.name AS knowledge_point_name
     FROM study_plans p
     LEFT JOIN subjects s ON s.id = p.subject_id
     LEFT JOIN knowledge_points k ON k.id = p.knowledge_point_id
     WHERE p.exam_id = ? AND p.date >= ? AND p.date <= ?
     ORDER BY p.date, p.sort_order, p.created_at`,
    [examId, from, to],
  )
}

export async function getTodayPlans(examId: string): Promise<PlanWithNames[]> {
  const today = businessToday()
  return query<PlanWithNames>(
    `SELECT p.*, s.name AS subject_name, k.name AS knowledge_point_name
     FROM study_plans p
     LEFT JOIN subjects s ON s.id = p.subject_id
     LEFT JOIN knowledge_points k ON k.id = p.knowledge_point_id
     WHERE p.exam_id = ? AND p.date = ?
     ORDER BY p.sort_order, p.created_at`,
    [examId, today],
  )
}

export async function updatePlanStatus(id: string, status: PlanStatus): Promise<void> {
  // 标记完成时，若未填实际时长则用计划时长填充
  if (status === 'completed') {
    const rows = await query<{ planned_duration: number | null; actual_duration: number | null }>(
      'SELECT planned_duration, actual_duration FROM study_plans WHERE id = ?',
      [id],
    )
    const r = rows[0]
    if (r && (r.actual_duration === null || r.actual_duration === 0) && r.planned_duration) {
      await update('study_plans', id, { status, actual_duration: r.planned_duration })
      return
    }
  }
  await update('study_plans', id, { status })
}

export interface PlanUpdateInput {
  date?: string
  planned_duration?: number | null
  planned_tasks?: string | null
  actual_duration?: number | null
  status?: PlanStatus
}

/** 手动调整任务（标记 user_modified=1） */
export async function updatePlan(id: string, input: PlanUpdateInput): Promise<void> {
  await update('study_plans', id, { ...input, user_modified: 1 })
}

/** 拖拽排序：按 ids 顺序更新 sort_order */
export async function reorderPlans(ids: string[]): Promise<void> {
  for (let i = 0; i < ids.length; i++) {
    await update('study_plans', ids[i], { sort_order: i, user_modified: 1 })
  }
}

export interface CompareStats {
  totalPlanned: number
  totalActual: number
  completionRate: number
  deviationDays: number
  bySubject: { subject: string; planned: number; actual: number }[]
  dailyCompletion: { date: string; rate: number }[]
  deviations: { type: 'subject'; name: string; planned: number; actual: number; rate: number }[]
}

export async function getCompareStats(examId: string): Promise<CompareStats> {
  const plans = await query<StudyPlan & { subject_name: string | null }>(
    `SELECT p.*, s.name AS subject_name FROM study_plans p
     LEFT JOIN subjects s ON s.id = p.subject_id
     WHERE p.exam_id = ? ORDER BY p.date`,
    [examId],
  )
  const totalPlanned = plans.reduce((a, p) => a + (p.planned_duration ?? 0), 0)
  const totalActual = plans.reduce((a, p) => a + (p.actual_duration ?? 0), 0)
  const completed = plans.filter((p) => p.status === 'completed').length
  const completionRate = plans.length ? Math.round((completed / plans.length) * 100) : 0

  const subjMap = new Map<string, { subject: string; planned: number; actual: number }>()
  plans.forEach((p) => {
    const e = subjMap.get(p.subject_id) ?? {
      subject: p.subject_name ?? '未知',
      planned: 0,
      actual: 0,
    }
    e.planned += p.planned_duration ?? 0
    e.actual += p.actual_duration ?? 0
    subjMap.set(p.subject_id, e)
  })
  const bySubject = [...subjMap.values()]

  const dateMap = new Map<string, { date: string; total: number; completed: number }>()
  plans.forEach((p) => {
    const e = dateMap.get(p.date) ?? { date: p.date, total: 0, completed: 0 }
    e.total++
    if (p.status === 'completed') e.completed++
    dateMap.set(p.date, e)
  })
  const dailyCompletion = [...dateMap.values()]
    .sort((a, b) => a.date.localeCompare(b.date))
    .map((d) => ({ date: d.date, rate: d.total ? Math.round((d.completed / d.total) * 100) : 0 }))

  // 偏差：实际 < 计划 * 80%（偏差 > 20%）
  const deviations = bySubject
    .filter((s) => s.planned > 0 && s.actual < s.planned * 0.8)
    .map((s) => ({
      type: 'subject' as const,
      name: s.subject,
      planned: s.planned,
      actual: s.actual,
      rate: Math.round((s.actual / s.planned) * 100),
    }))
  const deviationDays = dailyCompletion.filter((d) => d.rate < 80).length

  return {
    totalPlanned,
    totalActual,
    completionRate,
    deviationDays,
    bySubject,
    dailyCompletion,
    deviations,
  }
}
