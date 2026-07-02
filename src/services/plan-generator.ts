// 学习计划生成（本地算法 + AI Agent）。
// 本地算法作为 LLM 不可用时的降级；AI 调用 llm-adapter，失败自动降级本地。
// 已有计划处理：重新生成保留 actual_*（已记录完成），仅替换未来日期计划。

import { execute, getById, getSetting, insert, query } from './db'
import { callLLM, parseJsonResponse } from './llm-adapter'
import { planGenerationPrompt } from './prompts'
import type { ParsedPlan, ParsedPlanPhase, PlanPromptData } from './prompts'
import { businessToday } from './record-service'
import type { Exam, KnowledgePoint, Subject } from '@/types'

export interface PlanOptions {
  dailyHours?: number
}

export interface PlanGenerationResult {
  totalDays: number
  phases: ParsedPlanPhase[]
  planCount: number
  generatedBy: 'ai' | 'local'
  message: string
}

function fmtDate(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(
    d.getDate(),
  ).padStart(2, '0')}`
}

/** 收集考试/科目/知识点(含掌握度/权重/难度)为 LLM Prompt 数据 */
export async function preparePlanPromptData(examId: string): Promise<PlanPromptData> {
  const exam = await getById<Exam>('exams', examId)
  if (!exam) throw new Error('考试不存在')
  const subjects = await query<Subject>(
    'SELECT * FROM subjects WHERE exam_id = ? ORDER BY sort_order, created_at',
    [examId],
  )
  const subjectIds = subjects.map((s) => s.id)
  const kps: KnowledgePoint[] = subjectIds.length
    ? await query<KnowledgePoint>(
        `SELECT * FROM knowledge_points WHERE subject_id IN (${subjectIds
          .map(() => '?')
          .join(',')}) ORDER BY sort_order, created_at`,
        subjectIds,
      )
    : []
  const today = businessToday()
  const daysLeft = Math.ceil(
    (new Date(exam.exam_date + 'T00:00:00').getTime() -
      new Date(today + 'T00:00:00').getTime()) /
      86400000,
  )
  const dailyHours = Number(await getSetting('daily_study_hours')) || 6
  return {
    exam: { name: exam.name, exam_date: exam.exam_date, total_score: exam.total_score },
    subjects: subjects.map((s) => ({
      id: s.id,
      name: s.name,
      target_score: s.target_score,
      current_level: s.current_level,
      weight: s.weight,
    })),
    knowledgePoints: kps.map((k) => ({
      id: k.id,
      subject_id: k.subject_id,
      name: k.name,
      weight: k.weight,
      difficulty_level: k.difficulty_level,
      current_mastery: k.current_mastery,
    })),
    dailyHours,
    examDate: exam.exam_date,
    daysLeft,
  }
}

/** 写入生成的计划（保留 actual_*；替换未来日期；今日已保留 actual 的同科目跳过） */
export async function applyGeneratedPlan(
  examId: string,
  parsed: ParsedPlan,
  generatedBy: 'ai' | 'local',
): Promise<number> {
  const today = businessToday()
  // 删除未来计划(date>today) + 今日无 actual 的计划
  await execute(
    `DELETE FROM study_plans WHERE exam_id = ? AND (date > ? OR (date = ? AND (actual_duration IS NULL OR actual_duration = 0)))`,
    [examId, today, today],
  )
  // 今日已保留 actual 的科目（跳过重复插入）
  const kept = await query<{ subject_id: string }>(
    'SELECT DISTINCT subject_id FROM study_plans WHERE exam_id = ? AND date = ? AND actual_duration IS NOT NULL AND actual_duration > 0',
    [examId, today],
  )
  const keptTodaySubjects = new Set(kept.map((r) => r.subject_id))

  let count = 0
  for (const day of parsed.dailyPlans) {
    let order = 0
    for (const t of day.tasks) {
      if (day.date === today && keptTodaySubjects.has(t.subject_id)) continue
      await insert('study_plans', {
        exam_id: examId,
        subject_id: t.subject_id,
        knowledge_point_id: t.knowledge_point_id ?? null,
        date: day.date,
        planned_tasks: t.task,
        planned_duration: t.duration,
        actual_duration: null,
        actual_tasks: null,
        status: 'pending',
        generated_by: generatedBy,
        ai_suggestion: null,
        user_modified: 0,
        sort_order: order++,
      })
      count++
    }
  }
  return count
}

/** 本地算法生成计划（降级用） */
export async function generateLocalPlan(
  examId: string,
  options?: PlanOptions,
): Promise<PlanGenerationResult> {
  const data = await preparePlanPromptData(examId)
  if (data.daysLeft <= 0)
    throw new Error('考试日期已过或就是今天，无法生成计划，请修改考试日期')
  const dailyHours = options?.dailyHours ?? data.dailyHours

  // 阶段划分（<7 天则压缩为冲刺）
  let baseDays: number, strenDays: number, sprintDays: number
  if (data.daysLeft < 7) {
    baseDays = 0
    strenDays = 0
    sprintDays = data.daysLeft
  } else {
    baseDays = Math.max(1, Math.floor(data.daysLeft * 0.5))
    strenDays = Math.max(1, Math.floor(data.daysLeft * 0.3))
    sprintDays = Math.max(1, data.daysLeft - baseDays - strenDays)
  }

  // 日期列表（今天 → 考试前一天）
  const days: string[] = []
  const start = new Date()
  start.setHours(0, 0, 0, 0)
  for (let i = 0; i < data.daysLeft; i++) {
    const d = new Date(start)
    d.setDate(d.getDate() + i)
    days.push(fmtDate(d))
  }

  // 科目权重：水平低 + 权重高 → 分配更多
  const subjWeights = data.subjects.map((s) => ({
    ...s,
    gap: (6 - s.current_level) * s.weight,
  }))
  const totalGap = subjWeights.reduce((a, s) => a + s.gap, 0) || 1

  // 各科目知识点（低掌握度优先，轮转）
  const kpsBySubject = new Map<string, typeof data.knowledgePoints>()
  for (const s of data.subjects) {
    kpsBySubject.set(
      s.id,
      data.knowledgePoints
        .filter((k) => k.subject_id === s.id)
        .sort((a, b) => a.current_mastery - b.current_mastery),
    )
  }
  const kpIdx = new Map<string, number>()
  for (const s of data.subjects) kpIdx.set(s.id, 0)

  const dailyPlans = days.map((date) => {
    const dow = new Date(date + 'T00:00:00').getDay()
    const hours = dow === 0 ? dailyHours / 2 : dailyHours // 周日半天复习/缓冲
    const totalMin = Math.round(hours * 60)
    const tasks = [] as { subject_id: string; knowledge_point_id: string | null; task: string; duration: number }[]
    for (const s of subjWeights) {
      const min = Math.round(totalMin * (s.gap / totalGap))
      if (min < 10) continue
      const kps = kpsBySubject.get(s.id) ?? []
      let kpId: string | null = null
      let taskName = `${s.name} 综合复习`
      if (kps.length) {
        const idx = kpIdx.get(s.id)! % kps.length
        const kp = kps[idx]
        kpId = kp.id
        taskName = dow === 0 ? `复习：${kp.name}` : `学习：${kp.name}`
        kpIdx.set(s.id, idx + 1)
      }
      tasks.push({ subject_id: s.id, knowledge_point_id: kpId, task: taskName, duration: min })
    }
    return { date, tasks }
  })

  const phases: ParsedPlanPhase[] = []
  if (baseDays > 0)
    phases.push({ name: '基础期', start: days[0], end: days[baseDays - 1] })
  if (strenDays > 0)
    phases.push({
      name: '强化期',
      start: days[baseDays],
      end: days[baseDays + strenDays - 1],
    })
  if (sprintDays > 0)
    phases.push({
      name: '冲刺期',
      start: days[baseDays + strenDays],
      end: days[days.length - 1],
    })

  const parsed: ParsedPlan = { phases, dailyPlans }
  const planCount = await applyGeneratedPlan(examId, parsed, 'local')
  return {
    totalDays: data.daysLeft,
    phases,
    planCount,
    generatedBy: 'local',
    message: '已生成基础计划（本地算法）。配置 AI 后可获得更精准的个性化计划。',
  }
}

/** AI 生成计划；LLM 不可用或失败 → 降级本地 */
export async function generateAIPlan(
  examId: string,
  llmConfig: import('@/types').LLMConfig | null,
  options?: PlanOptions,
): Promise<PlanGenerationResult> {
  if (!llmConfig || !llmConfig.apiKey || !llmConfig.baseUrl || !llmConfig.model) {
    return generateLocalPlan(examId, options)
  }
  try {
    const data = await preparePlanPromptData(examId)
    if (data.daysLeft <= 0)
      throw new Error('考试日期已过或就是今天，无法生成计划')
    const { system, user } = planGenerationPrompt(data)
    const resp = await callLLM(
      llmConfig,
      [
        { role: 'system', content: system },
        { role: 'user', content: user },
      ],
      { timeoutMs: 60000 },
    )
    const parsed = parseJsonResponse<ParsedPlan>(resp.content)
    if (!parsed.dailyPlans || !Array.isArray(parsed.dailyPlans))
      throw new Error('AI 返回的 JSON 缺少 dailyPlans 字段')
    // 过滤掉引用了非法 subject_id 的任务
    const validSubjectIds = new Set(data.subjects.map((s) => s.id))
    parsed.dailyPlans.forEach((d) => {
      d.tasks = d.tasks.filter((t) => validSubjectIds.has(t.subject_id))
    })
    const planCount = await applyGeneratedPlan(examId, parsed, 'ai')
    return {
      totalDays: data.daysLeft,
      phases: parsed.phases ?? [],
      planCount,
      generatedBy: 'ai',
      message: 'AI 已生成个性化复习计划。',
    }
  } catch (e) {
    const local = await generateLocalPlan(examId, options)
    return {
      ...local,
      message: `AI 调用失败（${(e as Error).message}），已降级生成本地基础计划。`,
    }
  }
}
