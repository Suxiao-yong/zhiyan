import { describe, it, expect, vi } from 'vitest'

// 本地统计聚合测试（Task 9：只保留确定性事实层，删除 AI 文案函数）。
// mock 掉 db（plugin-sql）与 record-service（Tauri 时间相关）
vi.mock('./db', () => ({ query: vi.fn() }))
vi.mock('./record-service', () => ({ businessToday: () => '2026-07-01' }))

import { aggregateDaily } from './analyzer'
import { query } from './db'

describe('analyzer 本地统计事实层', () => {
  it('aggregateDaily 统计来自 SQL 聚合，不由 LLM 计算', async () => {
    vi.mocked(query).mockImplementation(async (sql: string) => {
      if (sql.includes('FROM subjects')) {
        return [{ id: 's1', name: '数学', exam_id: 'exam-1', sort_order: 0 }]
      }
      if (sql.includes('study_records')) {
        return [
          {
            id: 'r1',
            subject_id: 's1',
            date: '2026-07-01',
            duration_min: 120,
            correct_count: 10,
            questions_count: 20,
          },
        ]
      }
      return []
    })
    const agg = await aggregateDaily('exam-1', '2026-07-01')
    expect(agg.bySubject.length).toBe(1)
    expect(agg.bySubject[0].subject).toBe('数学')
    expect(agg.bySubject[0].totalMin).toBe(120)
    expect(agg.bySubject[0].avgCorrectRate).toBe(50)
    expect(query).toHaveBeenCalled()
  })
})
