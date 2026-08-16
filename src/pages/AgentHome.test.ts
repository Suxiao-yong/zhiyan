// @vitest-environment jsdom
/// <reference lib="es2015" />

import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { AgentApproval, AgentMessage, AgentPlannerTurn, AgentRun, AgentSession } from '@/types'
import { useExamStore } from '@/stores/exam'
import { useSettingsStore } from '@/stores/settings'
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
  resolveAgentApproval: vi.fn(),
  undoAgentTool: vi.fn(),
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
  preview: {
    tool: 'plan.apply_preview',
    risk: 3,
    undo_available: false,
    action: '应用计划草案',
    affected_count: 60,
    summary: '将应用 2030-01-01~2030-01-30 共 30 天的本地生成计划：60 项任务，10080 分钟',
    conflicts: [
      {
        kind: 'replace_future_plans',
        affected_count: 3,
        detail: '将替换考试日期前的 3 条未来计划',
      },
    ],
    date_range: '2030-01-01~2030-01-30',
    fields: {},
  },
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
    expect(wrapper.find('[data-test=agent-sidebar]').exists()).toBe(true)
    expect(wrapper.find('[data-test=conversation-pane]').exists()).toBe(true)
    expect(wrapper.find('[data-test=workbench-host]').exists()).toBe(true)
    expect(wrapper.find('[data-test=workbench-checkin]').exists()).toBe(true)
  })

  it('loads sessions and switches to a selected session', async () => {
    client.agentSessionList.mockResolvedValue([
      session('s-1', '第一个会话'),
      session('s-2', '第二个会话'),
    ])
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
    expect(wrapper.find('[data-test=status-running]').exists()).toBe(true)
  })

  it('renders the daily brief and folds it after acknowledge', async () => {
    const { wrapper } = mountPage()
    await flushPromises()

    expect(wrapper.find('[data-test=brief-card]').exists()).toBe(true)
    expect(wrapper.get('[data-test=brief-summary]').text()).toContain('今日计划 2 项')
    expect(wrapper.get('[data-test=brief-overdue]').text()).toContain('1')

    await wrapper.get('[data-test=brief-acknowledge]').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-test=brief-acknowledge]').exists()).toBe(false)
    expect(wrapper.find('[data-test=brief-card]').exists()).toBe(true)
  })

  it('renders pending approvals with the standardized preview and resolves them', async () => {
    client.agentApprovalList.mockResolvedValue([approval('ap-1', 'pending')])
    client.resolveAgentApproval.mockResolvedValue(approval('ap-1', 'approved'))
    const { wrapper } = mountPage()
    await flushPromises()

    expect(wrapper.find('[data-test=approval-ap-1]').exists()).toBe(true)
    // Task 10: the card renders the sanitized preview (action, summary,
    // conflicts) — never raw request fields like plan_ids.
    expect(wrapper.get('[data-test=action-preview-action]').text()).toContain('应用计划草案')
    expect(wrapper.get('[data-test=action-preview-summary]').text()).toContain('60 项任务')
    expect(wrapper.get('[data-test=action-preview-conflicts]').text()).toContain('3 条未来计划')
    expect(wrapper.find('[data-test=approval-preview]').exists()).toBe(false)

    await wrapper.get('[data-test=approval-approve-ap-1]').trigger('click')
    await flushPromises()
    // Approve goes through the executing rust path, never the state-only one.
    expect(client.decideAgentApproval).not.toHaveBeenCalled()
    expect(client.resolveAgentApproval).toHaveBeenCalledWith('ap-1', true)
  })

  it('switches workbenches in the right pane without losing the conversation', async () => {
    const { wrapper } = mountPage()
    await flushPromises()

    // Default workbench: check-in board is mounted in the host.
    expect(wrapper.find('[data-test=workbench-host]').exists()).toBe(true)
    expect(wrapper.get('[data-test=workbench-host]').text()).toContain('今日打卡')

    // Switch to the plan workbench; the conversation pane stays mounted.
    await wrapper.get('[data-test=workbench-tab-plan]').trigger('click')
    await flushPromises()
    expect(wrapper.get('[data-test=workbench-host]').text()).toContain('计划')
    expect(wrapper.get('[data-test=workbench-host]').text()).not.toContain('今日打卡')
    expect(wrapper.find('[data-test=conversation-pane]').exists()).toBe(true)

    // Switch again to records.
    await wrapper.get('[data-test=workbench-tab-record]').trigger('click')
    await flushPromises()
    expect(wrapper.get('[data-test=workbench-host]').text()).toContain('记录')
  })

  it('exposes only the three core workbench tabs (Task 3)', async () => {
    const { wrapper } = mountPage()
    await flushPromises()
    expect(wrapper.find('[data-test=workbench-tab-checkin]').exists()).toBe(true)
    expect(wrapper.find('[data-test=workbench-tab-plan]').exists()).toBe(true)
    expect(wrapper.find('[data-test=workbench-tab-record]').exists()).toBe(true)
    expect(wrapper.find('[data-test=workbench-tab-analysis]').exists()).toBe(false)
    expect(wrapper.find('[data-test=workbench-tab-visualization]').exists()).toBe(false)
  })

  it('shows the explicit cloud-off state when no LLM provider is configured', async () => {
    const { wrapper } = mountPage()
    await flushPromises()

    // Settings store defaults to no provider, so the conversation pane shows
    // the explicit state instead of pretending "local mode" is a model answer.
    expect(wrapper.get('[data-test=messages-empty]').text()).toContain('尚未连接云端模型')
    expect(wrapper.get('[data-test=messages-empty]').text()).toContain('连接模型后可使用对话')
  })

  it('shows the configure-exam state when no exam is active', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    const router = createRouter({
      history: createMemoryHistory(),
      routes: [{ path: '/', component: { template: '<div />' } }],
    })
    const wrapper = mount(AgentHome, {
      global: {
        plugins: [pinia, router],
        stubs: {
          PlanCheckinBoard: { template: '<div data-test="workbench-checkin" />' },
          StudyPlan: { template: '<div data-test="workbench-plan" />' },
          StudyRecord: { template: '<div data-test="workbench-record" />' },
        },
      },
    })
    await flushPromises()

    expect(wrapper.get('[data-test=messages-empty]').text()).toContain('请先配置考试')
  })

  it('shows the ready-state prompt once a cloud provider is configured', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    useExamStore().setActiveExam('exam-1')
    useSettingsStore().llmConfig = {
      provider: 'deepseek',
      baseUrl: 'https://api.deepseek.com',
      model: 'deepseek-chat',
      temperature: 0.7,
    }
    const router = createRouter({
      history: createMemoryHistory(),
      routes: [{ path: '/', component: { template: '<div />' } }],
    })
    const wrapper = mount(AgentHome, {
      global: {
        plugins: [pinia, router],
        stubs: {
          PlanCheckinBoard: { template: '<div data-test="workbench-checkin" />' },
          StudyPlan: { template: '<div data-test="workbench-plan" />' },
          StudyRecord: { template: '<div data-test="workbench-record" />' },
        },
      },
    })
    await flushPromises()

    expect(wrapper.get('[data-test=messages-empty]').text()).toContain('开始今天的对话')
  })

  it('refreshes approvals and messages after an approval is resolved', async () => {
    client.agentApprovalList.mockResolvedValueOnce([approval('ap-1', 'pending')])
    client.resolveAgentApproval.mockResolvedValue(approval('ap-1', 'approved'))
    const { wrapper } = mountPage()
    await flushPromises()

    // Establish an active session so the post-approval message refresh runs.
    await wrapper.get('[data-test=agent-new-session]').trigger('click')
    await flushPromises()

    await wrapper.get('[data-test=approval-approve-ap-1]').trigger('click')
    await flushPromises()

    // Task 15: after resolving, the approval list and the conversation are
    // refreshed from the backend so the executed result is visible.
    expect(client.resolveAgentApproval).toHaveBeenCalledWith('ap-1', true)
    expect(client.agentApprovalList).toHaveBeenCalledTimes(2)
    expect(client.agentSessionMessages).toHaveBeenCalled()
  })

  it('rejects an approval without dispatching a write (cancel writes nothing)', async () => {
    client.agentApprovalList.mockResolvedValue([approval('ap-1', 'pending')])
    client.resolveAgentApproval.mockResolvedValue(approval('ap-1', 'rejected'))
    const { wrapper } = mountPage()
    await flushPromises()

    await wrapper.get('[data-test=approval-reject-ap-1]').trigger('click')
    await flushPromises()

    // Cancel goes through the resolving Rust path with approve=false; no write
    // tool is ever dispatched on the client side.
    expect(client.resolveAgentApproval).toHaveBeenCalledWith('ap-1', false)
    expect(client.decideAgentApproval).not.toHaveBeenCalled()
  })

  it('renders the real draft rows and precondition from the sanitized preview', async () => {
    const preview = {
      ...approval('ap-1', 'pending').preview!,
      fields: {
        precondition_hash: 'hash-abc',
        draft_row_count: 60,
        rows: [
          {
            date: '2030-01-01',
            subject_name: '数学',
            planned_tasks: '学习：函数',
            planned_duration: 270,
          },
          {
            date: '2030-01-01',
            subject_name: '英语',
            planned_tasks: '学习：词汇',
            planned_duration: 90,
          },
        ],
        kept_subjects: [],
        conflicts: [],
      },
    }
    client.agentApprovalList.mockResolvedValue([{ ...approval('ap-1', 'pending'), preview }])
    const { wrapper } = mountPage()
    await flushPromises()

    // The sanitized rows render compactly; the precondition line is present.
    expect(wrapper.find('[data-test=action-preview-rows]').exists()).toBe(true)
    expect(wrapper.get('[data-test=action-preview-rows]').text()).toContain('学习：函数')
    expect(wrapper.get('[data-test=action-preview-rows]').text()).toContain('2030-01-01')
    expect(wrapper.get('[data-test=action-preview-rows]').text()).toContain('270 分钟')
    expect(wrapper.get('[data-test=action-preview-precondition]').text()).toContain('已校验')
  })

  it('a double click on confirm resolves the approval exactly once', async () => {
    client.agentApprovalList.mockResolvedValue([approval('ap-1', 'pending')])
    client.resolveAgentApproval.mockResolvedValue(approval('ap-1', 'approved'))
    const { wrapper } = mountPage()
    await flushPromises()

    // Two clicks in the same tick: the per-approval claim guard must collapse
    // them into a single resolve invoke.
    const button = wrapper.get('[data-test=approval-approve-ap-1]')
    await button.trigger('click')
    await button.trigger('click')
    await flushPromises()

    expect(client.resolveAgentApproval).toHaveBeenCalledTimes(1)
    expect(client.resolveAgentApproval).toHaveBeenCalledWith('ap-1', true)
  })

  it('offers an undo for an executed plan apply and routes it through Rust', async () => {
    client.agentApprovalList.mockResolvedValue([
      {
        ...approval('ap-1', 'approved'),
        preview: { ...approval('ap-1', 'approved').preview!, undo_available: true },
      },
    ])
    client.undoAgentTool.mockResolvedValue({
      step_id: 'step-ap-1',
      output: {
        kind: 'plan.apply_preview.v1',
        exam_id: 'exam-1',
        inserted_plan_ids: ['draft-1'],
        restored_plan_ids: ['plan-old'],
        restored_record_count: 0,
        status: 'undone',
      },
    })
    const { wrapper } = mountPage()
    await flushPromises()

    expect(wrapper.find('[data-test=approval-undo-ap-1]').exists()).toBe(true)
    await wrapper.get('[data-test=approval-undo-ap-1]').trigger('click')
    await flushPromises()

    expect(client.undoAgentTool).toHaveBeenCalledWith('step-ap-1')
    // The approval list and brief are refreshed so the restored state shows.
    expect(client.agentApprovalList).toHaveBeenCalledTimes(2)
  })

  it('keeps record.checkin_plan undoable through the same Rust path', async () => {
    client.agentApprovalList.mockResolvedValue([
      {
        ...approval('ap-2', 'approved'),
        step_id: 'step-ap-2',
        preview: {
          ...approval('ap-2', 'approved').preview!,
          tool: 'record.checkin_plan',
          undo_available: true,
        },
      },
    ])
    client.undoAgentTool.mockResolvedValue({
      step_id: 'step-ap-2',
      output: {
        kind: 'record.checkin_plan.v1',
        record_id: 'rec-1',
        plan_id: 'plan-1',
        removed_wrong_question_ids: [],
        actual_duration: 0,
        actual_tasks: null,
        status: 'pending',
      },
    })
    const { wrapper } = mountPage()
    await flushPromises()

    expect(wrapper.find('[data-test=approval-undo-ap-2]').exists()).toBe(true)
    await wrapper.get('[data-test=approval-undo-ap-2]').trigger('click')
    await flushPromises()

    expect(client.undoAgentTool).toHaveBeenCalledWith('step-ap-2')
  })

  it('does not offer an undo when the preview says it is unavailable', async () => {
    client.agentApprovalList.mockResolvedValue([
      {
        ...approval('ap-1', 'approved'),
        preview: { ...approval('ap-1', 'approved').preview!, undo_available: false },
      },
    ])
    const { wrapper } = mountPage()
    await flushPromises()

    expect(wrapper.find('[data-test=approval-undo-ap-1]').exists()).toBe(false)
    expect(client.undoAgentTool).not.toHaveBeenCalled()
  })
})
