import { beforeEach, describe, expect, it, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))

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
vi.mock('@tauri-apps/api/core', () => ({ invoke }))
vi.mock('@tauri-apps/api/path', () => ({ appConfigDir: vi.fn(), appDataDir: vi.fn() }))

import * as db from './db'
import * as fs from '@tauri-apps/plugin-fs'
import * as pathApi from '@tauri-apps/api/path'
import * as processApi from '@tauri-apps/plugin-process'
import { importData, restoreDatabase, validateBundle } from './export'

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

  it('restores into the canonical config database after both pools close', async () => {
    const order: string[] = []
    vi.mocked(db.closeDb).mockImplementation(async () => {
      order.push('closeDb')
    })
    vi.mocked(pathApi.appDataDir).mockResolvedValue('/legacy-data')
    vi.mocked(pathApi.appConfigDir).mockResolvedValue('/canonical-config')
    vi.mocked(fs.readFile).mockResolvedValue(new Uint8Array([1, 2, 3]))
    vi.mocked(fs.writeFile).mockImplementation(async () => {
      order.push('writeFile')
    })
    vi.mocked(processApi.relaunch).mockImplementation(async () => {
      order.push('relaunch')
    })
    invoke.mockImplementation(async (command: string) => {
      order.push(command)
    })
    const { open } = await import('@tauri-apps/plugin-dialog')
    vi.mocked(open).mockResolvedValue('/backup.db')

    await restoreDatabase()

    expect(invoke).toHaveBeenCalledWith('agent_prepare_database_restore')
    expect(fs.writeFile).toHaveBeenCalledWith(
      '/canonical-config/zhiyan.db',
      expect.any(Uint8Array),
    )
    expect(order.indexOf('closeDb')).toBeLessThan(order.indexOf('agent_prepare_database_restore'))
    expect(order.indexOf('agent_prepare_database_restore')).toBeLessThan(order.indexOf('writeFile'))
    expect(order.indexOf('writeFile')).toBeLessThan(order.indexOf('relaunch'))
  })
})
