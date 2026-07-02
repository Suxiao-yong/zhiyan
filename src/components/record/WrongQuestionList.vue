<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { Check, Close, View, RefreshLeft, Search } from '@element-plus/icons-vue'
import { useRecordStore } from '@/stores/record'
import { useExamStore } from '@/stores/exam'
import type { KnowledgePoint } from '@/types'
import type { WrongWithNames } from '@/services/record-service'

const store = useRecordStore()
const examStore = useExamStore()

const filter = ref({
  subjectId: '',
  knowledgePointId: '',
  errorType: '',
  mastered: 'all' as 'all' | 'no' | 'yes',
  page: 1,
  pageSize: 20,
})
const kpOptions = ref<{ id: string; name: string }[]>([])
const detailVisible = ref(false)
const detailRow = ref<WrongWithNames | null>(null)

const errorTypes = ['概念不清', '计算错误', '粗心', '其他']
const masteredOptions = [
  { label: '全部', value: 'all' },
  { label: '未掌握', value: 'no' },
  { label: '已掌握', value: 'yes' },
]

onMounted(async () => {
  if (!examStore.activeExamId && examStore.exams.length) {
    await examStore.setActiveExam(examStore.exams[0].id)
  }
  if (examStore.activeExamId) await examStore.loadSubjects()
  await load()
})

async function load() {
  await store.loadWrongQuestions({
    subjectId: filter.value.subjectId || undefined,
    knowledgePointId: filter.value.knowledgePointId || undefined,
    errorType: filter.value.errorType || undefined,
    mastered:
      filter.value.mastered === 'no' ? false : filter.value.mastered === 'yes' ? true : undefined,
    page: filter.value.page,
    pageSize: filter.value.pageSize,
  })
}
function onFilter() {
  filter.value.page = 1
  load()
}
function onPage(p: number) {
  filter.value.page = p
  load()
}

async function onSubjectChange(subjectId: string) {
  filter.value.knowledgePointId = ''
  if (!subjectId) {
    kpOptions.value = []
    return
  }
  await examStore.loadKnowledgeTree(subjectId)
  const flat: { id: string; name: string }[] = []
  const walk = (nodes: KnowledgePoint[]) =>
    nodes.forEach((n) => {
      flat.push({ id: n.id, name: n.name })
      if (n.children) walk(n.children)
    })
  walk(examStore.knowledgeTree)
  kpOptions.value = flat
}

async function toggleMastered(row: WrongWithNames, m: boolean) {
  await store.setWrongMastered(row.id, m)
  ElMessage.success(m ? '已标记掌握' : '已取消掌握')
  await load()
}
async function incReview(id: string) {
  await store.incrementWrongReview(id)
  ElMessage.success('复习次数 +1')
  await load()
}
function viewDetail(row: WrongWithNames) {
  detailRow.value = row
  detailVisible.value = true
}
</script>

<template>
  <el-card shadow="never">
    <div class="filter-bar">
      <el-select v-model="filter.subjectId" clearable placeholder="科目" class="sel-120" @change="onSubjectChange">
        <el-option v-for="s in examStore.subjects" :key="s.id" :label="s.name" :value="s.id" />
      </el-select>
      <el-select v-model="filter.knowledgePointId" clearable placeholder="知识点" class="sel-140">
        <el-option v-for="k in kpOptions" :key="k.id" :label="k.name" :value="k.id" />
      </el-select>
      <el-select v-model="filter.errorType" clearable placeholder="错误类型" class="sel-120">
        <el-option v-for="t in errorTypes" :key="t" :label="t" :value="t" />
      </el-select>
      <el-select v-model="filter.mastered" placeholder="掌握状态" class="sel-120">
        <el-option v-for="o in masteredOptions" :key="o.value" :label="o.label" :value="o.value" />
      </el-select>
      <el-button :icon="Search" type="primary" @click="onFilter">筛选</el-button>
    </div>

    <el-table :data="store.wrongQuestions" stripe class="wrong-table">
      <el-table-column label="题目描述" min-width="220" show-overflow-tooltip>
        <template #default="{ row }">{{ row.question_desc ?? '—' }}</template>
      </el-table-column>
      <el-table-column label="知识点" width="130">
        <template #default="{ row }">{{ row.knowledge_point_name ?? '—' }}</template>
      </el-table-column>
      <el-table-column label="错误类型" width="100">
        <template #default="{ row }">{{ row.error_type ?? '—' }}</template>
      </el-table-column>
      <el-table-column label="复习" width="70" prop="review_count" class-name="tnum" />
      <el-table-column label="状态" width="90">
        <template #default="{ row }">
          <el-tag size="small" :type="row.mastered ? 'success' : 'warning'" effect="light">
            {{ row.mastered ? '已掌握' : '未掌握' }}
          </el-tag>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="240">
        <template #default="{ row }">
          <el-button v-if="!row.mastered" size="small" link :icon="Check" @click="toggleMastered(row, true)">已掌握</el-button>
          <el-button v-else size="small" link :icon="Close" @click="toggleMastered(row, false)">取消</el-button>
          <el-button size="small" link :icon="View" @click="viewDetail(row)">详情</el-button>
          <el-button size="small" link :icon="RefreshLeft" @click="incReview(row.id)">复习+1</el-button>
        </template>
      </el-table-column>
      <template #empty>
        <el-empty description="错题库为空，做题时记录联动会自动录入" :image-size="60" />
      </template>
    </el-table>

    <div class="pager">
      <el-pagination
        v-model:current-page="filter.page"
        :page-size="filter.pageSize"
        :total="store.wrongTotal"
        layout="prev, pager, next, total"
        background
        @current-change="onPage"
      />
    </div>

    <el-dialog v-model="detailVisible" title="错题详情" width="540px">
      <el-descriptions v-if="detailRow" :column="1" border>
        <el-descriptions-item label="题目来源">{{ detailRow.question_source ?? '—' }}</el-descriptions-item>
        <el-descriptions-item label="题目描述">{{ detailRow.question_desc ?? '—' }}</el-descriptions-item>
        <el-descriptions-item label="正确答案">{{ detailRow.correct_answer ?? '—' }}</el-descriptions-item>
        <el-descriptions-item label="我的答案">{{ detailRow.my_answer ?? '—' }}</el-descriptions-item>
        <el-descriptions-item label="错误类型">{{ detailRow.error_type ?? '—' }}</el-descriptions-item>
        <el-descriptions-item label="错误原因">{{ detailRow.error_reason ?? '—' }}</el-descriptions-item>
        <el-descriptions-item label="科目">{{ detailRow.subject_name ?? '—' }}</el-descriptions-item>
        <el-descriptions-item label="知识点">{{ detailRow.knowledge_point_name ?? '—' }}</el-descriptions-item>
        <el-descriptions-item label="复习次数">{{ detailRow.review_count }}</el-descriptions-item>
        <el-descriptions-item label="掌握状态">{{ detailRow.mastered ? '已掌握' : '未掌握' }}</el-descriptions-item>
      </el-descriptions>
    </el-dialog>
  </el-card>
</template>

<style scoped>
.filter-bar {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  flex-wrap: wrap;
}
.sel-120 {
  width: 120px;
}
.sel-140 {
  width: 140px;
}
.wrong-table {
  width: 100%;
  margin-top: var(--sp-3);
}
.pager {
  margin-top: var(--sp-3);
  display: flex;
  justify-content: flex-end;
}
</style>
