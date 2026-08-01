// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  AgentRun,
  AgentSession,
  AgentToolCallResponse,
  AgentToolUndoResponse,
  ListedAgentTool,
} from '@/types'
import { useExamStore } from '@/stores/exam'

const client = vi.hoisted(() => ({
  agentHealth: vi.fn(),
  createAgentSession: vi.fn(),
  createAgentRun: vi.fn(),
  startAgentRun: vi.fn(),
  cancelAgentRun: vi.fn(),
  listAgentTools: vi.fn(),
  executeAgentTool: vi.fn(),
  undoAgentTool: vi.fn(),
  runAgentPlanner: vi.fn(),
  listAgentContextAudit: vi.fn(),
  listAgentMemories: vi.fn(),
  createAgentMemory: vi.fn(),
  confirmAgentMemory: vi.fn(),
  updateAgentMemory: vi.fn(),
  deactivateAgentMemory: vi.fn(),
  deleteAgentMemory: vi.fn(),
  listAgentJobs: vi.fn(),
  scheduleAgentJob: vi.fn(),
  agentBriefPreview: vi.fn(),
}))

vi.mock('@/services/agent-client', () => client)

const eventApi = vi.hoisted(() => ({ listen: vi.fn().mockResolvedValue(() => {}) }))
vi.mock('@tauri-apps/api/event', () => eventApi)

import AgentDebug from './AgentDebug.vue'

const session: AgentSession = {
  id: 'session-1',
  exam_id: 'exam-1',
  title: 'Runtime test',
  status: 'active',
  created_at: '2026-07-18T00:00:00Z',
  updated_at: '2026-07-18T00:00:00Z',
}

const queuedRun: AgentRun = {
  id: 'run-1',
  session_id: 'session-1',
  goal: 'Inspect today plan',
  status: 'queued',
  trigger_source: 'user',
  current_step: 0,
  error_code: null,
  created_at: '2026-07-18T00:00:00Z',
  updated_at: '2026-07-18T00:00:00Z',
  started_at: null,
  completed_at: null,
}

const runningRun: AgentRun = { ...queuedRun, status: 'running', started_at: '2026-07-18T00:01:00Z' }

function listedTool(
  name: 'plan.get_today' | 'record.checkin_plan',
  ownership: ListedAgentTool['ownership'],
): ListedAgentTool {
  return {
    descriptor: {
      name,
      version: '1',
      risk: name === 'plan.get_today' ? 'R0' : 'R1',
      confirmation: 'automatic',
      supports_undo: name === 'record.checkin_plan',
      timeout_ms: 5_000,
      idempotency: name === 'plan.get_today' ? 'retry_safe' : 'required_exactly_once',
      data_permissions: ['study_plans'],
      input_schema: {},
      output_schema: {},
    },
    ownership,
  }
}

const defaultTools: ListedAgentTool[] = [
  listedTool('plan.get_today', 'shadow'),
  listedTool('record.checkin_plan', 'typescript'),
]

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise
  })
  return { promise, resolve }
}

function mountPage() {
  const pinia = createPinia()
  setActivePinia(pinia)
  useExamStore().setActiveExam('exam-1')
  return mount(AgentDebug, { global: { plugins: [pinia] } })
}

