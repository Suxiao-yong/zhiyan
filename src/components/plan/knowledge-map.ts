// Pure assembly logic for the knowledge mind map (v0.3.0 Task 8). Kept out of
// the SFC so the option building / mastery coloring / source_ref parsing can be
// unit-tested without a canvas.
import type { KnowledgePointNode, KnowledgeTreeSubject } from '@/services/agent-client'

export const MASTERY_COLORS = {
  weak: '#f56c6c', // 掌握度 1-2 红
  mid: '#e6a23c', // 3 黄
  good: '#67c23a', // 4-5 绿
} as const

/** 掌握度 → 节点颜色：1-2 红、3 黄、4-5 绿（0/未知按弱掌握处理）。 */
export function masteryColor(mastery: number): string {
  if (mastery >= 4) return MASTERY_COLORS.good
  if (mastery === 3) return MASTERY_COLORS.mid
  return MASTERY_COLORS.weak
}

/**
 * Parse `§N` or `§N-§M` into an inclusive [start, end] paragraph range.
 * Returns null when the ref doesn't match the canonical shape.
 */
export function parseSourceRef(sourceRef: string | null | undefined): [number, number] | null {
  const match = /^§(\d+)(?:\s*-\s*§?(\d+))?$/.exec((sourceRef ?? '').trim())
  if (!match) return null
  const start = Number(match[1])
  const end = match[2] ? Number(match[2]) : start
  if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) return null
  return [start, end]
}

/** Split material content into numbered paragraphs on blank lines. */
export function splitMaterialParagraphs(content: string): string[] {
  return content
    .split(/\n{2,}/)
    .map((p) => p.trim())
    .filter(Boolean)
}

interface EChartsTreeNode {
  name: string
  value: { mastery: number; wrongCount: number }
  itemStyle: { color: string }
  raw: KnowledgePointNode
  children: EChartsTreeNode[]
}

function toChartNode(kp: KnowledgePointNode): EChartsTreeNode {
  return {
    name: `${kp.name} · 掌握 ${kp.mastery}/5`,
    value: { mastery: kp.mastery, wrongCount: kp.wrong_count },
    itemStyle: { color: masteryColor(kp.mastery) },
    raw: kp,
    children: kp.children.map(toChartNode),
  }
}

/** Build the echarts tree series option (LR forest, one root per subject). */
export function buildTreeOption(subjects: KnowledgeTreeSubject[]) {
  return {
    tooltip: { trigger: 'item' },
    series: [
      {
        type: 'tree',
        orient: 'LR',
        roam: true,
        symbol: 'circle',
        symbolSize: 14,
        top: '2%',
        left: '8%',
        bottom: '2%',
        right: '20%',
        label: { position: 'left', verticalAlign: 'middle', align: 'right', fontSize: 12 },
        leaves: { label: { position: 'right', verticalAlign: 'middle', align: 'left' } },
        expandAndCollapse: true,
        animationDuration: 300,
        data: subjects.map((subject) => ({
          name: subject.name,
          value: { mastery: 0, wrongCount: 0 },
          itemStyle: { color: '#909399' },
          raw: null,
          children: subject.children.map(toChartNode),
        })),
      },
    ],
  // SAFETY: echarts tree option shape is validated at runtime by echarts; importing full EChartsOption
  // types for this one call site would add heavy type deps for no safety gain.
  } as any
}
