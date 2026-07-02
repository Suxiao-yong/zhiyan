<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { marked } from 'marked'
import { ElMessage } from 'element-plus'
import {
  Calendar,
  DataLine,
  Aim,
  TrendCharts,
  Setting,
  Document,
  MagicStick,
  Warning,
} from '@element-plus/icons-vue'
import { useAnalysisStore } from '@/stores/analysis'
import { useExamStore } from '@/stores/exam'
import { useSettingsStore } from '@/stores/settings'
import PageHeader from '@/components/common/PageHeader.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import SuggestionCard from '@/components/analysis/SuggestionCard.vue'
import ScoreGauge from '@/components/analysis/ScoreGauge.vue'
import { businessToday } from '@/services/record-service'
import type { AiAnalysis } from '@/types'
import type { Recommendation, PredictionResult } from '@/services/analyzer'

const analysisStore = useAnalysisStore()
const examStore = useExamStore()
const settingsStore = useSettingsStore()
const running = ref('')

function weekMonday(): string {
  const x = new Date()
  x.setHours(0, 0, 0, 0)
  const d = x.getDay()
  x.setDate(x.getDate() - (d === 0 ? 6 : d - 1))
  return `${x.getFullYear()}-${String(x.getMonth() + 1).padStart(2, '0')}-${String(
    x.getDate(),
  ).padStart(2, '0')}`
}

const groups = computed(() => {
  const today = businessToday()
  const wk = weekMonday()
  const g: { label: string; items: AiAnalysis[] }[] = [
    { label: '今日', items: [] },
    { label: '本周', items: [] },
    { label: '更早', items: [] },
  ]
  for (const a of analysisStore.analyses) {
    const d = (a.created_at || '').slice(0, 10)
    if (d === today) g[0].items.push(a)
    else if (d >= wk) g[1].items.push(a)
    else g[2].items.push(a)
  }
  return g.filter((x) => x.items.length)
})

const current = computed(() => analysisStore.current)
const contentHtml = computed(() =>
  current.value ? marked.parse(current.value.content ?? '') : '',
)
const suggestions = computed<Recommendation[]>(() => {
  if (!current.value?.suggestions) return []
  try {
    return JSON.parse(current.value.suggestions) as Recommendation[]
  } catch {
    return []
  }
})
const prediction = computed<PredictionResult | null>(() => {
  if (!current.value?.scores_prediction) return null
  try {
    return JSON.parse(current.value.scores_prediction) as PredictionResult
  } catch {
    return null
  }
})

const typeLabels: Record<string, string> = {
  daily: '每日',
  weekly: '每周',
  phase: '阶段',
  prediction: '预测',
  adjustment: '调整',
}
// 视觉映射：分析类型 → 图标组件（取代原 emoji 字符串）
const typeIcons: Record<string, any> = {
  daily: Calendar,
  weekly: DataLine,
  phase: Aim,
  prediction: TrendCharts,
  adjustment: Setting,
}

onMounted(async () => {
  await examStore.loadExams()
  if (!examStore.activeExamId && examStore.exams.length)
    examStore.setActiveExam(examStore.exams[0].id)
  await analysisStore.loadAnalyses()
})

async function run(type: 'daily' | 'weekly' | 'phase') {
  if (!examStore.activeExamId) return ElMessage.warning('请先选择考试')
  running.value = type
  try {
    if (type === 'daily')
      await analysisStore.runDaily(examStore.activeExamId, settingsStore.llmConfig)
    else if (type === 'weekly')
      await analysisStore.runWeekly(examStore.activeExamId, settingsStore.llmConfig)
    else await analysisStore.runPhase(examStore.activeExamId, settingsStore.llmConfig)
    ElMessage.success('分析完成')
  } catch (e) {
    ElMessage.error((e as Error).message ?? '分析失败')
  } finally {
    running.value = ''
  }
}
async function confirm() {
  if (!current.value) return
  await analysisStore.confirmSuggestion(current.value.id)
  ElMessage.success('已确认')
}
async function reject() {
  if (!current.value) return
  await analysisStore.rejectSuggestion(current.value.id)
  ElMessage.success('已拒绝')
}
async function apply() {
  if (!current.value) return
  await analysisStore.applySuggestion(current.value.id)
  ElMessage.success('已标记应用（请参照建议手动调整计划/科目）')
}
</script>

