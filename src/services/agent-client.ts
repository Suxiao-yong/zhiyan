import { invoke } from '@tauri-apps/api/core'
import type { AgentRun, AgentSession } from '@/types'

export function agentHealth(): Promise<void> {
  return invoke<void>('agent_health')
}

export function createAgentSession(examId: string | null, title: string): Promise<AgentSession> {
  return invoke<AgentSession>('agent_create_session', { examId, title })
}

export function createAgentRun(sessionId: string, goal: string): Promise<AgentRun> {
  return invoke<AgentRun>('agent_create_run', { sessionId, goal })
}

export function startAgentRun(runId: string): Promise<AgentRun> {
  return invoke<AgentRun>('agent_start_run', { runId })
}

export function cancelAgentRun(runId: string): Promise<AgentRun> {
  return invoke<AgentRun>('agent_cancel_run', { runId })
}
