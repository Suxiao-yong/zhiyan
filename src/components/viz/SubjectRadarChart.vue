<script setup lang="ts">
import { computed } from 'vue'
import VChart from 'vue-echarts'
import { Aim } from '@element-plus/icons-vue'
import { SUBJECT_PALETTE, useChartTheme } from '@/services/theme'

const props = defineProps<{
  data: { subject: string; duration: number; correctRate: number; mastery: number }[]
}>()
const { theme } = useChartTheme()

const maxDur = computed(() => Math.max(1, ...props.data.map((d) => d.duration)))

const option = computed(() => ({
  tooltip: {},
  legend: { bottom: 0, data: props.data.map((d) => d.subject) },
  toolbox: { feature: { saveAsImage: { name: 'subject-radar' } } },
  radar: {
    indicator: [
      { name: '时长', max: 100 },
      { name: '正确率', max: 100 },
      { name: '掌握度', max: 100 },
    ],
  },
  series: [
    {
      type: 'radar',
      data: props.data.map((d, i) => {
        const color = SUBJECT_PALETTE[i % SUBJECT_PALETTE.length]
        return {
          value: [Math.round((d.duration / maxDur.value) * 100), d.correctRate, d.mastery],
          name: d.subject,
          lineStyle: { color },
          itemStyle: { color },
          areaStyle: { color, opacity: 0.1 },
        }
      }),
    },
  ],
}) as any)
</script>

<template>
  <el-card shadow="never" class="chart-card">
    <template #header>
      <div class="card-head">
        <el-icon class="card-head__icon"><Aim /></el-icon>
        <span class="card-head__title">各科进度雷达</span>
      </div>
    </template>
    <v-chart v-if="data.length" class="chart" :option="option" :theme="theme" autoresize />
    <el-empty v-else description="无数据" :image-size="50" />
  </el-card>
</template>

<style scoped>
.chart-card {
  height: 100%;
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
.chart {
  height: 280px;
}
</style>
