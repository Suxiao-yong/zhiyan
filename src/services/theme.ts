// 智研 · 共享主题模块
// 统一分类色板、计划状态色、语义色、科目取色哈希、ECharts 主题名。
// 消除此前散落在各组件的重复 palette / statusColor 定义（含 #9093aa 拼写错误）。

import { computed } from 'vue'
import type { PlanStatus } from '@/types'
import { useSettingsStore } from '@/stores/settings'

/**
 * 分类色板（与紫罗兰主色和谐；用于多系列图表与科目 tinted 标签）。
 * 同一科目经 colorForSubject 稳定取色，跨图表同色。
 */
export const SUBJECT_PALETTE = [
  '#6d5dc8', // violet
  '#2f8f9a', // teal
  '#c2891f', // amber
  '#c4526a', // rose
  '#5666c9', // indigo
  '#4f9a5e', // sage
  '#71747e', // slate
  '#9a5dc8', // plum
] as const

/** 计划状态色（ECharts 填充 + 实心标签底色；与白字对比达标） */
export const STATUS_COLORS: Record<PlanStatus, string> = {
  pending: '#71747e',
  in_progress: '#5c4fb3',
  completed: '#297a4d',
  skipped: '#b83838',
}

/** 语义色（供需要 hex 的图表/内联使用，与 CSS 令牌一致） */
export const SEMANTIC = {
  primary: '#6d5dc8',
  primaryHover: '#5c4fb3',
  success: '#2f8a57',
  warning: '#b07d1f',
  danger: '#c44545',
  info: '#4a7bb5',
} as const

/** 掌握度/预测分色：低→危险，中→警告，高→成功 */
export function levelColor(ratio: number): string {
  if (ratio < 0.6) return SEMANTIC.danger
  if (ratio < 0.85) return SEMANTIC.warning
  return SEMANTIC.success
}

/** 按名称稳定哈希取分类色（同一科目跨图表同色） */
export function colorForSubject(
  name: string | null,
  palette: readonly string[] = SUBJECT_PALETTE,
): string {
  if (!name) return palette[6]
  let h = 0
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) >>> 0
  return palette[h % palette.length]
}

/** ECharts 主题名 */
export const CHART_THEME_LIGHT = 'zhiyan-light'
export const CHART_THEME_DARK = 'zhiyan-dark'

/** 图表主题 composable：返回当前主题名与 isDark，供 <v-chart :theme> 与少量内联色使用 */
export function useChartTheme() {
  const settings = useSettingsStore()
  const isDark = computed(() => settings.theme === 'dark')
  const theme = computed(() => (isDark.value ? CHART_THEME_DARK : CHART_THEME_LIGHT))
  return { isDark, theme }
}
