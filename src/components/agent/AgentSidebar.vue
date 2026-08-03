<script setup lang="ts">
import { onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { Plus, ChatDotRound, Odometer, Calendar, EditPen, Monitor } from '@element-plus/icons-vue'
import { useAgentStore } from '@/stores/agent'

const router = useRouter()
const agent = useAgentStore()

const workbenchLinks = [
  { path: '/dashboard', label: '仪表盘', icon: Odometer },
  { path: '/study-plan', label: '学习计划', icon: Calendar },
  { path: '/study-record', label: '学习记录', icon: EditPen },
  { path: '/agent-debug', label: 'Agent 调试', icon: Monitor },
]

onMounted(async () => {
  await agent.refreshSessions()
})

function formatTime(value: string): string {
  return value.slice(11, 16)
}
</script>

<template>
  <aside class="agent-sidebar" data-test="agent-sidebar">
    <div class="sidebar-brand">智研 Agent</div>
    <button
      class="new-session"
      data-test="agent-new-session"
      type="button"
      :disabled="agent.busy"
      @click="agent.createSession()"
    >
      <el-icon><Plus /></el-icon>
      新会话
    </button>

    <div class="sidebar-section-label">最近会话</div>
    <ul class="session-list" data-test="agent-session-list">
      <li v-if="agent.sessions.length === 0" class="session-empty" data-test="agent-sessions-empty">
        暂无会话
      </li>
      <li
        v-for="session in agent.sessions"
        :key="session.id"
        class="session-item"
        :class="{ active: session.id === agent.activeSessionId }"
        :data-test="`agent-session-${session.id}`"
        @click="agent.selectSession(session.id)"
      >
        <el-icon class="session-icon"><ChatDotRound /></el-icon>
        <div class="session-text">
          <div class="session-title">{{ session.title }}</div>
          <div class="session-time">{{ formatTime(session.updated_at) }}</div>
        </div>
      </li>
    </ul>

    <div class="sidebar-section-label">工作台</div>
    <ul class="workbench-links" data-test="agent-workbench-links">
      <li v-for="link in workbenchLinks" :key="link.path">
        <el-link :underline="false" @click="router.push(link.path)">
          <el-icon><component :is="link.icon" /></el-icon>
          {{ link.label }}
        </el-link>
      </li>
    </ul>
  </aside>
</template>

<style scoped>
.agent-sidebar {
  width: 240px;
  border-right: 1px solid var(--el-border-color);
  background: var(--el-bg-color-page);
  display: flex;
  flex-direction: column;
  overflow-y: auto;
  padding: 12px;
}
.sidebar-brand {
  font-weight: 600;
  margin-bottom: 12px;
  color: var(--el-text-color-primary);
}
.new-session {
  display: flex;
  align-items: center;
  gap: 6px;
  justify-content: center;
  padding: 8px;
  border: 1px dashed var(--el-border-color);
  border-radius: 6px;
  background: transparent;
  cursor: pointer;
  margin-bottom: 12px;
  color: var(--el-text-color-primary);
}
.new-session:hover {
  border-color: var(--el-color-primary);
  color: var(--el-color-primary);
}
.sidebar-section-label {
  font-size: 12px;
  color: var(--el-text-color-secondary);
  margin: 8px 0 4px;
}
.session-list,
.workbench-links {
  list-style: none;
  margin: 0;
  padding: 0;
}
.session-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px;
  border-radius: 6px;
  cursor: pointer;
}
.session-item:hover {
  background: var(--el-fill-color-light);
}
.session-item.active {
  background: var(--el-color-primary-light-9);
  color: var(--el-color-primary);
}
.session-icon {
  flex-shrink: 0;
}
.session-text {
  min-width: 0;
}
.session-title {
  font-size: 13px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.session-time {
  font-size: 11px;
  color: var(--el-text-color-secondary);
}
.session-empty {
  padding: 8px;
  color: var(--el-text-color-secondary);
  font-size: 13px;
}
.workbench-links li {
  padding: 6px 8px;
}
.workbench-links .el-link {
  gap: 6px;
  font-size: 13px;
}
</style>
