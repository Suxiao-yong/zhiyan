<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { useRecordStore } from '@/stores/record'
import { defaultCheckinDuration } from './checkin-ui'
import WrongQuestionInline from './WrongQuestionInline.vue'
import {
  PlanCheckinSavedWithWarning,
  type PlanCheckinInput,
  type WrongQuestionInput,
} from '@/services/record-service'
import type { PlanWithNames } from '@/services/plan-service'

const props = defineProps<{
  modelValue: boolean
  plan: PlanWithNames | null
}>()
const emit = defineEmits<{ 'update:modelValue': [value: boolean]; saved: [] }>()

const recordStore = useRecordStore()
const submitting = ref(false)
const visible = computed({
  get: () => props.modelValue,
  set: (value) => emit('update:modelValue', value),
})

interface WrongDraft {
  question_source: string
  question_desc: string
  correct_answer: string
  my_answer: string
  error_type: string
  error_reason: string
}

const form = ref({
  duration_min: 30,
  content: '',
  questions_count: 0,
  correct_count: 0,
  mastery_rating: 0,
  session_time: 'evening' as string | null,
  mood: 0,
})
const wrongs = ref<WrongDraft[]>([])

const correctRate = computed(() =>
  form.value.questions_count > 0
    ? Math.round((form.value.correct_count / form.value.questions_count) * 100)
    : 0,
)
const showWrong = computed(
  () => form.value.questions_count > 0 && form.value.correct_count < form.value.questions_count,
)

const sessionOptions = [
  { label: '上午', value: 'morning' },
  { label: '下午', value: 'afternoon' },
  { label: '晚上', value: 'evening' },
]
const moodTexts = ['很差', '不佳', '一般', '良好', '很好']

function reset() {
  const plan = props.plan
  Object.assign(form.value, {
    duration_min: plan ? defaultCheckinDuration(plan) : 30,
    content: plan?.planned_tasks ?? '',
    questions_count: 0,
    correct_count: 0,
    mastery_rating: 0,
    session_time: 'evening',
    mood: 0,
  })
  wrongs.value = []
}

watch(
  () => [props.modelValue, props.plan?.id],
  ([open]) => {
    if (open) reset()
  },
)

async function submit(finish: boolean) {
  if (!props.plan || submitting.value) return
  if (form.value.duration_min <= 0) return ElMessage.warning('学习时长须大于 0')
  if (form.value.correct_count > form.value.questions_count)
    return ElMessage.warning('正确数不能大于做题数')

  submitting.value = true
  try {
    const input: PlanCheckinInput = {
      duration_min: form.value.duration_min,
      content: form.value.content || null,
      questions_count: form.value.questions_count,
      correct_count: form.value.correct_count,
      mastery_rating: form.value.mastery_rating || null,
      session_time: form.value.session_time,
      mood: form.value.mood || null,
    }
    const wrongInputs: WrongQuestionInput[] = wrongs.value
      .filter((wrong) => wrong.question_desc.trim())
      .map((wrong) => ({
        subject_id: props.plan!.subject_id,
        knowledge_point_id: props.plan!.knowledge_point_id,
        question_source: wrong.question_source || null,
        question_desc: wrong.question_desc || null,
        correct_answer: wrong.correct_answer || null,
        my_answer: wrong.my_answer || null,
        error_type: wrong.error_type || null,
        error_reason: wrong.error_reason || null,
      }))
    await recordStore.createPlanCheckin(props.plan.id, input, finish, wrongInputs)
    ElMessage.success(finish ? '任务已完成并记录' : '学习进度已保存')
    emit('saved')
    visible.value = false
  } catch (error) {
    if (error instanceof PlanCheckinSavedWithWarning) {
      ElMessage.warning(error.message)
      emit('saved')
      visible.value = false
    } else {
      ElMessage.error((error as Error).message || '打卡失败')
    }
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <el-dialog
    v-model="visible"
    title="计划任务打卡"
    width="580px"
    append-to-body
    :close-on-click-modal="false"
  >
    <div v-if="plan" class="plan-summary">
      <div class="plan-summary__top">
        <el-tag effect="light">{{ plan.subject_name ?? '未知科目' }}</el-tag>
        <span v-if="plan.knowledge_point_name" class="muted">
          {{ plan.knowledge_point_name }}
        </span>
        <span class="plan-summary__date tnum">{{ plan.date }}</span>
      </div>
      <div class="plan-summary__task">{{ plan.planned_tasks || '未填写任务内容' }}</div>
      <div class="plan-summary__meta tnum">
        计划 {{ plan.planned_duration ?? 0 }} 分钟 · 已记录 {{ plan.actual_duration ?? 0 }} 分钟
      </div>
    </div>

    <el-form v-if="plan" label-width="86px" @submit.prevent>
      <el-form-item label="本次时长" required>
        <el-input-number
          v-model="form.duration_min"
          :min="1"
          :step="15"
          controls-position="right"
        />
        <span class="hint">分钟，可分多次记录</span>
      </el-form-item>
      <el-form-item label="实际内容">
        <el-input
          v-model="form.content"
          type="textarea"
          :rows="2"
          maxlength="300"
          show-word-limit
          placeholder="本次实际完成了什么"
        />
      </el-form-item>
      <el-form-item label="做题情况">
        <div class="question-row">
          <span>共</span>
          <el-input-number v-model="form.questions_count" :min="0" controls-position="right" />
          <span>题，对</span>
          <el-input-number v-model="form.correct_count" :min="0" controls-position="right" />
          <span>题</span>
          <el-tag v-if="form.questions_count" size="small" type="info">
            正确率 {{ correctRate }}%
          </el-tag>
        </div>
      </el-form-item>
      <el-form-item label="掌握程度">
        <el-rate
          v-model="form.mastery_rating"
          :max="5"
          show-text
          :texts="['', '未掌握', '略懂', '了解', '熟悉', '掌握']"
        />
      </el-form-item>
      <el-form-item label="学习时段">
        <el-radio-group v-model="form.session_time">
          <el-radio-button v-for="item in sessionOptions" :key="item.value" :value="item.value">
            {{ item.label }}
          </el-radio-button>
        </el-radio-group>
      </el-form-item>
      <el-form-item label="心情">
        <el-rate v-model="form.mood" clearable show-text :texts="moodTexts" />
      </el-form-item>

      <WrongQuestionInline v-if="showWrong" v-model="wrongs" />
    </el-form>

    <template #footer>
      <el-button :disabled="submitting" @click="visible = false">取消</el-button>
      <el-button :loading="submitting" @click="submit(false)">保存进度</el-button>
      <el-button type="primary" :loading="submitting" @click="submit(true)">完成任务</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.plan-summary {
  margin-bottom: var(--sp-4);
  padding: var(--sp-3);
  border: 1px solid var(--c-border);
  border-radius: var(--r-md);
  background: var(--c-surface-2);
}
.plan-summary__top,
.question-row {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
}
.plan-summary__date {
  margin-left: auto;
  color: var(--c-ink-3);
  font-size: var(--fs-xs);
}
.plan-summary__task {
  margin-top: var(--sp-2);
  color: var(--c-ink);
  font-weight: 600;
}
.plan-summary__meta,
.muted,
.hint {
  color: var(--c-ink-3);
  font-size: var(--fs-xs);
}
.plan-summary__meta {
  margin-top: var(--sp-1);
}
.hint {
  margin-left: var(--sp-2);
}
.question-row {
  flex-wrap: wrap;
  color: var(--c-ink-2);
  font-size: var(--fs-sm);
}
</style>
