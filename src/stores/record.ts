// 学习记录 store（业务数据，不持久化——数据只存 SQLite）。
// 作为组件与 record-service 之间的状态层。

import { defineStore } from 'pinia'
import { ref } from 'vue'
import * as recordService from '@/services/record-service'
import type {
  RecordFilter,
  RecordInput,
  RecordWithNames,
  PlanCheckinInput,
  WrongFilter,
  WrongQuestionInput,
  WrongWithNames,
} from '@/services/record-service'

export const useRecordStore = defineStore('record', () => {
  const records = ref<RecordWithNames[]>([])
  const recordsTotal = ref(0)
  const calendarMonth = ref<{ date: string; minutes: number; count: number }[]>([])
  const wrongQuestions = ref<WrongWithNames[]>([])
  const wrongTotal = ref(0)

  async function loadRecords(filter: RecordFilter) {
    const r = await recordService.getRecords(filter)
    records.value = r.rows
    recordsTotal.value = r.total
  }
  async function loadCalendarMonth(ym: string) {
    calendarMonth.value = await recordService.getCalendarMonth(ym)
  }
  async function loadWrongQuestions(filter: WrongFilter) {
    const r = await recordService.getWrongQuestions(filter)
    wrongQuestions.value = r.rows
    wrongTotal.value = r.total
  }

  /** 创建记录并联动写入错题 */
  async function createRecord(input: RecordInput, wrongs: WrongQuestionInput[] = []) {
    const r = await recordService.createRecord(input)
    for (const w of wrongs) {
      await recordService.createWrongQuestion({ ...w, record_id: r.id })
    }
    return r
  }
  async function createPlanCheckin(
    planId: string,
    input: PlanCheckinInput,
    finish: boolean,
    wrongs: WrongQuestionInput[] = [],
  ) {
    return recordService.createPlanCheckin(planId, input, finish, wrongs)
  }
  function getPlanCheckins(planId: string) {
    return recordService.getPlanCheckins(planId)
  }
  async function restorePlan(planId: string) {
    await recordService.restorePlan(planId)
  }
  async function updateRecord(id: string, input: Partial<RecordInput>) {
    await recordService.updateRecord(id, input)
  }
  async function deleteRecord(id: string) {
    await recordService.deleteRecord(id)
  }
  async function setWrongMastered(id: string, mastered: boolean) {
    await recordService.setWrongMastered(id, mastered)
  }
  async function incrementWrongReview(id: string) {
    await recordService.incrementWrongReview(id)
  }

  return {
    records,
    recordsTotal,
    calendarMonth,
    wrongQuestions,
    wrongTotal,
    loadRecords,
    loadCalendarMonth,
    loadWrongQuestions,
    createRecord,
    createPlanCheckin,
    getPlanCheckins,
    restorePlan,
    updateRecord,
    deleteRecord,
    setWrongMastered,
    incrementWrongReview,
  }
})
