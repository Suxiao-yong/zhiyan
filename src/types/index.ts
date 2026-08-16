// 智研核心类型定义（与 SQLite schema 一一对应；时间均为 ISO 8601 / localtime）

/** 考试类型 */
export type ExamType = 'postgrad' | 'civil' | 'cert' | 'custom'

/** 考试配置 */
export interface Exam {
  id: string
  name: string
  exam_type: string | null
  exam_date: string // YYYY-MM-DD
  total_score: number | null
  description: string | null
  created_at: string
  updated_at: string
}

/** 科目 */
export interface Subject {
  id: string
  exam_id: string
  name: string
  target_score: number | null
  current_level: number // 1-5
  weight: number
  sort_order: number
  created_at: string
  updated_at: string
}

/** 知识点（支持树形结构） */
export interface KnowledgePoint {
  id: string
  subject_id: string
  name: string
  parent_id: string | null
  weight: number
  difficulty_level: number // 1-5
  current_mastery: number // 1-5：Welcome 自评初始化，学习记录聚合更新
  chapter: string | null
  sort_order: number
  created_at: string
  updated_at: string
  children?: KnowledgePoint[] // 树形构建用
}

/** 学习记录 */
export interface StudyRecord {
  id: string
  plan_id: string | null
  date: string
  subject_id: string
  knowledge_point_id: string | null
  duration_min: number
  content: string | null
  questions_count: number
  correct_count: number
  mastery_rating: number | null // 1-5
  difficulty_notes: string | null
  mood: number | null // 1-5
  session_time: string | null // morning/afternoon/evening
  created_at: string
  updated_at: string
}

/** 学习计划 */
export type PlanStatus = 'pending' | 'in_progress' | 'completed' | 'skipped'
export type GeneratedBy = 'ai' | 'local'

export interface StudyPlan {
  id: string
  exam_id: string
  subject_id: string
  knowledge_point_id: string | null
  date: string
  planned_tasks: string | null
  planned_duration: number | null
  actual_duration: number | null
  actual_tasks: string | null
  status: PlanStatus
  generated_by: GeneratedBy
  ai_suggestion: string | null
  user_modified: number // 0/1
  sort_order: number
  created_at: string
  updated_at: string
}

/** 错题 */
export interface WrongQuestion {
  id: string
  record_id: string | null
  subject_id: string
  knowledge_point_id: string | null
  question_source: string | null
  question_desc: string | null
  correct_answer: string | null
  my_answer: string | null
  error_type: string | null // 概念不清/计算错误/粗心/其他
  error_reason: string | null
  review_count: number
  mastered: number // 0/1
  created_at: string
  last_review_at: string | null
}

/** LLM 非敏感配置（保存于 settings 表）。API Key 只存在于 OS 凭据管理器，
 * 由 Rust 内部读取，永不进入前端 store / 页面状态 / 消息。 */
export interface LLMConfig {
  provider: string // openai/deepseek/qwen/kimi/custom
  baseUrl: string
  model: string
  temperature: number
}

/** LLM 消息（供 adapter 使用） */
export interface LLMMessage {
  role: 'system' | 'user' | 'assistant'
  content: string
}

/** 系统设置（键值表，value 统一字符串） */
export interface Setting {
  key: string
  value: string | null
  description: string | null
  updated_at: string
}

/** Agent runtime session */
export interface AgentSession {
  id: string
  exam_id: string | null
  title: string
  status: 'active' | 'archived'
  created_at: string
  updated_at: string
}

/** One conversation message (M5 Agent OS). */
export interface AgentMessage {
  id: string
  session_id: string
  run_id: string | null
  role: 'user' | 'assistant' | 'system'
  text: string
  content_json: string | null
  prompt_tokens: number
  completion_tokens: number
  model: string | null
  created_at: string
}

/** Agent runtime run state */
export type AgentRunStatus =
  'queued' | 'running' | 'waiting_approval' | 'completed' | 'cancelled' | 'failed' | 'interrupted'

/** Agent runtime run */
export interface AgentRun {
  id: string
  session_id: string
  goal: string
  status: AgentRunStatus
  trigger_source: 'user' | 'startup' | 'schedule' | 'recovery'
  current_step: number
  error_code: string | null
  created_at: string
  updated_at: string
  started_at: string | null
  completed_at: string | null
}

/** Execution ownership of a registered Agent tool. Task 13: every tool is
 * Rust-owned; the legacy typescript/shadow/unavailable states are gone. */
