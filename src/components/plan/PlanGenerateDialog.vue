<script setup lang="ts">
import { computed } from 'vue'
import { ElMessage } from 'element-plus'
import { useExamStore } from '@/stores/exam'
import { usePlanStore } from '@/stores/plan'
import { useSettingsStore } from '@/stores/settings'

const props = defineProps<{ modelValue: boolean }>()
const emit = defineEmits<{ 'update:modelValue': [v: boolean]; done: [] }>()

const examStore = useExamStore()
const planStore = usePlanStore()
const settingsStore = useSettingsStore()

const visible = computed({
  get: () => props.modelValue,
  set: (v) => emit('update:modelValue', v),
})

const hasLLM = computed(() => {
  const c = settingsStore.llmConfig
  if (!c || !c.baseUrl || !c.model) return false
  if (c.provider === 'ollama') return true
  return !!c.apiKey
})

async function generate() {
  if (!examStore.activeExamId) {
    ElMessage.warning('请先选择考试')
    return
  }
  const result = await planStore.generatePlan(
    examStore.activeExamId,
    settingsStore.llmConfig,
  )
  ElMessage.success(result.message)
  emit('done')
  visible.value = false
}
</script>

<template>
  <el-dialog v-model="visible" title="生成学习计划" width="500px">
    <el-alert
      type="info"
      :closable="false"
      show-icon
      title="已有计划处理策略"
      description="重新生成会保留已记录的实际完成情况（actual_*），仅替换今天及之后的未完成计划。"
    />
    <div class="mode">
      <p>
        当前模式：
        <el-tag :type="hasLLM ? 'success' : 'warning'" effect="light" size="default">
          {{ hasLLM ? 'AI 生成（已配置 LLM）' : '本地算法（未配置 AI，降级）' }}
        </el-tag>
      </p>
      <p v-if="!hasLLM" class="hint">
        在「设置」页配置 LLM 后，可获得更精准的个性化计划。
      </p>
      <p v-if="planStore.lastResult" class="hint">
        上次：{{ planStore.lastResult.message }}（{{ planStore.lastResult.planCount }} 条任务）
      </p>
    </div>
    <template #footer>
      <el-button @click="visible = false">取消</el-button>
      <el-button type="primary" :loading="planStore.generating" @click="generate">
        {{ planStore.generating ? '生成中…' : '生成计划' }}
      </el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.mode {
  margin-top: var(--sp-3);
}
.mode p {
  margin: var(--sp-1) 0;
  font-size: var(--fs-sm);
  color: var(--c-ink-2);
}
.hint {
  color: var(--c-ink-3);
  font-size: var(--fs-xs);
}
</style>
