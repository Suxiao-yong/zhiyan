<script setup lang="ts">
import { ref } from 'vue'
import AgentSidebar from '@/components/agent/AgentSidebar.vue'
import ConversationPane from '@/components/agent/ConversationPane.vue'
import DailyBrief from '@/components/agent/DailyBrief.vue'
import ApprovalCard from '@/components/agent/ApprovalCard.vue'
import WorkbenchHost from '@/components/agent/WorkbenchHost.vue'
import { WORKBENCHES, type WorkbenchKey } from '@/components/agent/workbench'

// M6 Task 4: the right pane hosts a workbench registry; switching never
// unmounts the conversation pane.
const activeWorkbench = ref<WorkbenchKey>('checkin')
</script>

<template>
  <div class="agent-home" data-test="agent-home">
    <AgentSidebar />
    <main class="agent-center">
      <DailyBrief />
      <ConversationPane />
      <ApprovalCard />
    </main>
    <div class="agent-workbench">
      <div class="workbench-switcher" data-test="workbench-switcher">
        <button
          v-for="workbench in WORKBENCHES"
          :key="workbench.key"
          type="button"
          class="workbench-tab"
          :class="{ active: activeWorkbench === workbench.key }"
          :data-test="`workbench-tab-${workbench.key}`"
          @click="activeWorkbench = workbench.key"
        >
          {{ workbench.label }}
        </button>
      </div>
      <WorkbenchHost :workbench="activeWorkbench" />
    </div>
  </div>
</template>

<style scoped>
.agent-home {
  display: flex;
  height: 100vh;
  width: 100%;
  overflow: hidden;
  background: var(--el-bg-color-page);
}
.agent-center {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow-y: auto;
}
.agent-workbench {
  width: 46%;
  min-width: 380px;
  display: flex;
  flex-direction: column;
  border-left: 1px solid var(--el-border-color);
}
.workbench-switcher {
  padding: 8px 12px;
  border-bottom: 1px solid var(--el-border-color);
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}
.workbench-tab {
  border: 1px solid var(--el-border-color);
  background: var(--el-bg-color);
  border-radius: 4px;
  padding: 4px 10px;
  font-size: 12px;
  cursor: pointer;
  color: var(--el-text-color-regular);
}
.workbench-tab.active {
  border-color: var(--el-color-primary);
  color: var(--el-color-primary);
  background: var(--el-color-primary-light-9);
}
</style>