export type AgentToolOwnership = 'rust-owned'
export type AgentToolRisk = 'R0' | 'R1' | 'R2' | 'R3' | 'R4'
export type AgentToolConfirmation =
  'automatic' | 'summary_or_setting' | 'required' | 'navigation_only'
export type AgentToolIdempotency = 'retry_safe' | 'required_exactly_once' | 'no_automatic_retry'

/** Static metadata serialized by Rust's ToolDescriptor. */
export interface AgentToolDescriptor {
  name: string
  version: string
  input_schema: unknown
  output_schema: unknown
  risk: AgentToolRisk
  confirmation: AgentToolConfirmation
  supports_undo: boolean
  timeout_ms: number
  idempotency: AgentToolIdempotency
  data_permissions: string[]
}

/** Static descriptor paired with its Rust execution ownership (Task 13). */
export interface ListedAgentTool {
  descriptor: AgentToolDescriptor
  ownership: AgentToolOwnership
}

export type JsonValue =
  null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue }

export interface AgentToolCallRequest {
  run_id: string
  step_index: number
  tool_name: string
  tool_version: string
  input: JsonValue
  idempotency_key: string | null
  approval_id: string | null
}

export type AgentToolCallResponse =
  | {
      state: 'completed'
      step_id: string
      output: unknown
      replayed: boolean
      undo_available: boolean
    }
  | {
      state: 'waiting_approval'
      step_id: string
      approval_id: string
      preview: unknown
      expires_at: string
    }
  | {
      state: 'summary_required'
      step_id: string
      preview: unknown
    }
  | {
      state: 'navigation_required'
      route: string
      reason: string
    }

export interface AgentActionPreviewConflict {
  kind: string
  affected_count?: number
  detail: string
}

/**
 * 标准化的审批预览（Task 10）:由 Rust 生成并脱敏,包含动作名称、影响
 * 对象数量、日期范围、前后摘要、结构化字段、冲突、风险与 undo 可用性。
 * 绝不包含 API Key、模型原始输出或完整请求体。
 */
export interface AgentActionPreview {
  tool: string
  risk: number
  undo_available: boolean
  action: string
  affected_count: number
  summary: string
  conflicts: AgentActionPreviewConflict[]
  date_range: string
  fields?: Record<string, unknown>
}

export interface AgentApproval {
  id: string
  run_id: string
  step_id: string
  risk: number
  preview: AgentActionPreview | null
  precondition_hash: string
  status: string
  expires_at: string
  decided_at: string | null
  created_at: string
}

export type AgentApprovalRecord = AgentApproval

export interface AgentToolUndoResponse {
  step_id: string
  output:
    | {
        kind: 'record.checkin_plan.v1'
        record_id: string
        plan_id: string
        removed_wrong_question_ids: string[]
        actual_duration: number
        actual_tasks: string | null
        status: string
      }
    | {
        kind: 'plan.apply_preview.v1'
        exam_id: string
        inserted_plan_ids: string[]
        restored_plan_ids: string[]
        restored_record_count: number
        status: string
      }
}

export type AgentPlannerTraceEntry =
  | { kind: 'tool_called'; name: string; step_id: string; replayed: boolean }
  | { kind: 'tool_waiting_approval'; name: string; approval_id: string }
  | { kind: 'tool_navigation_required'; name: string; route: string }
  | { kind: 'tool_summary_required'; name: string }
  | { kind: 'max_iterations' }
  | { kind: 'local_fallback'; reason: string }

export interface AgentPlannerTurn {
  mode: 'model' | 'local'
  final_text: string
  iterations: number
  model_calls: number
  prompt_tokens: number
  completion_tokens: number
  estimated_cost_usd: number
  trace: AgentPlannerTraceEntry[]
}

/** M4 background job types. */
/** The daily brief (local skeleton, optionally with an LLM explanation). */
export interface AgentBrief {
  date: string
  mode: 'model' | 'local'
  summary: string
  explanation: string | null
  today_planned: number
  today_completed: number
  today_duration_min: number
  overdue_count: number
  week_completion_rate: number
  due_wrong_questions: number
  weak_areas: {
    subject_id: string
    subject_name: string
    knowledge_point_id: string | null
    knowledge_point_name: string | null
    total_questions: number
    correct_questions: number
    correctness: number
  }[]
}

/** One model-call audit row from the Context Inspector (never raw content). */
export interface AgentContextAuditRow {
  id: string
  call_seq: number
  purpose: string
  local: boolean
  prompt_tokens: number
  completion_tokens: number
  tools_offered: string[]
  categories: string[]
  record_ids: Record<string, string[]>
  field_sets: Record<string, string[]>
  created_at: string
}
