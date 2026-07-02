// 数据导入导出 + 数据库备份恢复。
// 大数据量 UI 阻塞防护：导出分批 500 查、导入分批 100 写 + setTimeout 让出主线程。
// 导入前 schema 校验；冲突按 UUID 处理（skip/overwrite/merge）。
// 备份用 VACUUM INTO（一致性快照，不直接复制 db 文件——WAL 模式下直接复制可能不一致）。

import { execute, getById, insert, query } from './db'
import { closeDb } from './db'
import { readFile, writeFile, writeTextFile } from '@tauri-apps/plugin-fs'
import { open, save } from '@tauri-apps/plugin-dialog'
import { relaunch } from '@tauri-apps/plugin-process'
import { appDataDir } from '@tauri-apps/api/path'

const CHUNK = 500
const IMPORT_CHUNK = 100

function yieldUI(): Promise<void> {
  return new Promise((r) => setTimeout(r, 0))
}

/** 分批查询（LIMIT/OFFSET，每批让出主线程） */
async function queryChunked<T>(sql: string, params: unknown[] = []): Promise<T[]> {
  const out: T[] = []
  let offset = 0
  while (true) {
    const rows = await query<T>(`${sql} LIMIT ${CHUNK} OFFSET ${offset}`, params)
    out.push(...rows)
    if (rows.length < CHUNK) break
    offset += CHUNK
    await yieldUI()
  }
  return out
}

export interface ExportBundle {
  version: 1
  exportedAt: string
  exams: any[]
  subjects: any[]
  knowledge_points: any[]
  study_records: any[]
  study_plans: any[]
  wrong_questions: any[]
  ai_analyses: any[]
}

const TABLES = [
  'exams',
  'subjects',
  'knowledge_points',
  'study_records',
  'study_plans',
  'wrong_questions',
  'ai_analyses',
] as const

export type ExportScope = 'all' | 'exam' | 'date'

export async function exportData(range: {
  scope: ExportScope
  examId?: string
  from?: string
  to?: string
}): Promise<ExportBundle> {
  const examFilter = range.scope === 'exam' && range.examId ? 'WHERE exam_id = ?' : ''
  const examParams = range.scope === 'exam' && range.examId ? [range.examId] : []

  const exams =
    range.scope === 'exam' && range.examId
      ? await queryChunked<any>('SELECT * FROM exams WHERE id = ?', [range.examId])
      : await queryChunked<any>('SELECT * FROM exams')

  const subjects = await queryChunked<any>(`SELECT * FROM subjects ${examFilter}`, examParams)

  // 该考试下的科目 id（用于 kps/records/wrong 的 IN 过滤）
  const subjectIds = (subjects as any[]).map((s) => s.id)

  const kpWhere =
    range.scope === 'exam' && subjectIds.length
      ? `WHERE subject_id IN (${subjectIds.map(() => '?').join(',')})`
      : ''
  const kpParams = range.scope === 'exam' && subjectIds.length ? subjectIds : []
  const knowledge_points = await queryChunked<any>(
    `SELECT * FROM knowledge_points ${kpWhere}`,
    kpParams,
  )

  // study_records：exam→按科目 IN；date→按日期；all→全部
  let recSql = 'SELECT * FROM study_records'
  let recParams: unknown[] = []
  if (range.scope === 'exam' && subjectIds.length) {
    recSql += ` WHERE subject_id IN (${subjectIds.map(() => '?').join(',')})`
    recParams = subjectIds
  } else if (range.scope === 'date' && range.from && range.to) {
    recSql += ' WHERE date >= ? AND date <= ?'
    recParams = [range.from, range.to]
  }
  const study_records = await queryChunked<any>(recSql, recParams)

  // study_plans：exam→exam_id；date→日期；all→全部
  let planSql = 'SELECT * FROM study_plans'
  let planParams: unknown[] = []
  if (range.scope === 'exam' && range.examId) {
    planSql += ' WHERE exam_id = ?'
    planParams = [range.examId]
  } else if (range.scope === 'date' && range.from && range.to) {
    planSql += ' WHERE date >= ? AND date <= ?'
    planParams = [range.from, range.to]
  }
  const study_plans = await queryChunked<any>(planSql, planParams)

  // wrong_questions：exam→按科目 IN
  let wqSql = 'SELECT * FROM wrong_questions'
  let wqParams: unknown[] = []
  if (range.scope === 'exam' && subjectIds.length) {
    wqSql += ` WHERE subject_id IN (${subjectIds.map(() => '?').join(',')})`
    wqParams = subjectIds
  }
  const wrong_questions = await queryChunked<any>(wqSql, wqParams)

  const ai_analyses = await queryChunked<any>('SELECT * FROM ai_analyses')

  return {
    version: 1,
    exportedAt: new Date().toISOString(),
    exams,
    subjects,
    knowledge_points,
    study_records,
    study_plans,
    wrong_questions,
    ai_analyses,
  }
}

