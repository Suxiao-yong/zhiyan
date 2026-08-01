import { invoke } from '@tauri-apps/api/core'
import type {
  AgentApproval,
  AgentContextAuditRow,
  AgentPlannerTurn,
  AgentRun,
  AgentSession,
  AgentToolCallRequest,
  AgentToolCallResponse,
  AgentToolUndoResponse,
  ListedAgentTool,
} from '@/types'

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

export function listAgentTools(): Promise<ListedAgentTool[]> {
  return invoke<ListedAgentTool[]>('agent_list_tools')
}

export function executeAgentTool(request: AgentToolCallRequest): Promise<AgentToolCallResponse> {
  return invoke<AgentToolCallResponse>('agent_execute_tool', { request })
}

export function decideAgentApproval(approvalId: string, approve: boolean): Promise<AgentApproval> {
  return invoke<AgentApproval>('agent_decide_approval', { approvalId, approve })
}

export function undoAgentTool(stepId: string): Promise<AgentToolUndoResponse> {
  return invoke<AgentToolUndoResponse>('agent_undo_tool', { stepId })
}

/** Hidden planner entry point (M3 Part 1): run one model -> tool loop. */
export function runAgentPlanner(runId: string, goal: string): Promise<AgentPlannerTurn> {
  return invoke<AgentPlannerTurn>('agent_run_planner', { runId, goal })
}

/** Context Inspector (M3 Part 3): every model-call audit row of a run. */
export function listAgentContextAudit(runId: string): Promise<AgentContextAuditRow[]> {
  return invoke<AgentContextAuditRow[]>('agent_context_audit_list', { runId })
}
