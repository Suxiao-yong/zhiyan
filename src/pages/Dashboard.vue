<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { Clock, Star, DataLine, CircleCheck } from '@element-plus/icons-vue'
import { useRecordStore } from '@/stores/record'
import { usePlanStore } from '@/stores/plan'
import { useExamStore } from '@/stores/exam'
import StatCard from '@/components/common/StatCard.vue'
import WeeklyTrendChart from '@/components/charts/WeeklyTrendChart.vue'
import SubjectRatioChart from '@/components/charts/SubjectRatioChart.vue'
import TodayTodoList from '@/components/dashboard/TodayTodoList.vue'
import AiDailySummary from '@/components/dashboard/AiDailySummary.vue'

const router = useRouter()
const store = useRecordStore()
const planStore = usePlanStore()
const examStore = useExamStore()
const loading = ref(true)
const trendFilter = ref<string | null>(null)

onMounted(async () => {
  try {
    await examStore.loadExams()
    if (!examStore.activeExamId && examStore.exams.length) {
      examStore.setActiveExam(examStore.exams[0].id)
    }
    await store.loadDashboardStats()
    if (examStore.activeExamId) await planStore.loadTodayTasks(examStore.activeExamId)
  } finally {
    loading.value = false
  }
})

const weeklyTotal = computed(() =>
  store.weeklyTrend.reduce((acc, d) => acc + d.minutes, 0),
)

// 今日计划完成率（除零保护：无计划显示"—"）
const todayCompletion = computed(() => {
  const total = planStore.todayTasks.length
  if (!total) return '—'
  const done = planStore.todayTasks.filter((t) => t.status === 'completed').length
  return Math.round((done / total) * 100) + '%'
})

function onDrilldown(date: string) {
  router.push({ path: '/study-record', query: { date } })
}
function onSubjectFilter(sid: string) {
  if (trendFilter.value === sid) {
    trendFilter.value = null
    store.setTrendFilter(null)
  } else {
    trendFilter.value = sid
    store.setTrendFilter(sid)
  }
}
</script>

<template>
  <div v-loading="loading" class="dashboard">
    <el-row :gutter="16" class="row">
      <el-col :xs="12" :sm="12" :md="6">
        <StatCard title="今日学习时长" :value="store.todayMinutes" unit="分钟" :icon="Clock" />
      </el-col>
      <el-col :xs="12" :sm="12" :md="6">
        <StatCard title="连续打卡" :value="store.streak" unit="天" :icon="Star" />
      </el-col>
      <el-col :xs="12" :sm="12" :md="6">
        <StatCard title="本周总时长" :value="weeklyTotal" unit="分钟" :icon="DataLine" />
      </el-col>
      <el-col :xs="12" :sm="12" :md="6">
        <StatCard
          title="今日计划完成率"
          :value="todayCompletion"
          :icon="CircleCheck"
          hint="今日无计划时显示 — "
        />
      </el-col>
    </el-row>

    <el-row :gutter="16" class="row">
      <el-col :xs="24" :md="16">
        <WeeklyTrendChart @drilldown="onDrilldown" />
      </el-col>
      <el-col :xs="24" :md="8">
        <SubjectRatioChart @filter="onSubjectFilter" />
      </el-col>
    </el-row>

    <el-row :gutter="16" class="row">
      <el-col :xs="24" :md="12">
        <TodayTodoList />
      </el-col>
      <el-col :xs="24" :md="12">
        <AiDailySummary />
      </el-col>
    </el-row>
  </div>
</template>

<style scoped>
.dashboard {
  display: flex;
  flex-direction: column;
  gap: var(--sp-4);
}
.row {
  margin-bottom: 0;
}
.row + .row {
  margin-top: 0;
}
</style>
