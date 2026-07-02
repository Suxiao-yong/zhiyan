<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { useSettingsStore } from '@/stores/settings'
import { runChatTurn, finalizePlan } from '@/services/plan-chat-agent'
import { generateLocalPlan } from '@/services/plan-generator'
import { getById } from '@/services/db'
import type { Exam } from '@/types'

const props = defineProps<{
  modelValue: boolean
  examId: string
  examName: string
}>()
const emit = defineEmits<{ 'update:modelValue': [v: boolean]; done: [] }>()

const settingsStore = useSettingsStore()
const visible = computed({
  get: () => props.modelValue,
  set: (v) => emit('update:modelValue', v),
})

const llmMessages = ref<any[]>([])
const chat = ref<{ role: 'user' | 'assistant'; content: string }[]>([])
const input = ref('')
const loading = ref(false)
const searchHint = ref('')
const scrollRef = ref<HTMLElement>()

const hasLLM = computed(() => {
  const c = settingsStore.llmConfig
  return !!c && !!c.baseUrl && !!c.model && (c.provider === 'ollama' || !!c.apiKey)
})

function scrollToBottom() {
  nextTick(() => {
    if (scrollRef.value) scrollRef.value.scrollTop = scrollRef.value.scrollHeight
  })
}

async function start() {
  llmMessages.value = []
  chat.value = []
  if (!hasLLM.value) {
    chat.value.push({
      role: 'assistant',
      content:
        '⚠️ 未配置可用的 LLM。请先到「设置」配置（推荐 DeepSeek）。或点下方「用本地算法生成」走降级方案。',
    })
    return
  }
  const exam = await getById<Exam>('exams', props.examId)
  const sys = `你是智研的 AI 规划助手。用户刚添加了考试：${props.examName}（类型：${exam?.exam_type ?? '自定义'}，考试日期：${exam?.exam_date ?? ''}，总分：${exam?.total_score ?? '未定'}）。

请用 search_web 工具研究该考试的大纲、重点知识点、备考策略与推荐教材，然后：
1. 提出建议的科目（含目标分、当前水平估计 1-5、权重）与每个科目的 3-6 个核心知识点
2. 提出分阶段（基础期/强化期/冲刺期）的复习策略与每科每周时长
3. 与用户讨论调整（用户可能说某科弱、每天可用时长、用某教材等）

讨论确定后，用户会点「采用此计划」，届时你需输出最终计划 JSON。用中文回复，简洁清晰。`
  llmMessages.value = [
    { role: 'system', content: sys },
    { role: 'user', content: `请研究「${props.examName}」并给出科目、知识点与复习策略建议。` },
  ]
  await runTurn()
}

async function runTurn() {
  loading.value = true
  searchHint.value = ''
  try {
    const res = await runChatTurn(llmMessages.value, settingsStore.llmConfig!)
    llmMessages.value = res.messages
    chat.value.push({ role: 'assistant', content: res.content || '（无回复）' })
    if (res.searching) searchHint.value = '已联网搜索'
  } catch (e) {
    chat.value.push({
      role: 'assistant',
      content: `⚠️ AI 调用失败：${(e as Error).message}\n\n可点「用本地算法生成」走降级方案。`,
    })
  } finally {
    loading.value = false
    searchHint.value = ''
    scrollToBottom()
  }
}

async function send() {
  const text = input.value.trim()
  if (!text || loading.value) return
  llmMessages.value.push({ role: 'user', content: text })
  chat.value.push({ role: 'user', content: text })
  input.value = ''
  scrollToBottom()
  await runTurn()
}

async function adopt() {
  if (!hasLLM.value) return
  loading.value = true
  try {
    const r = await finalizePlan(props.examId, llmMessages.value, settingsStore.llmConfig!)
    ElMessage.success(`已采用：${r.subjectCount} 科目，${r.planCount} 条计划任务`)
    emit('done')
    visible.value = false
  } catch (e) {
    ElMessage.error('采用失败：' + (e as Error).message)
  } finally {
    loading.value = false
  }
}

async function localFallback() {
  loading.value = true
  try {
    const r = await generateLocalPlan(props.examId)
    ElMessage.success(r.message)
    emit('done')
    visible.value = false
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    loading.value = false
  }
}

watch(visible, (v) => {
  if (v) start()
})
</script>

<template>
  <el-dialog
    v-model="visible"
    :title="`AI 规划助手 · ${examName}`"
    width="680px"
    top="6vh"
    :close-on-click-modal="false"
  >
    <div ref="scrollRef" class="chat-box">
      <div
        v-for="(m, i) in chat"
        :key="i"
        class="msg"
        :class="m.role"
      >
        <div class="bubble">{{ m.content }}</div>
      </div>
      <div v-if="loading" class="msg assistant">
        <div class="bubble thinking">
          <span class="dot" /> <span class="dot" /> <span class="dot" />
          <span class="think-text">{{ searchHint || 'AI 思考中…' }}</span>
        </div>
      </div>
    </div>

    <div class="input-bar">
      <el-input
        v-model="input"
        class="chat-input"
        type="textarea"
        :rows="2"
        placeholder="与 AI 讨论（如：数学基础弱，多给时间；每天能学 5 小时）"
        @keydown.enter.exact.prevent="send"
        :disabled="loading"
      />
      <el-button type="primary" :loading="loading" @click="send">发送</el-button>
    </div>

    <template #footer>
      <el-button v-if="!hasLLM" type="primary" @click="localFallback">用本地算法生成</el-button>
      <el-button @click="visible = false">关闭</el-button>
      <el-button v-if="hasLLM" type="success" :loading="loading" @click="adopt">采用此计划</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.chat-box {
  height: 50vh;
  overflow-y: auto;
  padding: var(--sp-2) var(--sp-1);
  display: flex;
  flex-direction: column;
  gap: var(--sp-3);
}
.msg {
  display: flex;
}
.msg.user {
  justify-content: flex-end;
}
.msg.assistant {
  justify-content: flex-start;
}
.bubble {
  max-width: 85%;
  padding: var(--sp-2) var(--sp-3);
  border-radius: var(--r-lg);
  font-size: var(--fs-sm);
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-word;
}
.msg.user .bubble {
  background: var(--c-primary);
  color: white;
  border-bottom-right-radius: var(--r-sm);
}
.msg.assistant .bubble {
  background: var(--c-surface-2);
  color: var(--c-ink);
  border-bottom-left-radius: var(--r-sm);
}
.bubble.thinking {
  display: inline-flex;
  align-items: center;
  gap: var(--sp-1);
}
.dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--c-ink-muted);
  animation: blink 1.2s infinite;
}
.dot:nth-child(2) { animation-delay: 0.2s; }
.dot:nth-child(3) { animation-delay: 0.4s; }
.think-text {
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
}
@keyframes blink {
  0%, 100% { opacity: 0.3; }
  50% { opacity: 1; }
}
.input-bar {
  margin-top: var(--sp-3);
  display: flex;
  gap: var(--sp-2);
  align-items: flex-end;
}
.chat-input {
  flex: 1;
  min-width: 0;
}
</style>
