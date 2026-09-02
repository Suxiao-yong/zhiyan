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
  height: 100%;
  min-height: 0;
  min-width: 0;
  width: 100%;
  overflow: auto;
  background: var(--el-bg-color-page);
}
.agent-center {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow-y: auto;
  overflow-x: hidden;
  /* Task 15: cap the reading width of the conversation column. */
  max-width: 920px;
  margin: 0 auto;
  width: 100%;
}
.agent-workbench {
  /* Task 15: the workbench never exceeds 36% of the window width. */
  width: 36%;
  min-width: 320px;
  max-width: 520px;
  min-height: 0;
  display: flex;
  flex-direction: column;
  border-left: 1px solid var(--el-border-color);
  overflow-y: auto;
}
/* 窄窗适配：1024 为 Tauri 最小宽，workbench 过宽会导致横向被裁切无法操作 */
@media (max-width: 1180px) {
  .agent-workbench {
    min-width: 280px;
    max-width: 360px;
  }
}
@media (max-width: 980px) {
  .agent-home {
    flex-direction: column;
    height: auto;
    min-height: 100%;
  }
  .agent-center {
    max-width: none;
    overflow: visible;
  }
  .agent-workbench {
    width: 100%;
    min-width: 0;
    max-width: none;
    border-left: none;
    border-top: 1px solid var(--el-border-color);
    overflow: visible;
  }
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
