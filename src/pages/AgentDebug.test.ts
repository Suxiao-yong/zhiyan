// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { AgentRun, AgentSession, ListedAgentTool } from '@/types'
import { useExamStore } from '@/stores/exam'

const client = vi.hoisted(() => ({
  agentHealth: vi.fn(),
  createAgentSession: vi.fn(),
  createAgentRun: vi.fn(),
  startAgentRun: vi.fn(),
  cancelAgentRun: vi.fn(),
  listAgentTools: vi.fn(),
  runAgentPlanner: vi.fn(),
  listAgentContextAudit: vi.fn(),
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

function listedTool(name: 'plan.get_today' | 'record.checkin_plan'): ListedAgentTool {
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
    ownership: 'rust-owned',
  }
}

const defaultTools: ListedAgentTool[] = [
  listedTool('plan.get_today'),
  listedTool('record.checkin_plan'),
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

  it('lists every tool as rust-owned with its descriptor', async () => {
    const wrapper = mountPage()
    await flushPromises()

    expect(client.listAgentTools).toHaveBeenCalledTimes(1)
    expect(wrapper.get('[data-test=tool-plan-descriptor]').text()).toContain('plan.get_today')
    expect(wrapper.get('[data-test=tool-plan-ownership]').text()).toContain('rust-owned')
    expect(wrapper.get('[data-test=tool-checkin-descriptor]').text()).toContain(
      'record.checkin_plan',
    )
    expect(wrapper.get('[data-test=tool-checkin-ownership]').text()).toContain('rust-owned')
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
})
