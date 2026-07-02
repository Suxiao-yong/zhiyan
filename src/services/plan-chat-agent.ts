// AI 规划聊天 Agent：function calling 循环（LLM 配 search_web 工具）+ 多轮讨论 + 定稿落库。
// 流程：LLM 自动调 search_web 研究考试 → 提建议 → 与用户讨论 → 输出最终计划 JSON → 建科目/知识点 + 展开写 study_plans。

import { callLLM, callLLMWithTools, parseJsonResponse } from './llm-adapter'
import { searchWeb } from './search'
import { applyGeneratedPlan } from './plan-generator'
import { createKnowledgePoint, createSubject } from './exam-service'
import type { LLMConfig } from '@/types'
import type { ParsedPlanDay, ParsedPlanPhase, ParsedPlanTask } from './prompts'

// search_web 工具定义（OpenAI function calling 格式）
const SEARCH_TOOL = {
  type: 'function',
  function: {
    name: 'search_web',
    description:
      '搜索互联网获取实时信息（如考试大纲、重点知识点、备考策略、推荐教材）。当需要了解某考试的具体内容时调用。',
    parameters: {
      type: 'object',
      properties: {
        query: { type: 'string', description: '搜索关键词' },
      },
      required: ['query'],
    },
  },
}

export interface ChatTurnResult {
  messages: any[] // 追加 assistant + tool 结果后的完整消息列表
  content: string // 给用户看的 assistant 文本
  searching: boolean // 这一轮是否触发了搜索
}

/** 执行一轮对话：LLM 可能调 search_web → app 执行 → 再调 LLM 基于结果给出文本 */
export async function runChatTurn(messages: any[], llmConfig: LLMConfig): Promise<ChatTurnResult> {
  const resp = await callLLMWithTools(llmConfig, messages, [SEARCH_TOOL], {
    timeoutMs: 60000,
    retries: 1,
  })
  const newMessages = [
    ...messages,
    {
      role: 'assistant',
      content: resp.content,
      ...(resp.tool_calls.length ? { tool_calls: resp.tool_calls } : {}),
    },
  ]
  let searching = false
  // 执行 tool_calls
  for (const tc of resp.tool_calls) {
    if (tc.function.name === 'search_web') {
      searching = true
      let result: string
      try {
        const args = JSON.parse(tc.function.arguments)
        result = await searchWeb(args.query)
      } catch (e) {
        result = `搜索失败：${(e as Error).message}`
      }
      newMessages.push({ role: 'tool', tool_call_id: tc.id, content: result })
    }
  }
  // 若有 tool 调用，再调一轮让 LLM 基于搜索结果产出文本
  if (resp.tool_calls.length) {
    const next = await callLLMWithTools(llmConfig, newMessages, [SEARCH_TOOL], {
      timeoutMs: 60000,
      retries: 1,
    })
    newMessages.push({ role: 'assistant', content: next.content })
    return { messages: newMessages, content: next.content, searching }
  }
  return { messages: newMessages, content: resp.content, searching: false }
}

function fmtDate(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(
    d.getDate(),
  ).padStart(2, '0')}`
}

/** 把 AI 的阶段策略展开为逐日计划（按科目每周时长分配，周日半天，知识点轮转） */
function expandStrategy(
  phases: any[],
  subjects: { id: string; name: string; kps: { id: string; name: string }[] }[],
): { phases: ParsedPlanPhase[]; dailyPlans: ParsedPlanDay[] } {
  const dailyPlans: ParsedPlanDay[] = []
  const kpIdx = new Map<string, number>()
  const ppPhases: ParsedPlanPhase[] = []
  for (const phase of phases) {
    if (!phase.start || !phase.end) continue
    ppPhases.push({ name: phase.name, start: phase.start, end: phase.end })
    const start = new Date(phase.start + 'T00:00:00')
    const end = new Date(phase.end + 'T00:00:00')
    if (isNaN(start.getTime()) || isNaN(end.getTime())) continue
    const weeklyHours: Record<string, number> =
      phase.weekly_hours ?? phase.subject_weekly_hours ?? {}
    for (let d = new Date(start); d <= end; d.setDate(d.getDate() + 1)) {
      const ds = fmtDate(d)
      const isSunday = d.getDay() === 0
      const tasks: ParsedPlanTask[] = []
      for (const s of subjects) {
        const hours = weeklyHours[s.name] ?? 3
        const dailyHours = isSunday ? hours / 14 : hours / 7
        const min = Math.round(dailyHours * 60)
        if (min < 10) continue
        let kpId: string | null = null
        let taskName = `${s.name} 综合复习`
        if (s.kps.length) {
          const idx = (kpIdx.get(s.id) ?? 0) % s.kps.length
          kpId = s.kps[idx].id
          taskName = isSunday ? `复习：${s.kps[idx].name}` : `学习：${s.kps[idx].name}`
          kpIdx.set(s.id, (kpIdx.get(s.id) ?? 0) + 1)
        }
        tasks.push({ subject_id: s.id, knowledge_point_id: kpId, task: taskName, duration: min })
      }
      if (tasks.length) dailyPlans.push({ date: ds, tasks })
    }
  }
  return { phases: ppPhases, dailyPlans }
}

/**
 * 定稿：让 AI 输出最终计划 JSON → 建科目/知识点 → 展开写 study_plans。
 * 返回创建的科目数与计划任务数。
 */
export async function finalizePlan(
  examId: string,
  messages: any[],
  llmConfig: LLMConfig,
): Promise<{ subjectCount: number; planCount: number }> {
  const finalMessages = [
    ...messages,
    {
      role: 'user',
      content:
        '请基于以上讨论，输出最终学习计划 JSON（不要加任何额外文字或 Markdown 标记）。格式：\n{"subjects":[{"name":"科目名","target_score":数字,"current_level":1到5,"weight":数字,"knowledge_points":[{"name":"知识点名","mastery":1到5}]}],"phases":[{"name":"基础期","start":"YYYY-MM-DD","end":"YYYY-MM-DD","weekly_hours":{"科目名":每周小时数}}]}\n要求：phases 覆盖从今天到考试日期；weekly_hours 的 key 用科目名。',
    },
  ]
  const resp = await callLLM(llmConfig, finalMessages, { timeoutMs: 60000, retries: 1 })
  const plan = parseJsonResponse<any>(resp.content)
  if (!Array.isArray(plan.subjects)) throw new Error('AI 未返回有效的 subjects')

  // 创建科目 + 知识点
  const created: { id: string; name: string; kps: { id: string; name: string }[] }[] = []
  for (const s of plan.subjects) {
    const subj = await createSubject({
      exam_id: examId,
      name: String(s.name),
      target_score: s.target_score ?? null,
      current_level: Number(s.current_level) || 3,
      weight: Number(s.weight) || 1,
    })
    const kps: { id: string; name: string }[] = []
    for (const k of s.knowledge_points ?? []) {
      const kp = await createKnowledgePoint({
        subject_id: subj.id,
        name: String(k.name),
        parent_id: null,
        weight: 1,
        difficulty_level: 3,
        current_mastery: Number(k.mastery) || 3,
        chapter: null,
      })
      kps.push({ id: kp.id, name: kp.name })
    }
    created.push({ id: subj.id, name: subj.name, kps })
  }

  // 展开阶段策略为逐日 + 写 study_plans
  const { phases, dailyPlans } = expandStrategy(plan.phases ?? [], created)
  const planCount = await applyGeneratedPlan(examId, { phases, dailyPlans }, 'ai')
  return { subjectCount: created.length, planCount }
}
