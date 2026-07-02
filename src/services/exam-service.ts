// 考试配置业务规则层。
// 与 db.ts 的通用 insert 分离：这里处理日期校验、级联删除计数、知识点树构建、排序等业务逻辑。
// （Phase 2 的 record-service.ts 将沿用同一模式处理跨天 04:00 归一化等。）

import {
  count,
  execute,
  getById,
  insert,
  query,
  remove,
  update,
} from './db'
import type {
  Exam,
  KnowledgePoint,
  Subject,
} from '@/types'

// ---------------- 考试 ----------------

export interface ExamInput {
  name: string
  exam_type: string | null
  exam_date: string // YYYY-MM-DD
  total_score: number | null
  description: string | null
}

/** 考试日期校验：必须晚于今天（不允许过去或当天，否则计划生成剩余天数为负） */
export function validateExamDate(date: string): void {
  if (!date) throw new Error('考试日期不能为空')
  const t = new Date()
  const todayStr = `${t.getFullYear()}-${String(t.getMonth() + 1).padStart(2, '0')}-${String(
    t.getDate(),
  ).padStart(2, '0')}`
  if (date <= todayStr) throw new Error('考试日期必须晚于今天')
}

export async function getAllExams(): Promise<Exam[]> {
  return query<Exam>('SELECT * FROM exams ORDER BY exam_date ASC')
}

export async function getExam(id: string): Promise<Exam | null> {
  return getById<Exam>('exams', id)
}

export async function createExam(input: ExamInput): Promise<Exam> {
  validateExamDate(input.exam_date)
  const id = await insert('exams', {
    name: input.name,
    exam_type: input.exam_type,
    exam_date: input.exam_date,
    total_score: input.total_score,
    description: input.description,
  })
  return (await getById<Exam>('exams', id))!
}

export async function updateExam(id: string, input: ExamInput): Promise<void> {
  validateExamDate(input.exam_date)
  await update('exams', id, {
    name: input.name,
    exam_type: input.exam_type,
    exam_date: input.exam_date,
    total_score: input.total_score,
    description: input.description,
  })
}

/** 统计删除考试将级联删除的数据量（供确认框展示） */
export async function getExamCascadeCounts(examId: string): Promise<{
  subjects: number
  knowledge_points: number
  study_records: number
  study_plans: number
  wrong_questions: number
}> {
  const subjects = await query<{ id: string }>(
    'SELECT id FROM subjects WHERE exam_id = ?',
    [examId],
  )
  const subjectIds = subjects.map((s) => s.id)
  if (!subjectIds.length) {
    return {
      subjects: 0,
      knowledge_points: 0,
      study_records: 0,
      study_plans: 0,
      wrong_questions: 0,
    }
  }
  const ph = subjectIds.map(() => '?').join(',')
  return {
    subjects: subjectIds.length,
    knowledge_points: await count(
      'knowledge_points',
      `subject_id IN (${ph})`,
      subjectIds,
    ),
    study_records: await count(
      'study_records',
      `subject_id IN (${ph})`,
      subjectIds,
    ),
    study_plans: await count('study_plans', 'exam_id = ?', [examId]),
    wrong_questions: await count(
      'wrong_questions',
      `subject_id IN (${ph})`,
      subjectIds,
    ),
  }
}

/**
 * 删除考试（应用层级联兜底）。
 * 实测 tauri-plugin-sql 2.4.0 源码：用 sqlx 多连接池（Pool<Sqlite>）且插件自身不启用
 * PRAGMA foreign_keys；db.ts 的 post-load PRAGMA 只对池中一条连接生效，DB 的 ON DELETE CASCADE
 * 不可靠。故在此显式按依赖顺序删子数据，保证无论 FK 状态如何都不留孤儿。
 * 顺序：wrong_questions → study_plans → study_records → knowledge_points → subjects → exams
 */
export async function deleteExam(id: string): Promise<void> {
  const subjects = await query<{ id: string }>(
    'SELECT id FROM subjects WHERE exam_id = ?',
    [id],
  )
  for (const s of subjects) {
    await deleteSubjectCascade(s.id)
  }
  // study_plans 同时按 exam_id 引用，兜底再清一次
  await execute('DELETE FROM study_plans WHERE exam_id = ?', [id])
  await remove('exams', id)
}

