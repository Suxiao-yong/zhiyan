<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import {
  flashcardListDue,
  reviewListDue,
  type DueReviewItem,
  type FlashcardDueItem,
} from '@/services/agent-client'
import { useAgentStore } from '@/stores/agent'

/**
 * Right-pane workbench card: today's due wrong questions + flashcards (local read).
 * Either source failing degrades to an empty group, never blocks the other.
 * "开始复习" hands off to the Agent conversation with a prefilled goal.
 */
const agent = useAgentStore()
const loading = ref(true)
const wrongItems = ref<DueReviewItem[]>([])
const flashcards = ref<FlashcardDueItem[]>([])

onMounted(async () => {
  const [wrong, cards] = await Promise.allSettled([reviewListDue(null), flashcardListDue(null)])
  if (wrong.status === 'fulfilled') wrongItems.value = wrong.value.items
  if (cards.status === 'fulfilled') flashcards.value = cards.value.items
  loading.value = false
})

const total = computed(() => wrongItems.value.length + flashcards.value.length)

async function startReview(): Promise<void> {
  await agent.sendMessage('帮我安排今天的错题和闪卡复习')
}
</script>

<template>
  <section class="review-card" data-test="review-card">
    <p v-if="loading" class="review-empty" data-test="review-loading">加载中…</p>
    <template v-else>
      <p class="review-total" data-test="review-total">今日待复习 {{ total }} 项</p>
      <p v-if="total === 0" class="review-empty" data-test="review-empty">暂无待复习内容。</p>
      <template v-else>
        <div v-if="wrongItems.length" data-test="wrong-section">
          <p class="section-title">错题({{ wrongItems.length }})</p>
          <ul class="review-list" data-test="review-list">
            <li v-for="item in wrongItems" :key="item.id" class="review-item">
              <span class="review-subject">{{ item.subject_name }}</span>
              <span class="review-desc">{{ item.question_desc ?? '（无题干）' }}</span>
            </li>
          </ul>
        </div>
        <div v-if="flashcards.length" data-test="flashcard-section">
          <p class="section-title">闪卡({{ flashcards.length }})</p>
          <ul class="review-list">
            <li v-for="card in flashcards" :key="card.id" class="review-item">
              <span v-if="card.knowledge_point_name" class="review-subject">
                {{ card.knowledge_point_name }}
              </span>
              <span class="review-desc">{{ card.front }}</span>
            </li>
          </ul>
        </div>
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
.review-empty,
.review-total {
  margin: 0;
  color: var(--el-text-color-secondary);
}
.section-title {
  margin: 0 0 6px;
  font-weight: 600;
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
