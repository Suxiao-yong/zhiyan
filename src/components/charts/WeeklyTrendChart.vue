<script setup lang="ts">
import { computed } from 'vue'
import VChart from 'vue-echarts'
import { TrendCharts } from '@element-plus/icons-vue'
import { useRecordStore } from '@/stores/record'
import { useChartTheme } from '@/services/theme'

const store = useRecordStore()
const emit = defineEmits<{ drilldown: [date: string] }>()
const { theme } = useChartTheme()

const option = computed(() => {
  const data = store.weeklyTrend.map((d) => ({
    value: d.minutes,
    date: d.date,
  }))
  return {
    tooltip: {
      trigger: 'axis',
      axisPointer: { type: 'shadow' },
      formatter: (params: any) => {
        const p = params[0]
        const item = data[p.dataIndex]
        return `${item.date}<br/>${p.value} 分钟`
      },
    },
    grid: { left: 44, right: 16, top: 16, bottom: 28 },
    xAxis: {
      type: 'category',
      data: store.weeklyTrend.map((d) => d.label),
    },
    yAxis: {
      type: 'value',
    },
    series: [
      {
        type: 'bar',
        data,
        barWidth: '50%',
      },
    ],
  } as any
})

function onClick(params: any) {
  const item = store.weeklyTrend[params.dataIndex]
  if (item) emit('drilldown', item.date)
}
</script>

<template>
  <el-card shadow="never" class="chart-card">
    <template #header>
      <div class="card-head">
        <el-icon class="card-head__icon"><TrendCharts /></el-icon>
        <span class="card-head__title">本周学习时长趋势</span>
        <span class="card-head__hint">点击柱子查看当日记录</span>
      </div>
    </template>
    <VChart
      v-if="store.weeklyTrend.length"
      class="chart"
      :option="option"
      :theme="theme"
      autoresize
      @click="onClick"
    />
    <el-empty v-else description="本周暂无学习记录" :image-size="60" />
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
.card-head__hint {
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
  margin-left: auto;
}
.chart {
  height: 300px;
}
</style>
