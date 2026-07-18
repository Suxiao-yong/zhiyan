<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import type {
  AgentRun,
  AgentSession,
  AgentToolCallResponse,
  AgentToolUndoResponse,
  ListedAgentTool,
} from '@/types'
import { useExamStore } from '@/stores/exam'
import {
  agentHealth,
  cancelAgentRun,
  createAgentRun,
  createAgentSession,
  executeAgentTool,
  listAgentTools,
  startAgentRun,
  undoAgentTool,
} from '@/services/agent-client'

const examStore = useExamStore()
const busy = ref(false)
const toolListFailed = ref(false)
const tools = ref<ListedAgentTool[]>([])
const planOutput = ref<unknown>(null)
const checkinReceipt = ref<Extract<AgentToolCallResponse, { state: 'completed' }> | null>(null)
const undoOutput = ref<AgentToolUndoResponse | null>(null)
const state = reactive({
  healthy: false,
  session: null as AgentSession | null,
  run: null as AgentRun | null,
  goal: 'Inspect today plan',
  error: '',
  planExamId: examStore.activeExamId ?? '',
  checkinPlanId: '',
  checkinDuration: 1,
})

const planTool = computed(() =>
  tools.value.find((tool) => tool.descriptor.name === 'plan.get_today'),
)
const checkinTool = computed(() =>
  tools.value.find((tool) => tool.descriptor.name === 'record.checkin_plan'),
)
const canCancelRun = computed(() =>
  state.run
    ? ['queued', 'running', 'waiting_approval', 'interrupted'].includes(state.run.status)
    : false,
)
const planExecutable = computed(
  () =>
    !!planTool.value &&
    ['shadow', 'rust-owned'].includes(planTool.value.ownership) &&
    state.run?.status === 'running' &&
    !!state.planExamId.trim() &&
    !toolListFailed.value,
)
const checkinExecutable = computed(
  () =>
    checkinTool.value?.ownership === 'rust-owned' &&
    state.run?.status === 'running' &&
    !!state.checkinPlanId.trim() &&
    Number.isSafeInteger(state.checkinDuration) &&
    state.checkinDuration > 0 &&
    !checkinReceipt.value &&
    !toolListFailed.value,
)
const planOutputJson = computed(() => JSON.stringify(planOutput.value, null, 2))
const undoOutputJson = computed(() => JSON.stringify(undoOutput.value, null, 2))

function toolSlug(name: string): string {
  return name === 'plan.get_today' ? 'plan' : 'checkin'
}

function errorMessage(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const { message } = error as { message?: unknown }
    if (typeof message === 'string') return message
  }
  return '运行命令失败'
}

function advanceLocalRun(response: AgentToolCallResponse, submittedStep: number): void {
  if (response.state === 'completed' && state.run) {
    state.run = {
      ...state.run,
      current_step: Math.max(state.run.current_step, submittedStep + 1),
    }
  }
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
    planOutput.value = null
    checkinReceipt.value = null
    undoOutput.value = null
  })
}

async function cancelRun(): Promise<void> {
  if (!state.run) return
  await perform(async () => {
    state.run = await cancelAgentRun(state.run!.id)
  })
}

async function executePlanRead(): Promise<void> {
  if (!state.run || !planTool.value || !planExecutable.value) return
  await perform(async () => {
    const submittedStep = state.run!.current_step
    const response = await executeAgentTool({
      run_id: state.run!.id,
      step_index: submittedStep,
      tool_name: planTool.value!.descriptor.name,
      tool_version: planTool.value!.descriptor.version,
      input: { exam_id: state.planExamId.trim() },
      idempotency_key: null,
      approval_id: null,
    })
    planOutput.value = response.state === 'completed' ? response.output : response
    advanceLocalRun(response, submittedStep)
  })
}

async function executeCheckin(): Promise<void> {
  if (!state.run || !checkinTool.value || !checkinExecutable.value) return
  await perform(async () => {
    const submittedStep = state.run!.current_step
    const response = await executeAgentTool({
      run_id: state.run!.id,
      step_index: submittedStep,
      tool_name: checkinTool.value!.descriptor.name,
      tool_version: checkinTool.value!.descriptor.version,
      input: {
        plan_id: state.checkinPlanId.trim(),
        duration_min: state.checkinDuration,
        finish: false,
      },
      idempotency_key: `agent-debug:${state.run!.id}:${submittedStep}`,
      approval_id: null,
    })
    if (response.state === 'completed') checkinReceipt.value = response
    advanceLocalRun(response, submittedStep)
  })
}

