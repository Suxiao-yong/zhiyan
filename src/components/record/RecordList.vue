<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Edit, Delete, Search } from '@element-plus/icons-vue'
import { useRecordStore } from '@/stores/record'
import { useExamStore } from '@/stores/exam'
import QuickRecordDialog from './QuickRecordDialog.vue'
import type { RecordWithNames } from '@/services/record-service'

const props = defineProps<{ preselectDate?: string }>()
const route = useRoute()
const store = useRecordStore()
const examStore = useExamStore()

const filter = ref({
  dateFrom: '' as string,
  dateTo: '' as string,
  subjectId: '',
  knowledgePointId: '',
  page: 1,
  pageSize: 20,
})

const editVisible = ref(false)
const editRecord = ref<RecordWithNames | null>(null)

const masteryLabels = ['', '未掌握', '略懂', '了解', '熟悉', '掌握']

onMounted(async () => {
  const d = route.query.date
  if (typeof d === 'string' && d) {
    filter.value.dateFrom = d
    filter.value.dateTo = d
  }
  if (props.preselectDate) {
    filter.value.dateFrom = props.preselectDate
    filter.value.dateTo = props.preselectDate
  }
  if (!examStore.activeExamId && examStore.exams.length) {
    await examStore.setActiveExam(examStore.exams[0].id)
  }
  if (examStore.activeExamId) await examStore.loadSubjects()
  await load()
})

// 响应日历点击：预筛到该日期
watch(
  () => props.preselectDate,
  (d) => {
    if (d) {
      filter.value.dateFrom = d
      filter.value.dateTo = d
      filter.value.page = 1
      load()
    }
  },
)

async function load() {
  await store.loadRecords({
    dateFrom: filter.value.dateFrom || undefined,
    dateTo: filter.value.dateTo || undefined,
    subjectId: filter.value.subjectId || undefined,
    knowledgePointId: filter.value.knowledgePointId || undefined,
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
function clearDateFilter() {
  filter.value.dateFrom = ''
  filter.value.dateTo = ''
  filter.value.page = 1
  load()
}

function startEdit(row: RecordWithNames) {
  editRecord.value = row
  editVisible.value = true
}

async function del(id: string) {
  try {
    await ElMessageBox.confirm(
      '确认删除该学习记录？关联的错题将保留为独立条目。',
      '删除确认',
      { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' },
    )
    await store.deleteRecord(id)
    ElMessage.success('已删除')
    await load()
  } catch (e) {
    if (e !== 'cancel' && e !== 'close') ElMessage.error((e as Error).message ?? '删除失败')
  }
}

function rate(row: RecordWithNames) {
  if (!row.questions_count) return '—'
  return Math.round((row.correct_count / row.questions_count) * 100) + '%'
}

defineExpose({ refresh: load })
</script>

<template>
  <el-card shadow="never">
    <div class="filter-bar">
      <el-date-picker
        v-model="filter.dateFrom"
        type="date"
        placeholder="开始"
        value-format="YYYY-MM-DD"
        class="date-input"
      />
      <span class="sep">-</span>
      <el-date-picker
        v-model="filter.dateTo"
        type="date"
        placeholder="结束"
        value-format="YYYY-MM-DD"
        class="date-input"
      />
      <el-select v-model="filter.subjectId" clearable placeholder="科目" class="subject-select">
        <el-option v-for="s in examStore.subjects" :key="s.id" :label="s.name" :value="s.id" />
      </el-select>
      <el-button :icon="Search" type="primary" @click="onFilter">筛选</el-button>
      <el-button v-if="filter.dateFrom" link @click="clearDateFilter">清除日期筛选</el-button>
    </div>

    <el-table :data="store.records" stripe class="record-table">
      <el-table-column prop="date" label="日期" width="110" class-name="tnum" />
      <el-table-column label="科目" min-width="100">
        <template #default="{ row }">{{ row.subject_name ?? '—' }}</template>
      </el-table-column>
      <el-table-column label="时长" width="80" class-name="tnum">
        <template #default="{ row }">{{ row.duration_min }}分</template>
      </el-table-column>
      <el-table-column label="正确率" width="90" class-name="tnum">
        <template #default="{ row }">{{ rate(row) }}</template>
      </el-table-column>
      <el-table-column label="掌握度" width="90">
        <template #default="{ row }">
          <el-tag v-if="row.mastery_rating" size="small" effect="light">{{ masteryLabels[row.mastery_rating] }}</el-tag>
          <span v-else>—</span>
        </template>
      </el-table-column>
      <el-table-column label="内容" min-width="160" show-overflow-tooltip>
        <template #default="{ row }">{{ row.content ?? '—' }}</template>
      </el-table-column>
      <el-table-column label="操作" width="120">
        <template #default="{ row }">
          <el-button size="small" link :icon="Edit" @click="startEdit(row)">编辑</el-button>
          <el-button size="small" link type="danger" :icon="Delete" @click="del(row.id)">删除</el-button>
        </template>
      </el-table-column>
      <template #empty>
        <el-empty description="还没有学习记录，点击右上角「快速记录」开始" :image-size="60" />
      </template>
    </el-table>

    <div class="pager">
      <el-pagination
        v-model:current-page="filter.page"
        :page-size="filter.pageSize"
        :total="store.recordsTotal"
        layout="prev, pager, next, total"
        background
        @current-change="onPage"
      />
    </div>

    <QuickRecordDialog v-model="editVisible" :edit-record="editRecord ?? undefined" @saved="load" />
  </el-card>
</template>

<style scoped>
.filter-bar {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  flex-wrap: wrap;
}
.sep {
  color: var(--c-ink-3);
}
.date-input {
  width: 140px;
}
.subject-select {
  width: 130px;
}
.record-table {
  width: 100%;
  margin-top: var(--sp-3);
}
.pager {
  margin-top: var(--sp-3);
  display: flex;
  justify-content: flex-end;
}
</style>
