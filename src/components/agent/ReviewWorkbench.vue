<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { reviewListDue, type DueReviewItem } from '@/services/agent-client'
import { useAgentStore } from '@/stores/agent'

/**
 * Right-pane workbench card: today's due wrong-question reviews (local read).
 * "开始复习" hands off to the Agent conversation with a prefilled goal.
 */
const agent = useAgentStore()
const loading = ref(true)
const items = ref<DueReviewItem[]>([])

onMounted(async () => {
  try {
    const output = await reviewListDue(null)
    items.value = output.items
  } catch {
    items.value = []
  } finally {
    loading.value = false
  }
})

async function startReview(): Promise<void> {
  await agent.sendMessage('帮我安排今天的错题复习')
}
</script>

<template>
  <section class="review-card" data-test="review-card">
    <p v-if="loading" class="review-empty" data-test="review-loading">加载中…</p>
    <template v-else>
      <p v-if="items.length === 0" class="review-empty" data-test="review-empty">
        暂无待复习错题。
      </p>
      <template v-else>
        <ul class="review-list" data-test="review-list">
          <li v-for="item in items" :key="item.id" class="review-item">
            <span class="review-subject">{{ item.subject_name }}</span>
            <span class="review-desc">{{ item.question_desc ?? '（无题干）' }}</span>
          </li>
        </ul>
        <el-button type="primary" size="small" plain data-test="review-start" @click="startReview">
          开始复习
        </el-button>
      </template>
    </template>
  </section>
</template>

<style scoped>
.review-card {
  font-size: 13px;
}
.review-empty {
  margin: 0;
  color: var(--el-text-color-secondary);
}
.review-list {
  list-style: none;
  margin: 0 0 10px;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.review-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  border: 1px solid var(--el-border-color);
  border-radius: 6px;
  padding: 8px;
  background: var(--el-bg-color);
}
.review-subject {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}
.review-desc {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
