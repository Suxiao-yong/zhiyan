import { describe, it, expect } from 'vitest'
import {
  planGenerationPrompt,
  diagnosticPrompt,
  adjustmentPrompt,
  predictionPrompt,
} from './prompts'

describe('prompts', () => {
  it('planGenerationPrompt 注入数据并要求严格 JSON', () => {
    const r = planGenerationPrompt({
      exam: { name: '考研', exam_date: '2027-01-01', total_score: 500 },
      subjects: [
        { id: 's1', name: '数学', target_score: 150, current_level: 3, weight: 1 },
      ],
      knowledgePoints: [],
      dailyHours: 6,
      examDate: '2027-01-01',
      daysLeft: 30,
    })
    expect(r.system).toContain('学习规划师')
    expect(r.user).toContain('数学')
    expect(r.user).toContain('2027-01-01')
    expect(r.user).toContain('严格 JSON')
  })

  it('diagnosticPrompt 含 weak_points / recommendations schema', () => {
    const r = diagnosticPrompt({ studyData: '...', period: 'weekly' })
    expect(r.user).toContain('weak_points')
    expect(r.user).toContain('recommendations')
  })

  it('adjustmentPrompt 含 deviation_summary / adjustment_proposal', () => {
    const r = adjustmentPrompt({
      originalPlan: 'p',
      actualProgress: 'a',
      deviation: 'd',
    })
    expect(r.user).toContain('deviation_summary')
    expect(r.user).toContain('adjustment_proposal')
  })

  it('predictionPrompt 含 predictions / alerts', () => {
    const r = predictionPrompt({ studySummary: 's', targets: '{}' })
    expect(r.user).toContain('predictions')
    expect(r.user).toContain('alerts')
  })
})
