import { invoke } from '@tauri-apps/api/core'
import type {
  AgentApproval,
  AgentBrief,
  AgentContextAuditRow,
  AgentMessage,
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

/**
 * Mandatory Task C: approve *executes* the approved tool through the Rust
 * executor (re-checking scope/precondition/schema); reject only updates state.
 * The confirm button must use this, never the state-only decideAgentApproval.
 */
export function resolveAgentApproval(approvalId: string, approve: boolean): Promise<AgentApproval> {
  return invoke<AgentApproval>('agent_resolve_approval', { approvalId, approve })
}

export function undoAgentTool(stepId: string): Promise<AgentToolUndoResponse> {
  return invoke<AgentToolUndoResponse>('agent_undo_tool', { stepId })
}

/** Hidden planner entry point (M3 Part 1): run one model -> tool loop. */
export function runAgentPlanner(runId: string, goal: string): Promise<AgentPlannerTurn> {
  return invoke<AgentPlannerTurn>('agent_run_planner', { runId, goal })
}

export interface CloudConsentStatus {
  configured: boolean
  consented: boolean
  fingerprint: string | null
}

/** Cloud LLM data-export consent status for the current provider config. */
export function cloudConsentStatus(): Promise<CloudConsentStatus> {
  return invoke<CloudConsentStatus>('agent_cloud_consent_status')
}

/** Record explicit user consent for the current provider/base/model tuple. */
export function confirmCloudConsent(): Promise<CloudConsentStatus> {
  return invoke<CloudConsentStatus>('agent_confirm_cloud_consent')
}

export interface ProviderTestResult {
  model: string
  latency_ms: number
  text_stream: boolean
  tool_call: boolean
  error_code?: string
}

/** Probe the configured cloud provider through the Rust planner (Task 6). */
export function testAgentProvider(): Promise<ProviderTestResult> {
  return invoke<ProviderTestResult>('agent_test_provider')
}

/** Context Inspector (M3 Part 3): every model-call audit row of a run. */
export function listAgentContextAudit(runId: string): Promise<AgentContextAuditRow[]> {
  return invoke<AgentContextAuditRow[]>('agent_context_audit_list', { runId })
}

/** Daily brief preview (M4). */
export function agentBriefPreview(examId?: string | null): Promise<AgentBrief> {
  return invoke<AgentBrief>('agent_brief_preview', { examId: examId ?? null })
}

export interface DueReviewItem {
  id: string
  question_desc: string | null
  subject_name: string
  knowledge_point_name: string | null
}

export interface ReviewListDueOutput {
  count: number
  items: DueReviewItem[]
}

/** Today's due wrong-question reviews for the right-pane workbench card. */
export function reviewListDue(examId?: string | null): Promise<ReviewListDueOutput> {
  return invoke<ReviewListDueOutput>('review_list_due', { examId: examId ?? null })
}

/** Read-only knowledge-tree node for the mind-map view (v0.3.0 Task 8). */
export interface KnowledgePointNode {
  id: string
  name: string
  mastery: number
  wrong_count: number
  material_id: string | null
  source_ref: string | null
  children: KnowledgePointNode[]
}

export interface KnowledgeTreeSubject {
  id: string
  name: string
  children: KnowledgePointNode[]
}

export interface KnowledgeTreeOutput {
  exam_id: string | null
  subjects: KnowledgeTreeSubject[]
}

/** Mind-map knowledge tree for the active/explicit exam; empty when no exam. */
export function knowledgeTree(examId?: string | null): Promise<KnowledgeTreeOutput> {
  return invoke<KnowledgeTreeOutput>('knowledge_tree', { examId: examId ?? null })
}

/** Agent OS reads (M5). */
export function agentSessionList(limit?: number): Promise<AgentSession[]> {
  return invoke<AgentSession[]>('agent_session_list', { limit: limit ?? null })
}

export function agentSessionMessages(sessionId: string): Promise<AgentMessage[]> {
  return invoke<AgentMessage[]>('agent_session_messages', { sessionId })
}

export function agentApprovalList(limit?: number): Promise<AgentApproval[]> {
  return invoke<AgentApproval[]>('agent_approval_list', { limit: limit ?? null })
}
