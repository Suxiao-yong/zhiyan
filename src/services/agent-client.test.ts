import { beforeEach, describe, expect, it, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))

vi.mock('@tauri-apps/api/core', () => ({ invoke }))

import {
  agentHealth,
  cancelAgentRun,
  cloudConsentStatus,
  confirmCloudConsent,
  createAgentRun,
  createAgentSession,
  decideAgentApproval,
  executeAgentTool,
  listAgentTools,
  resolveAgentApproval,
  runAgentPlanner,
  startAgentRun,
  testAgentProvider,
  undoAgentTool,
} from './agent-client'
import type { AgentToolCallRequest } from '@/types'

describe('agent runtime client', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('invokes agent_health without an empty args object', async () => {
    vi.mocked(invoke).mockResolvedValue(undefined)

    await agentHealth()

    expect(invoke).toHaveBeenCalledWith('agent_health')
  })

  it('uses snake_case Tauri arguments for agent session and run commands', async () => {
    const session = { id: 'session-1' }
    const run = { id: 'run-1' }
    vi.mocked(invoke).mockResolvedValueOnce(session).mockResolvedValue(run)

    await expect(createAgentSession('exam-1', 'First session')).resolves.toBe(session)
    await expect(createAgentRun('session-1', 'Plan today')).resolves.toBe(run)

    expect(invoke).toHaveBeenNthCalledWith(1, 'agent_create_session', {
      exam_id: 'exam-1',
      title: 'First session',
    })
    expect(invoke).toHaveBeenNthCalledWith(2, 'agent_create_run', {
      session_id: 'session-1',
      goal: 'Plan today',
    })
  })

  it('uses snake_case Tauri arguments to start and cancel a run', async () => {
    const started = { id: 'run-1', status: 'running' }
    const cancelled = { id: 'run-1', status: 'cancelled' }
    vi.mocked(invoke).mockResolvedValueOnce(started).mockResolvedValue(cancelled)

    await expect(startAgentRun('run-1')).resolves.toBe(started)
    await expect(cancelAgentRun('run-1')).resolves.toBe(cancelled)

    expect(invoke).toHaveBeenNthCalledWith(1, 'agent_start_run', { run_id: 'run-1' })
    expect(invoke).toHaveBeenNthCalledWith(2, 'agent_cancel_run', { run_id: 'run-1' })
  })

  it('preserves Tauri command errors', async () => {
    const commandError = { code: 'validation_error', message: 'goal must not be blank' }
    vi.mocked(invoke).mockRejectedValue(commandError)

    await expect(createAgentRun('session-1', '')).rejects.toBe(commandError)
  })

  it('invokes typed tool commands with camelCase boundary arguments', async () => {
    const request: AgentToolCallRequest = {
      run_id: 'run-1',
      step_index: 0,
      tool_name: 'plan.get_today',
      tool_version: '1',
      input: { exam_id: 'exam-1' },
      idempotency_key: null,
      approval_id: null,
    }
    vi.mocked(invoke).mockResolvedValue(undefined)

    await listAgentTools()
    expect(invoke).toHaveBeenLastCalledWith('agent_list_tools')
    await executeAgentTool(request)
    expect(invoke).toHaveBeenLastCalledWith('agent_execute_tool', { request })
    await decideAgentApproval('approval-1', true)
    expect(invoke).toHaveBeenLastCalledWith('agent_decide_approval', {
      approval_id: 'approval-1',
      approve: true,
    })
    await undoAgentTool('step-1')
    expect(invoke).toHaveBeenLastCalledWith('agent_undo_tool', { step_id: 'step-1' })
  })

  it('invokes the hidden planner command with snake_case arguments', async () => {
    const turn = {
      mode: 'local',
      final_text: 'ok',
      iterations: 0,
      model_calls: 0,
      prompt_tokens: 0,
      completion_tokens: 0,
      trace: [],
    }
    vi.mocked(invoke).mockResolvedValue(turn)

    await expect(runAgentPlanner('run-1', '看今天的计划')).resolves.toBe(turn)
    expect(invoke).toHaveBeenLastCalledWith('agent_run_planner', {
      run_id: 'run-1',
      goal: '看今天的计划',
    })
  })

  it('reads and confirms the cloud LLM consent status', async () => {
    const notConsented = { configured: true, consented: false, fingerprint: 'fp-1' }
    const consented = { configured: true, consented: true, fingerprint: 'fp-1' }
    vi.mocked(invoke).mockResolvedValueOnce(notConsented).mockResolvedValue(consented)

    await expect(cloudConsentStatus()).resolves.toBe(notConsented)
    expect(invoke).toHaveBeenLastCalledWith('agent_cloud_consent_status')
    await expect(confirmCloudConsent()).resolves.toBe(consented)
    expect(invoke).toHaveBeenLastCalledWith('agent_confirm_cloud_consent')
  })

  it('resolves an approval through the executing rust path', async () => {
    const resolved = { id: 'approval-1', status: 'approved' }
    vi.mocked(invoke).mockResolvedValue(resolved)

    await expect(resolveAgentApproval('approval-1', true)).resolves.toBe(resolved)
    expect(invoke).toHaveBeenLastCalledWith('agent_resolve_approval', {
      approval_id: 'approval-1',
      approve: true,
    })
    await resolveAgentApproval('approval-1', false)
    expect(invoke).toHaveBeenLastCalledWith('agent_resolve_approval', {
      approval_id: 'approval-1',
      approve: false,
    })
  })

  it('tests the provider through the rust planner', async () => {
    const result = {
      model: 'deepseek-chat',
      latency_ms: 812,
      text_stream: true,
      tool_call: true,
      error_code: undefined,
    }
    vi.mocked(invoke).mockResolvedValue(result)
    await expect(testAgentProvider()).resolves.toBe(result)
    expect(invoke).toHaveBeenLastCalledWith('agent_test_provider')
  })
})
