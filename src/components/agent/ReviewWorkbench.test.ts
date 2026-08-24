// @vitest-environment jsdom
/// <reference lib="es2015" />

import { flushPromises, mount } from '@vue/test-utils'
import ElementPlus from 'element-plus'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const client = vi.hoisted(() => ({ reviewListDue: vi.fn() }))
vi.mock('@/services/agent-client', () => client)

import ReviewWorkbench from './ReviewWorkbench.vue'

const item = (id: string, subjectName: string) => ({
  id,
  question_desc: `题目 ${id}`,
  subject_name: subjectName,
  knowledge_point_name: null,
})

function mountCard() {
  return mount(ReviewWorkbench, { global: { plugins: [createPinia(), ElementPlus] } })
}

describe('ReviewWorkbench', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    client.reviewListDue.mockReset()
  })

  it('renders due count items with subject names', async () => {
    client.reviewListDue.mockResolvedValue({
      count: 2,
      items: [item('wq-1', '数学'), item('wq-2', '英语')],
    })
    const wrapper = mountCard()
    await flushPromises()

    expect(wrapper.find('[data-test=review-list]').exists()).toBe(true)
    expect(wrapper.get('[data-test=review-list]').text()).toContain('数学')
    expect(wrapper.get('[data-test=review-list]').text()).toContain('英语')
    expect(wrapper.find('[data-test=review-start]').exists()).toBe(true)
  })

  it('shows the empty state when nothing is due', async () => {
    client.reviewListDue.mockResolvedValue({ count: 0, items: [] })
    const wrapper = mountCard()
    await flushPromises()

    expect(wrapper.get('[data-test=review-empty]').text()).toContain('暂无')
    expect(wrapper.find('[data-test=review-list]').exists()).toBe(false)
    expect(wrapper.find('[data-test=review-start]').exists()).toBe(false)
  })

  it('falls back to the empty state when the command fails', async () => {
    client.reviewListDue.mockRejectedValue(new Error('boom'))
    const wrapper = mountCard()
    await flushPromises()

    expect(wrapper.get('[data-test=review-empty]').text()).toContain('暂无')
  })
})
