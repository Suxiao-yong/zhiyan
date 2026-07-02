<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { Calendar } from '@element-plus/icons-vue'
import { usePlanStore } from '@/stores/plan'
import { useExamStore } from '@/stores/exam'
import TaskDetailDialog from './TaskDetailDialog.vue'
import { STATUS_COLORS } from '@/services/theme'
import type { PlanWithNames } from '@/services/plan-service'

const planStore = usePlanStore()
const examStore = useExamStore()
const value = ref(new Date())
const detailVisible = ref(false)
const detailDate = ref<string | null>(null)

const yearMonth = computed(() => {
  const d = value.value
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`
})
const from = computed(() => `${yearMonth.value}-01`)
const to = computed(() => {
  const d = new Date(value.value.getFullYear(), value.value.getMonth() + 1, 0)
  return `${yearMonth.value}-${String(d.getDate()).padStart(2, '0')}`
})

const plansByDate = computed(() => {
  const m = new Map<string, PlanWithNames[]>()
  for (const p of planStore.plans) {
    const arr = m.get(p.date) ?? []
    arr.push(p)
    m.set(p.date, arr)
  }
  return m
})

async function load() {
  if (!examStore.activeExamId) return
  await planStore.loadPlansByDateRange(examStore.activeExamId, from.value, to.value)
}
onMounted(async () => {
  await examStore.loadExams()
  if (!examStore.activeExamId && examStore.exams.length)
    examStore.setActiveExam(examStore.exams[0].id)
  await load()
})
watch(yearMonth, load)

function onCellClick(data: { day: string; type: string }) {
  if (data.type !== 'curr-month') return
  if (!plansByDate.value.has(data.day)) return
  detailDate.value = data.day
  detailVisible.value = true
}
function onDetailChanged() {
  load()
}
</script>

<template>
  <el-card shadow="never">
    <template #header>
      <div class="card-head">
        <el-icon class="card-head__icon"><Calendar /></el-icon>
        <span class="card-head__title">计划日历</span>
        <span class="card-head__hint">点击有任务的日期查看当日任务清单并切换状态</span>
      </div>
    </template>
    <el-calendar v-model="value">
      <template #date-cell="{ data }">
        <div
          class="cal-cell"
          :class="{ 'other-month': data.type !== 'curr-month', 'has-plan': plansByDate.has(data.day) }"
          @click="onCellClick(data)"
        >
          <span class="day">{{ Number(data.day.slice(-2)) }}</span>
          <div v-if="plansByDate.has(data.day)" class="tags">
            <span
              v-for="p in (plansByDate.get(data.day) || []).slice(0, 3)"
              :key="p.id"
              class="tag"
              :style="{ background: STATUS_COLORS[p.status] }"
            >{{ p.subject_name }}</span>
          </div>
        </div>
      </template>
    </el-calendar>

    <TaskDetailDialog v-model="detailVisible" :date="detailDate" @changed="onDetailChanged" />
  </el-card>
</template>

<style scoped>
.card-head {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
}
.card-head__icon {
  color: var(--c-primary);
  font-size: var(--fs-md);
}
.card-head__title {
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--c-ink);
}
.card-head__hint {
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
  margin-left: auto;
}
.cal-cell {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  padding-top: var(--sp-1);
  cursor: pointer;
  transition: background var(--dur-fast) var(--ease);
}
.cal-cell.other-month {
  opacity: 0.4;
}
.cal-cell.has-plan:hover {
  background: var(--c-primary-light);
}
.day {
  font-size: var(--fs-sm);
  color: var(--c-ink-2);
}
.tags {
  display: flex;
  flex-direction: column;
  gap: var(--sp-1);
  align-items: center;
  margin-top: var(--sp-1);
}
.tag {
  color: white;
  font-size: 10px;
  padding: 1px var(--sp-1);
  border-radius: var(--r-sm);
  max-width: 60px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
:deep(.el-calendar-table .el-calendar-day) {
  padding: 0;
  height: 64px;
}
</style>
