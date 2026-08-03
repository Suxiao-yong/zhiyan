// Workbench registry for the Agent OS right pane (M6 Task 4).
export type WorkbenchKey = 'checkin' | 'plan' | 'record' | 'analysis' | 'visualization'

export const WORKBENCHES: { key: WorkbenchKey; label: string }[] = [
  { key: 'checkin', label: '计划打卡' },
  { key: 'plan', label: '学习计划' },
  { key: 'record', label: '记录与错题' },
  { key: 'analysis', label: 'AI 分析' },
  { key: 'visualization', label: '数据可视化' },
]
