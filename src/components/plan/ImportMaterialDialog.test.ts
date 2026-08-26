// @vitest-environment jsdom
/// <reference lib="es2015" />

import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import ElementPlus from 'element-plus'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// el-dialog 会 teleport 到 body，stub 掉让表单内容留在 wrapper 内可查；
// el-select 的 jsdom 下拉无法真实展开，用最小 stub 模拟“点选项→更新值”（同 PlanCheckinDialog 重 stub 惯例）。
const SlotStub = { template: '<div><slot /><slot name="footer" /></div>' }
const SelectStub = {
  emits: ['update:modelValue'],
  template: `
    <div><button class="stub-pick" @click="$emit('update:modelValue', 'sub-1')">数学</button></div>`,
}

const db = vi.hoisted(() => ({ insert: vi.fn() }))
vi.mock('@/services/db', () => db)

const examService = vi.hoisted(() => ({ getSubjectsByExam: vi.fn() }))
vi.mock('@/services/exam-service', () => examService)

const agentStore = vi.hoisted(() => ({ sendMessage: vi.fn() }))
vi.mock('@/stores/agent', () => ({ useAgentStore: () => agentStore }))

vi.mock('@/stores/exam', () => ({
  useExamStore: () => ({ activeExamId: 'exam-1' }),
}))

import ImportMaterialDialog from './ImportMaterialDialog.vue'

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i

function mountDialog(): VueWrapper {
  return mount(ImportMaterialDialog, {
    props: { modelValue: true },
    attachTo: document.body,
    global: {
      plugins: [ElementPlus],
      stubs: { 'el-dialog': SlotStub, 'el-select': SelectStub },
    },
  })
}

async function pickSubject(wrapper: VueWrapper): Promise<void> {
  await wrapper.get('[data-test=subject-select] .stub-pick').trigger('click')
  await flushPromises()
}

describe('ImportMaterialDialog', () => {
  beforeEach(() => {
    db.insert.mockReset().mockResolvedValue('mat-1')
    agentStore.sendMessage.mockReset().mockResolvedValue(null)
    examService.getSubjectsByExam
      .mockReset()
      .mockResolvedValue([
        { id: 'sub-1', name: '数学' },
        { id: 'sub-2', name: '英语' },
      ])
  })

  afterEach(() => {
    document.body.innerHTML = ''
  })

  it('inserts the material via the db whitelist and asks the Agent to build flashcards', async () => {
    const wrapper = mountDialog()
    await flushPromises()

    await wrapper.get('[data-test=title-input]').setValue('考研数学重点')
    await pickSubject(wrapper)
    await wrapper.get('[data-test=content-input]').setValue('第一段\n\n第二段')

    await wrapper.get('[data-test=confirm]').trigger('click')
    await flushPromises()

    expect(examService.getSubjectsByExam).toHaveBeenCalledWith('exam-1')
    expect(db.insert).toHaveBeenCalledTimes(1)
    expect(db.insert).toHaveBeenCalledWith(
      'materials',
      expect.objectContaining({
        id: expect.stringMatching(UUID_RE),
        title: '考研数学重点',
        content: '第一段\n\n第二段',
        subject_id: 'sub-1',
      }),
    )

    expect(agentStore.sendMessage).toHaveBeenCalledTimes(1)
    const message = agentStore.sendMessage.mock.calls[0][0] as string
    expect(message).toContain('考研数学重点')
    expect(message).toContain('共 2 段')
    // 短消息只带标题与段数，不带正文全文
    expect(message).not.toContain('第一段')

    expect(wrapper.emitted('imported')).toHaveLength(1)
    const emitted = wrapper.emitted('update:modelValue') ?? []
    expect(emitted[emitted.length - 1]).toEqual([false])
    wrapper.unmount()
  })

  it('shows a live character counter and blocks over-limit content', async () => {
    const wrapper = mountDialog()
    await flushPromises()

    await wrapper.get('[data-test=title-input]').setValue('标题')
    await pickSubject(wrapper)
    await wrapper.get('[data-test=content-input]').setValue('abcd')

    const counter = () => wrapper.get('[data-test=char-count]').text()
    expect(counter()).toContain('4 / 50000')

    await wrapper
      .get('[data-test=content-input]')
      .setValue('好'.repeat(50001))
    expect(counter()).toContain('50001 / 50000')

    await wrapper.get('[data-test=confirm]').trigger('click')
    await flushPromises()

    expect(db.insert).not.toHaveBeenCalled()
    expect(agentStore.sendMessage).not.toHaveBeenCalled()
    expect(wrapper.emitted('imported')).toBeUndefined()
    wrapper.unmount()
  })
})
