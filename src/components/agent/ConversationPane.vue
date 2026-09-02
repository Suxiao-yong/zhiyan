<script setup lang="ts">
import { computed } from 'vue'
import { Promotion, ChatDotRound, Connection } from '@element-plus/icons-vue'
import { useAgentStore } from '@/stores/agent'
import { useExamStore } from '@/stores/exam'
import { useSettingsStore } from '@/stores/settings'
import AgentStatus from './AgentStatus.vue'
import AgentEmptyState from './AgentEmptyState.vue'

const agent = useAgentStore()
const settings = useSettingsStore()
const examStore = useExamStore()

// Task 9 Step 5: explicit cloud-state messaging instead of pretending "local
// mode" is a model answer.
const cloudAvailable = computed(() => {
  const config = settings.llmConfig
  return !!config && config.provider !== 'ollama' && !!config.baseUrl && !!config.model
})

// Task 15 Step 4: every empty state carries an explicit next step.
const emptyTitle = computed(() => {
  if (!examStore.activeExamId) return '请先配置考试'
  if (!cloudAvailable.value) return '尚未连接云端模型'
  return '开始今天的对话'
})

const emptyDescription = computed(() => {
  if (!examStore.activeExamId) {
    return '前往“设置”添加考试与科目后，Agent 才能基于你的学习数据给出建议。'
  }
  if (!cloudAvailable.value) {
    return '你仍可查看和编辑本地数据；连接模型后可使用对话、计划调整和智能复盘。'
  }
  return '发送一条消息，Agent 会结合今日计划、近期记录和错题给出建议。'
})

const emptyIcon = computed(() => {
  if (!examStore.activeExamId) return null
  if (!cloudAvailable.value) return Connection
  return ChatDotRound
})

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
      <AgentEmptyState
        v-if="agent.messages.length === 0"
        class="stream-empty"
        data-test="messages-empty"
        :title="emptyTitle"
        :description="emptyDescription"
        :icon="emptyIcon"
      />
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
            <span
              v-if="
                (message.prompt_cache_hit_tokens ?? 0) + (message.prompt_cache_miss_tokens ?? 0) > 0
              "
            >
              · 缓存 {{ message.prompt_cache_hit_tokens }}+{{ message.prompt_cache_miss_tokens }}
            </span>
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
