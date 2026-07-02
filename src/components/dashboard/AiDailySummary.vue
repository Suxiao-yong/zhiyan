<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { query } from '@/services/db'
import EmptyState from '@/components/common/EmptyState.vue'
import { TrendCharts } from '@element-plus/icons-vue'
import type { AiAnalysis } from '@/types'

const router = useRouter()
const analysis = ref<AiAnalysis | null>(null)
const loading = ref(true)

onMounted(async () => {
  try {
    const rows = await query<AiAnalysis>(
      "SELECT * FROM ai_analyses WHERE analysis_type = 'daily' ORDER BY created_at DESC LIMIT 1",
    )
    analysis.value = rows[0] ?? null
  } finally {
    loading.value = false
  }
})

const summary = () => {
  if (!analysis.value?.content) return ''
  // 取第一段非标题文本作为摘要
  const lines = analysis.value.content.split('\n').filter((l) => l.trim() && !l.startsWith('#'))
  return lines.slice(0, 2).join(' ')
}
</script>

<template>
  <el-card v-loading="loading" shadow="never" class="summary-card">
    <template #header>
      <div class="card-head">
        <el-icon class="card-head__icon"><TrendCharts /></el-icon>
        <span class="card-head__title">AI 每日分析摘要</span>
      </div>
    </template>
    <EmptyState
      v-if="!analysis"
      title="暂无 AI 分析"
      description="每日分析会在启动时自动补跑（或手动触发）。配置 LLM 后为 AI 分析，否则本地降级。"
      :icon="TrendCharts"
    />
    <div v-else class="summary">
      <div class="meta">
        <el-tag
          size="small"
          :type="analysis.generated_by === 'ai' ? 'success' : 'info'"
          effect="light"
        >
          {{ analysis.generated_by === 'ai' ? 'AI' : '本地' }}
        </el-tag>
        <span class="date">{{ (analysis.created_at || '').slice(0, 16) }}</span>
      </div>
      <p class="text">{{ summary() }}</p>
      <el-button size="small" type="primary" link @click="router.push('/analysis')">
        查看完整报告 →
      </el-button>
    </div>
  </el-card>
</template>

<style scoped>
.summary-card {
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
.meta {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  margin-bottom: var(--sp-2);
}
.date {
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
}
.text {
  font-size: var(--fs-sm);
  color: var(--c-ink-2);
  line-height: 1.7;
  margin: 0 0 var(--sp-2);
}
</style>
