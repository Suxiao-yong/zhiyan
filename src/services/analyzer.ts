// 本地数据分析事实层（Task 9/14 收敛）：只保留确定性统计聚合，
// 不再包含任何 AI 文案类型或 prompt 拼接。对话与简报文案由 Agent 侧
// ContextSnapshot / Daily Brief（Rust）负责。

import { query } from './db'
import type { KnowledgePoint, StudyRecord, Subject } from '@/types'

export interface SubjectStat {
  subjectId: string
  subject: string
  totalMin: number
  avgCorrectRate: number // 0-100
  trend: '上升' | '下降' | '平稳'
  records: number
}

function fmtDate(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(
    d.getDate(),
  ).padStart(2, '0')}`
}

/** 获取考试下全部科目 */
async function getExamSubjects(examId: string): Promise<Subject[]> {
  return query<Subject>('SELECT * FROM subjects WHERE exam_id = ? ORDER BY sort_order', [examId])
}

/** 聚合最近 days 天的学习记录，按科目分组 */
async function aggregateBySubject(
  examId: string,
  days: number,
): Promise<{ bySubject: SubjectStat[]; records: StudyRecord[] }> {
  const subjects = await getExamSubjects(examId)
  if (!subjects.length) return { bySubject: [], records: [] }
  const subjectIds = subjects.map((s) => s.id)
  const subjectName = new Map(subjects.map((s) => [s.id, s.name]))
  const today = new Date()
  today.setHours(0, 0, 0, 0)
  const from = new Date(today)
  from.setDate(from.getDate() - (days - 1))
  const records = await query<StudyRecord>(
    `SELECT * FROM study_records WHERE subject_id IN (${subjectIds
      .map(() => '?')
      .join(',')}) AND date >= ? ORDER BY date`,
    [...subjectIds, fmtDate(from)],
  )
  // 按科目聚合 + 前后半段对比看趋势
  const half = Math.floor(days / 2)
  const midDate = fmtDate(new Date(today.getTime() - half * 86400000))
  const stats: SubjectStat[] = subjects.map((s) => {
    const rs = records.filter((r) => r.subject_id === s.id)
    const totalMin = rs.reduce((a, r) => a + r.duration_min, 0)
    const totalQ = rs.reduce((a, r) => a + r.questions_count, 0)
    const totalC = rs.reduce((a, r) => a + r.correct_count, 0)
    const avgCorrectRate = totalQ > 0 ? Math.round((totalC / totalQ) * 100) : 0
    const firstMin = rs.filter((r) => r.date < midDate).reduce((a, r) => a + r.duration_min, 0)
    const lastMin = rs.filter((r) => r.date >= midDate).reduce((a, r) => a + r.duration_min, 0)
    const trend: SubjectStat['trend'] =
      lastMin > firstMin * 1.15 ? '上升' : lastMin < firstMin * 0.85 ? '下降' : '平稳'
    return {
      subjectId: s.id,
      subject: subjectName.get(s.id) ?? s.name,
      totalMin,
      avgCorrectRate,
      trend,
      records: rs.length,
    }
  })
  return { bySubject: stats, records }
}

/** 各知识点当前掌握度（低掌握度优先） */
async function getKpMastery(
  examId: string,
): Promise<{ kp: string; subject: string; mastery: number }[]> {
  const subjects = await getExamSubjects(examId)
  const subjectIds = subjects.map((s) => s.id)
  if (!subjectIds.length) return []
  const subjectName = new Map(subjects.map((s) => [s.id, s.name]))
  const kps = await query<KnowledgePoint>(
    `SELECT * FROM knowledge_points WHERE subject_id IN (${subjectIds
      .map(() => '?')
      .join(',')}) ORDER BY current_mastery ASC`,
    subjectIds,
  )
  return kps.map((k) => ({
    kp: k.name,
    subject: subjectName.get(k.subject_id) ?? '',
    mastery: k.current_mastery,
  }))
}

/** 计划完成率趋势（最近 N 天分两段） */
async function getPlanCompletion(
  examId: string,
  days: number,
): Promise<{ period: string; rate: number }[]> {
  const today = new Date()
  today.setHours(0, 0, 0, 0)
  const from = new Date(today)
  from.setDate(from.getDate() - (days - 1))
  const rows = await query<{ date: string; status: string }>(
    'SELECT date, status FROM study_plans WHERE exam_id = ? AND date >= ?',
    [examId, fmtDate(from)],
  )
  const half = Math.floor(days / 2)
  const midDate = fmtDate(new Date(today.getTime() - half * 86400000))
  const seg = (filter: (r: { date: string }) => boolean) => {
    const sub = rows.filter(filter)
    if (!sub.length) return 0
    return Math.round((sub.filter((r) => r.status === 'completed').length / sub.length) * 100)
  }
  return [
    { period: `前${half}天`, rate: seg((r) => r.date < midDate) },
    { period: `后${half}天`, rate: seg((r) => r.date >= midDate) },
  ]
}

export interface WeeklyAggregation {
  bySubject: SubjectStat[]
  kpMastery: { kp: string; subject: string; mastery: number }[]
  planCompletion: { period: string; rate: number }[]
  summaryText: string
}

/** 周聚合（最近 28 天）+ 摘要文本（≤2000 字符） */
export async function aggregateWeekly(examId: string): Promise<WeeklyAggregation> {
  const { bySubject } = await aggregateBySubject(examId, 28)
  const kpMastery = (await getKpMastery(examId)).slice(0, 20) // 限制条数防爆
  const planCompletion = await getPlanCompletion(examId, 28)
  const summaryText = JSON.stringify({
    period: '最近28天',
    subjects: bySubject.map((s) => ({
      name: s.subject,
      minutes: s.totalMin,
      correctRate: s.avgCorrectRate,
      trend: s.trend,
    })),
    weakKnowledgePoints: kpMastery
      .filter((k) => k.mastery <= 2)
      .map((k) => ({ kp: k.kp, subject: k.subject, mastery: k.mastery })),
    planCompletion,
  })
  return { bySubject, kpMastery, planCompletion, summaryText: summaryText.slice(0, 2000) }
}

/** 日聚合（当日） */
export async function aggregateDaily(
  examId: string,
  date: string,
): Promise<{ bySubject: SubjectStat[]; summaryText: string }> {
  const subjects = await getExamSubjects(examId)
  const subjectIds = subjects.map((s) => s.id)
  const subjectName = new Map(subjects.map((s) => [s.id, s.name]))
  const records = subjectIds.length
    ? await query<StudyRecord>(
        `SELECT * FROM study_records WHERE subject_id IN (${subjectIds
          .map(() => '?')
          .join(',')}) AND date = ?`,
        [...subjectIds, date],
      )
    : []
  const bySubject: SubjectStat[] = subjects.map((s) => {
    const rs = records.filter((r) => r.subject_id === s.id)
    const totalMin = rs.reduce((a, r) => a + r.duration_min, 0)
    const totalQ = rs.reduce((a, r) => a + r.questions_count, 0)
    const totalC = rs.reduce((a, r) => a + r.correct_count, 0)
    return {
      subjectId: s.id,
      subject: subjectName.get(s.id) ?? s.name,
      totalMin,
      avgCorrectRate: totalQ > 0 ? Math.round((totalC / totalQ) * 100) : 0,
      trend: '平稳',
      records: rs.length,
    }
  })
  const summaryText = JSON.stringify({
    date,
    subjects: bySubject.map((s) => ({
      name: s.subject,
      minutes: s.totalMin,
      correctRate: s.avgCorrectRate,
    })),
  })
  return { bySubject, summaryText: summaryText.slice(0, 2000) }
}
