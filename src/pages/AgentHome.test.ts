// @vitest-environment jsdom

import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  AgentApproval,
  AgentMessage,
  AgentPlannerTurn,
  AgentRun,
  AgentSession,
} from '@/types'
import { useExamStore } from '@/stores/exam'
import { useAgentStore } from '@/stores/agent'

const client = vi.hoisted(() => ({
  agentSessionList: vi.fn(),
  agentSessionMessages: vi.fn(),
  createAgentSession: vi.fn(),
  createAgentRun: vi.fn(),
  startAgentRun: vi.fn(),
  runAgentPlanner: vi.fn(),
  agentBriefPreview: vi.fn(),
  agentApprovalList: vi.fn(),
  decideAgentApproval: vi.fn(),
  cancelAgentRun: vi.fn(),
}))

vi.mock('@/services/agent-client', () => client)

const eventApi = vi.hoisted(() => ({ listen: vi.fn().mockResolvedValue(() => {}) }))
vi.mock('@tauri-apps/api/event', () => eventApi)

import AgentHome from './AgentHome.vue'

const session = (id: string, title: string): AgentSession => ({
  id,
  exam_id: 'exam-1',
  title,
  status: 'active',
  created_at: '2026-07-18T00:00:00',
  updated_at: '2026-07-18T00:00:00',
})

const message = (id: string, role: 'user' | 'assistant', text: string): AgentMessage => ({
  id,
  session_id: 'session-1',
  run_id: 'run-1',
  role,
  text,
  content_json: null,
  prompt_tokens: role === 'assistant' ? 40 : 0,
  completion_tokens: role === 'assistant' ? 5 : 0,
  model: null,
  created_at: '2026-07-18T00:00:00',
})

const queuedRun: AgentRun = {
  id: 'run-1',
  session_id: 'session-1',
  goal: '看今天的计划',
  status: 'queued',
  trigger_source: 'user',
  current_step: 0,
  error_code: null,
  created_at: '2026-07-18T00:00:00',
  updated_at: '2026-07-18T00:00:00',
  started_at: null,
  completed_at: null,
}

const turn: AgentPlannerTurn = {
  mode: 'local',
  final_text: '（本地模式）no llm provider configured，跳过模型推理。',
  iterations: 0,
  model_calls: 0,
  prompt_tokens: 0,
  completion_tokens: 0,
  estimated_cost_usd: 0,
  trace: [{ kind: 'local_fallback', reason: 'no llm provider configured' }],
}

const approval = (id: string, status: string): AgentApproval => ({
  id,
  run_id: 'run-1',
  step_id: `step-${id}`,
  risk: 3,
  preview: { plan_ids: ['p-1'] },
  precondition_hash: 'hash',
  status,
  expires_at: '2099-01-01 12:00:00',
  decided_at: null,
  created_at: '2026-07-18T00:00:00',
})

function mountPage() {
  const pinia = createPinia()
  setActivePinia(pinia)
  useExamStore().setActiveExam('exam-1')
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/', component: { template: '<div />' } }],
  })
  const wrapper = mount(AgentHome, {
    global: {
      plugins: [pinia, router],
      stubs: {
        // The workbench content has its own tests; stub the page components
        // here to isolate the Agent OS shell.
        PlanCheckinBoard: { template: '<div data-test="workbench-checkin" />' },
        StudyPlan: { template: '<div data-test="workbench-plan" />' },
        StudyRecord: { template: '<div data-test="workbench-record" />' },
        Analysis: { template: '<div data-test="workbench-analysis" />' },
        Visualization: { template: '<div data-test="workbench-visualization" />' },
      },
    },
  })
  return { wrapper, pinia }
}