/** schema 校验：结构非法 → 拒绝整批 */
export function validateBundle(b: any): { ok: boolean; errors: string[] } {
  const errors: string[] = []
  if (!b || typeof b !== 'object' || Array.isArray(b)) {
    return { ok: false, errors: ['不是有效 JSON 对象'] }
  }
  for (const t of TABLES) {
    if (!Array.isArray(b[t])) errors.push(`表 ${t} 缺失或非数组`)
  }
  if (Array.isArray(b.exams)) {
    b.exams.forEach((e: any, i: number) => {
      if (!e.id || !e.name || !e.exam_date) errors.push(`exams[${i}] 缺少 id/name/exam_date`)
    })
  }
  return { ok: errors.length === 0, errors }
}

export type ImportMode = 'skip' | 'overwrite' | 'merge'

export async function importData(
  b: any,
  mode: ImportMode,
): Promise<{ ok: number; skipped: number; failed: number; errors: string[] }> {
  let ok = 0
  let skipped = 0
  let failed = 0
  const errors: string[] = []
  for (const table of TABLES) {
    const rows: any[] = b[table] ?? []
    for (let i = 0; i < rows.length; i++) {
      const row = rows[i]
      try {
        if (mode === 'skip') {
          if (row.id && (await getById(table, row.id))) {
            skipped++
            continue
          }
          await insert(table, row)
          ok++
        } else if (mode === 'overwrite') {
          if (row.id) await execute(`DELETE FROM ${table} WHERE id = ?`, [row.id])
          await insert(table, row)
          ok++
        } else {
          // merge：已有非空字段保留，仅填充空字段
          const existing = row.id ? await getById<any>(table, row.id) : null
          if (existing) {
            const merged = { ...existing }
            for (const [k, v] of Object.entries(row)) {
              if (
                (merged[k] === null || merged[k] === undefined || merged[k] === '') &&
                v !== null &&
                v !== undefined &&
                v !== ''
              ) {
                merged[k] = v
              }
            }
            await execute(`DELETE FROM ${table} WHERE id = ?`, [row.id])
            await insert(table, merged)
            ok++
          } else {
            await insert(table, row)
            ok++
          }
        }
      } catch (e) {
        failed++
        if (errors.length < 50) errors.push(`${table}[${i}]: ${(e as Error).message}`)
      }
      if ((i + 1) % IMPORT_CHUNK === 0) await yieldUI()
    }
    await yieldUI()
  }
  return { ok, skipped, failed, errors }
}

/** 导出为 JSON 文件（用户选保存路径） */
export async function exportToFile(range: {
  scope: ExportScope
  examId?: string
  from?: string
  to?: string
}): Promise<string | null> {
  const bundle = await exportData(range)
  const examName = range.examId ? (await getById<any>('exams', range.examId))?.name : ''
  const dateStr = new Date().toISOString().slice(0, 10)
  const name = `智研导出_${examName || '全部'}_${dateStr}.json`
  const path = await save({ defaultPath: name, filters: [{ name: 'JSON', extensions: ['json'] }] })
  if (!path) return null
  await writeTextFile(path, JSON.stringify(bundle, null, 2))
  return path
}

/** 从 JSON 文件导入 */
export async function importFromFile(
  mode: ImportMode,
): Promise<{ ok: number; skipped: number; failed: number; errors: string[] } | null> {
  const sel = await open({
    filters: [{ name: 'JSON', extensions: ['json'] }],
    multiple: false,
  })
  const path = typeof sel === 'string' ? sel : null
  if (!path) return null
  const { readTextFile } = await import('@tauri-apps/plugin-fs')
  const text = await readTextFile(path)
  const bundle = JSON.parse(text)
  const v = validateBundle(bundle)
  if (!v.ok) {
    throw new Error('导入文件校验失败：\n' + v.errors.join('\n'))
  }
  return importData(bundle, mode)
}

/** 数据库备份：VACUUM INTO 一致性快照（用户选保存路径） */
export async function backupDatabase(): Promise<string | null> {
  const dateStr = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19)
  const path = await save({
    defaultPath: `智研备份_${dateStr}.db`,
    filters: [{ name: 'SQLite DB', extensions: ['db'] }],
  })
  if (!path) return null
  // VACUUM INTO 的目标路径不支持参数绑定，路径来自 dialog.save（安全），转义单引号后字面量拼入
  const safe = path.replace(/'/g, "''")
  await execute(`VACUUM INTO '${safe}'`)
  return path
}

/** 数据库恢复：读备份 → 覆盖 db 文件 → relaunch */
export async function restoreDatabase(): Promise<void> {
  const sel = await open({
    filters: [{ name: 'SQLite DB', extensions: ['db'] }],
    multiple: false,
  })
  const path = typeof sel === 'string' ? sel : null
  if (!path) return
  const bytes = await readFile(path)
  const dir = await appDataDir()
  const dbPath = `${dir}/zhiyan.db`
  await closeDb()
  await writeFile(dbPath, bytes)
  await relaunch()
}
