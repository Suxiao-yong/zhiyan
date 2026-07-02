<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import VChart from 'vue-echarts'
import { usePlanStore } from '@/stores/plan'
import { useExamStore } from '@/stores/exam'
import StatCard from '@/components/common/StatCard.vue'
import { TrendCharts, Warning } from '@element-plus/icons-vue'
import { SEMANTIC, useChartTheme } from '@/services/theme'

const planStore = usePlanStore()
const examStore = useExamStore()
const loading = ref(false)
const { theme } = useChartTheme()

async function load() {
  if (!examStore.activeExamId) return
  loading.value = true
  try {
    await planStore.loadCompareStats(examStore.activeExamId)
  } finally {
    loading.value = false
  }
}
onMounted(async () => {
  await examStore.loadExams()
  if (!examStore.activeExamId && examStore.exams.length)
    examStore.setActiveExam(examStore.exams[0].id)
  await load()
})
watch(() => examStore.activeExamId, load)

const stats = computed(() => planStore.compareStats)

const subjectBarOption = computed(() => {
  const s = stats.value
  if (!s) return {} as any
  return {
    tooltip: { trigger: 'axis', axisPointer: { type: 'shadow' } },
    legend: { data: ['计划', '实际'] },
    grid: { left: 48, right: 16, top: 32, bottom: 32 },
    xAxis: { type: 'category', data: s.bySubject.map((x) => x.subject) },
    yAxis: { type: 'value', name: '分钟' },
    series: [
      {
        name: '计划',
        type: 'bar',
        data: s.bySubject.map((x) => x.planned),
        itemStyle: { color: SEMANTIC.primary, borderRadius: [4, 4, 0, 0] },
      },
      {
        name: '实际',
        type: 'bar',
        data: s.bySubject.map((x) => x.actual),
        itemStyle: { color: SEMANTIC.success, borderRadius: [4, 4, 0, 0] },
      },
    ],
  } as any
})

const dailyLineOption = computed(() => {
  const s = stats.value
  if (!s) return {} as any
  return {
    tooltip: { trigger: 'axis' },
    grid: { left: 48, right: 16, top: 24, bottom: 32 },
    xAxis: { type: 'category', data: s.dailyCompletion.map((d) => d.date) },
    yAxis: { type: 'value', max: 100, name: '完成率%' },
    series: [
      {
        type: 'line',
        data: s.dailyCompletion.map((d) => d.rate),
        smooth: true,
        areaStyle: { opacity: 0.15 },
        markLine: {
          silent: true,
          data: [{ yAxis: 80, lineStyle: { color: SEMANTIC.warning, type: 'dashed' } }],
        },
      },
    ],
  } as any
})
</script>

<template>
  <div v-loading="loading">
    <el-row :gutter="12" class="row">
      <el-col :span="6">
        <StatCard
          title="总计划时长"
          :value="stats?.totalPlanned ?? 0"
          unit="分"
          :icon="TrendCharts"
        />
      </el-col>
      <el-col :span="6">
        <StatCard
          title="总实际时长"
          :value="stats?.totalActual ?? 0"
          unit="分"
          :icon="TrendCharts"
        />
      </el-col>
      <el-col :span="6">
        <StatCard
          title="完成率"
          :value="stats ? stats.completionRate + '%' : '-'"
          :icon="TrendCharts"
        />
      </el-col>
      <el-col :span="6">
        <StatCard
          title="偏差天数"
          :value="stats?.deviationDays ?? 0"
          unit="天"
          :icon="Warning"
          hint="完成率<80%的天数"
        />
      </el-col>
    </el-row>

    <el-card shadow="never" class="row">
      <template #header>
        <div class="card-head">
          <el-icon class="card-head__icon"><TrendCharts /></el-icon>
          <span class="card-head__title">科目维度</span>
          <span class="card-head__hint">计划 vs 实际</span>
        </div>
      </template>
      <v-chart
        v-if="stats && stats.bySubject.length"
        class="chart"
        :option="subjectBarOption"
        :theme="theme"
        autoresize
      />
      <el-empty v-else description="暂无数据" :image-size="50" />
    </el-card>

    <el-card shadow="never" class="row">
      <template #header>
        <div class="card-head">
          <el-icon class="card-head__icon"><TrendCharts /></el-icon>
          <span class="card-head__title">每日完成率趋势</span>
          <span class="card-head__hint">虚线 = 80%</span>
        </div>
      </template>
      <v-chart
        v-if="stats && stats.dailyCompletion.length"
        class="chart"
        :option="dailyLineOption"
        :theme="theme"
        autoresize
      />
      <el-empty v-else description="暂无数据" :image-size="50" />
    </el-card>

    <el-card shadow="never" class="row">
      <template #header>
        <div class="card-head">
          <el-icon class="card-head__icon"><Warning /></el-icon>
          <span class="card-head__title">偏差分析</span>
          <span class="card-head__hint">实际 &lt; 计划 × 80%</span>
        </div>
      </template>
      <div v-if="stats && stats.deviations.length" class="devs">
        <el-tag
          v-for="d in stats.deviations"
          :key="d.name"
          type="warning"
          effect="light"
          class="dev-tag tnum"
        >
          {{ d.name }}：{{ d.rate }}%
        </el-tag>
      </div>
      <el-empty v-else description="无明显偏差" :image-size="50" />
    </el-card>
  </div>
</template>

<style scoped>
.row {
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
.chart {
  height: 280px;
}
.devs {
  display: flex;
  flex-wrap: wrap;
  gap: var(--sp-2);
}
</style>
