// 考试配置 store（业务数据，不持久化——数据只存 SQLite）。
// 作为组件与 exam-service 之间的状态层：加载列表、CRUD 后刷新、维护当前活跃考试/科目/知识点树。

import { defineStore } from 'pinia'
import { ref } from 'vue'
import { setSetting } from '@/services/db'
import type { Exam, KnowledgePoint, Subject } from '@/types'
import * as examService from '@/services/exam-service'
import type { ExamInput, KnowledgePointInput, SubjectInput } from '@/services/exam-service'

/** 同步当前活跃考试给 Rust（scheduler 提醒目标），失败不影响本地状态 */
function persistActiveExam(id: string | null): void {
  setSetting('agent_active_exam_id', id ?? '').catch(() => {
    /* ignore */
  })
}

export const useExamStore = defineStore('exam', () => {
  const exams = ref<Exam[]>([])
  const activeExamId = ref<string | null>(null)
  const subjects = ref<Subject[]>([])
  const knowledgeTree = ref<KnowledgePoint[]>([])

  // ---- 考试 ----
  async function loadExams() {
    exams.value = await examService.getAllExams()
    if (!activeExamId.value && exams.value.length) {
      activeExamId.value = exams.value[0].id
      persistActiveExam(activeExamId.value)
    }
    if (activeExamId.value && !exams.value.some((e) => e.id === activeExamId.value)) {
      activeExamId.value = exams.value[0]?.id ?? null
      persistActiveExam(activeExamId.value)
    }
  }

  function setActiveExam(id: string | null) {
    activeExamId.value = id
    persistActiveExam(id)
  }

  async function createExam(input: ExamInput): Promise<Exam> {
    const exam = await examService.createExam(input)
    await loadExams()
    return exam
  }

  async function updateExam(id: string, input: ExamInput): Promise<void> {
    await examService.updateExam(id, input)
    await loadExams()
  }

  async function deleteExam(id: string): Promise<void> {
    await examService.deleteExam(id)
    if (activeExamId.value === id) activeExamId.value = null
    await loadExams()
  }

  function getExamCascadeCounts(examId: string) {
    return examService.getExamCascadeCounts(examId)
  }

  // ---- 科目 ----
  async function loadSubjects() {
    if (!activeExamId.value) {
      subjects.value = []
      return
    }
    subjects.value = await examService.getSubjectsByExam(activeExamId.value)
  }

  async function createSubject(input: SubjectInput): Promise<Subject> {
    const subject = await examService.createSubject(input)
    await loadSubjects()
    return subject
  }

  async function updateSubject(id: string, input: Partial<SubjectInput>): Promise<void> {
    await examService.updateSubject(id, input)
    await loadSubjects()
  }

  async function deleteSubject(id: string): Promise<void> {
    await examService.deleteSubject(id)
    await loadSubjects()
  }

  function getSubjectCascadeCounts(subjectId: string) {
    return examService.getSubjectCascadeCounts(subjectId)
  }

  // ---- 知识点 ----
  async function loadKnowledgeTree(subjectId: string) {
    knowledgeTree.value = await examService.getKnowledgeTree(subjectId)
  }

  async function createKnowledgePoint(
    input: KnowledgePointInput,
    reloadSubjectId?: string,
  ): Promise<KnowledgePoint> {
    const kp = await examService.createKnowledgePoint(input)
    if (reloadSubjectId) await loadKnowledgeTree(reloadSubjectId)
    return kp
  }

  async function updateKnowledgePoint(
    id: string,
    input: Partial<KnowledgePointInput>,
    reloadSubjectId?: string,
  ): Promise<void> {
    await examService.updateKnowledgePoint(id, input)
    if (reloadSubjectId) await loadKnowledgeTree(reloadSubjectId)
  }

  async function deleteKnowledgePoint(id: string, reloadSubjectId?: string): Promise<void> {
    await examService.deleteKnowledgePoint(id)
    if (reloadSubjectId) await loadKnowledgeTree(reloadSubjectId)
  }

  async function reorderKnowledgePoints(ids: string[], subjectId: string): Promise<void> {
    await examService.reorderKnowledgePoints(ids)
    await loadKnowledgeTree(subjectId)
  }

  return {
    exams,
    activeExamId,
    subjects,
    knowledgeTree,
    loadExams,
    setActiveExam,
    createExam,
    updateExam,
    deleteExam,
    getExamCascadeCounts,
    loadSubjects,
    createSubject,
    updateSubject,
    deleteSubject,
    getSubjectCascadeCounts,
    loadKnowledgeTree,
    createKnowledgePoint,
    updateKnowledgePoint,
    deleteKnowledgePoint,
    reorderKnowledgePoints,
  }
})
