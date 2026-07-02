<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import VChart from 'vue-echarts'
import { TrendCharts } from '@element-plus/icons-vue'
import { usePlanStore } from '@/stores/plan'
import { useExamStore } from '@/stores/exam'
import { STATUS_COLORS, useChartTheme } from '@/services/theme'
import type { PlanStatus } from '@/types'
import type { PlanWithNames } from '@/services/plan-service'

const planStore = usePlanStore()
const examStore = useExamStore()
const loading = ref(false)
const { theme } = useChartTheme()

async function load() {
  if (!examStore.activeExamId) return
  loading.value = true
  try {
    // 加载该考试全部计划（宽范围）
    await planStore.loadPlansByDateRange(examStore.activeExamId, '1900-01-01', '2100-12-31')
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

const option = computed(() => {
  const plans = planStore.plans as PlanWithNames[]
  const subjects = [...new Set(plans.map((p) => p.subject_name ?? '未知'))]
  const subjectIdx = new Map(subjects.map((s, i) => [s, i]))
  const data = plans.map((p) => ({
    value: [
      new Date(p.date + 'T00:00:00').getTime(),
      subjectIdx.get(p.subject_name ?? '未知') ?? 0,
      p.planned_duration ?? 0,
      p.planned_tasks ?? '',
      p.status,
      p.subject_name ?? '未知',
      p.date,
    ],
  }))
  return {
    tooltip: {
      formatter: (p: any) => {
        const v = p.data.value
        return `${v[5]} · ${v[6]}<br/>${v[3]}<br/>${v[2]} 分钟 · ${v[4]}`
      },
    },
    grid: { left: 90, right: 24, top: 16, bottom: 40 },
    xAxis: {
      type: 'time',
    },
    yAxis: {
      type: 'category',
      data: subjects,
    },
    dataZoom: [
      { type: 'inside', xAxisIndex: 0 },
      { type: 'slider', xAxisIndex: 0, height: 16 },
    ],
    series: [
      {
        type: 'custom',
        data,
        renderItem: (_params: any, api: any) => {
          const ts = api.value(0)
          const rowIdx = api.value(1)
          const status = api.value(4)
          const point = api.coord([ts, rowIdx])
          const dayW = api.size([86400000, 0])[0]
          const w = Math.max(4, dayW * 0.7)
          const h = 14
          return {
            type: 'rect',
            shape: {
              x: point[0] - w / 2,
              y: point[1] - h / 2,
              width: w,
              height: h,
              r: 3,
            },
            style: {
              fill: STATUS_COLORS[status as PlanStatus] ?? STATUS_COLORS.pending,
            },
          }
        },
        encode: { x: 0, y: 1 },
      },
    ],
  } as any
})
</script>

<template>
  <el-card v-loading="loading" shadow="never">
    <template #header>
      <div class="card-head">
        <el-icon class="card-head__icon"><TrendCharts /></el-icon>
        <span class="card-head__title">甘特图</span>
        <span class="card-head__hint">
          每行一科，色块 = 任务（灰未开始 / 蓝进行中 / 绿完成 / 红跳过）
        </span>
      </div>
    </template>
    <v-chart
      v-if="planStore.plans.length"
      class="chart"
      :option="option"
      :theme="theme"
      autoresize
    />
    <el-empty v-else description="尚未生成计划" :image-size="60" />
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
.chart {
  height: 360px;
}
</style>
