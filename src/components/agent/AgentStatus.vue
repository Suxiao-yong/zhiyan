<script setup lang="ts">
import { computed } from 'vue'
import { useAgentStore } from '@/stores/agent'

const agent = useAgentStore()

const statusLabel = computed(() => {
  if (!agent.run) return 'idle'
  return agent.run.status
})
</script>

<template>
  <div class="agent-status" data-test="agent-status">
    <span class="status-pill" :class="`status-${statusLabel}`" :data-test="`status-${statusLabel}`">
      {{ statusLabel }}
    </span>
    <el-button
      v-if="agent.run && (agent.run.status === 'queued' || agent.run.status === 'running')"
      data-test="status-cancel"
      size="small"
      @click="agent.cancelRun()"
    >
      取消
    </el-button>
    <span v-if="agent.error" class="status-error" data-test="status-error">{{ agent.error }}</span>
  </div>
</template>

<style scoped>
.agent-status {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 16px;
  border-bottom: 1px solid var(--el-border-color);
  background: var(--el-bg-color);
}
.status-pill {
  font-size: 12px;
  padding: 2px 10px;
  border-radius: 10px;
  background: var(--el-fill-color);
  color: var(--el-text-color-secondary);
}
.status-running,
.status-waiting_approval {
  background: var(--el-color-warning-light-8);
  color: var(--el-color-warning);
}
.status-completed {
  background: var(--el-color-success-light-8);
  color: var(--el-color-success);
}
.status-failed,
.status-cancelled,
.status-interrupted {
  background: var(--el-color-danger-light-8);
  color: var(--el-color-danger);
}
.status-error {
  color: var(--el-color-danger);
  font-size: 12px;
}
</style>
