<script setup lang="ts">
import { DataAnalysis } from '@element-plus/icons-vue'

defineProps<{
  title: string
  description?: string
  icon?: any
}>()

defineEmits<{ action: [] }>()
</script>

<template>
  <div class="empty-state">
    <div class="empty-icon">
      <el-icon :size="32"><component :is="icon ?? DataAnalysis" /></el-icon>
    </div>
    <h3 class="empty-title">{{ title }}</h3>
    <p v-if="description" class="empty-desc">{{ description }}</p>
    <div class="empty-action"><slot /></div>
  </div>
</template>

<style scoped>
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: var(--sp-10) var(--sp-6);
  text-align: center;
}
.empty-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 56px;
  height: 56px;
  border-radius: var(--r-pill);
  background: var(--c-primary-soft);
  color: var(--c-primary);
  margin-bottom: var(--sp-4);
}
.empty-title {
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--c-ink-2);
  margin: 0 0 var(--sp-2);
}
.empty-desc {
  font-size: var(--fs-sm);
  color: var(--c-ink-3);
  max-width: 440px;
  line-height: 1.6;
  margin: 0 0 var(--sp-4);
}
.empty-action {
  margin-top: var(--sp-2);
}
</style>
