<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { useExamStore } from '@/stores/exam'
import { useRecordStore } from '@/stores/record'
import WrongQuestionInline from './WrongQuestionInline.vue'
import type { RecordInput, RecordWithNames, WrongQuestionInput } from '@/services/record-service'
import type { KnowledgePoint } from '@/types'

const props = defineProps<{
  modelValue: boolean
  presetDate?: string
  editRecord?: RecordWithNames
}>()
const emit = defineEmits<{ 'update:modelValue': [v: boolean]; saved: [] }>()

const examStore = useExamStore()
const recordStore = useRecordStore()

const editMode = computed(() => !!props.editRecord)

const visible = computed({
  get: () => props.modelValue,
  set: (v) => emit('update:modelValue', v),
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
  subject_id: '',
  knowledge_point_id: null as string | null,
  duration_min: 60,
  content: '',
  questions_count: 0,
  correct_count: 0,
  mastery_rating: 0,
  session_time: 'evening' as string | null,
  mood: 0,
  date: props.presetDate ?? '', // 空=实时(归一化)；有值=补记/编辑(不归一化)
})
const wrongs = ref<WrongDraft[]>([])
const kpOptions = ref<{ id: string; name: string }[]>([])

// 做题联动：有做题且未全对 → 显示错题录入（编辑模式下不管理错题）
const showWrong = computed(
  () =>
    !editMode.value &&
    form.value.questions_count > 0 &&
    form.value.correct_count < form.value.questions_count,
)
const correctRate = computed(() =>
  form.value.questions_count > 0
    ? Math.round((form.value.correct_count / form.value.questions_count) * 100)
    : 0,
)

const moods = [
  { value: 1, icon: '😢', label: '很差' },
  { value: 2, icon: '☹️', label: '不佳' },
  { value: 3, icon: '😐', label: '一般' },
  { value: 4, icon: '🙂', label: '良好' },
  { value: 5, icon: '😄', label: '很好' },
]

const sessionOptions = [
  { label: '上午', value: 'morning' },
  { label: '下午', value: 'afternoon' },
  { label: '晚上', value: 'evening' },
]

function reset() {
  Object.assign(form.value, {
    subject_id: '',
    knowledge_point_id: null,
    duration_min: 60,
    content: '',
    questions_count: 0,
    correct_count: 0,
    mastery_rating: 0,
    session_time: 'evening',
    mood: 0,
    date: props.presetDate ?? '',
  })
  wrongs.value = []
  kpOptions.value = []
}

watch(visible, async (v) => {
  if (!v) {
    reset()
    return
  }
  if (!examStore.activeExamId && examStore.exams.length) {
    await examStore.setActiveExam(examStore.exams[0].id)
  }
  if (examStore.activeExamId) await examStore.loadSubjects()
  if (props.editRecord) {
    // 编辑：预填表单（date 用已存储值，不重新归一化）
    const r = props.editRecord
    Object.assign(form.value, {
      subject_id: r.subject_id,
      knowledge_point_id: r.knowledge_point_id,
      duration_min: r.duration_min,
      content: r.content ?? '',
      questions_count: r.questions_count,
      correct_count: r.correct_count,
      mastery_rating: r.mastery_rating ?? 0,
      session_time: r.session_time ?? 'evening',
      mood: r.mood ?? 0,
      date: r.date,
    })
    await onSubjectChange(r.subject_id)
  } else {
    form.value.date = props.presetDate ?? ''
  }
})

async function onSubjectChange(subjectId: string) {
  form.value.knowledge_point_id = null
  if (!subjectId) {
    kpOptions.value = []
    return
  }
  await examStore.loadKnowledgeTree(subjectId)
  const flat: { id: string; name: string }[] = []
  const walk = (nodes: KnowledgePoint[]) => {
    nodes.forEach((n) => {
      flat.push({ id: n.id, name: n.name })
      if (n.children) walk(n.children)
    })
  }
  walk(examStore.knowledgeTree)
  kpOptions.value = flat
}

