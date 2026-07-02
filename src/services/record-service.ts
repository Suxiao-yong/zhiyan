// 学习记录业务层。
// 核心业务规则：跨天 04:00 归一化（实时记录在 00:00-03:59 提交→归属前一天；补记=用户手动选日期→不归一化）。
// 严格在此专属函数处理，不污染 db.ts 通用 insert。本地时间，与 DB datetime('now','localtime') 一致。

import { count, execute, getAll, getById, insert, query, remove, setSetting, update } from './db'
import type { StudyRecord, Subject, WrongQuestion } from '@/types'

// ---------------- 工具 ----------------

function fmtDate(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(
    d.getDate(),
  ).padStart(2, '0')}`
}

/** 当前"业务今日"：本地时间 <04:00 时为前一天（与归一化一致） */
export function businessToday(): string {
  const now = new Date()
  if (now.getHours() < 4) {
    const prev = new Date(now)
    prev.setDate(prev.getDate() - 1)
    return fmtDate(prev)
  }
  return fmtDate(now)
}

/**
 * 跨天 04:00 归一化。
 * @param manualDate 用户在日期选择器手动指定 → 补记，尊重选择，不归一化。
 *                   未传（undefined）→ 实时记录，00:00-03:59 归属前一天。
 */
export function normalizeDate(manualDate?: string): string {
  if (manualDate) return manualDate
  return businessToday()
}

// ---------------- 学习记录 ----------------

export interface RecordInput {
  date?: string // 手动指定=补记(不归一化)；undefined=实时(归一化)
  subject_id: string
  knowledge_point_id?: string | null
  duration_min: number
  content?: string | null
  questions_count?: number
  correct_count?: number
  mastery_rating?: number | null // 1-5
  difficulty_notes?: string | null
  mood?: number | null // 1-5
  session_time?: string | null // morning/afternoon/evening
}

export type RecordWithNames = StudyRecord & {
  subject_name: string | null
  knowledge_point_name: string | null
}

export interface RecordFilter {
  dateFrom?: string
  dateTo?: string
  subjectId?: string
  knowledgePointId?: string
  page: number
  pageSize: number
}

export async function createRecord(input: RecordInput): Promise<StudyRecord> {
  const date = normalizeDate(input.date)
  const data = {
    date,
    subject_id: input.subject_id,
    knowledge_point_id: input.knowledge_point_id ?? null,
    duration_min: input.duration_min,
    content: input.content ?? null,
    questions_count: input.questions_count ?? 0,
    correct_count: input.correct_count ?? 0,
    mastery_rating: input.mastery_rating ?? null,
    difficulty_notes: input.difficulty_notes ?? null,
    mood: input.mood ?? null,
    session_time: input.session_time ?? null,
  }
  // 诊断：记录入参与报错到 settings，便于排查"录入失败"
  try {
    await setSetting('last_record_attempt', JSON.stringify(data))
  } catch {
    /* ignore */
  }
  try {
    const id = await insert('study_records', data)
    return (await getById<StudyRecord>('study_records', id))!
  } catch (e) {
    try {
      await setSetting('last_record_error', `${(e as Error).message}`)
    } catch {
      /* ignore */
    }
    throw e
  }
}

export async function updateRecord(id: string, input: Partial<RecordInput>): Promise<void> {
  const data: Record<string, unknown> = { ...input }
  if (input.date !== undefined) data.date = normalizeDate(input.date)
  await update('study_records', id, data)
}

export async function deleteRecord(id: string): Promise<void> {
  // 关联错题 record_id 置空（FK off 时 DB 的 SET NULL 不可靠，应用层兜底；错题保留为独立条目）
  await execute('UPDATE wrong_questions SET record_id = NULL WHERE record_id = ?', [id])
  await remove('study_records', id)
}

function buildRecordCond(filter: RecordFilter): { cond: string; params: unknown[] } {
  const where: string[] = []
  const params: unknown[] = []
  if (filter.dateFrom) {
    where.push('r.date >= ?')
    params.push(filter.dateFrom)
  }
  if (filter.dateTo) {
    where.push('r.date <= ?')
    params.push(filter.dateTo)
  }
  if (filter.subjectId) {
    where.push('r.subject_id = ?')
    params.push(filter.subjectId)
  }
  if (filter.knowledgePointId) {
    where.push('r.knowledge_point_id = ?')
    params.push(filter.knowledgePointId)
  }
  return { cond: where.join(' AND '), params }
}

export async function getRecords(
  filter: RecordFilter,
): Promise<{ rows: RecordWithNames[]; total: number }> {
  const { cond, params } = buildRecordCond(filter)
  const total = cond
    ? await count('study_records', cond.replace(/r\./g, ''), params)
    : await count('study_records')
  const offset = (filter.page - 1) * filter.pageSize
  const rows = await query<RecordWithNames>(
    `SELECT r.*, s.name AS subject_name, k.name AS knowledge_point_name
     FROM study_records r
     LEFT JOIN subjects s ON s.id = r.subject_id
     LEFT JOIN knowledge_points k ON k.id = r.knowledge_point_id
     ${cond ? 'WHERE ' + cond : ''}
     ORDER BY r.date DESC, r.created_at DESC
     LIMIT ? OFFSET ?`,
    [...params, filter.pageSize, offset],
  )
  return { rows, total }
}

export async function getRecordsByDate(date: string): Promise<RecordWithNames[]> {
  return query<RecordWithNames>(
    `SELECT r.*, s.name AS subject_name, k.name AS knowledge_point_name
     FROM study_records r
     LEFT JOIN subjects s ON s.id = r.subject_id
     LEFT JOIN knowledge_points k ON k.id = r.knowledge_point_id
     WHERE r.date = ?
     ORDER BY r.session_time, r.created_at`,
    [date],
  )
}

/** 某月打卡数据（yearMonth: YYYY-MM） */
export async function getCalendarMonth(
  yearMonth: string,
): Promise<{ date: string; minutes: number; count: number }[]> {
  return query<{ date: string; minutes: number; count: number }>(
    `SELECT date, SUM(duration_min) AS minutes, COUNT(*) AS count
     FROM study_records WHERE date LIKE ? GROUP BY date`,
    [`${yearMonth}-%`],
  )
}

// ---------------- 仪表盘 stats ----------------

export async function getTodayMinutes(): Promise<number> {
  const rows = await query<{ s: number }>(
    'SELECT COALESCE(SUM(duration_min),0) AS s FROM study_records WHERE date = ?',
    [businessToday()],
  )
  return rows[0]?.s ?? 0
}

/** 连续打卡天数（按归一化 date 逐日比对；今日暂无记录不算断，从昨日起算） */
export async function getStreak(): Promise<number> {
  let streak = 0
  const cursor = new Date()
  if (cursor.getHours() < 4) cursor.setDate(cursor.getDate() - 1)
  cursor.setHours(0, 0, 0, 0)
  for (let i = 0; i < 366; i++) {
    const ds = fmtDate(cursor)
    const c = await count('study_records', 'date = ?', [ds])
    if (c > 0) {
      streak++
    } else if (i === 0) {
      // 今日暂无记录，不算断，继续看昨天
    } else {
      break
    }
    cursor.setDate(cursor.getDate() - 1)
  }
  return streak
}

/** 本周（周一至周日）每日时长；可选按科目过滤（饼图点击筛选用） */
export async function getWeeklyTrend(
  subjectId?: string,
): Promise<{ date: string; minutes: number; label: string }[]> {
  const now = new Date()
  now.setHours(0, 0, 0, 0)
  const day = now.getDay() // 0=周日..6=周六
  const cursor = new Date(now)
  cursor.setDate(cursor.getDate() - (day === 0 ? 6 : day - 1)) // 回到周一
  const labels = ['周一', '周二', '周三', '周四', '周五', '周六', '周日']
  const out: { date: string; minutes: number; label: string }[] = []
  for (let i = 0; i < 7; i++) {
    const ds = fmtDate(cursor)
    const sql = subjectId
      ? 'SELECT COALESCE(SUM(duration_min),0) AS s FROM study_records WHERE date = ? AND subject_id = ?'
      : 'SELECT COALESCE(SUM(duration_min),0) AS s FROM study_records WHERE date = ?'
    const rows = await query<{ s: number }>(sql, subjectId ? [ds, subjectId] : [ds])
    out.push({ date: ds, minutes: rows[0]?.s ?? 0, label: labels[i] })
    cursor.setDate(cursor.getDate() + 1)
  }
  return out
}

/** 本周各科时长占比 */
export async function getSubjectRatioThisWeek(): Promise<
  { subjectId: string; subjectName: string; minutes: number }[]
> {
  const now = new Date()
  now.setHours(0, 0, 0, 0)
  const day = now.getDay()
  const mon = new Date(now)
  mon.setDate(mon.getDate() - (day === 0 ? 6 : day - 1))
  const sun = new Date(mon)
  sun.setDate(sun.getDate() + 6)
  const rows = await query<{ subject_id: string; minutes: number }>(
    `SELECT subject_id, SUM(duration_min) AS minutes FROM study_records
     WHERE date >= ? AND date <= ? GROUP BY subject_id`,
    [fmtDate(mon), fmtDate(sun)],
  )
  const subjects = await getAll<Subject>('subjects')
  const nameMap = new Map(subjects.map((s) => [s.id, s.name]))
  return rows.map((r) => ({
    subjectId: r.subject_id,
    subjectName: nameMap.get(r.subject_id) ?? '未知科目',
    minutes: r.minutes,
  }))
}

/** 今日计划完成率分母（今日 plan 数）—— Phase 2 计划为空，Phase 3 填充 */
export async function getTodayPlanCount(): Promise<number> {
  const rows = await query<{ c: number }>('SELECT COUNT(*) AS c FROM study_plans WHERE date = ?', [
    businessToday(),
  ])
  return rows[0]?.c ?? 0
}

// ---------------- 错题 ----------------

export interface WrongQuestionInput {
  record_id?: string | null
  subject_id: string
  knowledge_point_id?: string | null
  question_source?: string | null
  question_desc?: string | null
  correct_answer?: string | null
  my_answer?: string | null
  error_type?: string | null
  error_reason?: string | null
}

export type WrongWithNames = WrongQuestion & {
  subject_name: string | null
  knowledge_point_name: string | null
}

export interface WrongFilter {
  subjectId?: string
  knowledgePointId?: string
  errorType?: string
  mastered?: boolean
  page: number
  pageSize: number
}

export async function createWrongQuestion(input: WrongQuestionInput): Promise<WrongQuestion> {
  const id = await insert('wrong_questions', { ...input })
  return (await getById<WrongQuestion>('wrong_questions', id))!
}

export async function updateWrongQuestion(
  id: string,
  input: Partial<WrongQuestionInput>,
): Promise<void> {
  await update('wrong_questions', id, input)
}

function buildWrongCond(filter: WrongFilter): { cond: string; params: unknown[] } {
  const where: string[] = []
  const params: unknown[] = []
  if (filter.subjectId) {
    where.push('w.subject_id = ?')
    params.push(filter.subjectId)
  }
  if (filter.knowledgePointId) {
    where.push('w.knowledge_point_id = ?')
    params.push(filter.knowledgePointId)
  }
  if (filter.errorType) {
    where.push('w.error_type = ?')
    params.push(filter.errorType)
  }
  if (filter.mastered !== undefined) {
    where.push('w.mastered = ?')
    params.push(filter.mastered ? 1 : 0)
  }
  return { cond: where.join(' AND '), params }
}

export async function getWrongQuestions(
  filter: WrongFilter,
): Promise<{ rows: WrongWithNames[]; total: number }> {
  const { cond, params } = buildWrongCond(filter)
  const total = cond
    ? await count('wrong_questions', cond.replace(/w\./g, ''), params)
    : await count('wrong_questions')
  const offset = (filter.page - 1) * filter.pageSize
  const rows = await query<WrongWithNames>(
    `SELECT w.*, s.name AS subject_name, k.name AS knowledge_point_name
     FROM wrong_questions w
     LEFT JOIN subjects s ON s.id = w.subject_id
     LEFT JOIN knowledge_points k ON k.id = w.knowledge_point_id
     ${cond ? 'WHERE ' + cond : ''}
     ORDER BY w.created_at DESC
     LIMIT ? OFFSET ?`,
    [...params, filter.pageSize, offset],
  )
  return { rows, total }
}

export async function setWrongMastered(id: string, mastered: boolean): Promise<void> {
  await update('wrong_questions', id, { mastered: mastered ? 1 : 0 })
}

export async function incrementWrongReview(id: string): Promise<void> {
  await execute(
    'UPDATE wrong_questions SET review_count = review_count + 1, last_review_at = ? WHERE id = ?',
    [new Date().toISOString(), id],
  )
}
