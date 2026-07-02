<script setup lang="ts">
import { computed, ref } from 'vue'
import VChart from 'vue-echarts'
import { PieChart } from '@element-plus/icons-vue'
import { useChartTheme } from '@/services/theme'

const props = defineProps<{ data: { subjectName: string; minutes: number }[] }>()
const donut = ref(true)
const { isDark, theme } = useChartTheme()

const option = computed(
  () =>
    ({
      tooltip: { trigger: 'item', formatter: '{b}: {c}分 ({d}%)' },
      legend: { bottom: 0, type: 'scroll' },
      toolbox: { feature: { saveAsImage: { name: 'subject-ratio' } } },
      series: [
        {
          type: 'pie',
          radius: donut.value ? ['40%', '70%'] : '70%',
          center: ['50%', '45%'],
          itemStyle: { borderColor: isDark.value ? '#1b1c20' : '#ffffff', borderWidth: 2 },
          label: { formatter: '{b}\n{d}%' },
          data: props.data.map((d) => ({ value: d.minutes, name: d.subjectName })),
        },
      ],
    }) as any,
)
</script>

<template>
  <el-card shadow="never" class="chart-card">
    <template #header>
      <div class="card-head">
        <el-icon class="card-head__icon"><PieChart /></el-icon>
        <span class="card-head__title">各科时长占比</span>
        <el-radio-group v-model="donut" size="small" class="card-head__actions">
          <el-radio-button :value="true">环形</el-radio-button>
          <el-radio-button :value="false">饼图</el-radio-button>
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
