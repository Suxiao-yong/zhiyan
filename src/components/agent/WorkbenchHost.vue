<script setup lang="ts">
import PlanCheckinBoard from '@/components/record/PlanCheckinBoard.vue'

/**
 * Right pane of the Agent OS shell. M5 hosts exactly one workbench: the plan
 * check-in board, reused from the existing page components (spec §14.3).
 */
</script>

<template>
  <section class="workbench-host" data-test="workbench-host">
    <header class="workbench-title">计划打卡</header>
    <div class="workbench-body">
      <PlanCheckinBoard data-test="workbench-checkin" />
    </div>
  </section>
</template>

<style scoped>
.workbench-host {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.workbench-title {
  padding: 10px 16px;
  font-weight: 600;
  border-bottom: 1px solid var(--el-border-color);
}
.workbench-body {
  flex: 1;
  overflow-y: auto;
  padding: 12px;
}
</style>