async function handleSubmit() {
  if (!form.value.subject_id) return ElMessage.warning('请选择科目')
  if (form.value.duration_min <= 0) return ElMessage.warning('学习时长须大于 0')
  if (form.value.correct_count > form.value.questions_count)
    return ElMessage.warning('正确数不能大于做题数')
  try {
    const input: RecordInput = {
      date: form.value.date || undefined, // 空=实时(归一化)
      subject_id: form.value.subject_id,
      knowledge_point_id: form.value.knowledge_point_id || null,
      duration_min: form.value.duration_min,
      content: form.value.content || null,
      questions_count: form.value.questions_count,
      correct_count: form.value.correct_count,
      mastery_rating: form.value.mastery_rating || null,
      session_time: form.value.session_time,
      mood: form.value.mood || null,
    }
    if (editMode.value && props.editRecord) {
      await recordStore.updateRecord(props.editRecord.id, input)
      ElMessage.success('记录已更新')
    } else {
      const wrongInputs: WrongQuestionInput[] = wrongs.value
        .filter((w) => w.question_desc.trim())
        .map((w) => ({
          subject_id: form.value.subject_id,
          knowledge_point_id: form.value.knowledge_point_id || null,
          question_source: w.question_source || null,
          question_desc: w.question_desc || null,
          correct_answer: w.correct_answer || null,
          my_answer: w.my_answer || null,
          error_type: w.error_type || null,
          error_reason: w.error_reason || null,
        }))
      await recordStore.createRecord(input, wrongInputs)
      ElMessage.success('记录已保存')
    }
    emit('saved')
    visible.value = false
  } catch (e) {
    ElMessage.error((e as Error).message ?? '保存失败')
  }
}
</script>

<template>
  <el-dialog
    v-model="visible"
    :title="editMode ? '编辑记录' : '快速记录'"
    width="560px"
    :close-on-click-modal="false"
  >
    <el-form label-width="86px" @submit.prevent>
      <el-form-item label="日期">
        <el-date-picker
          v-model="form.date"
          type="date"
          placeholder="留空 = 今天（实时记录，按 04:00 归一化）"
          value-format="YYYY-MM-DD"
          class="full"
        />
        <span class="hint">选了日期 = 补记（不归一化）</span>
      </el-form-item>
      <el-form-item label="科目" required>
        <el-select
          v-model="form.subject_id"
          placeholder="选择科目"
          class="full"
          @change="onSubjectChange"
        >
          <el-option
            v-for="s in examStore.subjects"
            :key="s.id"
            :label="s.name"
            :value="s.id"
          />
        </el-select>
        <span v-if="!examStore.subjects.length" class="hint warn">
          当前考试无科目，请先在「考试配置」中添加
        </span>
      </el-form-item>
      <el-form-item label="知识点">
        <el-select
          v-model="form.knowledge_point_id"
          clearable
          placeholder="可选（快速记录可空）"
          class="full"
        >
          <el-option
            v-for="k in kpOptions"
            :key="k.id"
            :label="k.name"
            :value="k.id"
          />
        </el-select>
      </el-form-item>
      <el-form-item label="学习时长" required>
        <el-input-number v-model="form.duration_min" :min="1" :step="15" controls-position="right" />
        <span class="hint">分钟</span>
      </el-form-item>
      <el-form-item label="学习内容">
        <el-input v-model="form.content" type="textarea" :rows="2" placeholder="学了什么" maxlength="300" show-word-limit />
      </el-form-item>
      <el-form-item label="做题情况">
        <div class="qs-row">
          <span>共</span>
          <el-input-number v-model="form.questions_count" :min="0" :step="1" controls-position="right" />
          <span>题，对</span>
          <el-input-number v-model="form.correct_count" :min="0" :step="1" controls-position="right" />
          <span>题</span>
          <el-tag v-if="form.questions_count > 0" size="small" type="info" effect="light">正确率 {{ correctRate }}%</el-tag>
        </div>
      </el-form-item>
      <el-form-item label="掌握程度">
        <el-rate v-model="form.mastery_rating" :max="5" show-text :texts="['', '未掌握', '略懂', '了解', '熟悉', '掌握']" />
      </el-form-item>
      <el-form-item label="学习时段">
        <el-radio-group v-model="form.session_time">
          <el-radio-button v-for="s in sessionOptions" :key="s.value" :value="s.value">{{ s.label }}</el-radio-button>
        </el-radio-group>
      </el-form-item>
      <el-form-item label="心情">
        <el-rate
          v-model="form.mood"
          :max="moods.length"
          clearable
          show-text
          :texts="moods.map((m) => m.label)"
        />
      </el-form-item>

      <WrongQuestionInline v-if="showWrong" v-model="wrongs" />
    </el-form>

    <template #footer>
      <el-button @click="visible = false">取消</el-button>
      <el-button type="primary" @click="handleSubmit">{{ editMode ? '保存' : '保存' }}</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.full {
  width: 100%;
}
.hint {
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
  margin-left: var(--sp-2);
}
.hint.warn {
  color: var(--c-warning);
}
.qs-row {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  font-size: var(--fs-sm);
  color: var(--c-ink-2);
}
</style>
