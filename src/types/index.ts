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

/** AI 分析 */
export type AnalysisType = 'daily' | 'weekly' | 'phase' | 'prediction' | 'adjustment'
export type UserConfirmed = 0 | 1 | 2 // 未处理/确认/拒绝

export interface AiAnalysis {
  id: string
  analysis_type: AnalysisType
  period_start: string | null
  period_end: string | null
  subjects_analyzed: string | null // JSON
  content: string | null
  suggestions: string | null // JSON
  scores_prediction: string | null // JSON
  generated_by: GeneratedBy
  user_confirmed: UserConfirmed
  applied: number // 0/1
  applied_at: string | null
  created_at: string
}

/** LLM 配置（apiKey 为解密后明文，仅存内存，绝不进入持久化 store） */
export interface LLMConfig {
  provider: string // openai/deepseek/qwen/kimi/ollama/custom
  apiKey: string
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
