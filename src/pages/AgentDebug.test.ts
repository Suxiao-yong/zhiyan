// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { AgentRun, AgentSession } from '@/types'
import { useExamStore } from '@/stores/exam'

const client = vi.hoisted(() => ({
  agentHealth: vi.fn(),
  createAgentSession: vi.fn(),
  createAgentRun: vi.fn(),
  startAgentRun: vi.fn(),
  cancelAgentRun: vi.fn(),
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
})
