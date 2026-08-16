<script setup lang="ts">
import PlanCheckinBoard from '@/components/record/PlanCheckinBoard.vue'
import StudyPlan from '@/pages/StudyPlan.vue'
import StudyRecord from '@/pages/StudyRecord.vue'
import { WORKBENCHES, type WorkbenchKey } from './workbench'

/**
 * Right pane of the Agent OS shell. Task 3: only the three core study
 * surfaces (check-in / plan / records) mount here; Analysis and Visualization
 * are no longer page embeddings. Switching workbenches never loses the
 * conversation (the pane is isolated from the center pane).
 */
defineProps<{ workbench: WorkbenchKey }>()
</script>

<template>
  <section class="workbench-host" data-test="workbench-host">
    <header class="workbench-title">
      {{ WORKBENCHES.find((w) => w.key === workbench)?.label }}
    </header>
    <div class="workbench-body">
      <PlanCheckinBoard v-if="workbench === 'checkin'" data-test="workbench-checkin" />
      <StudyPlan v-else-if="workbench === 'plan'" data-test="workbench-plan" />
      <StudyRecord
        v-else-if="workbench === 'record'"
        data-test="workbench-record"
        initial-tab="records"
      />
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
