<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import type { AgentRun, AgentSession } from '@/types'
import { useExamStore } from '@/stores/exam'
import {
  agentHealth,
  cancelAgentRun,
  createAgentRun,
  createAgentSession,
  startAgentRun,
} from '@/services/agent-client'

const examStore = useExamStore()
const busy = ref(false)
const state = reactive({
  healthy: false,
  session: null as AgentSession | null,
  run: null as AgentRun | null,
  goal: 'Inspect today plan',
  error: '',
})

const canCancelRun = computed(() =>
  state.run
    ? ['queued', 'running', 'waiting_approval', 'interrupted'].includes(state.run.status)
    : false,
)

function errorMessage(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const { message } = error as { message?: unknown }
    if (typeof message === 'string') return message
  }
  return '运行命令失败'
}

async function perform(operation: () => Promise<void>): Promise<void> {
  if (busy.value) return
  state.error = ''
  busy.value = true
  try {
    await operation()
  } catch (error) {
    state.error = errorMessage(error)
  } finally {
    busy.value = false
  }
}

async function createSession(): Promise<void> {
  await perform(async () => {
    state.session = await createAgentSession(examStore.activeExamId, 'Runtime test')
  })
}

async function createAndStartRun(): Promise<void> {
  if (!state.session) return
  await perform(async () => {
    state.run = await createAgentRun(state.session!.id, state.goal)
    state.run = await startAgentRun(state.run.id)
  })
}

async function cancelRun(): Promise<void> {
  if (!state.run) return
  await perform(async () => {
    state.run = await cancelAgentRun(state.run!.id)
  })
}

onMounted(() => {
  void perform(async () => {
    await agentHealth()
    state.healthy = true
  })
})
</script>

<template>
  <section class="agent-debug">
    <h1>Agent Runtime Debug</h1>
    <p data-test="health">健康状态：{{ state.healthy ? '可用' : '不可用' }}</p>
    <p data-test="session">会话：{{ state.session?.id ?? '未创建' }}</p>
    <p data-test="run">运行：{{ state.run?.id ?? '未创建' }}</p>
    <p data-test="run-status">状态：{{ state.run?.status ?? 'idle' }}</p>

    <label>
      目标
      <input v-model="state.goal" data-test="goal" type="text" />
    </label>

    <div class="actions">
      <button data-test="create-session" type="button" :disabled="busy" @click="createSession">创建会话</button>
      <button data-test="start-run" type="button" :disabled="busy || !state.session" @click="createAndStartRun">
        创建并启动
      </button>
      <button data-test="cancel-run" type="button" :disabled="busy || !canCancelRun" @click="cancelRun">取消</button>
    </div>

    <p v-if="state.error" role="alert">{{ state.error }}</p>
  </section>
</template>

<style scoped>
.agent-debug {
  max-width: 560px;
  padding: var(--sp-6);
}

.actions {
  display: flex;
  gap: var(--sp-2);
  margin-top: var(--sp-4);
}

input {
  display: block;
  width: 100%;
  margin-top: var(--sp-1);
}
</style>
