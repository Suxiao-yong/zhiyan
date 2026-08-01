<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import type {
  AgentBrief,
  AgentContextAuditRow,
  AgentJob,
  AgentJobType,
  AgentMemoryCreateInput,
  AgentMemoryRecord,
  AgentMemorySource,
  AgentMemoryType,
  AgentPlannerTurn,
  AgentRun,
  AgentSession,
  AgentToolCallResponse,
  AgentToolUndoResponse,
  ListedAgentTool,
} from '@/types'
import { useExamStore } from '@/stores/exam'
import {
  agentBriefPreview,
  agentHealth,
  cancelAgentRun,
  confirmAgentMemory,
  createAgentMemory,
  createAgentRun,
  createAgentSession,
  deactivateAgentMemory,
  deleteAgentMemory,
  executeAgentTool,
  listAgentContextAudit,
  listAgentJobs,
  listAgentMemories,
  listAgentTools,
  runAgentPlanner,
  scheduleAgentJob,
  startAgentRun,
  undoAgentTool,
  updateAgentMemory,
} from '@/services/agent-client'

const examStore = useExamStore()
const busy = ref(false)
const toolListFailed = ref(false)
const tools = ref<ListedAgentTool[]>([])
const planOutput = ref<unknown>(null)
const checkinReceipt = ref<Extract<AgentToolCallResponse, { state: 'completed' }> | null>(null)
const undoOutput = ref<AgentToolUndoResponse | null>(null)
const plannerTurn = ref<AgentPlannerTurn | null>(null)
const plannerStream = ref('')
const contextAudit = ref<AgentContextAuditRow[]>([])
const contextAuditFailed = ref(false)
const memories = ref<AgentMemoryRecord[]>([])
const memoryForm = reactive<AgentMemoryCreateInput>({
  exam_id: examStore.activeExamId ?? null,
  memory_type: 'schedule_preference',
  content: '',
  source: 'user_statement',
  confidence: 0.7,
})
const editingMemoryId = ref<string | null>(null)
const editingContent = ref('')
const memoryTypes: AgentMemoryType[] = [
  'schedule_preference',
  'daily_capacity',
  'subject_preference',
  'learning_constraint',
  'reminder_preference',
  'strategy_preference',
  'confirmed_weakness',
]
const memorySources: AgentMemorySource[] = ['user_statement', 'behavior_inferred', 'model_candidate']
const jobs = ref<AgentJob[]>([])
const jobForm = reactive({
  job_type: 'daily_brief' as AgentJobType,
  dedup_key: '',
  scheduled_at: '',
})
const jobTypes: AgentJobType[] = [
  'daily_brief',
  'task_reminder',
  'overdue_check',
  'weekly_report',
  'retry_failed',
  'cleanup_failed',
]
const brief = ref<AgentBrief | null>(null)
let unlistenPlanner: (() => void) | undefined
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
const plannerOutputJson = computed(() => JSON.stringify(plannerTurn.value, null, 2))
const plannerExecutable = computed(
  () =>
    state.run?.status === 'running' && !!state.goal.trim() && !toolListFailed.value,
)

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

async function loadMemories(): Promise<void> {
  try {
    memories.value = await listAgentMemories(null, true)
  } catch (error) {
    state.error = errorMessage(error)
  }
}

async function createMemory(): Promise<void> {
  const content = memoryForm.content.trim()
  if (!content) {
    state.error = '记忆内容不能为空'
    return
  }
  await perform(async () => {
    const created = await createAgentMemory({ ...memoryForm, content })
    memoryForm.content = ''
    memories.value = [created, ...memories.value]
  })
}

function startEditingMemory(memory: AgentMemoryRecord): void {
  editingMemoryId.value = memory.id
  editingContent.value = memory.content
}

function cancelEditingMemory(): void {
  editingMemoryId.value = null
  editingContent.value = ''
}

