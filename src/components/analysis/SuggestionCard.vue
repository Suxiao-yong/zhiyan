<script setup lang="ts">
import type { Recommendation } from '@/services/analyzer'

defineProps<{
  suggestion: Recommendation
  state: number // user_confirmed: 0/1/2
  applied: boolean
}>()

defineEmits<{ confirm: []; reject: []; apply: [] }>()

// 优先级 → el-tag 类型（取代原 hex 数组）：1 紧急/2 高/3 中/4 低/5 很低
const priorityTypes = ['', 'danger', 'warning', 'primary', 'info', 'info']
const priorityLabels = ['', '紧急', '高', '中', '低', '很低']
</script>

<template>
  <div class="sug-card" :class="{ applied, rejected: state === 2, confirmed: state === 1 }">
    <div class="sug-card__head">
      <el-tag size="small" :type="priorityTypes[suggestion.priority] || 'info'" effect="light">
        {{ priorityLabels[suggestion.priority] || '普通' }}
      </el-tag>
      <span class="action">{{ suggestion.action }}</span>
      <span v-if="applied" class="badge applied">已应用</span>
      <span v-else-if="state === 1" class="badge confirmed">已确认</span>
      <span v-else-if="state === 2" class="badge rejected">已拒绝</span>
    </div>
    <p v-if="suggestion.reason" class="reason">{{ suggestion.reason }}</p>
    <div class="sug-card__actions">
      <el-button v-if="!applied && state !== 1" size="small" type="primary" @click="$emit('apply')">
        确认应用
      </el-button>
      <el-button v-if="state === 1 && !applied" size="small" type="success" @click="$emit('apply')">
        应用到数据
      </el-button>
      <el-button v-if="state === 0" size="small" @click="$emit('confirm')">确认</el-button>
      <el-button
        v-if="state !== 2 && !applied"
        size="small"
        link
        type="danger"
        @click="$emit('reject')"
      >
        拒绝
      </el-button>
    </div>
  </div>
</template>

<style scoped>
.sug-card {
  border: 1px solid var(--c-border);
  border-radius: var(--r-md);
  padding: var(--sp-3) var(--sp-4);
  background: var(--c-surface);
  transition:
    border-color var(--dur-fast) var(--ease),
    background var(--dur-fast) var(--ease);
}
.sug-card.applied {
  border-color: var(--c-success);
  background: var(--c-success-light);
}
.sug-card.rejected {
  opacity: 0.55;
  border-color: var(--c-danger);
}
.sug-card.confirmed {
  border-color: var(--c-primary);
}
.sug-card__head {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  margin-bottom: var(--sp-2);
}
.action {
  font-size: var(--fs-base);
  font-weight: 500;
  color: var(--c-ink);
  flex: 1;
  min-width: 0;
}
.badge {
  font-size: var(--fs-xs);
  font-weight: 500;
  padding: 1px var(--sp-2);
  border-radius: var(--r-sm);
  color: var(--c-bg);
  flex-shrink: 0;
}
.badge.applied {
  background: var(--c-success);
}
.badge.confirmed {
  background: var(--c-primary);
}
.badge.rejected {
  background: var(--c-danger);
}
.reason {
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
  margin: 0 0 var(--sp-2);
}
.sug-card__actions {
  display: flex;
  gap: var(--sp-2);
  flex-wrap: wrap;
}
</style>
