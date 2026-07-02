// AI Agent 工作流引擎。
// daily/weekly/phase 分析：聚合(analyzer) → AI(prompt+callLLM) 或本地降级 → 去重写入 ai_analyses。
// 启动补偿：runPendingAnalyses 对比时间戳补跑错过的。AI 建议先展示，用户确认后 applied=1。

import { execute, getSetting, insert, query, setSetting, update } from './db'
import { callLLM, parseJsonResponse } from './llm-adapter'
import { diagnosticPrompt, predictionPrompt } from './prompts'
import {
  aggregateDaily,
  aggregateWeekly,
  localDailyAnalysis,
  localPrediction,
  localWeeklyAnalysis,
  type AnalysisContent,
} from './analyzer'
import { businessToday } from './record-service'
import type { LLMConfig, Subject } from '@/types'

function fmtDate(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(
    d.getDate(),
  ).padStart(2, '0')}`
}

/** 本周一日期（用作 weekly 去重 key） */
function weekMonday(d = new Date()): string {
  const x = new Date(d)
  x.setHours(0, 0, 0, 0)
  const day = x.getDay()
  x.setDate(x.getDate() - (day === 0 ? 6 : day - 1))
  return fmtDate(x)
}

/** LLM 是否已配置且可用（仅检查配置，不 ping） */
export function checkLLMAvailable(llmConfig: LLMConfig | null): boolean {
  if (!llmConfig || !llmConfig.baseUrl || !llmConfig.model) return false
  if (llmConfig.provider === 'ollama') return true
  return !!llmConfig.apiKey
}

async function getTargets(examId: string): Promise<{ subject: string; target: number }[]> {
  const subjects = await query<Subject>('SELECT * FROM subjects WHERE exam_id = ?', [examId])
  return subjects.map((s) => ({ subject: s.name, target: s.target_score ?? 100 }))
}

function diagnosticResultToContent(parsed: any, title: string): AnalysisContent {
  const wp = parsed.weak_points ?? []
  const eff = parsed.efficiency_analysis ?? {}
  const md = [
    `### ${title}`,
    '',
    parsed.summary ?? '',
    '',
    '**薄弱点：**',
    ...wp.map(
      (w: any) =>
        `- ${w.subject ?? ''}${w.knowledge_point ? ' · ' + w.knowledge_point : ''}：${
          w.evidence ?? ''
        }（${w.severity ?? ''}）`,
    ),
    '',
    `**效率分析：** 趋势${eff.trend ?? '—'}${
      eff.avg_duration_per_day != null ? `，日均 ${eff.avg_duration_per_day} 分钟` : ''
    }`,
  ].join('\n')
  return {
    content: md,
    suggestions: (parsed.recommendations ?? []) as AnalysisContent['suggestions'],
    scores_prediction: null,
  }
}

/** 去重写入：删当日同 type 旧记录，再插入 */
async function saveAnalysis(
  _examId: string,
  type: 'daily' | 'weekly' | 'phase' | 'prediction' | 'adjustment',
  content: AnalysisContent,
  generatedBy: 'ai' | 'local',
  dedupKey: string,
  periodStart?: string,
  periodEnd?: string,
): Promise<string> {
  // 去重：同 dedupKey（当日/当周）同 type 只留最新
  await execute('DELETE FROM ai_analyses WHERE analysis_type = ? AND period_start = ?', [
    type,
    dedupKey,
  ])
  return insert('ai_analyses', {
    analysis_type: type,
    period_start: periodStart ?? dedupKey,
    period_end: periodEnd ?? dedupKey,
    subjects_analyzed: null,
    content: content.content,
    suggestions: JSON.stringify(content.suggestions),
    scores_prediction: content.scores_prediction ? JSON.stringify(content.scores_prediction) : null,
    generated_by: generatedBy,
    user_confirmed: 0,
    applied: 0,
  })
}

/** 每日分析 */
export async function runDailyAnalysis(examId: string, llmConfig: LLMConfig | null): Promise<void> {
  const today = businessToday()
  const agg = await aggregateDaily(examId, today)
  let content: AnalysisContent
  let generatedBy: 'ai' | 'local'
  if (checkLLMAvailable(llmConfig)) {
    try {
      const { system, user } = diagnosticPrompt({
        studyData: agg.summaryText,
        period: 'daily',
      })
      const resp = await callLLM(
        llmConfig!,
        [
          { role: 'system', content: system },
          { role: 'user', content: user },
        ],
        { timeoutMs: 60000 },
      )
      content = diagnosticResultToContent(parseJsonResponse(resp.content), '🤖 AI 每日诊断')
      generatedBy = 'ai'
    } catch (e) {
      content = localDailyAnalysis(agg.bySubject, today)
      content.content =
        `⚠️ AI 诊断失败（${(e as Error).message}），已降级本地分析。\n\n` + content.content
      generatedBy = 'local'
    }
  } else {
    content = localDailyAnalysis(agg.bySubject, today)
    generatedBy = 'local'
  }
  await saveAnalysis(examId, 'daily', content, generatedBy, today, today, today)
  await setSetting('last_daily_at', today)
}

