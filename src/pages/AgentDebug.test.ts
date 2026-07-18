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
}))

vi.mock('@/services/agent-client', () => client)

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
    ]) {
      expect(wrapper.get(selector).attributes('disabled')).toBeDefined()
    }
    expect(client.executeAgentTool).not.toHaveBeenCalled()
    expect(client.undoAgentTool).not.toHaveBeenCalled()
  })
})
