// Prompt 模板。
// Phase 3：计划生成 Agent。
// Phase 4：学习诊断 / 动态调整 / 预测预警 Agent。
// 解析容错复用 llm-adapter.parseJsonResponse，由 agent-engine 调用。

// ============ 计划生成（Phase 3） ============

export interface PlanPromptSubject {
  id: string
  name: string
  target_score: number | null
  current_level: number
  weight: number
}

export interface PlanPromptKp {
  id: string
  subject_id: string
  name: string
  weight: number
  difficulty_level: number
  current_mastery: number
}

export interface PlanPromptData {
  exam: { name: string; exam_date: string; total_score: number | null }
  subjects: PlanPromptSubject[]
  knowledgePoints: PlanPromptKp[]
  dailyHours: number
  examDate: string
  daysLeft: number
}

export interface ParsedPlanTask {
  subject_id: string
  knowledge_point_id?: string | null
  task: string
  duration: number
}

export interface ParsedPlanDay {
  date: string
  tasks: ParsedPlanTask[]
}

export interface ParsedPlanPhase {
  name: string
  start: string
  end: string
}

export interface ParsedPlan {
  phases: ParsedPlanPhase[]
  dailyPlans: ParsedPlanDay[]
}

export function planGenerationPrompt(data: PlanPromptData): {
  system: string
  user: string
} {
  const system =
    '你是一位资深的学习规划师。请根据考试信息生成科学的个性化复习计划。' +
    '严格按要求输出 JSON，不要在 JSON 前后加任何文字或 Markdown 标记。'
  const user = `考试信息：${JSON.stringify({
    name: data.exam.name,
    exam_date: data.exam.exam_date,
    total_score: data.exam.total_score,
  })}
各科目（id/名称/目标分/当前水平1-5/权重）：${JSON.stringify(data.subjects)}
各科知识点（id/subject_id/名称/权重/难度1-5/当前掌握1-5）：${JSON.stringify(
    data.knowledgePoints,
  )}
每天可用学习时长：${data.dailyHours} 小时
考试日期：${data.examDate}
距考试剩余：${data.daysLeft} 天

要求：
1. 按基础期(前50%时间)、强化期(中间30%)、冲刺期(最后20%)分阶段
2. 每日总学习量不超过 ${data.dailyHours} 小时
3. 每周留出半天作为复习日和弹性缓冲
4. 薄弱科目(当前水平远低于目标)与低掌握度知识点多分配时间
5. 任务引用的 subject_id 必须来自上面提供的科目 id；knowledge_point_id 可选，若用须来自对应科目的知识点 id
6. 日期必须是从今天到考试日期之间的真实日期(YYYY-MM-DD)，不能凭空捏造

请以严格 JSON 输出（不要任何额外文字）：
{"phases":[{"name":"基础期","start":"YYYY-MM-DD","end":"YYYY-MM-DD"}],"dailyPlans":[{"date":"YYYY-MM-DD","tasks":[{"subject_id":"<科目id>","knowledge_point_id":"<知识点id或null>","task":"任务描述","duration":120}]}]}`
  return { system, user }
}

// ============ 计划阶段策略（Phase 5 优化：小输出，本地展开为逐日） ============

/**
 * 让 AI 只输出阶段策略（每阶段每科每周时长 + 重点知识点），输出小、快（10-30s），
 * 由 plan-generator.expandStrategy 展开为逐日计划。避免逐日生成上万 token 导致慢/超时/超 token 上限。
 */
