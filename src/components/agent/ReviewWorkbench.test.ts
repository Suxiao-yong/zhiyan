// @vitest-environment jsdom
/// <reference lib="es2015" />

import { flushPromises, mount } from '@vue/test-utils'
import ElementPlus from 'element-plus'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const client = vi.hoisted(() => ({ reviewListDue: vi.fn(), flashcardListDue: vi.fn() }))
vi.mock('@/services/agent-client', () => client)

const agentStore = vi.hoisted(() => ({ sendMessage: vi.fn() }))
vi.mock('@/stores/agent', () => ({ useAgentStore: () => agentStore }))

import ReviewWorkbench from './ReviewWorkbench.vue'

const wrongItem = (id: string, subjectName: string) => ({
  id,
  question_desc: `题目 ${id}`,
  subject_name: subjectName,
  knowledge_point_name: null,
})

const flashcard = (id: string, front: string) => ({
  id,
  front,
  source_ref: null,
  knowledge_point_name: null,
})

function mountCard() {
  return mount(ReviewWorkbench, { global: { plugins: [ElementPlus] } })
}

describe('ReviewWorkbench', () => {
  beforeEach(() => {
    client.reviewListDue.mockReset()
    client.flashcardListDue.mockReset()
    agentStore.sendMessage.mockReset().mockResolvedValue(null)
  })

  it('renders wrong-question and flashcard groups with the total count', async () => {
    client.reviewListDue.mockResolvedValue({
      count: 2,
      items: [wrongItem('wq-1', '数学'), wrongItem('wq-2', '英语')],
    })
    client.flashcardListDue.mockResolvedValue({
      count: 3,
      items: [flashcard('fc-1', '导数定义'), flashcard('fc-2', '泰勒展开'), flashcard('fc-3', '洛必达')],
    })
    const wrapper = mountCard()
    await flushPromises()

    expect(wrapper.get('[data-test=wrong-section]').text()).toContain('错题(2)')
    expect(wrapper.get('[data-test=review-list]').text()).toContain('数学')
    expect(wrapper.get('[data-test=flashcard-section]').text()).toContain('闪卡(3)')
    expect(wrapper.get('[data-test=flashcard-section]').text()).toContain('导数定义')
    expect(wrapper.get('[data-test=review-total]').text()).toContain('5')
    expect(wrapper.find('[data-test=review-empty]').exists()).toBe(false)
    expect(wrapper.find('[data-test=review-start]').exists()).toBe(true)
  })

  it('hides the flashcard section when no flashcards are due', async () => {
    client.reviewListDue.mockResolvedValue({
      count: 1,
      items: [wrongItem('wq-1', '数学')],
    })
    client.flashcardListDue.mockResolvedValue({ count: 0, items: [] })
    const wrapper = mountCard()
    await flushPromises()

    expect(wrapper.find('[data-test=wrong-section]').exists()).toBe(true)
    expect(wrapper.find('[data-test=flashcard-section]').exists()).toBe(false)
    expect(wrapper.get('[data-test=review-total]').text()).toContain('1')
    expect(wrapper.find('[data-test=review-start]').exists()).toBe(true)
  })

  it('hides the wrong-question section when no wrong questions are due', async () => {
    client.reviewListDue.mockResolvedValue({ count: 0, items: [] })
    client.flashcardListDue.mockResolvedValue({
      count: 2,
      items: [flashcard('fc-1', '动量守恒'), flashcard('fc-2', '楞次定律')],
    })
    const wrapper = mountCard()
    await flushPromises()

    expect(wrapper.find('[data-test=wrong-section]').exists()).toBe(false)
    expect(wrapper.find('[data-test=flashcard-section]').exists()).toBe(true)
    expect(wrapper.find('[data-test=review-start]').exists()).toBe(true)
  })

  it('renders the flashcard section when the wrong-question command fails', async () => {
    client.reviewListDue.mockRejectedValue(new Error('boom'))
    client.flashcardListDue.mockResolvedValue({
      count: 2,
      items: [flashcard('fc-1', '动量守恒'), flashcard('fc-2', '楞次定律')],
    })
    const wrapper = mountCard()
    await flushPromises()

    expect(wrapper.find('[data-test=wrong-section]').exists()).toBe(false)
    expect(wrapper.get('[data-test=flashcard-section]').text()).toContain('闪卡(2)')
    expect(wrapper.get('[data-test=review-total]').text()).toContain('2')
    expect(wrapper.find('[data-test=review-start]').exists()).toBe(true)
  })

  it('shows the empty state and no start button when nothing at all is due', async () => {
    client.reviewListDue.mockResolvedValue({ count: 0, items: [] })
    client.flashcardListDue.mockResolvedValue({ count: 0, items: [] })
    const wrapper = mountCard()
    await flushPromises()

    expect(wrapper.get('[data-test=review-empty]').text()).toContain('暂无')
    expect(wrapper.find('[data-test=wrong-section]').exists()).toBe(false)
    expect(wrapper.find('[data-test=flashcard-section]').exists()).toBe(false)
    expect(wrapper.find('[data-test=review-start]').exists()).toBe(false)
  })

  it('falls back to the empty state when both commands fail', async () => {
    client.reviewListDue.mockRejectedValue(new Error('boom'))
    client.flashcardListDue.mockRejectedValue(new Error('boom'))
    const wrapper = mountCard()
    await flushPromises()

    expect(wrapper.get('[data-test=review-empty]').text()).toContain('暂无')
  })

  it('hands off to the Agent with the combined review goal', async () => {
    client.reviewListDue.mockResolvedValue({ count: 1, items: [wrongItem('wq-1', '数学')] })
    client.flashcardListDue.mockResolvedValue({ count: 1, items: [flashcard('fc-1', '导数')] })
    const wrapper = mountCard()
    await flushPromises()

    await wrapper.get('[data-test=review-start]').trigger('click')
    await flushPromises()

    expect(agentStore.sendMessage).toHaveBeenCalledTimes(1)
    expect(agentStore.sendMessage).toHaveBeenCalledWith('帮我安排今天的错题和闪卡复习')
  })
})
