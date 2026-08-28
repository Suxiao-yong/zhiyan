<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import type { FormInstance, FormRules } from 'element-plus'
import { v4 as uuidv4 } from 'uuid'
import { insert } from '@/services/db'
import { getSubjectsByExam } from '@/services/exam-service'
import type { Subject } from '@/types'
import { useAgentStore } from '@/stores/agent'
import { useExamStore } from '@/stores/exam'

/**
 * Material import dialog (v0.3.0 Task 9): writes straight through the db
 * whitelist (no chat prefill), then hands a short message to the Agent so it
 * can split concepts and generate flashcards.
 */
const props = defineProps<{ modelValue: boolean }>()
const emit = defineEmits<{ 'update:modelValue': [v: boolean]; imported: [] }>()

const MAX_CHARS = 50000

const agent = useAgentStore()
const examStore = useExamStore()

const formRef = ref<FormInstance>()
const submitting = ref(false)
const error = ref('')
const subjects = ref<Subject[]>([])
const form = reactive({ title: '', subject_id: '', content: '' })

/** Unicode 字符口径（code point），不是 UTF-16 code unit。 */
const charCount = computed(() => [...form.content].length)

function validateCharLimit(_rule: unknown, value: string, callback: (err?: Error) => void): void {
  if ([...(value ?? '')].length > MAX_CHARS) {
    callback(new Error(`正文不能超过 ${MAX_CHARS} 字符`))
  } else {
    callback()
  }
}

const rules: FormRules = {
  title: [
    { required: true, message: '请输入材料标题', trigger: 'blur' },
    { min: 1, max: 200, message: '标题需为 1..200 字符', trigger: 'blur' },
  ],
  subject_id: [{ required: true, message: '请选择科目', trigger: 'change' }],
  content: [
    { required: true, message: '请粘贴材料正文', trigger: 'blur' },
    { validator: validateCharLimit, trigger: 'input' },
  ],
}

watch(
  () => props.modelValue,
  async (open) => {
    if (!open) return
    form.title = ''
    form.subject_id = ''
    form.content = ''
    error.value = ''
    try {
      subjects.value = await getSubjectsByExam(examStore.activeExamId ?? '')
    } catch {
      subjects.value = []
    }
  },
  { immediate: true },
)

async function submit(): Promise<void> {
  // 显式守卫：不依赖 el-form 规则管道（jsdom 下其聚合校验不可靠），确保非法输入绝不入库。
  if (!form.title.trim() || !form.subject_id || charCount.value > MAX_CHARS) {
    error.value = !form.title.trim()
      ? '请输入材料标题'
      : !form.subject_id
        ? '请选择科目'
        : `正文不能超过 ${MAX_CHARS} 字符`
    return
  }
  const valid = await formRef.value?.validate().catch(() => false)
  if (!valid) return
  submitting.value = true
  error.value = ''
  try {
    // insert 成功即视为导入成功：后续消息失败不再阻塞关闭/emit，避免用户重试产生重复行。
    await insert('materials', {
      id: uuidv4(),
      title: form.title.trim(),
      content: form.content,
      subject_id: form.subject_id,
    })
    emit('update:modelValue', false)
    emit('imported')
    // 空行分段：按连续空行切段并丢弃空白段，得到有效段落数。
    const segments = form.content.split(/\n{2,}/).filter((seg) => seg.trim().length > 0).length
    try {
      const sent = await agent.sendMessage(
        `已导入材料《${form.title.trim()}》共 ${segments} 段，请拆解概念并生成闪卡`,
      )
      if (!sent) ElMessage.warning('材料已入库，但复习消息发送失败')
    } catch {
      ElMessage.warning('材料已入库，但复习消息发送失败')
    }
  } catch (caught) {
    error.value = String(caught)
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <el-dialog
    :model-value="modelValue"
    title="导入材料"
    width="560px"
    @update:model-value="(v: boolean) => emit('update:modelValue', v)"
  >
    <el-form ref="formRef" :model="form" :rules="rules" label-width="64px">
      <el-form-item label="标题" prop="title">
        <el-input
          v-model="form.title"
          data-test="title-input"
          placeholder="材料标题（必填，1-200 字符）"
        />
      </el-form-item>
      <el-form-item label="科目" prop="subject_id">
        <el-select v-model="form.subject_id" data-test="subject-select" placeholder="选择科目">
          <el-option v-for="s in subjects" :key="s.id" :label="s.name" :value="s.id" />
        </el-select>
      </el-form-item>
      <el-form-item label="正文" prop="content">
        <div class="content-wrap">
          <el-input
            v-model="form.content"
            type="textarea"
            :rows="10"
            data-test="content-input"
            placeholder="粘贴材料正文（空行分段）"
          />
          <span class="char-count tnum" data-test="char-count">{{ charCount }} / {{ MAX_CHARS }}</span>
        </div>
      </el-form-item>
    </el-form>
    <p v-if="error" class="dialog-error">{{ error }}</p>
    <template #footer>
      <el-button @click="emit('update:modelValue', false)">取消</el-button>
      <el-button type="primary" :loading="submitting" data-test="confirm" @click="submit">
        导入并生成闪卡
      </el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.content-wrap {
  display: flex;
  flex-direction: column;
  gap: 4px;
  width: 100%;
}
.char-count {
  align-self: flex-end;
  font-size: 12px;
  color: var(--el-text-color-secondary);
}
.dialog-error {
  margin: 0;
  color: var(--el-color-danger);
  font-size: 13px;
}
</style>
