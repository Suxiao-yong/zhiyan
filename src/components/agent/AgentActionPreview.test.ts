// @vitest-environment jsdom
/// <reference lib="es2015" />

import { mount } from '@vue/test-utils'
import ElementPlus from 'element-plus'
import { describe, expect, it } from 'vitest'

import type { AgentActionPreview as AgentActionPreviewData } from '@/types'
import AgentActionPreview from './AgentActionPreview.vue'

function previewWithRows(rows: unknown[]): AgentActionPreviewData {
  return {
    tool: 'plan.preview_generate',
    risk: 2,
    undo_available: false,
    action: '生成计划预览',
    affected_count: rows.length,
    summary: '将写入计划',
    conflicts: [],
    date_range: '',
    fields: { rows },
  }
}

function mountCard(preview: AgentActionPreviewData) {
  return mount(AgentActionPreview, {
    props: { preview },
    global: { plugins: [ElementPlus] },
  })
}

describe('AgentActionPreview evidence source_ref', () => {
  it('shows 出处 §N when the row evidence carries a source_ref', () => {
    const wrapper = mountCard(
      previewWithRows([
        {
          date: '2030-01-04',
          subject_name: '数学',
          planned_tasks: '学习：函数',
          planned_duration: 120,
          evidence: {
            mastery: 2,
            wrong_question_count: 3,
            days_to_exam: 6,
            reason: '掌握度偏低(2/5)且有3道未掌握错题，优先攻克',
            source_ref: '§1-§2',
          },
        },
      ]),
    )

    const ref = wrapper.find('[data-test=action-preview-row-source-ref]')
    expect(ref.exists()).toBe(true)
    expect(wrapper.get('[data-test=action-preview-rows]').text()).toContain('出处：§1-§2')
  })

  it('hides the source line when the evidence has no source_ref', () => {
    const wrapper = mountCard(
      previewWithRows([
        {
          date: '2030-01-04',
          subject_name: '数学',
          planned_tasks: '学习：函数',
          planned_duration: 120,
          evidence: {
            mastery: 4,
            wrong_question_count: 0,
            days_to_exam: 6,
            reason: '巩固提升(4/5)',
          },
        },
      ]),
    )

    expect(wrapper.find('[data-test=action-preview-row-source-ref]').exists()).toBe(false)
    expect(wrapper.get('[data-test=action-preview-row-evidence]').text()).not.toContain('出处')
  })
})
