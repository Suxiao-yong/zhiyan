// Workbench registry for the Agent OS right pane. Core study surfaces —
// Today check-in, Plan, Records — plus the review card (learning loop).
// Analysis and visualization are no longer page embeddings (analysis moves
// into the conversation; the two statistics blocks live inside Plan/Records).
export type WorkbenchKey = 'checkin' | 'plan' | 'record' | 'review'

export const WORKBENCHES: { key: WorkbenchKey; label: string }[] = [
  { key: 'checkin', label: '今日打卡' },
  { key: 'plan', label: '计划' },
  { key: 'record', label: '记录' },
  { key: 'review', label: '今日复习' },
]
