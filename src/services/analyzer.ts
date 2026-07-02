// 本地数据分析（聚合 + Token 防爆 + 降级诊断/预测）。
// 聚合后的摘要 ≤2000 字符，供 AI prompt；LLM 不可用时用本地统计算法降级生成分析（generated_by='local'）。

import { query } from './db'
import type { KnowledgePoint, StudyRecord, Subject } from '@/types'

export interface Recommendation {
  action: string
  reason: string
  priority: number // 1-5
}

export interface PredictionResult {
  predictions: {
    subject: string
    predicted_score: number
    target_score: number
    gap: number
    confidence: string
    trend: string
    risk_level: string
  }[]
  alerts: { subject: string; issue: string; severity: string }[]
  overall_assessment: string
  urgent_actions: string[]
}

export interface AnalysisContent {
  content: string // Markdown
  suggestions: Recommendation[]
  scores_prediction: PredictionResult | null
}

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
    const firstMin = rs
      .filter((r) => r.date < midDate)
      .reduce((a, r) => a + r.duration_min, 0)
    const lastMin = rs
      .filter((r) => r.date >= midDate)
      .reduce((a, r) => a + r.duration_min, 0)
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
async function getKpMastery(examId: string): Promise<
  { kp: string; subject: string; mastery: number }[]
> {
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
async function getPlanCompletion(examId: string, days: number): Promise<
  { period: string; rate: number }[]
> {
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

// ============ 本地降级（LLM 不可用时） ============

function buildMarkdown(
  summary: string,
  weakPoints: { subject: string; kp?: string; evidence: string; severity: string }[],
  efficiency: { bestSession?: string; avgPerDay?: number; trend: string },
  generatedBy: 'local',
): string {
  const md = [
    `### ${generatedBy === 'local' ? '📊 本地分析（非 AI）' : '📊 学习诊断'}`,
    '',
    summary,
    '',
    '**薄弱点：**',
    ...weakPoints.map(
      (w) =>
        `- ${w.subject}${w.kp ? ' · ' + w.kp : ''}：${w.evidence}（严重度：${w.severity}）`,
    ),
    '',
    `**效率分析：** 趋势${efficiency.trend}${
      efficiency.avgPerDay != null ? `，日均 ${efficiency.avgPerDay} 分钟` : ''
    }`,
  ].join('\n')
  return md
}

export function localWeeklyAnalysis(agg: WeeklyAggregation): AnalysisContent {
  const weak: { subject: string; kp?: string; evidence: string; severity: string }[] = [
    ...agg.bySubject
      .filter((s) => s.avgCorrectRate < 70 && s.records > 0)
      .map((s) => ({
        subject: s.subject,
        evidence: `正确率 ${s.avgCorrectRate}%（${s.totalMin}分钟）`,
        severity: s.avgCorrectRate < 50 ? '高' : '中',
      })),
    ...agg.kpMastery
      .filter((k) => k.mastery <= 2)
      .slice(0, 5)
      .map((k) => ({
        subject: k.subject,
        kp: k.kp,
        evidence: `掌握度 ${k.mastery}/5`,
        severity: k.mastery <= 1 ? '高' : '中',
      })),
  ]
  const totalMin = agg.bySubject.reduce((a, s) => a + s.totalMin, 0)
  const avgPerDay = Math.round(totalMin / 28)
  const trend =
    agg.bySubject.some((s) => s.trend === '上升')
      ? '上升'
      : agg.bySubject.some((s) => s.trend === '下降')
      ? '下降'
      : '平稳'
  const summary = `本周（28天）共学习 ${totalMin} 分钟，日均 ${avgPerDay} 分钟，整体趋势${trend}。`
  const suggestions: Recommendation[] = weak.slice(0, 3).map((w, i) => ({
    action: w.kp ? `重点复习「${w.kp}」` : `加强「${w.subject}」练习`,
    reason: w.evidence,
    priority: w.severity === '高' ? 1 : 2 + i,
  }))
  return {
    content: buildMarkdown(summary, weak, { avgPerDay, trend }, 'local'),
    suggestions,
    scores_prediction: null,
  }
}

export function localDailyAnalysis(
  bySubject: SubjectStat[],
  date: string,
): AnalysisContent {
  const totalMin = bySubject.reduce((a, s) => a + s.totalMin, 0)
  const weak = bySubject
    .filter((s) => s.records > 0 && s.avgCorrectRate < 70)
    .map((s) => ({
      subject: s.subject,
      evidence: `正确率 ${s.avgCorrectRate}%`,
      severity: s.avgCorrectRate < 50 ? '高' : '中',
    }))
  const summary = `${date} 共学习 ${totalMin} 分钟，涉及 ${bySubject.filter((s) => s.records > 0).length} 个科目。`
  return {
    content: buildMarkdown(
      summary,
      weak,
      { trend: '平稳' },
      'local',
    ),
    suggestions: weak.slice(0, 2).map((w) => ({
      action: `复盘「${w.subject}」错题`,
      reason: w.evidence,
      priority: w.severity === '高' ? 1 : 2,
    })),
    scores_prediction: null,
  }
}

export function localPrediction(
  bySubject: SubjectStat[],
  targets: { subject: string; target: number }[],
): PredictionResult {
  const targetMap = new Map(targets.map((t) => [t.subject, t.target]))
  const predictions = bySubject
    .filter((s) => s.records > 0)
    .map((s) => {
      const target = targetMap.get(s.subject) ?? 100
      // 粗略线性外推：预测分 = 目标 × 正确率%
      const predicted = Math.round(target * (s.avgCorrectRate / 100))
      const gap = predicted - target
      return {
        subject: s.subject,
        predicted_score: predicted,
        target_score: target,
        gap,
        confidence: '低（非AI）',
        trend: s.trend,
        risk_level: gap < -target * 0.2 ? '高风险' : gap < 0 ? '中风险' : '低风险',
      }
    })
  const alerts = predictions
    .filter((p) => p.risk_level !== '低风险')
    .map((p) => ({
      subject: p.subject,
      issue: `预估 ${p.predicted_score} 距目标 ${p.target_score} 差 ${Math.abs(p.gap)}`,
      severity: p.risk_level === '高风险' ? 'critical' : 'warning',
    }))
  return {
    predictions,
    alerts,
    overall_assessment: `基于正确率的线性外推（置信度低，非 AI）。${predictions.length} 科预估，${alerts.length} 科存在风险。`,
    urgent_actions: alerts.slice(0, 2).map((a) => `优先补强「${a.subject}」`),
  }
}
