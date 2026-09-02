import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { useExamStore } from '@/stores/exam'
import {
  agentApprovalList,
  agentBriefPreview,
  agentSessionList,
  agentSessionMessages,
  cancelAgentRun,
  createAgentRun,
  createAgentSession,
  resolveAgentApproval,
  runAgentPlanner,
  startAgentRun,
  undoAgentTool,
} from '@/services/agent-client'
import type {
  AgentApproval,
  AgentBrief,
  AgentMessage,
  AgentPlannerTurn,
  AgentRun,
  AgentSession,
} from '@/types'

/**
 * Agent OS state (M5): sessions, the active conversation, the most recent run,
 * the daily brief card, and pending approvals. Every action maps to one Tauri
 * command; the backend persists messages per planner turn.
 */
export const useAgentStore = defineStore('agent', () => {
  const examStore = useExamStore()

  const sessions = ref<AgentSession[]>([])
  const activeSessionId = ref<string | null>(null)
  const messages = ref<AgentMessage[]>([])
  const run = ref<AgentRun | null>(null)
  const brief = ref<AgentBrief | null>(null)
  const briefHandled = ref(false)
  const approvals = ref<AgentApproval[]>([])
  const busy = ref(false)
  const error = ref('')
  const inputText = ref('')

  function setInputText(value: string): void {
    inputText.value = value
  }

  const activeSession = computed(
    () => sessions.value.find((session) => session.id === activeSessionId.value) ?? null,
  )
  const hasBrief = computed(() => brief.value !== null && !briefHandled.value)

  async function refreshSessions(): Promise<void> {
    sessions.value = await agentSessionList(50)
  }

  async function createSession(): Promise<void> {
    busy.value = true
    error.value = ''
    try {
      const session = await createAgentSession(
        examStore.activeExamId,
        `会话 ${new Date().toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })}`,
      )
      sessions.value = [session, ...sessions.value]
      await selectSession(session.id)
    } catch (caught) {
      error.value = String(caught)
    } finally {
      busy.value = false
    }
  }

  async function selectSession(sessionId: string): Promise<void> {
    busy.value = true
    error.value = ''
    try {
      activeSessionId.value = sessionId
      messages.value = await agentSessionMessages(sessionId)
    } catch (caught) {
      error.value = String(caught)
    } finally {
      busy.value = false
    }
  }

  /** Send a message: create a session if needed, run one planner turn, and
   * reload the persisted messages (user + assistant rows written by the
   * backend). */
  async function sendMessage(text: string): Promise<AgentPlannerTurn | null> {
    const goal = text.trim()
    if (!goal || busy.value) return null
    busy.value = true
    error.value = ''
    try {
      let sessionId = activeSessionId.value
      if (!sessionId) {
        const session = await createAgentSession(examStore.activeExamId, goal.slice(0, 20))
        sessionId = session.id
        activeSessionId.value = sessionId
        sessions.value = [session, ...sessions.value]
      }
      const created = await createAgentRun(sessionId, goal)
      run.value = await startAgentRun(created.id)
      const turn = await runAgentPlanner(created.id, goal)
      messages.value = await agentSessionMessages(sessionId)
      // cache-friendly tail snapshot may have landed on WAITING_APPROVAL — surface it immediately
      await refreshApprovals()
      return turn
    } catch (caught) {
      error.value = String(caught)
      return null
    } finally {
      busy.value = false
    }
  }

  async function loadBrief(): Promise<void> {
    try {
      brief.value = await agentBriefPreview(examStore.activeExamId)
      briefHandled.value = false
    } catch (caught) {
      error.value = String(caught)
    }
  }

  /** Fold the brief card into the conversation after the user acknowledges it. */
  function acknowledgeBrief(): void {
    briefHandled.value = true
  }

  async function refreshApprovals(): Promise<void> {
    try {
      approvals.value = await agentApprovalList(20)
    } catch (caught) {
      error.value = String(caught)
    }
  }

  /**
   * Task 10: resolve an approval through the Rust executor. Approving really
   * executes the approved tool (re-checking scope/precondition/schema) and
   * then refreshes the conversation, brief, and approval list; rejecting only
   * updates state. Never the state-only decideAgentApproval path. A
   * per-approval claim guard makes a double click on confirm resolve exactly
   * once.
   */
  const resolvingApprovals = new Set<string>()
  function isResolving(approvalId: string): boolean {
    return resolvingApprovals.has(approvalId)
  }
  async function resolveApproval(approvalId: string, approve: boolean): Promise<void> {
    if (resolvingApprovals.has(approvalId)) return
    resolvingApprovals.add(approvalId)
    try {
      await resolveAgentApproval(approvalId, approve)
      await refreshApprovals()
      if (approve && activeSessionId.value) {
        messages.value = await agentSessionMessages(activeSessionId.value)
      }
      await loadBrief()
    } catch (caught) {
      error.value = String(caught)
    } finally {
      resolvingApprovals.delete(approvalId)
    }
  }

  async function cancelRun(): Promise<void> {
    if (!run.value) return
    try {
      run.value = await cancelAgentRun(run.value.id)
    } catch (caught) {
      error.value = String(caught)
    }
  }

  /**
   * Task 5: undo an executed write tool (plan.apply_preview, record.checkin_plan)
   * through the Rust executor. Success refreshes the approval list, the
   * conversation, and the brief so the restored state is visible.
   */
  async function undoTool(stepId: string): Promise<void> {
    try {
      await undoAgentTool(stepId)
      await refreshApprovals()
      if (activeSessionId.value) {
        messages.value = await agentSessionMessages(activeSessionId.value)
      }
      await loadBrief()
    } catch (caught) {
      error.value = String(caught)
    }
  }

  return {
    sessions,
    activeSessionId,
    activeSession,
    messages,
    run,
    brief,
    briefHandled,
    hasBrief,
    approvals,
    busy,
    error,
    inputText,
    setInputText,
    refreshSessions,
    createSession,
    selectSession,
    sendMessage,
    loadBrief,
    acknowledgeBrief,
    refreshApprovals,
    resolveApproval,
    isResolving,
    cancelRun,
    undoTool,
  }
})
