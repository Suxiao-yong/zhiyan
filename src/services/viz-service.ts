// 数据可视化查询层（按时间范围 + 科目过滤）。全参数化，JOIN 取科目/知识点名。

import { query } from './db'

export interface VizFilter {
  from: string // YYYY-MM-DD
  to: string
  subjectIds: string[] // 空=全部科目
}

function buildCond(f: VizFilter): { cond: string; params: unknown[] } {
  const where: string[] = ['r.date >= ?', 'r.date <= ?']
  const params: unknown[] = [f.from, f.to]
  if (f.subjectIds.length) {
    where.push(`r.subject_id IN (${f.subjectIds.map(() => '?').join(',')})`)
    params.push(...f.subjectIds)
  }
  return { cond: 'WHERE ' + where.join(' AND '), params }
}

/** 每日学习时长趋势 */
export async function getDurationTrend(
  f: VizFilter,
): Promise<{ date: string; minutes: number }[]> {
  const { cond, params } = buildCond(f)
  return query<{ date: string; minutes: number }>(
    `SELECT r.date, SUM(r.duration_min) AS minutes FROM study_records r ${cond} GROUP BY r.date ORDER BY r.date`,
    params,
  )
}

/** 各科时长占比 */
export async function getSubjectDuration(
  f: VizFilter,
): Promise<{ subjectId: string; subjectName: string; minutes: number }[]> {
  const { cond, params } = buildCond(f)
  return query<{ subjectId: string; subjectName: string; minutes: number }>(
    `SELECT r.subject_id AS subjectId, s.name AS subjectName, SUM(r.duration_min) AS minutes
     FROM study_records r LEFT JOIN subjects s ON s.id = r.subject_id
     ${cond} GROUP BY r.subject_id, s.name ORDER BY minutes DESC`,
    params,
  )
}

/** 正确率变化曲线（每日每科） */
export async function getCorrectRateTrend(
  f: VizFilter,
): Promise<{ date: string; subjectName: string; rate: number }[]> {
  const { cond, params } = buildCond(f)
  return query<{ date: string; subjectName: string; rate: number }>(
    `SELECT r.date, s.name AS subjectName,
       CASE WHEN SUM(r.questions_count) > 0
         THEN ROUND(SUM(r.correct_count) * 100.0 / SUM(r.questions_count), 1)
         ELSE 0 END AS rate
     FROM study_records r LEFT JOIN subjects s ON s.id = r.subject_id
     ${cond} AND r.questions_count > 0
     GROUP BY r.date, r.subject_id, s.name ORDER BY r.date`,
    params,
  )
}

/** 各科进度雷达（时长/正确率/掌握度） */
export async function getSubjectRadar(
  f: VizFilter,
): Promise<{ subject: string; duration: number; correctRate: number; mastery: number }[]> {
  const { cond, params } = buildCond(f)
  const recStats = await query<{
    subject_id: string
    name: string
    duration: number
    correctRate: number
  }>(
    `SELECT r.subject_id, s.name, SUM(r.duration_min) AS duration,
       CASE WHEN SUM(r.questions_count) > 0
         THEN ROUND(SUM(r.correct_count) * 100.0 / SUM(r.questions_count), 1)
         ELSE 0 END AS correctRate
     FROM study_records r LEFT JOIN subjects s ON s.id = r.subject_id
     ${cond} GROUP BY r.subject_id, s.name`,
    params,
  )
  // 各科目 KP 平均掌握度
  const subjectIds = recStats.map((r) => r.subject_id)
  const masteryMap = new Map<string, number>()
  if (subjectIds.length) {
    const ms = await query<{ subject_id: string; m: number }>(
      `SELECT subject_id, ROUND(AVG(current_mastery), 1) AS m FROM knowledge_points
       WHERE subject_id IN (${subjectIds.map(() => '?').join(',')}) GROUP BY subject_id`,
      subjectIds,
    )
    ms.forEach((m) => masteryMap.set(m.subject_id, m.m))
  }
  return recStats.map((r) => ({
    subject: r.name,
    duration: r.duration,
    correctRate: r.correctRate,
    mastery: (masteryMap.get(r.subject_id) ?? 3) * 20, // 1-5 → 20-100
  }))
}

/** 知识点掌握热力图（知识点 × 日期） */
export async function getKpHeatmap(
  f: VizFilter,
): Promise<{ kpName: string; date: string; mastery: number }[]> {
  const { cond, params } = buildCond(f)
  return query<{ kpName: string; date: string; mastery: number }>(
    `SELECT k.name AS kpName, r.date, AVG(r.mastery_rating) AS mastery
     FROM study_records r
     LEFT JOIN knowledge_points k ON k.id = r.knowledge_point_id
     ${cond} AND r.knowledge_point_id IS NOT NULL AND r.mastery_rating IS NOT NULL
     GROUP BY r.knowledge_point_id, k.name, r.date
     ORDER BY k.name, r.date`,
    params,
  )
}
