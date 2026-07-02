<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useExamStore } from '@/stores/exam'
import * as vizService from '@/services/viz-service'
import { query } from '@/services/db'
import PageHeader from '@/components/common/PageHeader.vue'
import { Odometer } from '@element-plus/icons-vue'
import DurationTrendChart from '@/components/viz/DurationTrendChart.vue'
import SubjectPieChart from '@/components/viz/SubjectPieChart.vue'
import CorrectRateChart from '@/components/viz/CorrectRateChart.vue'
import SubjectRadarChart from '@/components/viz/SubjectRadarChart.vue'
import KpHeatmapChart from '@/components/viz/KpHeatmapChart.vue'
import ScoreGauge from '@/components/analysis/ScoreGauge.vue'
import type { PredictionResult } from '@/services/analyzer'

const examStore = useExamStore()
const range = ref('30') // 7/30/90/all/custom
const customRange = ref<string[]>(['', ''])
const subjectIds = ref<string[]>([])
const loading = ref(false)

const trend = ref<{ date: string; minutes: number }[]>([])
const subjectDur = ref<{ subjectName: string; minutes: number }[]>([])
const correctRate = ref<{ date: string; subjectName: string; rate: number }[]>([])
const radar = ref<{ subject: string; duration: number; correctRate: number; mastery: number }[]>([])
const heatmap = ref<{ kpName: string; date: string; mastery: number }[]>([])
const prediction = ref<PredictionResult | null>(null)

function fmt(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(
    d.getDate(),
  ).padStart(2, '0')}`
}

function computeFromTo() {
  if (range.value === 'all') return { from: '1900-01-01', to: '2100-12-31' }
  if (range.value === 'custom')
    return { from: customRange.value[0] || '1900-01-01', to: customRange.value[1] || '2100-12-31' }
  const days = Number(range.value)
  const today = new Date()
  today.setHours(0, 0, 0, 0)
  const from = new Date(today)
  from.setDate(from.getDate() - (days - 1))
  return { from: fmt(from), to: fmt(today) }
}

async function load() {
  if (!examStore.activeExamId) return
  loading.value = true
  try {
    const { from, to } = computeFromTo()
    const filter = { from, to, subjectIds: subjectIds.value }
    const [t, sd, cr, rd, hm] = await Promise.all([
      vizService.getDurationTrend(filter),
      vizService.getSubjectDuration(filter),
      vizService.getCorrectRateTrend(filter),
      vizService.getSubjectRadar(filter),
      vizService.getKpHeatmap(filter),
    ])
    trend.value = t
    subjectDur.value = sd
    correctRate.value = cr
    radar.value = rd
    heatmap.value = hm
    // 最新预测
    const rows = await query<{ scores_prediction: string | null }>(
      'SELECT scores_prediction FROM ai_analyses WHERE scores_prediction IS NOT NULL ORDER BY created_at DESC LIMIT 1',
    )
    prediction.value = rows[0]?.scores_prediction
      ? (JSON.parse(rows[0].scores_prediction) as PredictionResult)
      : null
  } finally {
    loading.value = false
  }
}

onMounted(async () => {
  await examStore.loadExams()
  if (!examStore.activeExamId && examStore.exams.length)
    examStore.setActiveExam(examStore.exams[0].id)
  if (examStore.activeExamId) await examStore.loadSubjects()
  await load()
})
watch([range, customRange, subjectIds], load, { deep: true })
</script>

<template>
  <div v-loading="loading">
    <PageHeader title="数据可视化" subtitle="多维度探索学习数据，图表可导出 PNG">
      <template #actions>
        <el-select v-model="range" class="range-select">
          <el-option label="最近 7 天" value="7" />
          <el-option label="最近 30 天" value="30" />
          <el-option label="最近 90 天" value="90" />
          <el-option label="全部" value="all" />
          <el-option label="自定义" value="custom" />
        </el-select>
        <el-date-picker
          v-if="range === 'custom'"
          v-model="customRange"
          type="daterange"
          value-format="YYYY-MM-DD"
          start-placeholder="开始"
          end-placeholder="结束"
          class="range-picker"
        />
        <el-select
          v-model="subjectIds"
          multiple
          collapse-tags
          placeholder="科目（空=全部）"
          class="subject-select"
        >
          <el-option v-for="s in examStore.subjects" :key="s.id" :label="s.name" :value="s.id" />
        </el-select>
      </template>
    </PageHeader>

    <el-row :gutter="16" class="grid">
      <el-col :xs="24" :md="12"><DurationTrendChart :data="trend" /></el-col>
      <el-col :xs="24" :md="12"><SubjectPieChart :data="subjectDur" /></el-col>
      <el-col :xs="24" :md="12"><CorrectRateChart :data="correctRate" /></el-col>
      <el-col :xs="24" :md="12"><SubjectRadarChart :data="radar" /></el-col>
      <el-col :xs="24" :md="12"><KpHeatmapChart :data="heatmap" /></el-col>
      <el-col :xs="24" :md="12">
        <el-card shadow="never" class="gauge-card">
          <template #header>
            <div class="card-head">
              <el-icon class="card-head__icon"><Odometer /></el-icon>
              <span class="card-head__title">分数预测仪表</span>
            </div>
          </template>
          <div v-if="prediction && prediction.predictions.length" class="gauges">
            <ScoreGauge
              v-for="p in prediction.predictions"
              :key="p.subject"
              :subject="p.subject"
              :predicted="p.predicted_score"
              :target="p.target_score"
            />
          </div>
          <el-empty v-else description="暂无预测（去「AI 分析」生成阶段预测）" :image-size="50" />
        </el-card>
      </el-col>
    </el-row>
  </div>
</template>

<style scoped>
.range-select {
  width: 110px;
}
.range-picker {
  width: 240px;
}
.subject-select {
  width: 220px;
}
.grid > .el-col {
  margin-bottom: var(--sp-4);
}
.gauge-card {
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
.gauges {
  display: flex;
  flex-wrap: wrap;
  gap: var(--sp-2);
  padding-top: var(--sp-2);
}
</style>
