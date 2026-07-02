import { describe, it, expect, vi } from 'vitest'

// mock 掉 db（plugin-sql）与 record-service（Tauri 时间相关）
vi.mock('./db', () => ({ query: vi.fn() }))
vi.mock('./record-service', () => ({ businessToday: () => '2026-07-01' }))

import { localWeeklyAnalysis, localPrediction } from './analyzer'
import type { WeeklyAggregation, SubjectStat } from './analyzer'

const bySubject: SubjectStat[] = [
  { subjectId: 's1', subject: '数学', totalMin: 120, avgCorrectRate: 50, trend: '下降', records: 5 },
  { subjectId: 's2', subject: '英语', totalMin: 80, avgCorrectRate: 85, trend: '上升', records: 5 },
]

describe('analyzer 本地降级', () => {
  it('localWeeklyAnalysis 生成内容 + 建议，标注本地', () => {
    const agg: WeeklyAggregation = {
      bySubject,
      kpMastery: [{ kp: '极限', subject: '数学', mastery: 2 }],
      planCompletion: [],
      summaryText: '',
    }
    const r = localWeeklyAnalysis(agg)
    expect(r.content).toContain('本地分析')
    expect(r.content).toContain('数学')
    // 数学正确率 50% < 70 → 应产生建议
    expect(r.suggestions.length).toBeGreaterThan(0)
    expect(r.scores_prediction).toBeNull()
  })

  it('localPrediction 低置信度 + 目标分', () => {
    const r = localPrediction(bySubject, [
      { subject: '数学', target: 150 },
      { subject: '英语', target: 100 },
    ])
    expect(r.predictions).toHaveLength(2)
    expect(r.predictions[0].confidence).toContain('低')
    expect(r.predictions[0].target_score).toBe(150)
    // 数学正确率 50% → 预测 75，距 150 差 -75 → 高风险
    expect(r.predictions[0].risk_level).toBe('高风险')
    // 英语正确率 85% → 预测 85，距 100 差 -15 → 中风险
    expect(r.predictions[1].risk_level).toBe('中风险')
  })
})
