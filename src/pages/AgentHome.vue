<script setup lang="ts">
import AgentSidebar from '@/components/agent/AgentSidebar.vue'
import ConversationPane from '@/components/agent/ConversationPane.vue'
import DailyBrief from '@/components/agent/DailyBrief.vue'
import ApprovalCard from '@/components/agent/ApprovalCard.vue'
import WorkbenchHost from '@/components/agent/WorkbenchHost.vue'
</script>

<template>
  <div class="agent-home" data-test="agent-home">
    <AgentSidebar />
    <main class="agent-center">
      <DailyBrief />
      <ConversationPane />
      <ApprovalCard />
    </main>
    <WorkbenchHost />
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
</style>
