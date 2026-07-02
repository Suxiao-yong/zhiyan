<script setup lang="ts">
import { computed } from 'vue'
import VChart from 'vue-echarts'
import { useChartTheme, levelColor } from '@/services/theme'

const props = defineProps<{
  subject: string
  predicted: number
  target: number
}>()

const { isDark, theme } = useChartTheme()

const max = computed(() => Math.max(Math.round(props.target * 1.25), 100))

const option = computed(() => {
  // 预测/目标比 → 掌握度色阶（danger < 0.6, warning < 0.85, 否则 success）
  const color = levelColor(props.predicted / Math.max(props.target, 1))
  const trackColor = isDark.value ? '#2d2f36' : '#e6e7eb'
  const subTextColor = isDark.value ? '#8e9098' : '#6e7178'
  const detailColor = isDark.value ? '#ececef' : '#1a1b1e'
  return {
    series: [
      {
        type: 'gauge',
        min: 0,
        max: max.value,
        radius: '90%',
        progress: { show: true, width: 8, itemStyle: { color } },
        axisLine: { lineStyle: { width: 8, color: [[1, trackColor]] } },
        pointer: { show: true, length: '55%', width: 3, itemStyle: { color } },
        axisTick: { show: false },
        splitLine: { show: false },
        axisLabel: { show: false },
        anchor: { show: false },
        detail: {
          valueAnimation: true,
          formatter: '{value}',
          fontSize: 18,
          offsetCenter: [0, '20%'],
          color: detailColor,
        },
        title: {
          offsetCenter: [0, '50%'],
          fontSize: 12,
          color: subTextColor,
        },
        data: [{ value: props.predicted, name: props.subject }],
      },
    ],
    graphic: [
      {
        type: 'text',
        left: 'center',
        bottom: 4,
        style: { text: `目标 ${props.target}`, fontSize: 11, fill: subTextColor },
      },
    ],
  } as any
})
</script>

<template>
  <div class="gauge-wrap">
    <v-chart class="gauge" :option="option" :theme="theme" autoresize />
  </div>
</template>

<style scoped>
.gauge-wrap {
  width: 150px;
}
.gauge {
  height: 130px;
}
</style>