<template>
  <div>
    <PageHeader title="AI 分析报告" subtitle="AI 诊断学习、预测分数；建议需你确认后才应用">
      <template #actions>
        <el-button :loading="running === 'daily'" @click="run('daily')">每日分析</el-button>
        <el-button :loading="running === 'weekly'" @click="run('weekly')">每周分析</el-button>
        <el-button :loading="running === 'phase'" @click="run('phase')">阶段预测</el-button>
      </template>
    </PageHeader>

    <el-row :gutter="16">
      <el-col :xs="24" :md="8">
        <el-card shadow="never" class="list-card">
          <template #header>
            <div class="card-head">
              <el-icon class="card-head__icon"><Document /></el-icon>
              <span class="card-head__title">报告列表</span>
            </div>
          </template>
          <el-empty v-if="!groups.length" description="还没有分析报告" :image-size="50" />
          <div v-for="g in groups" :key="g.label" class="group">
            <div class="group-title">{{ g.label }}</div>
            <div
              v-for="a in g.items"
              :key="a.id"
              class="report-item"
              :class="{ active: a.id === analysisStore.currentId }"
              @click="analysisStore.select(a.id)"
            >
              <el-icon class="icon">
                <component :is="typeIcons[a.analysis_type] || TrendCharts" />
              </el-icon>
              <div class="info">
                <div class="type">{{ typeLabels[a.analysis_type] || a.analysis_type }}</div>
                <div class="date tnum">{{ (a.created_at || '').slice(5, 16) }}</div>
              </div>
              <el-tag
                size="small"
                :type="a.generated_by === 'ai' ? 'success' : 'info'"
                effect="light"
              >
                {{ a.generated_by === 'ai' ? 'AI' : '本地' }}
              </el-tag>
            </div>
          </div>
        </el-card>
      </el-col>

      <el-col :xs="24" :md="16">
        <el-card v-if="current" shadow="never">
          <template #header>
            <div class="card-head">
              <el-icon class="card-head__icon">
                <component :is="typeIcons[current.analysis_type] || TrendCharts" />
              </el-icon>
              <span class="card-head__title">{{ typeLabels[current.analysis_type] }}报告</span>
              <div class="head-actions">
                <el-button size="small" @click="confirm">确认</el-button>
                <el-button size="small" type="primary" @click="apply">应用</el-button>
                <el-button size="small" type="danger" @click="reject">拒绝</el-button>
              </div>
            </div>
          </template>
          <div class="markdown" v-html="contentHtml"></div>

          <div v-if="suggestions.length" class="section">
            <div class="section-title">
              <el-icon><MagicStick /></el-icon>
              <span>建议</span>
            </div>
            <div class="sugs">
              <SuggestionCard
                v-for="(s, i) in suggestions"
                :key="i"
                :suggestion="s"
                :state="current.user_confirmed"
                :applied="!!current.applied"
                @confirm="confirm"
                @reject="reject"
                @apply="apply"
              />
            </div>
          </div>

          <div v-if="prediction" class="section">
            <div class="section-title">
              <el-icon><Aim /></el-icon>
              <span>分数预测</span>
            </div>
            <div class="gauges">
              <ScoreGauge
                v-for="p in prediction.predictions"
                :key="p.subject"
                :subject="p.subject"
                :predicted="p.predicted_score"
                :target="p.target_score"
              />
            </div>
            <div v-if="prediction.alerts.length" class="alerts">
              <el-tag
                v-for="(al, i) in prediction.alerts"
                :key="i"
                effect="light"
                :type="al.severity === 'critical' ? 'danger' : al.severity === 'warning' ? 'warning' : 'info'"
              >{{ al.subject }}：{{ al.issue }}</el-tag>
            </div>
            <p v-if="prediction.urgent_actions.length" class="urgent">
              <el-icon class="urgent__icon"><Warning /></el-icon>
              <span>{{ prediction.urgent_actions.join('；') }}</span>
            </p>
          </div>
        </el-card>
        <el-card v-else shadow="never">
          <EmptyState
            title="选择左侧报告查看"
            description="或点击右上角「每日/每周/阶段分析」生成（配置 LLM 为 AI，否则本地降级）"
          />
        </el-card>
      </el-col>
    </el-row>
  </div>
</template>

<style scoped>
/* 卡片头（与设计系统一致） */
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
.head-actions {
  margin-left: auto;
  display: flex;
  gap: var(--sp-2);
}

/* 报告列表 */
.list-card {
  max-height: 70vh;
  overflow: auto;
}
.group {
  margin-bottom: var(--sp-4);
}
.group-title {
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
  margin-bottom: var(--sp-2);
  padding-left: var(--sp-1);
}
.report-item {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  padding: var(--sp-2) var(--sp-3);
  border-radius: var(--r-sm);
  cursor: pointer;
  transition: background var(--dur-fast) var(--ease);
}
.report-item:hover {
  background: var(--c-surface-2);
}
.report-item.active {
  background: var(--c-primary-light);
}
.report-item .icon {
  font-size: var(--fs-lg);
  color: var(--c-ink-3);
  flex-shrink: 0;
  transition: color var(--dur-fast) var(--ease);
}
.report-item:hover .icon {
  color: var(--c-ink-2);
}
.report-item.active .icon {
  color: var(--c-primary);
}
.info {
  flex: 1;
  min-width: 0;
}
.type {
  font-size: var(--fs-sm);
  color: var(--c-ink);
  line-height: 1.4;
}
.report-item.active .type {
  color: var(--c-primary);
  font-weight: 500;
}
.date {
  font-size: var(--fs-xs);
  color: var(--c-ink-muted);
  line-height: 1.4;
}

