<script setup lang="ts">
import { useAgentStore } from '@/stores/agent'
import AgentActionPreview from './AgentActionPreview.vue'

const agent = useAgentStore()
</script>

<template>
  <section class="approval-section">
    <h2 class="section-title">待审批</h2>
    <p v-if="agent.approvals.length === 0" class="approval-empty" data-test="approvals-empty">
      暂无待审批操作。
    </p>
    <article
      v-for="approval in agent.approvals"
      :key="approval.id"
      class="approval-card"
      :data-test="`approval-${approval.id}`"
    >
      <div class="approval-head">
        <span class="approval-risk">R{{ approval.risk }}</span>
        <span class="approval-status">{{ approval.status }}</span>
        <span class="approval-expiry">截止 {{ approval.expires_at.slice(11, 16) }}</span>
      </div>
      <!-- Task 10: render the standardized, sanitized preview instead of raw
           request fields. -->
      <AgentActionPreview :preview="approval.preview" />
      <div v-if="approval.status === 'pending'" class="approval-actions">
        <el-button
          :data-test="`approval-approve-${approval.id}`"
          size="small"
          type="success"
          :disabled="agent.busy || agent.isResolving(approval.id)"
          @click="agent.resolveApproval(approval.id, true)"
        >
          确认执行
        </el-button>
        <el-button
          :data-test="`approval-reject-${approval.id}`"
          size="small"
          type="danger"
          plain
          :disabled="agent.busy || agent.isResolving(approval.id)"
          @click="agent.resolveApproval(approval.id, false)"
        >
          取消
        </el-button>
      </div>
      <!-- Task 5: an executed write tool that supports undo offers a real
           revert through the Rust executor (never a state-only refresh). -->
      <div
        v-else-if="approval.status === 'approved' && approval.preview?.undo_available"
        class="approval-actions"
      >
        <el-button
          :data-test="`approval-undo-${approval.id}`"
          size="small"
          plain
          :disabled="agent.busy"
          @click="agent.undoTool(approval.step_id)"
        >
          撤销本次执行
        </el-button>
      </div>
    </article>
  </section>
</template>

<style scoped>
.approval-section {
  padding: 12px 16px;
  border-bottom: 1px solid var(--el-border-color);
}
.section-title {
  font-size: 14px;
  margin: 0 0 8px;
}
.approval-empty {
  font-size: 13px;
  color: var(--el-text-color-secondary);
}
.approval-card {
  border: 1px solid var(--el-border-color);
  border-radius: 8px;
  padding: 10px;
  margin-bottom: 8px;
}
.approval-head {
  display: flex;
  gap: 8px;
  align-items: center;
  margin-bottom: 6px;
  font-size: 12px;
}
.approval-risk {
  background: var(--el-color-danger-light-8);
  color: var(--el-color-danger);
  padding: 1px 8px;
  border-radius: 8px;
}
.approval-status {
  color: var(--el-text-color-secondary);
}
.approval-expiry {
  margin-left: auto;
  color: var(--el-text-color-secondary);
}
.approval-preview {
  margin: 0 0 8px;
  font-size: 13px;
}
.approval-actions {
  display: flex;
  gap: 8px;
}
</style>
