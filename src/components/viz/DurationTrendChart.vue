<script setup lang="ts">
import { computed, ref } from 'vue'
import VChart from 'vue-echarts'
import { TrendCharts } from '@element-plus/icons-vue'
import { useChartTheme } from '@/services/theme'

const props = defineProps<{ data: { date: string; minutes: number }[] }>()
const type = ref<'line' | 'bar'>('line')
const { theme } = useChartTheme()

const option = computed(() => ({
  tooltip: { trigger: 'axis' },
  toolbox: { feature: { saveAsImage: { name: 'duration-trend' } } },
  grid: { left: 48, right: 16, top: 16, bottom: 32 },
  xAxis: { type: 'category', data: props.data.map((d) => d.date) },
  yAxis: { type: 'value', name: '分钟' },
  series: [
    {
      type: type.value,
      data: props.data.map((d) => d.minutes),
      smooth: true,
      areaStyle: type.value === 'line' ? { opacity: 0.15 } : undefined,
      itemStyle: { borderRadius: type.value === 'bar' ? [4, 4, 0, 0] : undefined },
    },
  ],
}) as any)
</script>

<template>
  <el-card shadow="never" class="chart-card">
    <template #header>
      <div class="card-head">
        <el-icon class="card-head__icon"><TrendCharts /></el-icon>
        <span class="card-head__title">每日学习时长趋势</span>
        <el-radio-group v-model="type" size="small" class="card-head__actions">
          <el-radio-button value="line">折线</el-radio-button>
          <el-radio-button value="bar">柱状</el-radio-button>
        </el-radio-group>
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
.card-head__actions {
  margin-left: auto;
}
.chart {
  height: 280px;
}
</style>