/* 报告正文（markdown 渲染） */
.markdown {
  font-size: var(--fs-sm);
  line-height: 1.7;
  color: var(--c-ink-2);
}
.markdown :deep(h1),
.markdown :deep(h2),
.markdown :deep(h3),
.markdown :deep(h4),
.markdown :deep(h5),
.markdown :deep(h6) {
  color: var(--c-ink);
  font-weight: 600;
  line-height: 1.3;
  margin: var(--sp-4) 0 var(--sp-2);
}
.markdown :deep(h1) {
  font-size: var(--fs-2xl);
}
.markdown :deep(h2) {
  font-size: var(--fs-xl);
}
.markdown :deep(h3) {
  font-size: var(--fs-lg);
}
.markdown :deep(h4) {
  font-size: var(--fs-md);
}
.markdown :deep(h5),
.markdown :deep(h6) {
  font-size: var(--fs-sm);
}
.markdown :deep(h1:first-child),
.markdown :deep(h2:first-child),
.markdown :deep(h3:first-child),
.markdown :deep(h4:first-child) {
  margin-top: 0;
}
.markdown :deep(p) {
  margin: 0 0 var(--sp-2);
}
.markdown :deep(ul),
.markdown :deep(ol) {
  padding-left: var(--sp-5);
  margin: 0 0 var(--sp-2);
}
.markdown :deep(li) {
  margin: var(--sp-1) 0;
}
.markdown :deep(a) {
  color: var(--c-primary);
  text-decoration: none;
}
.markdown :deep(a:hover) {
  text-decoration: underline;
}
.markdown :deep(strong) {
  color: var(--c-ink);
  font-weight: 600;
}
.markdown :deep(code) {
  font-family: var(--font-mono);
  font-size: 0.9em;
  background: var(--c-surface-3);
  color: var(--c-ink);
  padding: 1px var(--sp-1);
  border-radius: var(--r-sm);
}
.markdown :deep(pre) {
  background: var(--c-surface-2);
  border: 1px solid var(--c-border);
  border-radius: var(--r-md);
  padding: var(--sp-3);
  overflow-x: auto;
  margin: 0 0 var(--sp-2);
}
.markdown :deep(pre code) {
  background: transparent;
  padding: 0;
  font-size: var(--fs-xs);
  border-radius: 0;
  color: var(--c-ink);
}
.markdown :deep(blockquote) {
  border-left: 2px solid var(--c-border);
  color: var(--c-ink-3);
  padding-left: var(--sp-3);
  margin: 0 0 var(--sp-2);
}
.markdown :deep(table) {
  border-collapse: collapse;
  width: 100%;
  margin: 0 0 var(--sp-2);
}
.markdown :deep(th),
.markdown :deep(td) {
  border: 1px solid var(--c-border);
  padding: var(--sp-1) var(--sp-2);
  text-align: left;
}
.markdown :deep(th) {
  background: var(--c-surface-2);
  font-weight: 600;
  color: var(--c-ink);
}
.markdown :deep(hr) {
  border: none;
  border-top: 1px solid var(--c-border);
  margin: var(--sp-3) 0;
}

/* 报告内分区 */
.section {
  margin-top: var(--sp-5);
  border-top: 1px solid var(--c-border);
  padding-top: var(--sp-4);
}
.section-title {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--c-ink);
  margin: 0 0 var(--sp-3);
}
.section-title .el-icon {
  color: var(--c-primary);
  font-size: var(--fs-md);
}
.sugs {
  display: flex;
  flex-direction: column;
  gap: var(--sp-3);
}
.gauges {
  display: flex;
  flex-wrap: wrap;
  gap: var(--sp-2);
}
.alerts {
  display: flex;
  flex-wrap: wrap;
  gap: var(--sp-2);
  margin-top: var(--sp-3);
}
.urgent {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  margin-top: var(--sp-3);
  font-size: var(--fs-sm);
  color: var(--c-danger);
  background: var(--c-danger-light);
  border-left: 3px solid var(--c-danger);
  padding: var(--sp-2) var(--sp-3);
  border-radius: var(--r-sm);
}
.urgent__icon {
  flex-shrink: 0;
  font-size: var(--fs-md);
}
</style>
