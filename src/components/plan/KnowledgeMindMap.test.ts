// @vitest-environment jsdom
/// <reference lib="es2015" />

import { flushPromises, mount } from '@vue/test-utils'
import ElementPlus from 'element-plus'
import { defineComponent } from 'vue'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// jsdom 无 canvas，真渲染 echarts 必挂：mock vue-echarts，只捕获 option 并转发 click。
const chartStubs: Array<{ props: { option: any }; emitClick: (params: unknown) => void }> = []
vi.mock('vue-echarts', () => ({
  default: defineComponent({
    name: 'VueECharts',
    props: { option: { type: Object, required: true } },
    emits: ['click'],
    setup(props, { emit }) {
      const stub = {
        props,
        emitClick: (params: unknown) => emit('click', params),
      }
      chartStubs.push(stub)
      return stub
    },
    template: '<div class="vchart-stub" />',
  }),
}))

const client = vi.hoisted(() => ({ knowledgeTree: vi.fn() }))
vi.mock('@/services/agent-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/services/agent-client')>()),
  knowledgeTree: client.knowledgeTree,
}))

const db = vi.hoisted(() => ({ getById: vi.fn() }))
vi.mock('@/services/db', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/services/db')>()),
  getById: db.getById,
}))

import KnowledgeMindMap from './KnowledgeMindMap.vue'
import {
  masteryColor,
  parseSourceRef,
  splitMaterialParagraphs,
  buildTreeOption,
} from './knowledge-map'
import type { KnowledgePointNode, KnowledgeTreeSubject } from '@/services/agent-client'

const kp = (
  id: string,
  name: string,
  mastery: number,
  extra: Partial<KnowledgePointNode> = {},
): KnowledgePointNode => ({
  id,
  name,
  mastery,
  wrong_count: 0,
  material_id: null,
  source_ref: null,
  children: [],
  ...extra,
})

const subjectsFixture = (): KnowledgeTreeSubject[] => [
  {
    id: 'sub-1',
    name: '数学',
    children: [
      kp('kp-root', '微积分', 3, {
        material_id: 'mat-1',
        source_ref: '§2-§3',
        children: [kp('kp-a', '极限', 5), kp('kp-b', '导数', 1)],
      }),
    ],
  },
]

function mountMap() {
  return mount(KnowledgeMindMap, { global: { plugins: [ElementPlus] } })
}

describe('knowledge-map helpers', () => {
  it('colors nodes by mastery band', () => {
    expect(masteryColor(1)).toBe('#f56c6c')
    expect(masteryColor(2)).toBe('#f56c6c')
    expect(masteryColor(3)).toBe('#e6a23c')
    expect(masteryColor(4)).toBe('#67c23a')
    expect(masteryColor(5)).toBe('#67c23a')
  })

  it('parses §N and §N-§M refs and rejects other shapes', () => {
    expect(parseSourceRef('§3')).toEqual([3, 3])
    expect(parseSourceRef('§2-§4')).toEqual([2, 4])
    expect(parseSourceRef(null)).toBeNull()
    expect(parseSourceRef('第3节')).toBeNull()
    expect(parseSourceRef('§4-§2')).toBeNull()
  })

  it('splits material content on blank lines', () => {
    expect(splitMaterialParagraphs('第一段\n\n第二段\n\n\n\n第三段')).toEqual([
      '第一段',
      '第二段',
      '第三段',
    ])
  })

  it('builds an LR tree option with per-mastery colors and name+mastery labels', () => {
    const option = buildTreeOption(subjectsFixture())
    const series = option.series[0]
    expect(series.orient).toBe('LR')

    const math = series.data[0]
    expect(math.name).toBe('数学')
    const root = math.children[0]
    expect(root.name).toContain('微积分')
    expect(root.name).toContain('3/5')
    expect(root.itemStyle.color).toBe('#e6a23c')
    expect(root.children.map((c: any) => c.name)).toEqual(['极限 · 掌握 5/5', '导数 · 掌握 1/5'])
    expect(root.children[0].itemStyle.color).toBe('#67c23a')
    expect(root.children[1].itemStyle.color).toBe('#f56c6c')
  })
})

describe('KnowledgeMindMap', () => {
  beforeEach(() => {
    chartStubs.length = 0
    client.knowledgeTree.mockReset()
    db.getById.mockReset()
  })

  it('renders the tree and emits select with node data on click', async () => {
    client.knowledgeTree.mockResolvedValue({ exam_id: 'exam-1', subjects: subjectsFixture() })
    const wrapper = mountMap()
    await flushPromises()

    const stub = chartStubs[0]
    expect(stub).toBeTruthy()
    // 有 exam 时 header 图例可见（M-1：exam_id 必须来自命令返回值）。
    expect(wrapper.find('.legend').exists()).toBe(true)
    // option 组装走组件同一路径：series.data 即科目森林。
    expect(stub.props.option.series[0].orient).toBe('LR')
    expect(stub.props.option.series[0].data[0].children[0].raw.source_ref).toBe('§2-§3')

    const raw = stub.props.option.series[0].data[0].children[0].raw
    stub.emitClick({ data: stub.props.option.series[0].data[0] }) // 科目节点无 raw → 不 emit
    stub.emitClick({ data: stub.props.option.series[0].data[0].children[0] }) // 知识点节点
    expect(wrapper.emitted('select')).toHaveLength(1)
    expect(wrapper.emitted('select')![0][0]).toMatchObject({ id: raw.id, name: raw.name })
  })

  it('shows the empty state when no exam is configured', async () => {
    client.knowledgeTree.mockResolvedValue({ exam_id: null, subjects: [] })
    const wrapper = mountMap()
    await flushPromises()

    expect(chartStubs).toHaveLength(0)
    expect(wrapper.text()).toContain('请先完成考试配置')
    // 无考试时图例隐藏（exam_id 为 null，M-1 回归点）。
    expect(wrapper.find('.legend').exists()).toBe(false)
  })

  it('loads referenced material paragraphs and highlights §N-§M ranges', async () => {
    client.knowledgeTree.mockResolvedValue({ exam_id: 'exam-1', subjects: subjectsFixture() })
    db.getById.mockResolvedValue({
      title: '高数讲义',
      content: '第一节 极限\n\n第二节 导数定义\n\n第三节 求导法则',
    })
    const wrapper = mountMap()
    await flushPromises()

    // 通过图表点击路径打开溯源 dialog（openEvidence 不经 defineExpose，不直接调）。
    const rootStub = chartStubs[0].props.option.series[0].data[0].children[0]
    chartStubs[0].emitClick({ data: rootStub })
    await flushPromises()

    const dialog = wrapper.find('.el-dialog')
    expect(dialog.exists()).toBe(true)
    const items = dialog.findAll('.paragraphs li')
    expect(items).toHaveLength(3)
    expect(items[0].classes()).not.toContain('highlighted')
    expect(items[1].classes()).toContain('highlighted')
    expect(items[2].classes()).toContain('highlighted')
    expect(dialog.text()).toContain('掌握度')
    expect(dialog.text()).toContain('§2-§3')
  })
})
