// Workbench registry for the Agent OS right pane. Task 3: only the three core
// study surfaces — Today check-in, Plan, Records. Analysis and visualization
// are no longer page embeddings (analysis moves into the conversation; the
// two statistics blocks live inside Plan/Records).
export type WorkbenchKey = 'checkin' | 'plan' | 'record'

export const WORKBENCHES: { key: WorkbenchKey; label: string }[] = [
  { key: 'checkin', label: '今日打卡' },
  { key: 'plan', label: '计划' },
  { key: 'record', label: '记录' },
]