export function planStrategyPrompt(data: PlanPromptData): {
  system: string
  user: string
} {
  const system =
    '你是一位资深的学习规划师。请根据考试信息给出阶段化的复习策略。' +
    '严格按要求输出 JSON，不要加额外文字或 Markdown 标记。'
  const user = `考试：${JSON.stringify({
    name: data.exam.name,
    exam_date: data.exam.exam_date,
    total_score: data.exam.total_score,
  })}
科目（id/名称/目标分/当前水平1-5/权重）：${JSON.stringify(data.subjects)}
知识点（id/subject_id/名称/当前掌握1-5）：${JSON.stringify(
    data.knowledgePoints.map((k) => ({
      id: k.id,
      subject_id: k.subject_id,
      name: k.name,
      current_mastery: k.current_mastery,
    })),
  )}
每日可用学习时长：${data.dailyHours} 小时
考试日期：${data.examDate}，距考试剩余 ${data.daysLeft} 天

要求：
1. 按基础期(前50%时间)、强化期(中间30%)、冲刺期(最后20%)分阶段，给出每阶段起止日期(YYYY-MM-DD，从今天到考试日期)
2. 每阶段为每个科目给出"每周建议学习时长(小时)"与"重点知识点(focus_kps，用上面提供的知识点 id)"
3. 薄弱科目(当前水平低/目标分高)与低掌握度知识点多分配时长
4. subject_weekly_hours 的 key 必须是上面提供的科目 id

请以严格 JSON 输出（不要任何额外文字）：
{"phases":[{"name":"基础期","start":"YYYY-MM-DD","end":"YYYY-MM-DD","subject_weekly_hours":{"<科目id>":每周小时数},"focus_kps":["<知识点id>"]}]}`
  return { system, user }
}

// ============ 学习诊断（Phase 4） ============

export interface DiagnosticData {
  studyData: string // 聚合摘要，≤2000 字符
  period: 'daily' | 'weekly'
}

export function diagnosticPrompt(data: DiagnosticData): {
  system: string
  user: string
} {
  const system =
    '你是一位资深的学习分析师。请根据用户的学习数据诊断学习情况。' +
    '严格按要求输出 JSON，不要加额外文字或 Markdown 标记。'
  const user = `输入数据：
${data.studyData}

请以严格 JSON 输出：
{"summary":"学习情况总结(100字内)","weak_points":[{"subject":"科目名","knowledge_point":"薄弱知识点","evidence":"数据证据(如正确率XX%)","severity":"高/中/低"}],"efficiency_analysis":{"best_session":"效率最高时段(morning/afternoon/evening)","avg_duration_per_day":日均学习时长(分钟,数字),"trend":"上升/下降/平稳"},"recommendations":[{"action":"建议操作","reason":"原因","priority":1到5的数字}]}`
  return { system, user }
}

// ============ 动态调整（Phase 4） ============

export interface AdjustmentData {
  originalPlan: string
  actualProgress: string
  deviation: string
}

export function adjustmentPrompt(data: AdjustmentData): {
  system: string
  user: string
} {
  const system =
    '你是学习进度管理专家。计划执行出现偏差，请分析原因并给出调整建议。' +
    '严格按要求输出 JSON，不要加额外文字。'
  const user = `原计划：${data.originalPlan}
实际完成情况：${data.actualProgress}
偏差量：${data.deviation}

请以严格 JSON 输出：
{"deviation_summary":"偏差概述(50字内)","root_cause_analysis":"偏差原因分析","adjustment_proposal":{"strategy":"压缩/重排/删减/延期","specific_changes":[{"date":"日期","original":"原计划任务","adjusted":"调整后任务","reason":"调整原因"}]},"risk_assessment":{"if_not_adjusted":"不调整的后果","recommended_action":"强烈建议/建议考虑/可忽略"},"user_decision_required":true}`
  return { system, user }
}

// ============ 预测预警（Phase 4） ============

export interface PredictionData {
  studySummary: string
  targets: string // JSON {科目名:目标分}
}

export function predictionPrompt(data: PredictionData): {
  system: string
  user: string
} {
  const system =
    '你是考试预测分析专家。请根据用户的近期学习统计数据，预测各科的可能得分并识别风险点。' +
    '严格按要求输出 JSON，不要加额外文字。'
  const user = `各科学习数据摘要：${data.studySummary}
各科目标分数：${data.targets}

请以严格 JSON 输出：
{"predictions":[{"subject":"科目名","predicted_score":预估得分(数字),"target_score":目标分(数字),"gap":差距(数字),"confidence":"高/中/低","trend":"上升/下降/持平","risk_level":"高风险/中风险/低风险"}],"alerts":[{"subject":"科目","issue":"风险描述","severity":"critical/warning/info"}],"overall_assessment":"总体评估(100字内)","urgent_actions":["最紧急需要做的事"]}`
  return { system, user }
}
