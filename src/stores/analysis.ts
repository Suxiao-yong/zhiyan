// AI 分析 store（不持久化）。

import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { query } from '@/services/db'
import * as agentEngine from '@/services/agent-engine'
import type { AiAnalysis, LLMConfig } from '@/types'

export const useAnalysisStore = defineStore('analysis', () => {
  const analyses = ref<AiAnalysis[]>([])
  const currentId = ref<string | null>(null)

  const current = computed(
    () => analyses.value.find((a) => a.id === currentId.value) ?? null,
  )

  async function loadAnalyses() {
    analyses.value = await query<AiAnalysis>(
      'SELECT * FROM ai_analyses ORDER BY created_at DESC LIMIT 100',
    )
    if (currentId.value && !analyses.value.some((a) => a.id === currentId.value)) {
      currentId.value = analyses.value[0]?.id ?? null
    }
    if (!currentId.value && analyses.value.length) {
      currentId.value = analyses.value[0].id
    }
  }

  function select(id: string) {
    currentId.value = id
  }

  async function runDaily(examId: string, llmConfig: LLMConfig | null) {
    await agentEngine.runDailyAnalysis(examId, llmConfig)
    await loadAnalyses()
  }

  async function runWeekly(examId: string, llmConfig: LLMConfig | null) {
    await agentEngine.runWeeklyAnalysis(examId, llmConfig)
    await loadAnalyses()
  }

  async function runPhase(examId: string, llmConfig: LLMConfig | null) {
    await agentEngine.runPhaseAnalysis(examId, llmConfig)
    await loadAnalyses()
  }

  async function confirmSuggestion(id: string) {
    await agentEngine.confirmSuggestion(id)
    await loadAnalyses()
  }

  async function rejectSuggestion(id: string) {
    await agentEngine.rejectSuggestion(id)
    await loadAnalyses()
  }

  async function applySuggestion(id: string) {
    await agentEngine.applySuggestion(id)
    await loadAnalyses()
  }

  return {
    analyses,
    currentId,
    current,
    loadAnalyses,
    select,
    runDaily,
    runWeekly,
    runPhase,
    confirmSuggestion,
    rejectSuggestion,
    applySuggestion,
  }
})
