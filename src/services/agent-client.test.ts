import { beforeEach, describe, expect, it, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))

vi.mock('@tauri-apps/api/core', () => ({ invoke }))

import {
  agentHealth,
  cancelAgentRun,
  createAgentRun,
  createAgentSession,
  decideAgentApproval,
  executeAgentTool,
  listAgentTools,
  runAgentPlanner,
  startAgentRun,
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

  it('uses camelCase Tauri arguments for agent session and run commands', async () => {
    const session = { id: 'session-1' }
    const run = { id: 'run-1' }
    vi.mocked(invoke).mockResolvedValueOnce(session).mockResolvedValue(run)

    await expect(createAgentSession('exam-1', 'First session')).resolves.toBe(session)
    await expect(createAgentRun('session-1', 'Plan today')).resolves.toBe(run)

    expect(invoke).toHaveBeenNthCalledWith(1, 'agent_create_session', {
      examId: 'exam-1',
      title: 'First session',
    })
    expect(invoke).toHaveBeenNthCalledWith(2, 'agent_create_run', {
      sessionId: 'session-1',
      goal: 'Plan today',
    })
  })

  it('uses camelCase Tauri arguments to start and cancel a run', async () => {
    const started = { id: 'run-1', status: 'running' }
    const cancelled = { id: 'run-1', status: 'cancelled' }
    vi.mocked(invoke).mockResolvedValueOnce(started).mockResolvedValue(cancelled)

    await expect(startAgentRun('run-1')).resolves.toBe(started)
    await expect(cancelAgentRun('run-1')).resolves.toBe(cancelled)

    expect(invoke).toHaveBeenNthCalledWith(1, 'agent_start_run', { runId: 'run-1' })
    expect(invoke).toHaveBeenNthCalledWith(2, 'agent_cancel_run', { runId: 'run-1' })
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
      approvalId: 'approval-1',
      approve: true,
    })
    await undoAgentTool('step-1')
    expect(invoke).toHaveBeenLastCalledWith('agent_undo_tool', { stepId: 'step-1' })
  })

  it('invokes the hidden planner command with camelCase arguments', async () => {
    const turn = { mode: 'local', final_text: 'ok', iterations: 0, model_calls: 0, prompt_tokens: 0, completion_tokens: 0, trace: [] }
    vi.mocked(invoke).mockResolvedValue(turn)

    await expect(runAgentPlanner('run-1', '看今天的计划')).resolves.toBe(turn)
    expect(invoke).toHaveBeenLastCalledWith('agent_run_planner', {
      runId: 'run-1',
      goal: '看今天的计划',
    })
  })
})