async function saveMemoryEdit(memory: AgentMemoryRecord): Promise<void> {
  const content = editingContent.value.trim()
  if (!content) {
    state.error = '记忆内容不能为空'
    return
  }
  await perform(async () => {
    const updated = await updateAgentMemory(memory.id, content)
    memories.value = memories.value.map((entry) => (entry.id === updated.id ? updated : entry))
    cancelEditingMemory()
  })
}

async function confirmMemory(memory: AgentMemoryRecord): Promise<void> {
  await perform(async () => {
    const updated = await confirmAgentMemory(memory.id)
    memories.value = memories.value.map((entry) => (entry.id === updated.id ? updated : entry))
  })
}

async function deactivateMemory(memory: AgentMemoryRecord): Promise<void> {
  await perform(async () => {
    const updated = await deactivateAgentMemory(memory.id)
    memories.value = memories.value.map((entry) => (entry.id === updated.id ? updated : entry))
  })
}

async function removeMemory(memory: AgentMemoryRecord): Promise<void> {
  await perform(async () => {
    await deleteAgentMemory(memory.id)
    memories.value = memories.value.filter((entry) => entry.id !== memory.id)
  })
}

async function loadJobs(): Promise<void> {
  try {
    jobs.value = await listAgentJobs(50)
  } catch (error) {
    state.error = errorMessage(error)
  }
}

async function scheduleJob(): Promise<void> {
  const dedupKey = jobForm.dedup_key.trim()
  const scheduledAt = jobForm.scheduled_at.trim()
  if (!dedupKey || !scheduledAt) {
    state.error = 'dedup_key 和 scheduled_at 不能为空'
    return
  }
  await perform(async () => {
    await scheduleAgentJob(jobForm.job_type, dedupKey, scheduledAt)
    jobForm.dedup_key = ''
    jobForm.scheduled_at = ''
    await loadJobs()
  })
}

async function loadBrief(): Promise<void> {
  await perform(async () => {
    brief.value = await agentBriefPreview(examStore.activeExamId)
  })
}

