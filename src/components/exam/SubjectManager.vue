<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useExamStore } from '@/stores/exam'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Plus, Edit, Delete, Share } from '@element-plus/icons-vue'
import SubjectForm from './SubjectForm.vue'
import KnowledgeTree from './KnowledgeTree.vue'
import type { Subject } from '@/types'
import type { SubjectInput } from '@/services/exam-service'

const props = defineProps<{ examId: string }>()
defineEmits<{ close: [] }>()

const store = useExamStore()
const subjectDialogVisible = ref(false)
const editingSubject = ref<Subject | null>(null)
const kpDialogVisible = ref(false)
const kpSubject = ref<Subject | null>(null)

const levelLabels = ['', '入门', '基础', '一般', '熟练', '精通']

onMounted(async () => {
  await store.setActiveExam(props.examId)
  await store.loadSubjects()
})

function openAddSubject() {
  editingSubject.value = null
  subjectDialogVisible.value = true
}
function openEditSubject(s: Subject) {
  editingSubject.value = s
  subjectDialogVisible.value = true
}

async function handleSubmitSubject(data: SubjectInput) {
  try {
    if (editingSubject.value) {
      await store.updateSubject(editingSubject.value.id, data)
      ElMessage.success('科目已更新')
    } else {
      await store.createSubject(data)
      ElMessage.success('科目已添加')
    }
    subjectDialogVisible.value = false
  } catch (e) {
    ElMessage.error((e as Error).message ?? '保存失败')
  }
}

async function handleDeleteSubject(s: Subject) {
  try {
    const counts = await store.getSubjectCascadeCounts(s.id)
    await ElMessageBox.confirm(
      `删除科目「${s.name}」将连带删除：知识点 ${counts.knowledge_points} 条、学习记录 ${counts.study_records} 条、计划 ${counts.study_plans} 条、错题 ${counts.wrong_questions} 条。确认删除？`,
      '删除确认',
      { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' },
    )
    await store.deleteSubject(s.id)
    ElMessage.success('已删除')
  } catch (e) {
    if (e !== 'cancel' && e !== 'close') ElMessage.error((e as Error).message ?? '删除失败')
  }
}

function openKnowledge(s: Subject) {
  kpSubject.value = s
  kpDialogVisible.value = true
}
</script>

<template>
  <div class="subject-manager">
    <div class="subject-manager__header">
      <span class="title">科目管理</span>
      <el-button type="primary" :icon="Plus" @click="openAddSubject">添加科目</el-button>
    </div>

    <el-empty v-if="!store.subjects.length" description="还没有科目，点击右上角添加" />
    <el-table v-else :data="store.subjects" stripe>
      <el-table-column prop="name" label="科目" min-width="120" />
      <el-table-column label="目标分" width="90">
        <template #default="{ row }">
          <span class="tnum">{{ row.target_score ?? '-' }}</span>
        </template>
      </el-table-column>
      <el-table-column label="当前水平" width="100">
        <template #default="{ row }">
          <el-tag
            size="small"
            :type="row.current_level >= 4 ? 'success' : row.current_level <= 2 ? 'warning' : 'info'"
          >
            {{ levelLabels[row.current_level] }}
          </el-tag>
        </template>
      </el-table-column>
      <el-table-column prop="weight" label="权重" width="80">
        <template #default="{ row }">
          <span class="tnum">{{ row.weight }}</span>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="250">
        <template #default="{ row }">
          <el-button size="small" :icon="Share" @click="openKnowledge(row)">知识点</el-button>
          <el-button size="small" :icon="Edit" @click="openEditSubject(row)">编辑</el-button>
          <el-button size="small" type="danger" :icon="Delete" @click="handleDeleteSubject(row)">删除</el-button>
        </template>
      </el-table-column>
    </el-table>

    <el-dialog
      v-model="subjectDialogVisible"
      :title="editingSubject ? '编辑科目' : '添加科目'"
      width="480px"
      append-to-body
    >
      <SubjectForm
        :subject="editingSubject ?? undefined"
        :exam-id="props.examId"
        @submit="handleSubmitSubject"
        @cancel="subjectDialogVisible = false"
      />
    </el-dialog>

    <el-dialog
      v-model="kpDialogVisible"
      :title="`知识点管理 · ${kpSubject?.name ?? ''}`"
      width="780px"
      append-to-body
      destroy-on-close
    >
      <KnowledgeTree v-if="kpSubject" :subject-id="kpSubject.id" />
    </el-dialog>
  </div>
</template>

<style scoped>
.subject-manager__header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: var(--sp-4);
}
.title {
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--c-ink);
}
</style>