async function undoCheckin(): Promise<void> {
  if (!checkinReceipt.value?.undo_available || toolListFailed.value) return
  await perform(async () => {
    undoOutput.value = await undoAgentTool(checkinReceipt.value!.step_id)
    checkinReceipt.value = { ...checkinReceipt.value!, undo_available: false }
  })
}

onMounted(() => {
  void perform(async () => {
    await agentHealth()
    state.healthy = true
    try {
      tools.value = await listAgentTools()
    } catch (error) {
      toolListFailed.value = true
      throw error
    }
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
      <button
        data-test="create-session"
        type="button"
        :disabled="busy || toolListFailed"
        @click="createSession"
      >
        创建会话
      </button>
      <button
        data-test="start-run"
        type="button"
        :disabled="busy || toolListFailed || !state.session"
        @click="createAndStartRun"
      >
        创建并启动
      </button>
      <button
        data-test="cancel-run"
        type="button"
        :disabled="busy || toolListFailed || !canCancelRun"
        @click="cancelRun"
      >
        取消
      </button>
    </div>

    <section class="tool-list" aria-label="Agent tools">
      <article v-for="tool in tools" :key="tool.descriptor.name" class="tool-card">
        <p :data-test="`tool-${toolSlug(tool.descriptor.name)}-descriptor`">
          {{ tool.descriptor.name }} v{{ tool.descriptor.version }} · {{ tool.descriptor.risk }}
        </p>
        <p :data-test="`tool-${toolSlug(tool.descriptor.name)}-ownership`">
          ownership: {{ tool.ownership }}
        </p>
      </article>
    </section>

    <section class="tool-control">
      <h2>plan.get_today</h2>
      <label>
        Exam ID
        <input v-model="state.planExamId" data-test="tool-plan-exam-id" type="text" />
      </label>
      <button
        data-test="tool-plan-execute"
        type="button"
        :disabled="busy || !planExecutable"
        @click="executePlanRead"
      >
        Execute plan read
      </button>
      <pre v-if="planOutput" data-test="tool-plan-output">{{ planOutputJson }}</pre>
    </section>

    <section class="tool-control">
      <h2>record.checkin_plan</h2>
      <p data-test="tool-checkin-gate">gate: {{ checkinTool?.ownership ?? 'unavailable' }}</p>
      <label>
        Plan ID
        <input v-model="state.checkinPlanId" data-test="tool-checkin-plan-id" type="text" />
      </label>
      <label>
        Duration
        <input
          v-model.number="state.checkinDuration"
          data-test="tool-checkin-duration"
          type="number"
          min="1"
          step="1"
        />
      </label>
      <button
        data-test="tool-checkin-execute"
        type="button"
        :disabled="busy || !checkinExecutable"
        @click="executeCheckin"
      >
        Execute check-in
      </button>
      <p v-if="checkinReceipt" data-test="tool-checkin-receipt">
        step_id: {{ checkinReceipt.step_id }} · replayed: {{ checkinReceipt.replayed }} ·
        undo_available: {{ checkinReceipt.undo_available }}
      </p>
      <button
        data-test="tool-checkin-undo"
        type="button"
        :disabled="busy || toolListFailed || !checkinReceipt?.undo_available"
        @click="undoCheckin"
      >
        Undo check-in
      </button>
      <pre v-if="undoOutput" data-test="tool-checkin-undo-output">{{ undoOutputJson }}</pre>
    </section>

    <p v-if="state.error" role="alert">{{ state.error }}</p>
  </section>
</template>

<style scoped>
.agent-debug {
  max-width: 720px;
  padding: var(--sp-6);
}

.actions,
.tool-list {
  display: flex;
  gap: var(--sp-2);
  margin-top: var(--sp-4);
}

.tool-card,
.tool-control {
  margin-top: var(--sp-4);
  padding: var(--sp-3);
  border: 1px solid var(--c-border);
}

input {
  display: block;
  width: 100%;
  margin: var(--sp-1) 0 var(--sp-2);
}

pre {
  overflow: auto;
  white-space: pre-wrap;
}
</style>
