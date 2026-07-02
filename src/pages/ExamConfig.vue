<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useExamStore } from '@/stores/exam'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Plus, Document } from '@element-plus/icons-vue'
import PageHeader from '@/components/common/PageHeader.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import ExamCard from '@/components/exam/ExamCard.vue'
import ExamForm from '@/components/exam/ExamForm.vue'
import SubjectManager from '@/components/exam/SubjectManager.vue'
import PlanChatAssistant from '@/components/plan/PlanChatAssistant.vue'
import type { Exam } from '@/types'
import type { ExamInput } from '@/services/exam-service'

const store = useExamStore()

const examDialogVisible = ref(false)
const editingExam = ref<Exam | null>(null)
const subjectMgrExamId = ref<string | null>(null)
const subjectMgrVisible = ref(false)
const assistantVisible = ref(false)
const assistantExam = ref<{ id: string; name: string } | null>(null)

onMounted(() => store.loadExams())

function openCreate() {
  editingExam.value = null
  examDialogVisible.value = true
}
function openEdit(e: Exam) {
  editingExam.value = e
  examDialogVisible.value = true
}

async function handleSubmitExam(data: ExamInput) {
  try {
    if (editingExam.value) {
      await store.updateExam(editingExam.value.id, data)
      ElMessage.success('考试已更新')
    } else {
      const exam = await store.createExam(data)
      ElMessage.success('考试已创建')
      assistantExam.value = { id: exam.id, name: exam.name }
      assistantVisible.value = true
    }
    examDialogVisible.value = false
  } catch (e) {
    ElMessage.error((e as Error).message ?? '保存失败')
  }
}

async function handleDeleteExam(e: Exam) {
  try {
    const counts = await store.getExamCascadeCounts(e.id)
    await ElMessageBox.confirm(
      `删除考试「${e.name}」将连带删除：科目 ${counts.subjects} 个、知识点 ${counts.knowledge_points} 条、学习记录 ${counts.study_records} 条、计划 ${counts.study_plans} 条、错题 ${counts.wrong_questions} 条。确认删除？`,
      '删除确认',
      { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' },
    )
    await store.deleteExam(e.id)
    ElMessage.success('已删除')
  } catch (err) {
    if (err !== 'cancel' && err !== 'close') ElMessage.error((err as Error).message ?? '删除失败')
  }
}

function openSubjectMgr(e: Exam) {
  subjectMgrExamId.value = e.id
  subjectMgrVisible.value = true
}

const subjectMgrExamName = () =>
  store.exams.find((x) => x.id === subjectMgrExamId.value)?.name ?? ''
</script>

<template>
  <div class="exam-config">
    <PageHeader title="考试配置" subtitle="管理你的考试、科目与知识点体系">
      <template #actions>
        <el-button type="primary" :icon="Plus" @click="openCreate">新建考试</el-button>
      </template>
    </PageHeader>

    <EmptyState
      v-if="!store.exams.length"
      title="还没有考试配置"
      description="点击右上角「新建考试」开始，或在首次使用引导中创建你的第一个考试。"
      :icon="Document"
    >
      <el-button type="primary" :icon="Plus" @click="openCreate">新建考试</el-button>
    </EmptyState>

    <div v-else class="exam-grid">
      <ExamCard
        v-for="e in store.exams"
        :key="e.id"
        :exam="e"
        @edit="openEdit(e)"
        @delete="handleDeleteExam(e)"
        @manage-subjects="openSubjectMgr(e)"
      />
    </div>

    <el-dialog
      v-model="examDialogVisible"
      :title="editingExam ? '编辑考试' : '新建考试'"
      width="540px"
    >
      <ExamForm
        :exam="editingExam ?? undefined"
        @submit="handleSubmitExam"
        @cancel="examDialogVisible = false"
      />
    </el-dialog>

    <el-dialog
      v-model="subjectMgrVisible"
      :title="`科目管理 · ${subjectMgrExamName()}`"
      width="840px"
      top="6vh"
      destroy-on-close
    >
      <SubjectManager
        v-if="subjectMgrExamId"
        :exam-id="subjectMgrExamId"
        @close="subjectMgrVisible = false"
      />
    </el-dialog>

    <PlanChatAssistant
      v-model="assistantVisible"
      :exam-id="assistantExam?.id ?? ''"
      :exam-name="assistantExam?.name ?? ''"
    />
  </div>
</template>

<style scoped>
.exam-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
  gap: var(--sp-4);
}
</style>
