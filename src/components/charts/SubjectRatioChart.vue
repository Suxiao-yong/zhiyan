<script setup lang="ts">
import { computed } from 'vue'
import VChart from 'vue-echarts'
import { PieChart } from '@element-plus/icons-vue'
import { useRecordStore } from '@/stores/record'
import { useChartTheme } from '@/services/theme'

const store = useRecordStore()
const emit = defineEmits<{ filter: [subjectId: string] }>()
const { theme } = useChartTheme()

const option = computed(() => {
  return {
    tooltip: { trigger: 'item', formatter: '{b}: {c} 分钟 ({d}%)' },
    legend: { bottom: 0, type: 'scroll' },
    series: [
      {
        type: 'pie',
        radius: ['42%', '70%'],
        center: ['50%', '45%'],
        avoidLabelOverlap: true,
        label: { formatter: '{b}\n{d}%' },
        data: store.subjectRatio.map((s) => ({
          value: s.minutes,
          name: s.subjectName,
          subjectId: s.subjectId,
        })),
      },
    ],
  } as any
})

function onClick(params: any) {
  const sid = params.data?.subjectId
  if (sid) emit('filter', sid)
}
</script>

<template>
  <el-card shadow="never" class="chart-card">
    <template #header>
      <div class="card-head">
        <el-icon class="card-head__icon"><PieChart /></el-icon>
        <span class="card-head__title">各科时间占比</span>
        <span class="card-head__hint">点击切片按该科过滤趋势</span>
      </div>
    </template>
    <VChart
      v-if="store.subjectRatio.length"
      class="chart"
      :option="option"
      :theme="theme"
      autoresize
      @click="onClick"
    />
    <el-empty v-else description="本周暂无记录" :image-size="60" />
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