describe('AgentHome', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    client.agentSessionList.mockResolvedValue([])
    client.agentSessionMessages.mockResolvedValue([])
    client.createAgentSession.mockResolvedValue(session('session-1', '新会话'))
    client.createAgentRun.mockResolvedValue(queuedRun)
    client.startAgentRun.mockResolvedValue({ ...queuedRun, status: 'running' })
    client.runAgentPlanner.mockResolvedValue(turn)
    client.agentBriefPreview.mockResolvedValue({
      date: '2026-07-18',
      mode: 'local',
      summary: '今日计划 2 项，已完成 1 项（完成率 50%）。',
      explanation: null,
      today_planned: 2,
      today_completed: 1,
      today_duration_min: 120,
      overdue_count: 1,
      week_completion_rate: 0.5,
      due_wrong_questions: 0,
      weak_areas: [],
    })
    client.agentApprovalList.mockResolvedValue([])
    client.decideAgentApproval.mockResolvedValue(approval('ap-1', 'approved'))
  })

  it('renders the three-column shell', async () => {
    const { wrapper } = mountPage()
    await flushPromises()
    expect(wrapper.get('[data-test=agent-sidebar]').exists()).toBe(true)
    expect(wrapper.get('[data-test=conversation-pane]').exists()).toBe(true)
    expect(wrapper.get('[data-test=workbench-host]').exists()).toBe(true)
    expect(wrapper.get('[data-test=workbench-checkin]').exists()).toBe(true)
  })

  it('loads sessions and switches to a selected session', async () => {
    client.agentSessionList.mockResolvedValue([session('s-1', '第一个会话'), session('s-2', '第二个会话')])
    client.agentSessionMessages.mockResolvedValue([
      message('m-1', 'user', '看今天的计划'),
      message('m-2', 'assistant', '今日有一项复习任务。'),
    ])
    const { wrapper } = mountPage()
    await flushPromises()

    expect(client.agentSessionList).toHaveBeenCalledWith(50)
    await wrapper.get('[data-test=agent-session-s-2]').trigger('click')
    await flushPromises()

    expect(client.agentSessionMessages).toHaveBeenCalledWith('s-2')
    expect(wrapper.get('[data-test=message-m-1]').text()).toContain('看今天的计划')
    expect(wrapper.get('[data-test=message-m-2]').text()).toContain('今日有一项复习任务。')
    expect(wrapper.get('[data-test=message-m-2]').text()).toContain('tokens 40+5')
  })

  it('creates a new session from the sidebar', async () => {
    const { wrapper } = mountPage()
    await flushPromises()
    await wrapper.get('[data-test=agent-new-session]').trigger('click')
    await flushPromises()
    expect(client.createAgentSession).toHaveBeenCalledWith('exam-1', expect.any(String))
    expect(client.agentSessionMessages).toHaveBeenCalledWith('session-1')
  })

  it('sends a message and renders the persisted conversation', async () => {
    client.agentSessionMessages.mockResolvedValue([
      message('m-1', 'user', '看今天的计划'),
      message('m-2', 'assistant', '（本地模式）no llm provider configured，跳过模型推理。'),
    ])
    const { wrapper, pinia } = mountPage()
    await flushPromises()

    // Drive the composer through the store (el-input is not registered in
    // tests) and submit the form.
    useAgentStore(pinia).setInputText('看今天的计划')
    await wrapper.find('form.composer').trigger('submit')
    await flushPromises()

    expect(client.createAgentRun).toHaveBeenCalledWith(expect.any(String), '看今天的计划')
    expect(client.runAgentPlanner).toHaveBeenCalledWith('run-1', '看今天的计划')
    expect(client.agentSessionMessages).toHaveBeenCalled()
    expect(wrapper.get('[data-test=message-m-1]').text()).toContain('看今天的计划')
    expect(wrapper.get('[data-test=status-running]').exists()).toBe(true)
  })

  it('renders the daily brief and folds it after acknowledge', async () => {
    const { wrapper } = mountPage()
    await flushPromises()

    expect(wrapper.get('[data-test=brief-card]').exists()).toBe(true)
    expect(wrapper.get('[data-test=brief-summary]').text()).toContain('今日计划 2 项')
    expect(wrapper.get('[data-test=brief-overdue]').text()).toContain('1')

    await wrapper.get('[data-test=brief-acknowledge]').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-test=brief-acknowledge]').exists()).toBe(false)
    expect(wrapper.find('[data-test=brief-card]').exists()).toBe(true)
  })

  it('renders pending approvals and decides them', async () => {
    client.agentApprovalList.mockResolvedValue([approval('ap-1', 'pending')])
    const { wrapper } = mountPage()
    await flushPromises()

    expect(wrapper.get('[data-test=approval-ap-1]').exists()).toBe(true)
    expect(wrapper.get('[data-test=approval-preview]').text()).toContain('plan_ids: 1')

    await wrapper.get('[data-test=approval-approve-ap-1]').trigger('click')
    await flushPromises()
    expect(client.decideAgentApproval).toHaveBeenCalledWith('ap-1', true)
  })

  it('switches workbenches in the right pane without losing the conversation', async () => {
    const { wrapper } = mountPage()
    await flushPromises()

    // Default workbench: check-in board is mounted in the host.
    expect(wrapper.get('[data-test=workbench-host]').exists()).toBe(true)
    expect(wrapper.get('[data-test=workbench-host]').text()).toContain('计划打卡')

    // Switch to the plan workbench; the conversation pane stays mounted.
    await wrapper.get('[data-test=workbench-tab-plan]').trigger('click')
    await flushPromises()
    expect(wrapper.get('[data-test=workbench-host]').text()).toContain('学习计划')
    expect(wrapper.get('[data-test=workbench-host]').text()).not.toContain('计划打卡')
    expect(wrapper.get('[data-test=conversation-pane]').exists()).toBe(true)

    // Switch again to records, analysis, and visualization.
    await wrapper.get('[data-test=workbench-tab-record]').trigger('click')
    await flushPromises()
    expect(wrapper.get('[data-test=workbench-host]').text()).toContain('记录与错题')
    await wrapper.get('[data-test=workbench-tab-analysis]').trigger('click')
    await flushPromises()
    expect(wrapper.get('[data-test=workbench-host]').text()).toContain('AI 分析')
    await wrapper.get('[data-test=workbench-tab-visualization]').trigger('click')
    await flushPromises()
    expect(wrapper.get('[data-test=workbench-host]').text()).toContain('数据可视化')
  })
})
