<script setup lang="ts">
import { computed } from 'vue'
import { Promotion } from '@element-plus/icons-vue'
import { useAgentStore } from '@/stores/agent'
import AgentStatus from './AgentStatus.vue'

const agent = useAgentStore()

const draft = computed({
  get: () => agent.inputText,
  set: (value: string) => agent.setInputText(value),
})

function submit(): void {
  const text = agent.inputText.trim()
  if (!text || agent.busy) return
  agent.sendMessage(text)
  agent.setInputText('')
}

function messageClass(role: string): string {
  return role === 'user' ? 'bubble-user' : 'bubble-assistant'
}
</script>

<template>
  <section class="conversation-pane" data-test="conversation-pane">
    <AgentStatus />
    <div class="message-stream" data-test="message-stream">
      <p v-if="agent.messages.length === 0" class="stream-empty" data-test="messages-empty">
        发送一条消息，Agent 会分析今日计划并给出建议。
      </p>
      <article
        v-for="message in agent.messages"
        :key="message.id"
        class="message-row"
        :class="messageClass(message.role)"
        :data-test="`message-${message.id}`"
      >
        <div class="bubble">
          <p class="message-text">{{ message.text }}</p>
          <p v-if="message.role === 'assistant'" class="message-meta">
            tokens {{ message.prompt_tokens }}+{{ message.completion_tokens }}
          </p>
        </div>
      </article>
      <p v-if="agent.busy" class="stream-busy" data-test="messages-busy">Agent 正在处理…</p>
    </div>

    <form class="composer" @submit.prevent="submit">
      <el-input
        v-model="draft"
        data-test="composer-input"
        type="textarea"
        :rows="2"
        placeholder="输入你想让 Agent 做的事，例如：看今天的计划"
        :disabled="agent.busy"
      />
      <el-button
        data-test="composer-send"
        type="primary"
        native-type="submit"
        :disabled="agent.busy || !agent.inputText.trim()"
      >
        <el-icon><Promotion /></el-icon>
        发送
      </el-button>
    </form>
  </section>
</template>

<style scoped>
.conversation-pane {
  display: flex;
  flex-direction: column;
  min-width: 0;
  border-right: 1px solid var(--el-border-color);
}
.message-stream {
  flex: 1;
  overflow-y: auto;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.stream-empty {
  color: var(--el-text-color-secondary);
  text-align: center;
  margin-top: 40px;
}
.stream-busy {
  color: var(--el-color-primary);
  font-size: 13px;
}
.message-row {
  display: flex;
}
.message-row.bubble-user {
  justify-content: flex-end;
}
.bubble {
  max-width: 78%;
  padding: 8px 12px;
  border-radius: 10px;
  background: var(--el-fill-color-light);
}
.bubble-user .bubble {
  background: var(--el-color-primary-light-8);
}
.message-text {
  margin: 0;
  white-space: pre-wrap;
  font-size: 14px;
}
.message-meta {
  margin: 4px 0 0;
  font-size: 11px;
  color: var(--el-text-color-secondary);
}
.composer {
  display: flex;
  gap: 8px;
  padding: 12px;
  border-top: 1px solid var(--el-border-color);
  align-items: flex-end;
}
</style>
