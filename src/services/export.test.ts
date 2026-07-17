import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('./db', () => ({
  closeDb: vi.fn(),
  execute: vi.fn(),
  getById: vi.fn(),
  insert: vi.fn(),
  query: vi.fn(),
}))
vi.mock('@tauri-apps/plugin-fs', () => ({
  readFile: vi.fn(),
  writeFile: vi.fn(),
  writeTextFile: vi.fn(),
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }))
vi.mock('@tauri-apps/plugin-process', () => ({ relaunch: vi.fn() }))
vi.mock('@tauri-apps/api/path', () => ({ appDataDir: vi.fn() }))

import * as db from './db'
import { importData, validateBundle } from './export'

describe('导入数据兼容性', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(db.getById).mockResolvedValue(null)
  })

  it('接受学习记录没有 plan_id 的旧版备份', () => {
    const bundle = {
      version: 1,
      exportedAt: '2026-07-17T00:00:00.000Z',
      exams: [{ id: 'e1', name: '考试', exam_date: '2026-12-01' }],
      subjects: [],
      knowledge_points: [],
      study_records: [
        {
          id: 'r1',
          date: '2026-07-16',
          subject_id: 's1',
          duration_min: 30,
        },
      ],
      study_plans: [],
      wrong_questions: [],
      ai_analyses: [],
    }

    expect(validateBundle(bundle)).toEqual({ ok: true, errors: [] })
  })

  it('先导入计划再导入引用该计划的学习记录', async () => {
    const bundle = {
      exams: [],
      subjects: [],
      knowledge_points: [],
      study_plans: [{ id: 'p1', exam_id: 'e1', subject_id: 's1', date: '2026-07-17' }],
      study_records: [
        {
          id: 'r1',
          plan_id: 'p1',
          date: '2026-07-17',
          subject_id: 's1',
          duration_min: 30,
        },
      ],
      wrong_questions: [],
      ai_analyses: [],
    }

    await importData(bundle, 'skip')

    const tables = vi.mocked(db.insert).mock.calls.map(([table]) => table)
    expect(tables.indexOf('study_plans')).toBeLessThan(tables.indexOf('study_records'))
  })
})
