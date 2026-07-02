<script setup lang="ts">
import { computed } from 'vue'
import VChart from 'vue-echarts'
import { DataLine } from '@element-plus/icons-vue'
import { SUBJECT_PALETTE, useChartTheme } from '@/services/theme'

const props = defineProps<{ data: { date: string; subjectName: string; rate: number }[] }>()
const { theme } = useChartTheme()

const subjects = computed(() => [...new Set(props.data.map((d) => d.subjectName))])
const dates = computed(() => [...new Set(props.data.map((d) => d.date))].sort())

const option = computed(() => ({
  tooltip: { trigger: 'axis' },
  legend: { data: subjects.value, bottom: 0 },
  toolbox: { feature: { saveAsImage: { name: 'correct-rate' } } },
  grid: { left: 40, right: 16, top: 16, bottom: 40 },
  xAxis: { type: 'category', data: dates.value },
  yAxis: { type: 'value', max: 100, name: '%' },
  series: subjects.value.map((s, i) => {
    const color = SUBJECT_PALETTE[i % SUBJECT_PALETTE.length]
    return {
      name: s,
      type: 'line',
      smooth: true,
      itemStyle: { color },
      lineStyle: { color },
      data: dates.value.map((d) => {
        const r = props.data.find((x) => x.date === d && x.subjectName === s)
        return r ? r.rate : null
      }),
    }
  }),
}) as any)
</script>

<template>
  <el-card shadow="never" class="chart-card">
    <template #header>
      <div class="card-head">
        <el-icon class="card-head__icon"><DataLine /></el-icon>
        <span class="card-head__title">正确率变化曲线</span>
      </div>
    </template>
    <v-chart v-if="data.length" class="chart" :option="option" :theme="theme" autoresize />
    <el-empty v-else description="无数据（需有做题记录）" :image-size="50" />
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
