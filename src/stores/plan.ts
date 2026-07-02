// 学习计划 store（不持久化）。
// Phase 3 扩展：plans/今日任务/生成/对比统计。

import { defineStore } from 'pinia'
import { ref } from 'vue'
import * as planService from '@/services/plan-service'
import * as planGenerator from '@/services/plan-generator'
import type { LLMConfig, PlanStatus } from '@/types'
import type { CompareStats, PlanUpdateInput, PlanWithNames } from '@/services/plan-service'

export const usePlanStore = defineStore('plan', () => {
  const plans = ref<PlanWithNames[]>([])
  const todayTasks = ref<PlanWithNames[]>([])
  const generating = ref(false)
  const lastResult = ref<planGenerator.PlanGenerationResult | null>(null)
  const compareStats = ref<CompareStats | null>(null)

  async function loadPlansByDateRange(examId: string, from: string, to: string) {
    plans.value = await planService.getPlansByDateRange(examId, from, to)
  }
  async function loadTodayTasks(examId: string) {
    todayTasks.value = await planService.getTodayPlans(examId)
  }
  async function generatePlan(
    examId: string,
    llmConfig: LLMConfig | null,
    options?: planGenerator.PlanOptions,
  ) {
    generating.value = true
    try {
      const result = await planGenerator.generateAIPlan(examId, llmConfig, options)
      lastResult.value = result
      return result
    } finally {
      generating.value = false
    }
  }
  async function updatePlanStatus(id: string, status: PlanStatus) {
    await planService.updatePlanStatus(id, status)
  }
  async function updatePlan(id: string, input: PlanUpdateInput) {
    await planService.updatePlan(id, input)
  }
  async function reorderPlans(ids: string[]) {
    await planService.reorderPlans(ids)
  }
  async function loadCompareStats(examId: string) {
    compareStats.value = await planService.getCompareStats(examId)
  }

  return {
    plans,
    todayTasks,
    generating,
    lastResult,
    compareStats,
    loadPlansByDateRange,
    loadTodayTasks,
    generatePlan,
    updatePlanStatus,
    updatePlan,
    reorderPlans,
    loadCompareStats,
  }
})
