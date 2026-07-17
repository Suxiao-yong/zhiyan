// @vitest-environment jsdom

import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { nextTick } from 'vue'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('element-plus', () => ({
  ElMessage: { success: vi.fn(), warning: vi.fn(), error: vi.fn() },
}))

import PlanCheckinDialog from './PlanCheckinDialog.vue'
import { useRecordStore } from '@/stores/record'
import type { StudyRecord } from '@/types'
import type { PlanWithNames } from '@/services/plan-service'

const SlotStub = { template: '<div><slot /><slot name="footer" /></div>' }
const ButtonStub = {
  props: { disabled: Boolean },
  emits: ['click'],
  template: '<button :disabled="disabled" @click="$emit(\'click\')"><slot /></button>',
}

function plan(): PlanWithNames {
  return {
    id: 'p1',
    exam_id: 'e1',
    subject_id: 's1',
    knowledge_point_id: null,
    date: '2026-07-17',
    planned_tasks: '复习函数',
    planned_duration: 60,
    actual_duration: 0,
    actual_tasks: null,
    status: 'pending',
    generated_by: 'local',
    ai_suggestion: null,
    user_modified: 0,
    sort_order: 0,
    created_at: '',
    updated_at: '',
    subject_name: '数学',
    knowledge_point_name: null,
    record_count: 0,
  }
}

function record(): StudyRecord {
  return {
    id: 'r1',
    plan_id: 'p1',
    date: '2026-07-17',
    subject_id: 's1',
    knowledge_point_id: null,
    duration_min: 60,
    content: '复习函数',
    questions_count: 0,
    correct_count: 0,
    mastery_rating: null,
    difficulty_notes: null,
    mood: null,
    session_time: 'evening',
    created_at: '',
    updated_at: '',
  }
}

describe('PlanCheckinDialog', () => {
  beforeEach(() => vi.clearAllMocks())

  it('提交未完成时忽略重复点击', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    const store = useRecordStore()
    let resolveSave!: (value: StudyRecord) => void
    const pendingSave = new Promise<StudyRecord>((resolve) => {
      resolveSave = resolve
    })
    const create = vi.spyOn(store, 'createPlanCheckin').mockReturnValue(pendingSave)

    const wrapper = mount(PlanCheckinDialog, {
      props: { modelValue: true, plan: plan() },
      global: {
        plugins: [pinia],
        stubs: {
          'el-dialog': SlotStub,
          'el-form': SlotStub,
          'el-form-item': SlotStub,
          'el-button': ButtonStub,
          'el-tag': SlotStub,
          'el-input': true,
          'el-input-number': true,
          'el-rate': true,
          'el-radio-group': SlotStub,
          'el-radio-button': SlotStub,
          WrongQuestionInline: true,
        },
      },
    })
    await nextTick()
    const finishButton = wrapper.findAll('button').find((button) => button.text() === '完成任务')
    expect(finishButton).toBeDefined()

    await finishButton!.trigger('click')
    await finishButton!.trigger('click')
    expect(create).toHaveBeenCalledTimes(1)

    resolveSave(record())
    await flushPromises()
    expect(wrapper.emitted('saved')).toHaveLength(1)
  })
})
