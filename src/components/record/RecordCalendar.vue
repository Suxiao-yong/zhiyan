<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { Calendar } from '@element-plus/icons-vue'
import { useRecordStore } from '@/stores/record'

const store = useRecordStore()
const value = ref(new Date())
const emit = defineEmits<{ 'select-date': [date: string] }>()

const yearMonth = computed(() => {
  const d = value.value
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`
})

const recordMap = computed(
  () => new Map(store.calendarMonth.map((r) => [r.date, r])),
)

onMounted(() => store.loadCalendarMonth(yearMonth.value))
watch(yearMonth, (ym) => store.loadCalendarMonth(ym))

function onCellClick(data: { day: string; type: string }) {
  if (data.type !== 'curr-month') return
  emit('select-date', data.day)
}

defineExpose({ refresh: () => store.loadCalendarMonth(yearMonth.value) })
</script>

<template>
  <el-card class="record-calendar" shadow="never">
    <template #header>
      <div class="card-head">
        <el-icon class="card-head__icon"><Calendar /></el-icon>
        <span class="card-head__title">打卡日历</span>
        <span class="card-head__hint">绿点 = 当日有学习记录，点击日期可查看当日记录</span>
      </div>
    </template>
    <el-calendar v-model="value">
      <template #date-cell="{ data }">
        <div
          class="cal-cell"
          :class="{ 'has-record': recordMap.has(data.day), 'other-month': data.type !== 'curr-month' }"
          @click="onCellClick(data)"
        >
          <span class="cal-day">{{ Number(data.day.slice(-2)) }}</span>
          <span v-if="recordMap.has(data.day)" class="cal-dot" />
        </div>
      </template>
    </el-calendar>
  </el-card>
</template>

<style scoped>
.record-calendar {
  margin-bottom: var(--sp-4);
}
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
  justify-content: center;
  cursor: pointer;
  position: relative;
}
.cal-cell.other-month {
  opacity: 0.4;
}
.cal-cell.has-record {
  background: var(--c-success-light);
}
.cal-day {
  font-size: var(--fs-sm);
  color: var(--c-ink-2);
}
.cal-dot {
  width: 6px;
  height: 6px;
  border-radius: var(--r-pill);
  background: var(--c-success);
  margin-top: 2px;
}
:deep(.el-calendar-table .el-calendar-day) {
  padding: 0;
  height: 56px;
}
</style>