/** 按科目级联清理其下的错题/计划/记录/知识点，再删科目本身 */
async function deleteSubjectCascade(subjectId: string): Promise<void> {
  await execute('DELETE FROM wrong_questions WHERE subject_id = ?', [subjectId])
  await execute('DELETE FROM study_plans WHERE subject_id = ?', [subjectId])
  await execute('DELETE FROM study_records WHERE subject_id = ?', [subjectId])
  await execute('DELETE FROM knowledge_points WHERE subject_id = ?', [subjectId])
  await remove('subjects', subjectId)
}

// ---------------- 科目 ----------------

export interface SubjectInput {
  exam_id: string
  name: string
  target_score: number | null
  current_level: number // 1-5
  weight: number
  sort_order?: number
}

export async function getSubjectsByExam(examId: string): Promise<Subject[]> {
  return query<Subject>(
    'SELECT * FROM subjects WHERE exam_id = ? ORDER BY sort_order, created_at',
    [examId],
  )
}

export async function createSubject(input: SubjectInput): Promise<Subject> {
  const id = await insert('subjects', {
    exam_id: input.exam_id,
    name: input.name,
    target_score: input.target_score,
    current_level: input.current_level,
    weight: input.weight,
    sort_order: input.sort_order ?? 0,
  })
  return (await getById<Subject>('subjects', id))!
}

export async function updateSubject(
  id: string,
  input: Partial<SubjectInput>,
): Promise<void> {
  await update('subjects', id, input as Record<string, unknown>)
}

/** 统计删除科目将级联删除的数据量 */
export async function getSubjectCascadeCounts(
  subjectId: string,
): Promise<{
  knowledge_points: number
  study_records: number
  study_plans: number
  wrong_questions: number
}> {
  return {
    knowledge_points: await count('knowledge_points', 'subject_id = ?', [subjectId]),
    study_records: await count('study_records', 'subject_id = ?', [subjectId]),
    study_plans: await count('study_plans', 'subject_id = ?', [subjectId]),
    wrong_questions: await count('wrong_questions', 'subject_id = ?', [subjectId]),
  }
}

export async function deleteSubject(id: string): Promise<void> {
  await deleteSubjectCascade(id)
}

// ---------------- 知识点 ----------------

export interface KnowledgePointInput {
  subject_id: string
  name: string
  parent_id: string | null
  weight: number
  difficulty_level: number // 1-5
  current_mastery: number // 1-5
  chapter: string | null
  sort_order?: number
}

export async function getKnowledgePointsBySubject(
  subjectId: string,
): Promise<KnowledgePoint[]> {
  return query<KnowledgePoint>(
    'SELECT * FROM knowledge_points WHERE subject_id = ? ORDER BY sort_order, created_at',
    [subjectId],
  )
}

/** 构建知识点树（按 parent_id 组装 children） */
export async function getKnowledgeTree(
  subjectId: string,
): Promise<KnowledgePoint[]> {
  const all = await getKnowledgePointsBySubject(subjectId)
  const map = new Map<string, KnowledgePoint>()
  all.forEach((kp) => map.set(kp.id, { ...kp, children: [] }))
  const roots: KnowledgePoint[] = []
  all.forEach((kp) => {
    const node = map.get(kp.id)!
    if (kp.parent_id && map.has(kp.parent_id)) {
      map.get(kp.parent_id)!.children!.push(node)
    } else {
      roots.push(node)
    }
  })
  return roots
}

export async function createKnowledgePoint(
  input: KnowledgePointInput,
): Promise<KnowledgePoint> {
  const id = await insert('knowledge_points', {
    subject_id: input.subject_id,
    name: input.name,
    parent_id: input.parent_id,
    weight: input.weight,
    difficulty_level: input.difficulty_level,
    current_mastery: input.current_mastery,
    chapter: input.chapter,
    sort_order: input.sort_order ?? 0,
  })
  return (await getById<KnowledgePoint>('knowledge_points', id))!
}

export async function updateKnowledgePoint(
  id: string,
  input: Partial<KnowledgePointInput>,
): Promise<void> {
  await update('knowledge_points', id, input as Record<string, unknown>)
}

export async function deleteKnowledgePoint(id: string): Promise<void> {
  // 子知识点 parent_id 置空（FK off 时 DB 的 ON DELETE SET NULL 不可靠，显式上浮为顶层）
  await execute('UPDATE knowledge_points SET parent_id = NULL WHERE parent_id = ?', [id])
  await remove('knowledge_points', id)
}

/** 按 ids 顺序重排知识点 sort_order（拖拽排序用） */
export async function reorderKnowledgePoints(ids: string[]): Promise<void> {
  for (let i = 0; i < ids.length; i++) {
    await update('knowledge_points', ids[i], { sort_order: i })
  }
}
