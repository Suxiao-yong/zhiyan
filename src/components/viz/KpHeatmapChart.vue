<script setup lang="ts">
import { computed } from 'vue'
import VChart from 'vue-echarts'
import { Grid } from '@element-plus/icons-vue'
import { SEMANTIC, useChartTheme } from '@/services/theme'

const props = defineProps<{ data: { kpName: string; date: string; mastery: number }[] }>()
const { theme } = useChartTheme()

const kps = computed(() => [...new Set(props.data.map((d) => d.kpName))])
const dates = computed(() => [...new Set(props.data.map((d) => d.date))].sort())
const cells = computed<[number, number, number][]>(() => {
  const out: [number, number, number][] = []
  props.data.forEach((d) => {
    const x = dates.value.indexOf(d.date)
    const y = kps.value.indexOf(d.kpName)
    if (x >= 0 && y >= 0) out.push([x, y, Math.round(d.mastery * 20)]) // 1-5 → 20-100
  })
  return out
})

const option = computed(() => ({
  tooltip: {},
  toolbox: { feature: { saveAsImage: { name: 'kp-heatmap' } } },
  grid: { left: 110, right: 24, top: 16, bottom: 48 },
  xAxis: { type: 'category', data: dates.value },
  yAxis: { type: 'category', data: kps.value },
  visualMap: {
    min: 20,
    max: 100,
    calculable: true,
    orient: 'horizontal',
    left: 'center',
    bottom: 0,
    inRange: { color: [SEMANTIC.danger, SEMANTIC.warning, SEMANTIC.success] },
  },
  series: [{ type: 'heatmap', data: cells.value, label: { show: false } }],
}) as any)
</script>

<template>
  <el-card shadow="never" class="chart-card">
    <template #header>
      <div class="card-head">
        <el-icon class="card-head__icon"><Grid /></el-icon>
        <span class="card-head__title">知识点掌握热力图</span>
      </div>
    </template>
    <v-chart v-if="cells.length" class="chart" :option="option" :theme="theme" autoresize />
    <el-empty v-else description="无数据（需记录知识点掌握度）" :image-size="50" />
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
