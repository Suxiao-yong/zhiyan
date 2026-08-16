<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import type {
  AgentContextAuditRow,
  AgentPlannerTurn,
  AgentRun,
  AgentSession,
  ListedAgentTool,
} from '@/types'
import { useExamStore } from '@/stores/exam'
import {
  agentHealth,
  cancelAgentRun,
  createAgentRun,
  createAgentSession,
  listAgentContextAudit,
  listAgentTools,
  runAgentPlanner,
  startAgentRun,
} from '@/services/agent-client'

const examStore = useExamStore()
const busy = ref(false)
const toolListFailed = ref(false)
const tools = ref<ListedAgentTool[]>([])
const plannerTurn = ref<AgentPlannerTurn | null>(null)
const plannerStream = ref('')
const contextAudit = ref<AgentContextAuditRow[]>([])
const contextAuditFailed = ref(false)
let unlistenPlanner: (() => void) | undefined
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
const plannerOutputJson = computed(() => JSON.stringify(plannerTurn.value, null, 2))
const plannerExecutable = computed(
  () => state.run?.status === 'running' && !!state.goal.trim() && !toolListFailed.value,
)

function toolSlug(name: string): string {
  if (name === 'plan.get_today') return 'plan'
  if (name === 'record.checkin_plan') return 'checkin'
  return name.replace('.', '-')
}

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
    plannerTurn.value = null
    plannerStream.value = ''
  })
}

async function cancelRun(): Promise<void> {
  if (!state.run) return
  await perform(async () => {
    state.run = await cancelAgentRun(state.run!.id)
  })
}

async function runPlanner(): Promise<void> {
  if (!state.run || !plannerExecutable.value) return
  await perform(async () => {
    plannerStream.value = ''
    plannerTurn.value = await runAgentPlanner(state.run!.id, state.goal)
    // Authoritative final text (event stream may have missed tail fragments).
    plannerStream.value = plannerTurn.value.final_text
    await loadContextAudit()
  })
}

async function loadContextAudit(): Promise<void> {
  if (!state.run) return
  try {
    contextAudit.value = await listAgentContextAudit(state.run.id)
    contextAuditFailed.value = false
  } catch (error) {
    contextAuditFailed.value = true
    state.error = errorMessage(error)
  }
}

function auditRowJson(row: AgentContextAuditRow): string {
  return JSON.stringify(
    {
      call_seq: row.call_seq,
      purpose: row.purpose,
      local: row.local,
      prompt_tokens: row.prompt_tokens,
      completion_tokens: row.completion_tokens,
      tools_offered: row.tools_offered,
      categories: row.categories,
      record_ids: row.record_ids,
      field_sets: row.field_sets,
      created_at: row.created_at,
    },
    null,
    2,
  )
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
    unlistenPlanner = await listen<{ run_id: string; text: string }>(
      'agent-planner-chunk',
      (event) => {
        if (state.run?.id === event.payload.run_id) {
          plannerStream.value += event.payload.text
        }
      },
    )
  })
})

onUnmounted(() => {
  unlistenPlanner?.()
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
      <h2>planner loop</h2>
      <button
        data-test="planner-run"
        type="button"
        :disabled="busy || !plannerExecutable"
        @click="runPlanner"
      >
        Run planner turn
      </button>
      <pre v-if="plannerStream" data-test="planner-stream">{{ plannerStream }}</pre>
      <pre v-if="plannerTurn" data-test="planner-output">{{ plannerOutputJson }}</pre>
    </section>

    <section class="tool-control" aria-label="Context Inspector">
      <h2>Context Inspector</h2>
      <p v-if="contextAuditFailed" data-test="context-audit-failed">读取审计记录失败</p>
      <button
        data-test="context-audit-refresh"
        type="button"
        :disabled="busy || toolListFailed || !state.run"
        @click="loadContextAudit"
      >
        刷新审计
      </button>
      <p v-if="contextAudit.length === 0 && !contextAuditFailed" data-test="context-audit-empty">
        暂无模型调用记录。运行一次 planner turn 后出现。
      </p>
      <article
        v-for="row in contextAudit"
        :key="row.id"
        class="audit-row"
        :data-test="`context-audit-row-${row.call_seq}`"
      >
        <p :data-test="`context-audit-call-${row.call_seq}`">
          #{{ row.call_seq }} · {{ row.purpose }} · {{ row.local ? '本地模式' : '模型调用' }} ·
          {{ row.created_at }}
        </p>
        <p :data-test="`context-audit-tokens-${row.call_seq}`">
          tokens: {{ row.prompt_tokens }} + {{ row.completion_tokens }}
        </p>
        <pre :data-test="`context-audit-json-${row.call_seq}`">{{ auditRowJson(row) }}</pre>
      </article>
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
