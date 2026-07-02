// 数据库通用操作层（基于 @tauri-apps/plugin-sql）。
// 铁律：所有值均用 ? 参数化；表名/列名经白名单校验后再拼入 SQL，防止注入。
// updated_at 由触发器自动刷新，应用层不手传。

import Database from '@tauri-apps/plugin-sql'
import { v4 as uuidv4 } from 'uuid'

/** 查询参数（位置 ? 占位） */
export type QueryParams = unknown[]

/** execute 返回 */
export interface QueryResult {
  lastInsertId: number
  rowsAffected: number
}

/** 合法表名白名单 */
const ALLOWED_TABLES = [
  'exams',
  'subjects',
  'knowledge_points',
  'study_records',
  'study_plans',
  'wrong_questions',
  'ai_analyses',
  'settings',
] as const

/** 各表合法列白名单（用于 insert/update 列名校验） */
const COLUMNS: Record<string, readonly string[]> = {
  exams: ['id', 'name', 'exam_type', 'exam_date', 'total_score', 'description', 'created_at', 'updated_at'],
  subjects: ['id', 'exam_id', 'name', 'target_score', 'current_level', 'weight', 'sort_order', 'created_at', 'updated_at'],
  knowledge_points: ['id', 'subject_id', 'name', 'parent_id', 'weight', 'difficulty_level', 'current_mastery', 'chapter', 'sort_order', 'created_at', 'updated_at'],
  study_records: ['id', 'date', 'subject_id', 'knowledge_point_id', 'duration_min', 'content', 'questions_count', 'correct_count', 'mastery_rating', 'difficulty_notes', 'mood', 'session_time', 'created_at', 'updated_at'],
  study_plans: ['id', 'exam_id', 'subject_id', 'knowledge_point_id', 'date', 'planned_tasks', 'planned_duration', 'actual_duration', 'actual_tasks', 'status', 'generated_by', 'ai_suggestion', 'user_modified', 'sort_order', 'created_at', 'updated_at'],
  wrong_questions: ['id', 'record_id', 'subject_id', 'knowledge_point_id', 'question_source', 'question_desc', 'correct_answer', 'my_answer', 'error_type', 'error_reason', 'review_count', 'mastered', 'created_at', 'last_review_at'],
  ai_analyses: ['id', 'analysis_type', 'period_start', 'period_end', 'subjects_analyzed', 'content', 'suggestions', 'scores_prediction', 'generated_by', 'user_confirmed', 'applied', 'applied_at', 'created_at'],
  settings: ['key', 'value', 'description', 'updated_at'],
}

function assertTable(table: string): void {
  if (!(ALLOWED_TABLES as readonly string[]).includes(table)) {
    throw new Error(`非法表名: ${table}`)
  }
}

let dbInstance: Database | null = null

/**
 * 获取（懒加载）SQLite 数据库连接。
 * 数据库文件落在应用 AppData 目录（由 tauri-plugin-sql 默认决定）。
 * 连接级外键强制：加载后立即 execute PRAGMA foreign_keys=ON（双保险之一）；
 * 迁移 SQL 首行也含该 PRAGMA。Step 9 会实测级联删除是否生效。
 */
export async function getDb(): Promise<Database> {
  if (dbInstance) return dbInstance
  dbInstance = await Database.load('sqlite:zhiyan.db')
  try {
    await dbInstance.execute('PRAGMA foreign_keys = ON')
  } catch (e) {
    console.warn('设置 PRAGMA foreign_keys 失败（级联删除可能不生效，将在 Step 9 实测）', e)
  }
  return dbInstance
}

/** 通用查询（参数化 ?，返回行数组） */
export async function query<T = any>(sql: string, params: QueryParams = []): Promise<T[]> {
  const db = await getDb()
  const rows = await db.select(sql, params)
  return rows as T[]
}

/** 通用写入（参数化 ?） */
export async function execute(sql: string, params: QueryParams = []): Promise<QueryResult> {
  const db = await getDb()
  const r = await db.execute(sql, params)
  return r as unknown as QueryResult
}