function jobResultJson(job: AgentJob): string {
  return JSON.stringify(job.last_result, null, 2)
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
    await loadMemories()
    await loadJobs()
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
      <p v-if="contextAuditFailed" data-test="context-audit-failed">
        读取审计记录失败
      </p>
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

    <section class="tool-control" aria-label="Daily brief">
      <h2>Daily Brief（每日简报）</h2>
      <button
        data-test="brief-preview"
        type="button"
        :disabled="busy || toolListFailed"
        @click="loadBrief"
      >
        预览今日简报
      </button>
      <p v-if="brief" data-test="brief-mode">mode: {{ brief.mode }} · {{ brief.date }}</p>
      <p v-if="brief" data-test="brief-summary">{{ brief.summary }}</p>
      <p v-if="brief?.explanation" data-test="brief-explanation">{{ brief.explanation }}</p>
      <pre v-if="brief" data-test="brief-output">{{ JSON.stringify(brief, null, 2) }}</pre>
    </section>

    <section class="tool-control" aria-label="Background jobs">
      <h2>Background Jobs（后台任务）</h2>
      <button
        data-test="jobs-refresh"
        type="button"
        :disabled="busy || toolListFailed"
        @click="loadJobs"
      >
        刷新任务
      </button>

      <form class="job-create" @submit.prevent="scheduleJob">
        <label>
          类型
          <select v-model="jobForm.job_type" data-test="job-create-type">
            <option v-for="type in jobTypes" :key="type" :value="type">{{ type }}</option>
          </select>
        </label>
        <label>
          dedup_key
          <input v-model="jobForm.dedup_key" data-test="job-create-key" type="text" />
        </label>
        <label>
          scheduled_at
          <input v-model="jobForm.scheduled_at" data-test="job-create-at" type="text" />
        </label>
        <button
          data-test="job-create-submit"
          type="submit"
          :disabled="busy || toolListFailed"
        >
          排程任务
        </button>
      </form>

      <p v-if="jobs.length === 0" data-test="jobs-empty">暂无任务。</p>
      <article
        v-for="job in jobs"
        :key="job.id"
        class="job-row"
        :data-test="`job-row-${job.id}`"
      >
        <p :data-test="`job-meta-${job.id}`">
          {{ job.job_type }} · {{ job.status }} · {{ job.scheduled_at }} · runs {{ job.runs }}
        </p>
        <p :data-test="`job-key-${job.id}`">dedup: {{ job.dedup_key }}</p>
        <pre v-if="job.last_result" :data-test="`job-result-${job.id}`">
{{ jobResultJson(job) }}
        </pre>
      </article>
    </section>

    <section class="tool-control" aria-label="Memory management">
      <h2>Memory（结构化记忆）</h2>
      <button
        data-test="memory-refresh"
        type="button"
        :disabled="busy || toolListFailed"
        @click="loadMemories"
      >
        刷新记忆
      </button>

      <form class="memory-create" @submit.prevent="createMemory">
        <label>
          类型
          <select v-model="memoryForm.memory_type" data-test="memory-create-type">
            <option v-for="type in memoryTypes" :key="type" :value="type">{{ type }}</option>
          </select>
        </label>
        <label>
          来源
          <select v-model="memoryForm.source" data-test="memory-create-source">
            <option v-for="source in memorySources" :key="source" :value="source">
              {{ source }}
            </option>
          </select>
        </label>
        <label>
          置信度
          <input
            v-model.number="memoryForm.confidence"
            data-test="memory-create-confidence"
            type="number"
            min="0"
            max="1"
            step="0.1"
          />
        </label>
        <label>
          内容
          <input v-model="memoryForm.content" data-test="memory-create-content" type="text" />
        </label>
        <button
          data-test="memory-create-submit"
          type="submit"
          :disabled="busy || toolListFailed"
        >
          创建记忆
        </button>
      </form>

      <p v-if="memories.length === 0" data-test="memory-empty">暂无记忆。</p>
      <article
        v-for="memory in memories"
        :key="memory.id"
        class="memory-row"
        :data-test="`memory-row-${memory.id}`"
      >
        <p :data-test="`memory-meta-${memory.id}`">
          {{ memory.memory_type }} · {{ memory.status }} · {{ memory.source }} · confidence
          {{ memory.confidence }}
        </p>
        <p v-if="editingMemoryId !== memory.id" :data-test="`memory-content-${memory.id}`">
          {{ memory.content }}
        </p>
        <label v-else>
          内容
          <input
            v-model="editingContent"
            :data-test="`memory-edit-input-${memory.id}`"
            type="text"
          />
        </label>
        <div class="memory-actions">
          <button
            v-if="editingMemoryId === memory.id"
            data-test="memory-edit-save"
            type="button"
            :disabled="busy"
            @click="saveMemoryEdit(memory)"
          >
            保存
          </button>
          <button
            v-if="editingMemoryId === memory.id"
            data-test="memory-edit-cancel"
            type="button"
            :disabled="busy"
            @click="cancelEditingMemory"
          >
            取消
          </button>
          <template v-else>
            <button
              v-if="memory.status === 'candidate'"
              :data-test="`memory-confirm-${memory.id}`"
              type="button"
              :disabled="busy"
              @click="confirmMemory(memory)"
            >
              确认
            </button>
            <button
              v-if="memory.status !== 'inactive'"
              :data-test="`memory-deactivate-${memory.id}`"
              type="button"
              :disabled="busy"
              @click="deactivateMemory(memory)"
            >
              停用
            </button>
            <button
              :data-test="`memory-edit-${memory.id}`"
              type="button"
              :disabled="busy"
              @click="startEditingMemory(memory)"
            >
              编辑
            </button>
            <button
              :data-test="`memory-delete-${memory.id}`"
              type="button"
              :disabled="busy"
              @click="removeMemory(memory)"
            >
              删除
            </button>
          </template>
        </div>
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
