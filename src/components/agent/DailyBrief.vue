<script setup lang="ts">
import { onMounted, computed } from 'vue'
import { Calendar, Check } from '@element-plus/icons-vue'
import { useAgentStore } from '@/stores/agent'
import { useSettingsStore } from '@/stores/settings'

const agent = useAgentStore()
const settings = useSettingsStore()

// Task 9 Step 5: a cloud provider must be configured before the user can rely
// on model-generated content. Without one we show the explicit state instead
// of pretending "local mode" is a model answer.
const cloudAvailable = computed(() => {
  const config = settings.llmConfig
  return !!config && config.provider !== 'ollama' && !!config.baseUrl && !!config.model
})

onMounted(async () => {
  // settings 由 App.vue 统一加载；这里只读取已加载的配置状态。
  await agent.loadBrief()
  await agent.refreshApprovals()
  // Task 12: the brief is a pure on-demand local read — no event listener.
})

function formatRate(value: number): string {
  return `${Math.round(value * 100)}%`
}
</script>

<template>
  <section class="brief-section">
    <h2 class="section-title">每日简报</h2>

    <el-alert
      v-if="!cloudAvailable"
      data-test="brief-no-cloud"
      class="brief-no-cloud"
      type="info"
      :closable="false"
      show-icon
      title="尚未连接云端模型。你仍可查看和编辑本地数据；连接模型后可使用对话、计划调整和智能复盘。"
    />

    <article v-if="agent.brief" class="brief-card" data-test="brief-card">
      <p class="brief-date">
        <el-icon><Calendar /></el-icon>
        {{ agent.brief.date }} · 本地统计
      </p>
      <p class="brief-summary" data-test="brief-summary">{{ agent.brief.summary }}</p>
      <p v-if="agent.brief.explanation" class="brief-explanation" data-test="brief-explanation">
        {{ agent.brief.explanation }}
      </p>
      <div class="brief-stats">
        <span data-test="brief-today">
          今日 {{ agent.brief.today_completed }}/{{ agent.brief.today_planned }}
        </span>
        <span data-test="brief-overdue">逾期 {{ agent.brief.overdue_count }}</span>
        <span data-test="brief-week">本周 {{ formatRate(agent.brief.week_completion_rate) }}</span>
        <span v-if="agent.brief.due_wrong_questions > 0" data-test="brief-wrong">
          错题 {{ agent.brief.due_wrong_questions }}
        </span>
      </div>
      <el-button
        v-if="agent.hasBrief"
        data-test="brief-acknowledge"
        size="small"
        type="primary"
        plain
        @click="agent.acknowledgeBrief()"
      >
        <el-icon><Check /></el-icon>
        知道了
      </el-button>
    </article>
    <p v-else class="brief-empty" data-test="brief-empty">暂无简报（未选择考试或没有数据）。</p>
  </section>
</template>

<style scoped>
.brief-section {
  padding: 12px 16px;
  border-bottom: 1px solid var(--el-border-color);
}
.section-title {
  font-size: 14px;
  margin: 0 0 8px;
  color: var(--el-text-color-primary);
}
.brief-card {
  border: 1px solid var(--el-border-color);
  border-radius: 8px;
  padding: 12px;
  background: var(--el-bg-color);
}
.brief-date {
  display: flex;
  align-items: center;
  gap: 4px;
  margin: 0 0 6px;
  font-size: 12px;
  color: var(--el-text-color-secondary);
}
.brief-summary {
  margin: 0 0 6px;
  font-size: 14px;
}
.brief-explanation {
  margin: 0 0 8px;
  font-size: 13px;
  color: var(--el-color-primary);
}
.brief-stats {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
  margin-bottom: 8px;
  font-size: 12px;
  color: var(--el-text-color-secondary);
}
.brief-empty {
  font-size: 13px;
  color: var(--el-text-color-secondary);
}
</style>
