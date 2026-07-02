<script setup lang="ts">
defineProps<{
  title: string
  value: string | number
  unit?: string
  icon?: any
  hint?: string
}>()
</script>

<template>
  <div class="stat-card">
    <div class="stat-card__top">
      <span class="stat-card__title">{{ title }}</span>
      <span v-if="icon" class="stat-card__icon">
        <el-icon :size="18"><component :is="icon" /></el-icon>
      </span>
    </div>
    <div class="stat-card__value">
      <span class="stat-card__num tnum">{{ value }}</span>
      <span v-if="unit" class="stat-card__unit">{{ unit }}</span>
    </div>
    <div v-if="hint" class="stat-card__hint">{{ hint }}</div>
  </div>
</template>

<style scoped>
.stat-card {
  background: var(--c-surface);
  border: 1px solid var(--c-border);
  border-radius: var(--r-lg);
  padding: var(--sp-4) var(--sp-5);
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
  height: 100%;
}
.stat-card__top {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.stat-card__title {
  font-size: var(--fs-sm);
  color: var(--c-ink-3);
}
.stat-card__icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border-radius: var(--r-md);
  background: var(--c-primary-soft);
  color: var(--c-primary);
}
.stat-card__value {
  display: flex;
  align-items: baseline;
  gap: 4px;
}
.stat-card__num {
  font-size: var(--fs-3xl);
  font-weight: 700;
  color: var(--c-ink);
  line-height: 1.1;
  letter-spacing: -0.02em;
}
.stat-card__unit {
  font-size: var(--fs-sm);
  color: var(--c-ink-3);
  font-weight: 500;
}
.stat-card__hint {
  font-size: var(--fs-xs);
  color: var(--c-ink-muted);
}
</style>