describe('AgentDebug', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    client.agentHealth.mockResolvedValue(undefined)
    client.createAgentSession.mockResolvedValue(session)
    client.createAgentRun.mockResolvedValue(queuedRun)
    client.startAgentRun.mockResolvedValue(runningRun)
    client.cancelAgentRun.mockResolvedValue({ ...runningRun, status: 'cancelled' })
    client.listAgentTools.mockResolvedValue(defaultTools)
    client.executeAgentTool.mockResolvedValue(undefined)
    client.undoAgentTool.mockResolvedValue(undefined)
    client.runAgentPlanner.mockResolvedValue({
      mode: 'local',
      final_text: '（本地模式）no llm provider configured，跳过模型推理。',
      iterations: 0,
      model_calls: 0,
      prompt_tokens: 0,
      completion_tokens: 0,
      trace: [{ kind: 'local_fallback', reason: 'no llm provider configured' }],
    })
    client.listAgentContextAudit.mockResolvedValue([])
    client.listAgentMemories.mockResolvedValue([])
    client.createAgentMemory.mockResolvedValue(undefined)
    client.confirmAgentMemory.mockResolvedValue(undefined)
    client.updateAgentMemory.mockResolvedValue(undefined)
    client.deactivateAgentMemory.mockResolvedValue(undefined)
    client.deleteAgentMemory.mockResolvedValue(undefined)
    client.listAgentJobs.mockResolvedValue([])
    client.scheduleAgentJob.mockResolvedValue(null)
    client.agentBriefPreview.mockResolvedValue({
      date: '2026-07-18',
      mode: 'local',
      summary: '今日计划 2 项，已完成 1 项（完成率 50%）。',
      explanation: null,
      today_planned: 2,
      today_completed: 1,
      today_duration_min: 120,
      overdue_count: 0,
      week_completion_rate: 0.5,
      due_wrong_questions: 0,
      weak_areas: [],
    })
  })

  it('shows health and creates then starts a runtime run', async () => {
    const wrapper = mountPage()
    await flushPromises()

    expect(wrapper.get('[data-test=health]').text()).toContain('可用')

    await wrapper.get('[data-test=create-session]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=start-run]').trigger('click')
    await flushPromises()

    expect(client.createAgentSession).toHaveBeenCalledWith('exam-1', 'Runtime test')
    expect(client.createAgentRun).toHaveBeenCalledWith('session-1', 'Inspect today plan')
    expect(client.startAgentRun).toHaveBeenCalledWith('run-1')
    expect(wrapper.get('[data-test=run-status]').text()).toContain('running')
  })

  it('shows command errors in an alert', async () => {
    client.createAgentSession.mockRejectedValue({ message: 'session rejected' })
    const wrapper = mountPage()
    await flushPromises()

    await wrapper.get('[data-test=create-session]').trigger('click')
    await flushPromises()

    expect(wrapper.get('[role=alert]').text()).toContain('session rejected')
  })

  it('prevents a second session creation while the first request is pending', async () => {
    const pendingSession = deferred<AgentSession>()
    client.createAgentSession.mockReturnValueOnce(pendingSession.promise)
    const wrapper = mountPage()
    await flushPromises()

    const button = wrapper.get('[data-test=create-session]')
    await button.trigger('click')
    await button.trigger('click')

    expect(client.createAgentSession).toHaveBeenCalledTimes(1)
    expect(button.attributes('disabled')).toBeDefined()

    pendingSession.resolve(session)
    await flushPromises()
  })

  it('disables cancellation for terminal runs', async () => {
    client.startAgentRun.mockResolvedValue({ ...runningRun, status: 'completed' })
    const wrapper = mountPage()
    await flushPromises()

    await wrapper.get('[data-test=create-session]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=start-run]').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-test=cancel-run]').attributes('disabled')).toBeDefined()
  })

  it('shows an unavailable health state when the health command fails', async () => {
    client.agentHealth.mockRejectedValue({ message: 'agent unavailable' })
    const wrapper = mountPage()
    await flushPromises()

    expect(wrapper.get('[data-test=health]').text()).toContain('不可用')
    expect(wrapper.get('[role=alert]').text()).toContain('agent unavailable')
  })

  it('executes a shadow plan read and keeps a TypeScript-owned check-in disabled', async () => {
    const response: AgentToolCallResponse = {
      state: 'completed',
      step_id: 'step-plan',
      output: { business_date: '2026-07-18', plans: [{ id: 'plan-1' }] },
      replayed: false,
      undo_available: false,
    }
    client.executeAgentTool.mockResolvedValue(response)
    const wrapper = mountPage()
    await flushPromises()

    expect(wrapper.get('[data-test=tool-plan-descriptor]').text()).toContain('plan.get_today')
    expect(wrapper.get('[data-test=tool-plan-descriptor]').text()).toContain('v1')
    expect(wrapper.get('[data-test=tool-plan-descriptor]').text()).toContain('R0')
    expect(wrapper.get('[data-test=tool-plan-ownership]').text()).toContain('shadow')
    expect(wrapper.get('[data-test=tool-checkin-ownership]').text()).toContain('typescript')
    expect(wrapper.get('[data-test=tool-checkin-execute]').attributes('disabled')).toBeDefined()

    await wrapper.get('[data-test=create-session]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=start-run]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=tool-plan-exam-id]').setValue('exam-shadow')
    await wrapper.get('[data-test=tool-plan-execute]').trigger('click')
    await flushPromises()

    expect(client.executeAgentTool).toHaveBeenCalledWith({
      run_id: 'run-1',
      step_index: 0,
      tool_name: 'plan.get_today',
      tool_version: '1',
      input: { exam_id: 'exam-shadow' },
      idempotency_key: null,
      approval_id: null,
    })
    expect(wrapper.get('[data-test=tool-plan-output]').text()).toContain('business_date')
    expect(wrapper.get('[data-test=tool-plan-output]').text()).toContain('plan-1')
  })

  it('executes a rust-owned check-in exactly once and enables undo from its receipt', async () => {
    client.listAgentTools.mockResolvedValue([
      listedTool('plan.get_today', 'shadow'),
      listedTool('record.checkin_plan', 'rust-owned'),
    ])
    const response: AgentToolCallResponse = {
      state: 'completed',
      step_id: 'step-checkin',
      output: { record_id: 'record-1' },
      replayed: false,
      undo_available: true,
    }
    const undo: AgentToolUndoResponse = {
      step_id: 'step-checkin',
      output: {
        record_id: 'record-1',
        plan_id: 'plan-1',
        removed_wrong_question_ids: [],
        actual_duration: 0,
        actual_tasks: null,
        status: 'pending',
      },
    }
    client.executeAgentTool.mockResolvedValue(response)
    client.undoAgentTool.mockResolvedValue(undo)
    const wrapper = mountPage()
    await flushPromises()

    await wrapper.get('[data-test=create-session]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=start-run]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=tool-checkin-plan-id]').setValue('plan-1')
    await wrapper.get('[data-test=tool-checkin-execute]').trigger('click')
    await flushPromises()

    expect(client.executeAgentTool).toHaveBeenCalledTimes(1)
    expect(wrapper.get('[data-test=tool-checkin-receipt]').text()).toContain('step-checkin')
    expect(wrapper.get('[data-test=tool-checkin-receipt]').text()).toContain('false')
    expect(wrapper.get('[data-test=tool-checkin-receipt]').text()).toContain('true')
    expect(wrapper.get('[data-test=tool-checkin-undo]').attributes('disabled')).toBeUndefined()

    await wrapper.get('[data-test=tool-checkin-undo]').trigger('click')
    await flushPromises()

    expect(client.undoAgentTool).toHaveBeenCalledWith('step-checkin')
  })

  it('advances the local step between a fresh plan read and check-in in the same run', async () => {
    client.listAgentTools.mockResolvedValue([
      listedTool('plan.get_today', 'shadow'),
      listedTool('record.checkin_plan', 'rust-owned'),
    ])
    client.executeAgentTool
      .mockResolvedValueOnce({
        state: 'completed',
        step_id: 'step-plan',
        output: { business_date: '2026-07-18', plans: [] },
        replayed: false,
        undo_available: false,
      } satisfies AgentToolCallResponse)
      .mockResolvedValueOnce({
        state: 'completed',
        step_id: 'step-checkin',
        output: { record_id: 'record-1' },
        replayed: false,
        undo_available: true,
      } satisfies AgentToolCallResponse)
    const wrapper = mountPage()
    await flushPromises()

    await wrapper.get('[data-test=create-session]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=start-run]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=tool-plan-execute]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=tool-checkin-plan-id]').setValue('plan-1')
    await wrapper.get('[data-test=tool-checkin-execute]').trigger('click')
    await flushPromises()

    expect(client.executeAgentTool).toHaveBeenCalledTimes(2)
    expect(client.executeAgentTool).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({ tool_name: 'plan.get_today', step_index: 0 }),
    )
    expect(client.executeAgentTool).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({
        tool_name: 'record.checkin_plan',
        step_index: 1,
        idempotency_key: 'agent-debug:run-1:1',
      }),
    )
  })

  it('synchronizes past the submitted step when a completed call is replayed', async () => {
    client.listAgentTools.mockResolvedValue([
      listedTool('plan.get_today', 'shadow'),
      listedTool('record.checkin_plan', 'rust-owned'),
    ])
    client.executeAgentTool
      .mockResolvedValueOnce({
        state: 'completed',
        step_id: 'step-plan',
        output: { business_date: '2026-07-18', plans: [] },
        replayed: true,
        undo_available: false,
      } satisfies AgentToolCallResponse)
      .mockResolvedValueOnce({
        state: 'completed',
        step_id: 'step-checkin',
        output: { record_id: 'record-1' },
        replayed: false,
        undo_available: true,
      } satisfies AgentToolCallResponse)
    const wrapper = mountPage()
    await flushPromises()

    await wrapper.get('[data-test=create-session]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=start-run]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=tool-plan-execute]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=tool-checkin-plan-id]').setValue('plan-1')
    await wrapper.get('[data-test=tool-checkin-execute]').trigger('click')
    await flushPromises()

    expect(client.executeAgentTool).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({ step_index: 1, idempotency_key: 'agent-debug:run-1:1' }),
    )
  })

  it('does not move a newer local step backward for an old replay response', async () => {
    client.listAgentTools.mockResolvedValue([
      listedTool('plan.get_today', 'shadow'),
      listedTool('record.checkin_plan', 'rust-owned'),
    ])
    const oldReplay = deferred<AgentToolCallResponse>()
    client.executeAgentTool.mockReturnValueOnce(oldReplay.promise).mockResolvedValueOnce({
      state: 'completed',
      step_id: 'step-checkin',
      output: { record_id: 'record-1' },
      replayed: false,
      undo_available: true,
    } satisfies AgentToolCallResponse)
    const wrapper = mountPage()
    await flushPromises()

    await wrapper.get('[data-test=create-session]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=start-run]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=tool-plan-execute]').trigger('click')

    const vm = wrapper.vm as unknown as { state: { run: AgentRun | null } }
    vm.state.run = { ...vm.state.run!, current_step: 2 }
    oldReplay.resolve({
      state: 'completed',
      step_id: 'step-plan-old',
      output: { business_date: '2026-07-18', plans: [] },
      replayed: true,
      undo_available: false,
    })
    await flushPromises()
    await wrapper.get('[data-test=tool-checkin-plan-id]').setValue('plan-1')
    await wrapper.get('[data-test=tool-checkin-execute]').trigger('click')
    await flushPromises()

    expect(client.executeAgentTool).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({ step_index: 2, idempotency_key: 'agent-debug:run-1:2' }),
    )
  })

  it('disables plan and check-in execution when run start leaves it queued', async () => {
    client.listAgentTools.mockResolvedValue([
      listedTool('plan.get_today', 'shadow'),
      listedTool('record.checkin_plan', 'rust-owned'),
    ])
    client.startAgentRun.mockRejectedValue({ message: 'start rejected' })
    const wrapper = mountPage()
    await flushPromises()

    await wrapper.get('[data-test=tool-checkin-plan-id]').setValue('plan-1')
    await wrapper.get('[data-test=create-session]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=start-run]').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-test=run-status]').text()).toContain('queued')
    expect(wrapper.get('[data-test=tool-plan-execute]').attributes('disabled')).toBeDefined()
    expect(wrapper.get('[data-test=tool-checkin-execute]').attributes('disabled')).toBeDefined()
  })

  it('disables plan and check-in execution after the run is cancelled', async () => {
    client.listAgentTools.mockResolvedValue([
      listedTool('plan.get_today', 'shadow'),
      listedTool('record.checkin_plan', 'rust-owned'),
    ])
    const wrapper = mountPage()
    await flushPromises()

    await wrapper.get('[data-test=tool-checkin-plan-id]').setValue('plan-1')
    await wrapper.get('[data-test=create-session]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=start-run]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=cancel-run]').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-test=run-status]').text()).toContain('cancelled')
    expect(wrapper.get('[data-test=tool-plan-execute]').attributes('disabled')).toBeDefined()
    expect(wrapper.get('[data-test=tool-checkin-execute]').attributes('disabled')).toBeDefined()
  })

  it('rejects a fractional check-in duration in the debug gate', async () => {
    client.listAgentTools.mockResolvedValue([
      listedTool('plan.get_today', 'shadow'),
      listedTool('record.checkin_plan', 'rust-owned'),
    ])
    const wrapper = mountPage()
    await flushPromises()

    await wrapper.get('[data-test=create-session]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=start-run]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=tool-checkin-plan-id]').setValue('plan-1')
    const duration = wrapper.get('[data-test=tool-checkin-duration]')
    await duration.setValue('1.5')

    expect(duration.attributes('step')).toBe('1')
    expect(wrapper.get('[data-test=tool-checkin-execute]').attributes('disabled')).toBeDefined()
  })

  it('shows a redacted persistence list error and disables every write control', async () => {
    client.listAgentTools.mockRejectedValue({
      code: 'persistence_error',
      message: 'agent persistence failed',
    })
    const wrapper = mountPage()
    await flushPromises()

    expect(wrapper.get('[role=alert]').text()).toContain('agent persistence failed')
    for (const selector of [
      '[data-test=create-session]',
      '[data-test=start-run]',
      '[data-test=cancel-run]',
      '[data-test=tool-plan-execute]',
      '[data-test=tool-checkin-execute]',
      '[data-test=tool-checkin-undo]',
      '[data-test=planner-run]',
    ]) {
      expect(wrapper.get(selector).attributes('disabled')).toBeDefined()
    }
    expect(client.executeAgentTool).not.toHaveBeenCalled()
    expect(client.undoAgentTool).not.toHaveBeenCalled()
  })

  it('runs a planner turn and renders the trace when a run is active', async () => {
    client.listAgentContextAudit.mockResolvedValue([
      {
        id: 'audit-1',
        call_seq: 1,
        purpose: 'planner_turn',
        local: true,
        prompt_tokens: 0,
        completion_tokens: 0,
        tools_offered: [],
        categories: [],
        record_ids: {},
        field_sets: {},
        created_at: '2026-07-18T00:00:00',
      },
    ])
    const wrapper = mountPage()
    await flushPromises()

    // Disabled until a run is running.
    expect(wrapper.get('[data-test=planner-run]').attributes('disabled')).toBeDefined()

    await wrapper.get('[data-test=create-session]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=start-run]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=planner-run]').trigger('click')
    await flushPromises()

    expect(client.runAgentPlanner).toHaveBeenCalledWith('run-1', 'Inspect today plan')
    expect(wrapper.get('[data-test=planner-output]').text()).toContain('local')
    expect(wrapper.get('[data-test=planner-output]').text()).toContain('local_fallback')
    // The final streamed text is rendered live.
    expect(wrapper.get('[data-test=planner-stream]').text()).toContain('本地模式')
    expect(eventApi.listen).toHaveBeenCalledWith('agent-planner-chunk', expect.any(Function))
    // A planner turn refreshes the Context Inspector rows.
    expect(client.listAgentContextAudit).toHaveBeenCalledWith('run-1')
    expect(wrapper.get('[data-test=context-audit-call-1]').text()).toContain('本地模式')
    expect(wrapper.get('[data-test=context-audit-tokens-1]').text()).toContain('0 + 0')
  })

  it('renders Context Inspector rows with offered tools and data scope after a refresh', async () => {
    client.listAgentContextAudit.mockResolvedValue([
      {
        id: 'audit-1',
        call_seq: 1,
        purpose: 'planner_turn',
        local: false,
        prompt_tokens: 120,
        completion_tokens: 30,
        tools_offered: ['plan.get_today'],
        categories: ['exam', 'plan'],
        record_ids: { exam: ['exam-1'], plan: ['plan-1'] },
        field_sets: { plan: ['id', 'date', 'status'] },
        created_at: '2026-07-18T00:00:00',
      },
    ])
    const wrapper = mountPage()
    await flushPromises()

    expect(wrapper.get('[data-test=context-audit-empty]').text()).toContain('暂无模型调用记录')

    await wrapper.get('[data-test=create-session]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=start-run]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-test=context-audit-refresh]').trigger('click')
    await flushPromises()

    expect(client.listAgentContextAudit).toHaveBeenCalledWith('run-1')
    expect(wrapper.get('[data-test=context-audit-call-1]').text()).toContain('模型调用')
    const json = wrapper.get('[data-test=context-audit-json-1]').text()
    expect(json).toContain('plan.get_today')
    expect(json).toContain('exam-1')
    expect(json).toContain('plan-1')
    expect(json).toContain('field_sets')
    // No raw business content is shown.
    expect(json).not.toContain('今天复习数学')
  })

  it('loads memories on mount and renders their type, status, and source', async () => {
    client.listAgentMemories.mockResolvedValue([
      {
        id: 'memory-1',
        exam_id: null,
        memory_type: 'daily_capacity',
        content: '每天最多学习两小时',
        source: 'user_statement',
        confidence: 1,
        status: 'confirmed',
        created_at: '2026-07-18T00:00:00',
        updated_at: '2026-07-18T00:00:00',
        last_used_at: null,
      },
      {
        id: 'memory-2',
        exam_id: null,
        memory_type: 'confirmed_weakness',
        content: '二次函数压轴题',
        source: 'model_candidate',
        confidence: 0.5,
        status: 'candidate',
        created_at: '2026-07-18T00:00:00',
        updated_at: '2026-07-18T00:00:00',
        last_used_at: null,
      },
    ])
    const wrapper = mountPage()
    await flushPromises()

    expect(client.listAgentMemories).toHaveBeenCalledWith(null, true)
    expect(wrapper.get('[data-test=memory-meta-memory-1]').text()).toContain('daily_capacity')
    expect(wrapper.get('[data-test=memory-meta-memory-1]').text()).toContain('confirmed')
    expect(wrapper.get('[data-test=memory-content-memory-1]').text()).toContain('每天最多学习两小时')
    // A candidate memory shows a confirm action; a confirmed one does not.
    expect(wrapper.get('[data-test=memory-confirm-memory-2]').exists()).toBe(true)
    expect(wrapper.find('[data-test=memory-confirm-memory-1]').exists()).toBe(false)
  })

  it('creates a memory from the form and prepends it to the list', async () => {
    client.createAgentMemory.mockResolvedValue({
      id: 'memory-new',
      exam_id: null,
      memory_type: 'schedule_preference',
      content: '周末上午学习',
      source: 'user_statement',
      confidence: 0.7,
      status: 'confirmed',
      created_at: '2026-07-18T00:00:00',
      updated_at: '2026-07-18T00:00:00',
      last_used_at: null,
    })
    const wrapper = mountPage()
    await flushPromises()

    await wrapper.get('[data-test=memory-create-content]').setValue('周末上午学习')
    await wrapper.find('form.memory-create').trigger('submit')
    await flushPromises()

    expect(client.createAgentMemory).toHaveBeenCalledWith(
      expect.objectContaining({
        exam_id: 'exam-1',
        memory_type: 'schedule_preference',
        content: '周末上午学习',
        source: 'user_statement',
        confidence: 0.7,
      }),
    )
    expect(wrapper.get('[data-test=memory-meta-memory-new]').text()).toContain('confirmed')
  })

  it('confirms a candidate memory in place', async () => {
    client.listAgentMemories.mockResolvedValue([
      {
        id: 'memory-2',
        exam_id: null,
        memory_type: 'confirmed_weakness',
        content: '二次函数压轴题',
        source: 'model_candidate',
        confidence: 0.5,
        status: 'candidate',
        created_at: '2026-07-18T00:00:00',
        updated_at: '2026-07-18T00:00:00',
        last_used_at: null,
      },
    ])
    client.confirmAgentMemory.mockResolvedValue({
      id: 'memory-2',
      exam_id: null,
      memory_type: 'confirmed_weakness',
      content: '二次函数压轴题',
      source: 'model_candidate',
      confidence: 0.5,
      status: 'confirmed',
      created_at: '2026-07-18T00:00:00',
      updated_at: '2026-07-18T00:00:00',
      last_used_at: null,
    })
    const wrapper = mountPage()
    await flushPromises()

    await wrapper.get('[data-test=memory-confirm-memory-2]').trigger('click')
    await flushPromises()

    expect(client.confirmAgentMemory).toHaveBeenCalledWith('memory-2')
    expect(wrapper.get('[data-test=memory-meta-memory-2]').text()).toContain('confirmed')
    expect(wrapper.find('[data-test=memory-confirm-memory-2]').exists()).toBe(false)
  })

  it('edits a memory inline and deactivates and deletes it', async () => {
    client.listAgentMemories.mockResolvedValue([
      {
        id: 'memory-1',
        exam_id: null,
        memory_type: 'daily_capacity',
        content: '每天两小时',
        source: 'user_statement',
        confidence: 1,
        status: 'confirmed',
        created_at: '2026-07-18T00:00:00',
        updated_at: '2026-07-18T00:00:00',
        last_used_at: null,
      },
    ])
    client.updateAgentMemory.mockResolvedValue({
      id: 'memory-1',
      exam_id: null,
      memory_type: 'daily_capacity',
      content: '每天三小时',
      source: 'user_statement',
      confidence: 1,
      status: 'confirmed',
      created_at: '2026-07-18T00:00:00',
      updated_at: '2026-07-18T00:00:00',
      last_used_at: null,
    })
    client.deactivateAgentMemory.mockResolvedValue({
      id: 'memory-1',
      exam_id: null,
      memory_type: 'daily_capacity',
      content: '每天三小时',
      source: 'user_statement',
      confidence: 1,
      status: 'inactive',
      created_at: '2026-07-18T00:00:00',
      updated_at: '2026-07-18T00:00:00',
      last_used_at: null,
    })
    const wrapper = mountPage()
    await flushPromises()

    await wrapper.get('[data-test=memory-edit-memory-1]').trigger('click')
    await wrapper.get('[data-test=memory-edit-input-memory-1]').setValue('每天三小时')
    await wrapper.get('[data-test=memory-edit-save]').trigger('click')
    await flushPromises()
    expect(client.updateAgentMemory).toHaveBeenCalledWith('memory-1', '每天三小时')
    expect(wrapper.get('[data-test=memory-content-memory-1]').text()).toContain('每天三小时')

    await wrapper.get('[data-test=memory-deactivate-memory-1]').trigger('click')
    await flushPromises()
    expect(client.deactivateAgentMemory).toHaveBeenCalledWith('memory-1')
    expect(wrapper.get('[data-test=memory-meta-memory-1]').text()).toContain('inactive')

    await wrapper.get('[data-test=memory-delete-memory-1]').trigger('click')
    await flushPromises()
    expect(client.deleteAgentMemory).toHaveBeenCalledWith('memory-1')
    expect(wrapper.find('[data-test=memory-row-memory-1]').exists()).toBe(false)
  })

  it('loads background jobs on mount and schedules a new one', async () => {
    client.listAgentJobs.mockResolvedValue([
      {
        id: 'job-1',
        job_type: 'daily_brief',
        dedup_key: 'daily_brief:2026-07-18',
        scheduled_at: '2026-07-18 08:00:00',
        status: 'completed',
        last_result: { mode: 'local', overdue_count: 0 },
        retry_at: null,
        runs: 1,
        last_run_at: '2026-07-18 08:00:00',
        created_at: '2026-07-18 00:00:00',
      },
    ])
    const wrapper = mountPage()
    await flushPromises()

    expect(client.listAgentJobs).toHaveBeenCalledWith(50)
    expect(wrapper.get('[data-test=job-meta-job-1]').text()).toContain('daily_brief')
    expect(wrapper.get('[data-test=job-meta-job-1]').text()).toContain('completed')
    expect(wrapper.get('[data-test=job-result-job-1]').text()).toContain('overdue_count')

    await wrapper.get('[data-test=job-create-key]').setValue('overdue_check:2026-07-19')
    await wrapper.get('[data-test=job-create-at]').setValue('2026-07-19 09:00:00')
    await wrapper.find('form.job-create').trigger('submit')
    await flushPromises()

    expect(client.scheduleAgentJob).toHaveBeenCalledWith(
      'daily_brief',
      'overdue_check:2026-07-19',
      '2026-07-19 09:00:00',
    )
  })

  it('previews the daily brief', async () => {
    const wrapper = mountPage()
    await flushPromises()

    await wrapper.get('[data-test=brief-preview]').trigger('click')
    await flushPromises()

    expect(client.agentBriefPreview).toHaveBeenCalledWith('exam-1')
    expect(wrapper.get('[data-test=brief-mode]').text()).toContain('local')
    expect(wrapper.get('[data-test=brief-summary]').text()).toContain('今日计划 2 项')
    expect(wrapper.get('[data-test=brief-output]').text()).toContain('week_completion_rate')
  })
})
