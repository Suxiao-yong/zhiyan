import { describe, it, expect, vi } from 'vitest'

// mock 掉 db / record-service / llm-adapter（避免 Tauri 运行时依赖）
vi.mock('./db', () => ({
  query: vi.fn(),
  getById: vi.fn(),
  insert: vi.fn(async () => 'mock-id'),
  execute: vi.fn(async () => ({ lastInsertId: 0, rowsAffected: 0 })),
  getSetting: vi.fn(async () => null),
  update: vi.fn(),
}))
vi.mock('./record-service', () => ({ businessToday: () => '2026-07-01' }))
vi.mock('./llm-adapter', () => ({
  callLLM: vi.fn(),
  parseJsonResponse: vi.fn(),
}))

import { generateLocalPlan } from './plan-generator'
import * as db from './db'

describe('generateLocalPlan 本地算法', () => {
  it('按剩余天数生成计划 + 阶段', async () => {
    vi.mocked(db.getById).mockResolvedValue({
      id: 'e1',
      name: '考研',
      exam_type: 'postgrad',
      exam_date: '2026-12-31',
      total_score: 500,
      description: null,
      created_at: '',
      updated_at: '',
    } as any)
    vi.mocked(db.query).mockImplementation(async (sql: string) => {
      if (sql.includes('subjects')) {
        return [
          {
            id: 's1',
            exam_id: 'e1',
            name: '数学',
            target_score: 150,
            current_level: 3,
            weight: 1,
            sort_order: 0,
            created_at: '',
            updated_at: '',
          },
        ] as any
      }
      return [] as any
    })

    const r = await generateLocalPlan('e1')
    expect(r.generatedBy).toBe('local')
    // 2026-07-01 → 2026-12-31 约 183 天
    expect(r.totalDays).toBeGreaterThan(100)
    expect(r.phases.length).toBeGreaterThan(0)
    expect(r.phases.map((p) => p.name)).toEqual(['基础期', '强化期', '冲刺期'])
    expect(r.planCount).toBeGreaterThan(0)
    expect(r.message).toContain('本地算法')
  })

  it('考试日期已过 → 抛错', async () => {
    vi.mocked(db.getById).mockResolvedValue({
      id: 'e2',
      name: '过期',
      exam_type: 'custom',
      exam_date: '2020-01-01',
      total_score: null,
      description: null,
      created_at: '',
      updated_at: '',
    } as any)
    vi.mocked(db.query).mockResolvedValue([] as any)
    await expect(generateLocalPlan('e2')).rejects.toThrow('考试日期已过')
  })
})
