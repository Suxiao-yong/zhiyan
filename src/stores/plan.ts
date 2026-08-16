// 学习计划 store（不持久化）。
// Task 4: 计划生成不再走 TypeScript（plan-generator）；生成与调整由 Agent
// 通过 plan.preview_generate / plan.apply_preview 完成。此 store 只负责
// 查看、编辑、状态变更和排序。

import { defineStore } from 'pinia'
import { ref } from 'vue'
import * as planService from '@/services/plan-service'
import type { PlanStatus } from '@/types'
import type { CompareStats, PlanUpdateInput, PlanWithNames } from '@/services/plan-service'

export const usePlanStore = defineStore('plan', () => {
  const plans = ref<PlanWithNames[]>([])
  const todayTasks = ref<PlanWithNames[]>([])
  const compareStats = ref<CompareStats | null>(null)

  async function loadPlansByDateRange(examId: string, from: string, to: string) {
    plans.value = await planService.getPlansByDateRange(examId, from, to)
  }
  async function loadTodayTasks(examId: string) {
    todayTasks.value = await planService.getTodayPlans(examId)
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
    compareStats,
    loadPlansByDateRange,
    loadTodayTasks,
    updatePlanStatus,
    updatePlan,
    reorderPlans,
    loadCompareStats,
  }
})