/** 每周分析（诊断 + 预测） */
export async function runWeeklyAnalysis(
  examId: string,
  llmConfig: LLMConfig | null,
): Promise<void> {
  const weekKey = weekMonday()
  const agg = await aggregateWeekly(examId)
  const targets = await getTargets(examId)
  let content: AnalysisContent
  let generatedBy: 'ai' | 'local'
  if (checkLLMAvailable(llmConfig)) {
    try {
      const { system, user } = diagnosticPrompt({
        studyData: agg.summaryText,
        period: 'weekly',
      })
      const resp = await callLLM(
        llmConfig!,
        [
          { role: 'system', content: system },
          { role: 'user', content: user },
        ],
        { timeoutMs: 60000 },
      )
      content = diagnosticResultToContent(parseJsonResponse(resp.content), '🤖 AI 周报诊断')
      generatedBy = 'ai'
      // 预测
      try {
        const pp = predictionPrompt({
          studySummary: agg.summaryText,
          targets: JSON.stringify(targets),
        })
        const predResp = await callLLM(
          llmConfig!,
          [
            { role: 'system', content: pp.system },
            { role: 'user', content: pp.user },
          ],
          { timeoutMs: 60000 },
        )
        content.scores_prediction = parseJsonResponse(predResp.content)
      } catch (e) {
        content.scores_prediction = localPrediction(agg.bySubject, targets)
      }
    } catch (e) {
      content = localWeeklyAnalysis(agg)
      content.scores_prediction = localPrediction(agg.bySubject, targets)
      content.content =
        `⚠️ AI 诊断失败（${(e as Error).message}），已降级本地分析。\n\n` + content.content
      generatedBy = 'local'
    }
  } else {
    content = localWeeklyAnalysis(agg)
    content.scores_prediction = localPrediction(agg.bySubject, targets)
    generatedBy = 'local'
  }
  await saveAnalysis(examId, 'weekly', content, generatedBy, weekKey, weekKey, fmtDate(new Date()))
  await setSetting('last_weekly_at', weekKey)
}

/** 阶段分析（全量评估 + 预测） */
export async function runPhaseAnalysis(examId: string, llmConfig: LLMConfig | null): Promise<void> {
  const agg = await aggregateWeekly(examId)
  const targets = await getTargets(examId)
  let content: AnalysisContent
  let generatedBy: 'ai' | 'local'
  if (checkLLMAvailable(llmConfig)) {
    try {
      const pp = predictionPrompt({
        studySummary: agg.summaryText,
        targets: JSON.stringify(targets),
      })
      const resp = await callLLM(
        llmConfig!,
        [
          { role: 'system', content: pp.system },
          { role: 'user', content: pp.user },
        ],
        { timeoutMs: 60000 },
      )
      const pred = parseJsonResponse(resp.content)
      content = {
        content: `### 🤖 AI 阶段预测\n\n${pred.overall_assessment ?? ''}`,
        suggestions: (pred.urgent_actions ?? []).map((a: string) => ({
          action: a,
          reason: '',
          priority: 1,
        })),
        scores_prediction: pred,
      }
      generatedBy = 'ai'
    } catch (e) {
      content = {
        content: `### 📊 本地阶段预测（非 AI）\n\n⚠️ ${(e as Error).message}，已降级。`,
        suggestions: [],
        scores_prediction: localPrediction(agg.bySubject, targets),
      }
      generatedBy = 'local'
    }
  } else {
    content = {
      content: '### 📊 本地阶段预测（非 AI）',
      suggestions: [],
      scores_prediction: localPrediction(agg.bySubject, targets),
    }
    generatedBy = 'local'
  }
  await saveAnalysis(examId, 'phase', content, generatedBy, fmtDate(new Date()))
}

/** 启动补偿：跨过应触发点则补跑（桌面应用无后台进程） */
export async function runPendingAnalyses(
  examId: string,
  llmConfig: LLMConfig | null,
): Promise<void> {
  const today = businessToday()
  const lastDaily = await getSetting('last_daily_at')
  if (lastDaily !== today) {
    try {
      await runDailyAnalysis(examId, llmConfig)
    } catch (e) {
      console.warn('补跑 daily 失败', e)
    }
  }
  const weekKey = weekMonday()
  const lastWeekly = await getSetting('last_weekly_at')
  if (lastWeekly !== weekKey) {
    try {
      await runWeeklyAnalysis(examId, llmConfig)
    } catch (e) {
      console.warn('补跑 weekly 失败', e)
    }
  }
}

// ---- 用户确认/拒绝/应用 ----

export async function confirmSuggestion(id: string): Promise<void> {
  await update('ai_analyses', id, { user_confirmed: 1 })
}

export async function rejectSuggestion(id: string): Promise<void> {
  await update('ai_analyses', id, { user_confirmed: 2 })
}

/**
 * 应用建议：标记 applied=1。
 * 注：Phase 4 仅标记应用状态（视觉反馈）；从建议文本自动改计划/科目数据涉及语义解析，
 * 暂不实现，由用户参照建议手动调整（Phase 5 可增强）。
 */
export async function applySuggestion(id: string): Promise<void> {
  await update('ai_analyses', id, {
    user_confirmed: 1,
    applied: 1,
    applied_at: new Date().toISOString(),
  })
}