/** 查询表全部数据（表名经白名单校验） */
export async function getAll<T = any>(table: string): Promise<T[]> {
  assertTable(table)
  return query<T>(`SELECT * FROM ${table} ORDER BY rowid`)
}

/** 按 ID 查询（id 主键表；settings 用 getSetting） */
export async function getById<T = any>(table: string, id: string): Promise<T | null> {
  assertTable(table)
  const rows = await query<T>(`SELECT * FROM ${table} WHERE id = ?`, [id])
  return rows[0] ?? null
}

/** 通用插入（列名经白名单校验；自动生成 UUID 主键；不传 created_at/updated_at）。
 *  返回生成（或传入）的 id。 */
export async function insert(table: string, data: Record<string, unknown>): Promise<string> {
  assertTable(table)
  const cols = COLUMNS[table]
  const skip = new Set(['created_at', 'updated_at'])
  const entries = Object.entries(data).filter(([k]) => cols.includes(k) && !skip.has(k))

  let id: string | undefined
  if (cols.includes('id')) {
    const existing = entries.find(([k]) => k === 'id')
    if (existing) {
      id = existing[1] as string
    } else {
      id = uuidv4()
      entries.unshift(['id', id])
    }
  }

  const colList = entries.map(([k]) => k).join(', ')
  const placeholders = entries.map(() => '?').join(', ')
  const params = entries.map(([, v]) => v)
  await execute(`INSERT INTO ${table} (${colList}) VALUES (${placeholders})`, params)
  return id ?? ''
}

/** 通用更新（列名经白名单校验；updated_at 由触发器刷新） */
export async function update(
  table: string,
  id: string,
  data: Record<string, unknown>,
): Promise<void> {
  assertTable(table)
  const cols = COLUMNS[table]
  const skip = new Set(['id', 'created_at', 'updated_at'])
  const entries = Object.entries(data).filter(([k]) => cols.includes(k) && !skip.has(k))
  if (!entries.length) return
  const setClause = entries.map(([k]) => `${k} = ?`).join(', ')
  const params = [...entries.map(([, v]) => v), id]
  await execute(`UPDATE ${table} SET ${setClause} WHERE id = ?`, params)
}

/** 通用删除（外键级联由 DB 处理；调用方须先弹确认框并告知级联范围） */
export async function remove(table: string, id: string): Promise<void> {
  assertTable(table)
  await execute(`DELETE FROM ${table} WHERE id = ?`, [id])
}

/** 计数（where 文本须为调用方硬编码字面量，不含用户输入；值用 ? 参数化） */
export async function count(
  table: string,
  where?: string,
  params: QueryParams = [],
): Promise<number> {
  assertTable(table)
  const sql = where
    ? `SELECT COUNT(*) AS c FROM ${table} WHERE ${where}`
    : `SELECT COUNT(*) AS c FROM ${table}`
  const rows = await query<{ c: number }>(sql, params)
  return rows[0]?.c ?? 0
}

// ---- 系统设置（key-value，特殊处理：主键为 key，非 UUID）----

export async function getSetting(key: string): Promise<string | null> {
  const rows = await query<{ value: string | null }>(
    'SELECT value FROM settings WHERE key = ?',
    [key],
  )
  return rows[0]?.value ?? null
}

export async function setSetting(
  key: string,
  value: string,
  description?: string,
): Promise<void> {
  // UPSERT：存在则更新 value（description 仅在为空时保留原值）
  await execute(
    `INSERT INTO settings (key, value, description) VALUES (?, ?, ?)
     ON CONFLICT(key) DO UPDATE SET value = excluded.value,
       description = COALESCE(NULLIF(excluded.description, ''), settings.description)`,
    [key, value, description ?? null],
  )
}

/** 关闭数据库连接（恢复数据前调用，以解锁文件） */
export async function closeDb(): Promise<void> {
  if (dbInstance) {
    try {
      await dbInstance.close()
    } catch (e) {
      console.warn('关闭数据库失败', e)
    }
    dbInstance = null
  }
}

/** 删除指定设置项 */
export async function deleteSetting(key: string): Promise<void> {
  await execute('DELETE FROM settings WHERE key = ?', [key])
}
