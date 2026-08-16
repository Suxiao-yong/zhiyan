<script setup lang="ts">
/**
 * 统一的 Agent 空状态 / 引导状态组件（Task 15 Step 4）。只负责展示明确的
 * 中文标题、说明与可选图标/动作，禁止只显示 loading spinner 或空白面板。
 */
defineProps<{
  title: string
  description?: string
  icon?: unknown
}>()
</script>

<template>
  <div class="agent-empty" data-test="agent-empty">
    <el-icon v-if="icon" class="agent-empty__icon"><component :is="icon" /></el-icon>
    <p class="agent-empty__title" data-test="agent-empty-title">{{ title }}</p>
    <p v-if="description" class="agent-empty__description" data-test="agent-empty-description">
      {{ description }}
    </p>
    <div v-if="$slots.default" class="agent-empty__action">
      <slot />
    </div>
  </div>
</template>

<style scoped>
.agent-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--sp-2, 8px);
  padding: 32px 24px;
  text-align: center;
  color: var(--el-text-color-secondary);
}
.agent-empty__icon {
  font-size: 28px;
  color: var(--el-text-color-placeholder);
}
.agent-empty__title {
  margin: 0;
  font-size: 14px;
  font-weight: 500;
  color: var(--el-text-color-primary);
}
.agent-empty__description {
  margin: 0;
  max-width: 420px;
  font-size: 13px;
  line-height: 1.7;
}
</style>
