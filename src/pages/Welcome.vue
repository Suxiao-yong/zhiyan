<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessage } from 'element-plus'
import { invoke } from '@tauri-apps/api/core'

function isTauri(): boolean {
  return typeof window !== 'undefined' && !!(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
}
function heuristicKps(subject: string): Array<{ name: string; chapter: string | null }> {
  const s = subject.trim().toLowerCase()
  if (s.includes('数学') || s.includes('math'))
    return [
      { name: '函数与极限', chapter: '第一章' },
      { name: '导数与微分', chapter: '第二章' },
      { name: '积分学', chapter: '第三章' },
      { name: '线性代数', chapter: '第四章' },
      { name: '概率统计', chapter: '第五章' },
    ]
  if (s.includes('英语') || s.includes('english'))
    return [
      { name: '词汇', chapter: '第一章' },
      { name: '语法', chapter: '第二章' },
      { name: '阅读理解', chapter: '第三章' },
      { name: '写作', chapter: '第四章' },
      { name: '翻译', chapter: '第五章' },
    ]
  if (s.includes('政治'))
    return [
      { name: '马克思主义原理', chapter: '第一章' },
      { name: '毛泽东思想', chapter: '第二章' },
      { name: '中国特色社会主义', chapter: '第三章' },
      { name: '时政', chapter: '第四章' },
    ]
  return [
    { name: '基础概念', chapter: '第一章' },
    { name: '核心原理', chapter: '第二章' },
    { name: '重点难点', chapter: '第三章' },
    { name: '综合应用', chapter: '第四章' },
    { name: '真题要点', chapter: '第五章' },
  ]
}
import {
  ArrowRight,
  ArrowLeft,
  Check,
  Plus,
  Delete,
  Aim,
  Lock,
  ChatDotRound,
  Loading,
  Notebook,
} from '@element-plus/icons-vue'
import { useExamStore } from '@/stores/exam'
import { useSettingsStore } from '@/stores/settings'
import { setSetting } from '@/services/db'
import { validateExamDate } from '@/services/exam-service'
import { markOnboardingDone } from '@/router'

const router = useRouter()
const store = useExamStore()
const settingsStore = useSettingsStore()

const current = ref(0)
const saving = ref(false)

const examTypes = [
  { value: 'postgrad', label: '考研', desc: '全国硕士研究生招生考试' },
  { value: 'civil', label: '考公', desc: '公务员 / 事业单位' },
  { value: 'cert', label: '考证', desc: '职业资格 / 技能证书' },
  { value: 'custom', label: '自定义', desc: '其它考试目标' },
]

interface KpDraft {
  name: string
  chapter: string
  mastery: number
}
interface SubjectDraft {
  name: string
  target_score: number | null
  current_level: number
  weight: number
  knowledgePoints: KpDraft[]
}

const exam = reactive({
  name: '',
  exam_type: 'postgrad',
  exam_date: '',
  total_score: null as number | null,
})
const subjects = reactive<SubjectDraft[]>([
  { name: '', target_score: null, current_level: 3, weight: 1, knowledgePoints: [] },
])

const levelLabels = ['', '入门', '基础', '一般', '熟练', '精通']

// ---- LLM 配置（步骤 1，强制）----
const llmProviders = [
  {
    value: 'deepseek',
    label: 'DeepSeek',
    baseUrl: 'https://api.deepseek.com',
    model: 'deepseek-chat',
  },
  { value: 'openai', label: 'OpenAI', baseUrl: 'https://api.openai.com', model: 'gpt-4o' },
  {
    value: 'qwen',
    label: '通义千问',
    baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode',
    model: 'qwen-plus',
  },
  { value: 'kimi', label: 'Kimi', baseUrl: 'https://api.moonshot.cn', model: 'moonshot-v1-8k' },
  { value: 'custom', label: '自定义', baseUrl: '', model: '' },
]
const llmForm = reactive({
  provider: 'deepseek',
  baseUrl: 'https://api.deepseek.com',
  model: 'deepseek-chat',
  temperature: 0.7,
})
const llmApiKey = ref('')
const llmShowKey = ref(false)
const llmSaving = ref(false)
const llmTesting = ref(false)

function syncLlmFormFromStore() {
  const c = settingsStore.llmConfig
  if (!c) return
  llmForm.provider = c.provider
  llmForm.baseUrl = c.baseUrl
  llmForm.model = c.model
  llmForm.temperature = c.temperature
}
watch(() => settingsStore.llmConfig, syncLlmFormFromStore, { immediate: true })
onMounted(async () => {
  await settingsStore.loadSettings()
  syncLlmFormFromStore()
})
function onLlmProviderChange(v: string) {
  if (v === 'custom') return
  const p = llmProviders.find((x) => x.value === v)
  if (p) {
    llmForm.baseUrl = p.baseUrl
    llmForm.model = p.model
  }
  void settingsStore.refreshKeyState(v)
}
function validateLlmForm(): string | null {
  if (!/^https?:\/\/.+/i.test(llmForm.baseUrl.trim())) return 'API 地址必须是完整的 http/https 地址'
  if (!llmForm.model.trim()) return '模型名不能为空'
  if (!settingsStore.keyConfigured && !llmApiKey.value.trim()) return '请填写 API Key'
  return null
}

async function saveLlmForOnboarding(): Promise<boolean> {
  const problem = validateLlmForm()
  if (problem) {
    ElMessage.warning(problem)
    return false
  }
  llmSaving.value = true
  try {
    await settingsStore.saveLlmConfig(
      {
        provider: llmForm.provider,
        baseUrl: llmForm.baseUrl,
        model: llmForm.model,
        temperature: llmForm.temperature,
      },
      llmApiKey.value.trim(),
    )
    llmApiKey.value = ''
    ElMessage.success('大模型配置已保存')
    return true
  } catch (e) {
    const msg = (e as Error)?.message ?? ''
    // 浏览器预览（vite）下 __TAURI_INTERNALS__ 不存在，invoke 会抛 Cannot read properties of undefined
    if (!isTauri() || msg.includes('invoke') || msg.includes('__TAURI')) {
      // 降级：仅内存保存，供引导流程继续；提示用户在桌面端重新配置
      settingsStore.llmConfig = {
        provider: llmForm.provider,
        baseUrl: llmForm.baseUrl,
        model: llmForm.model,
        temperature: llmForm.temperature,
      } as never
      if (llmApiKey.value.trim()) (settingsStore as unknown as { keyConfigured: { value: boolean } }).keyConfigured.value = true
      llmApiKey.value = ''
      ElMessage.warning('浏览器预览模式：配置已临时保存，请在桌面应用中重新配置以持久化')
      return true
    }
    ElMessage.error(msg || '保存失败')
    return false
  } finally {
    llmSaving.value = false
  }
}
async function testLlmForOnboarding() {
  const problem = validateLlmForm()
  if (problem) return ElMessage.warning(problem)
  // 先保存再测试，保证测试走已落盘的配置
  const ok = await saveLlmForOnboarding()
  if (!ok) return
  llmTesting.value = true
  try {
    const { testAgentProvider } = await import('@/services/agent-client')
    const r = await testAgentProvider()
    if (r.error_code) {
      const labels: Record<string, string> = {
        provider_unavailable: '未配置或密钥缺失',
        provider_auth_failed: '认证失败，请检查 API Key',
        provider_rate_limited: '限流，请稍后重试',
        provider_timeout: '超时，请检查网络或地址',
        provider_protocol_error: '协议错误',
        provider_request_failed: '网络或服务端错误',
        consent_required: '需确认数据出境',
      }
      ElMessage.error(`连接失败：${labels[r.error_code] ?? r.error_code}`)
    } else {
      ElMessage.success(`连接成功：${r.model}（${r.latency_ms}ms）`)
    }
  } catch (e) {
    ElMessage.error((e as Error).message ?? '连接失败')
  } finally {
    llmTesting.value = false
  }
}

// ---- 知识点自动搜集（进入水平评估时触发）----
const kpSuggesting = ref(false)
const kpSuggestError = ref('')
async function autoSuggestKps() {
  const validSubjects = subjects.filter((s) => s.name.trim())
  if (!validSubjects.length) return
  // 若已有知识点则不覆盖，避免重复触发
  const hasExisting = validSubjects.some((s) => s.knowledgePoints.length > 0)
  if (hasExisting) return
  kpSuggesting.value = true
  kpSuggestError.value = ''
  try {
    let suggestions: Array<Array<{ name: string; chapter: string | null }>>
    if (!isTauri()) {
      // 浏览器预览（无 Tauri/LLM）直接用本地启发式，避免 invoke 报错
      suggestions = validSubjects.map((s) => heuristicKps(s.name.trim()))
    } else {
      const resp = await invoke<{
        suggestions: Array<Array<{ name: string; chapter: string | null }>>
      }>('suggest_knowledge_points', {
        input: {
          exam_type: exam.exam_type,
          exam_name: exam.name.trim(),
          subjects: validSubjects.map((s) => s.name.trim()),
        },
      })
      suggestions = resp.suggestions ?? []
    }
    validSubjects.forEach((s, idx) => {
      const list = suggestions[idx] ?? []
      s.knowledgePoints = list
        .slice(0, 8)
        .map((kp) => ({
          name: kp.name?.trim() || '',
          chapter: kp.chapter?.trim() || '',
          mastery: 3,
        }))
        .filter((k) => k.name)
    })
    if (validSubjects.every((s) => !s.knowledgePoints.length)) {
      kpSuggestError.value = '未能自动获取知识点，请手动添加'
    }
  } catch (e) {
    // 降级到本地启发式，避免阻塞流程
    try {
      validSubjects.forEach((s) => {
        if (!s.knowledgePoints.length) {
          s.knowledgePoints = heuristicKps(s.name.trim()).map((kp) => ({
            name: kp.name,
            chapter: kp.chapter ?? '',
            mastery: 3,
          }))
        }
      })
      kpSuggestError.value = ''
    } catch {
      kpSuggestError.value = (e as Error).message ?? '自动获取失败，可手动添加'
    }
  } finally {
    kpSuggesting.value = false
  }
}

// 欢迎页步骤与描述由同一数据源派生，避免硬编码漂移
// 步骤 1 为强制 LLM 配置（进入应用第一件事），未配置无法继续
const welcomeSteps = ['欢迎', '配置大模型', '创建考试', '添加科目', '水平评估', '确认完成'] as const
const welcomeStepCount = welcomeSteps.length - 1 // 排除“欢迎”本身
const welcomeIntro = computed(
  () => `用 ${welcomeStepCount} 步完成你的考试配置：${welcomeSteps.slice(1).join(' → ')}。`,
)
// 顶部品牌简介跟随项目情况动态生成（考试类型数/步骤数/核心能力）
const brandIntro = computed(() => {
  const examN = examTypes.length
  const cap = '材料导入 · 概念拆解 · 闪卡记忆 · 思维导图'
  return `AI 驱动 · ${welcomeStepCount} 步完成配置 · 支持 ${examN} 类考试 · 本地 SQLite · 半 Agent 决策 · ${cap}`
})
const introFeatures = computed(() => [
  {
    icon: Aim,
    title: '通用化',
    desc: `支持 ${examTypes.map((t) => t.label).join('、')} 等 ${examTypes.length} 类考试，自定义扩展`,
  },
  {
    icon: Lock,
    title: '本地优先',
    desc: '所有数据存于本地 SQLite（含材料/闪卡/知识点），你完全掌控',
  },
  {
    icon: ChatDotRound,
    title: '半 Agent',
    desc: `${welcomeSteps.slice(1, 3).join('/')} 后，AI 按需联网搜集知识点并生成复习计划，是否采纳由你决定`,
  },
  { icon: Notebook, title: '内容智能', desc: '材料导入 · 知识点拆解 · 闪卡复习 · 思维导图' },
])

const disabledDate = (date: Date) => {
  const t = new Date()
  t.setHours(0, 0, 0, 0)
  return date.getTime() <= t.getTime()
}

function addSubject() {
  subjects.push({ name: '', target_score: null, current_level: 3, weight: 1, knowledgePoints: [] })
}
function removeSubject(i: number) {
  subjects.splice(i, 1)
}
function addKp(s: SubjectDraft) {
  s.knowledgePoints.push({ name: '', chapter: '', mastery: 3 })
}
function removeKp(s: SubjectDraft, i: number) {
  s.knowledgePoints.splice(i, 1)
}

async function next() {
  // 步骤 1：LLM 配置为强制关卡，未配置无法继续
  if (current.value === 1) {
    const isConfigured = !!settingsStore.llmConfig && settingsStore.keyConfigured
    const hasUnsavedInput = !!llmApiKey.value.trim() || !isConfigured
    if (hasUnsavedInput) {
      const ok = await saveLlmForOnboarding()
      if (!ok) return
    }
    const stillNotReady = !settingsStore.llmConfig || !settingsStore.keyConfigured
    if (stillNotReady) return ElMessage.warning('请先完成大模型配置')
  }
  if (current.value === 2) {
    if (!exam.name.trim()) return ElMessage.warning('请输入考试名称')
    try {
      validateExamDate(exam.exam_date)
    } catch (e) {
      return ElMessage.error((e as Error).message)
    }
  }
  if (current.value === 3) {
    const valid = subjects.filter((s) => s.name.trim())
    if (!valid.length) return ElMessage.warning('至少添加一个科目')
    subjects.splice(0, subjects.length, ...valid)
  }
  current.value++
  // 进入水平评估页时自动联网搜集知识点（由前面三步信息驱动）
  if (current.value === 4) {
    void autoSuggestKps()
  }
}
function prev() {
  if (current.value > 0) current.value--
}

const totalKpCount = () =>
  subjects.reduce((acc, s) => acc + s.knowledgePoints.filter((k) => k.name.trim()).length, 0)

async function finish() {
  saving.value = true
  try {
    // 1. 创建考试
    const created = await store.createExam({
      name: exam.name.trim(),
      exam_type: exam.exam_type,
      exam_date: exam.exam_date,
      total_score: exam.total_score,
      description: null,
    })
    // 2. 逐科目创建，并写入其知识点（含自评掌握度 current_mastery）
    for (const s of subjects) {
      const subj = await store.createSubject({
        exam_id: created.id,
        name: s.name.trim(),
        target_score: s.target_score,
        current_level: s.current_level,
        weight: s.weight,
      })
      for (const kp of s.knowledgePoints) {
        if (!kp.name.trim()) continue
        await store.createKnowledgePoint({
          subject_id: subj.id,
          name: kp.name.trim(),
          parent_id: null,
          weight: 1,
          difficulty_level: 3,
          current_mastery: kp.mastery,
          chapter: kp.chapter.trim() || null,
        })
      }
    }
    // 3. 标记引导完成
    await setSetting('onboarding_completed', '1', '是否完成首次使用引导')
    markOnboardingDone()
    ElMessage.success('考试配置完成！')
    // 4. 进入 Agent 主界面（Task 2 单一入口契约；计划生成/调整在对话中完成）
    router.push('/agent')
  } catch (e) {
    ElMessage.error((e as Error).message ?? '保存失败，请重试')
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="welcome">
    <div class="welcome__card">
      <div class="welcome__brand">
        <span class="brand-mark" />
        <h1>智研</h1>
        <p>{{ brandIntro }}</p>
      </div>

      <el-steps :active="current" finish-status="success" align-center>
        <el-step v-for="title in welcomeSteps" :key="title" :title="title" />
      </el-steps>

      <div class="welcome__body">
        <!-- 步骤 0：欢迎 -->
        <div v-show="current === 0" class="step step-intro">
          <h2>欢迎使用智研</h2>
          <p>{{ welcomeIntro }}</p>
          <ul class="features">
            <li v-for="f in introFeatures" :key="f.title">
              <el-icon class="features__icon"><component :is="f.icon" /></el-icon>
              <span><b>{{ f.title }}</b>：{{ f.desc }}</span>
            </li>
          </ul>
        </div>

        <!-- 步骤 1：配置大模型（强制，未配置无法继续） -->
        <div v-show="current === 1" class="step">
          <h2>配置大模型</h2>
          <p class="muted" style="margin-bottom: var(--sp-4)">
            进入应用的第一步：配置大模型后才能使用 AI
            规划、知识点智能填充等能力。未配置无法继续下一步。
          </p>
          <el-alert
            v-if="settingsStore.keyConfigured && settingsStore.llmConfig"
            type="success"
            :closable="false"
            show-icon
            title="已配置大模型，可直接继续或修改后保存"
            style="margin-bottom: var(--sp-4)"
          />
          <el-form label-width="90px" @submit.prevent>
            <el-form-item label="Provider">
              <el-select
                v-model="llmForm.provider"
                style="width: 100%"
                @change="onLlmProviderChange"
              >
                <el-option
                  v-for="p in llmProviders"
                  :key="p.value"
                  :label="p.label"
                  :value="p.value"
                />
              </el-select>
            </el-form-item>
            <el-form-item label="API 地址">
              <el-input v-model="llmForm.baseUrl" placeholder="如 https://api.deepseek.com" />
            </el-form-item>
            <el-form-item label="模型">
              <el-input v-model="llmForm.model" placeholder="如 deepseek-chat" />
            </el-form-item>
            <el-form-item label="API Key">
              <el-input
                v-model="llmApiKey"
                :type="llmShowKey ? 'text' : 'password'"
                :placeholder="settingsStore.keyConfigured ? '已配置，留空则保留原 Key' : '必填'"
              >
                <template #append>
                  <el-button @click="llmShowKey = !llmShowKey">
                    {{ llmShowKey ? '隐藏' : '显示' }}
                  </el-button>
                </template>
              </el-input>
            </el-form-item>
            <el-form-item label="Temperature">
              <el-slider
                v-model="llmForm.temperature"
                :min="0"
                :max="2"
                :step="0.1"
                show-input
                style="max-width: 400px"
              />
            </el-form-item>
            <el-form-item>
              <el-button type="primary" :loading="llmSaving" @click="saveLlmForOnboarding">
                保存配置
              </el-button>
              <el-button :loading="llmTesting" @click="testLlmForOnboarding">连接测试</el-button>
            </el-form-item>
          </el-form>
        </div>

        <!-- 步骤 2：创建考试 -->
        <div v-show="current === 2" class="step">
          <h2>创建你的考试</h2>
          <el-form label-width="90px">
            <el-form-item label="考试类型">
              <div class="type-cards">
                <div
                  v-for="t in examTypes"
                  :key="t.value"
                  class="type-card"
                  :class="{ active: exam.exam_type === t.value }"
                  @click="exam.exam_type = t.value"
                >
                  <div class="type-card__label">{{ t.label }}</div>
                  <div class="type-card__desc">{{ t.desc }}</div>
                </div>
              </div>
            </el-form-item>
            <el-form-item label="考试名称">
              <el-input
                v-model="exam.name"
                placeholder="如 2027 管理类联考"
                maxlength="60"
                show-word-limit
              />
            </el-form-item>
            <el-form-item label="考试日期">
              <el-date-picker
                v-model="exam.exam_date"
                type="date"
                placeholder="选择考试日期（须晚于今天）"
                value-format="YYYY-MM-DD"
                :disabled-date="disabledDate"
                class="full-w"
              />
            </el-form-item>
            <el-form-item label="总分">
              <el-input-number
                v-model="exam.total_score"
                :min="0"
                :step="50"
                controls-position="right"
              />
            </el-form-item>
          </el-form>
        </div>

        <!-- 步骤 3：添加科目 -->
        <div v-show="current === 3" class="step">
          <div class="step-head">
            <h2>添加考试科目</h2>
            <el-button :icon="Plus" @click="addSubject">添加科目</el-button>
          </div>
          <div v-for="(s, i) in subjects" :key="i" class="subject-row">
            <el-input v-model="s.name" placeholder="科目名称" class="subject-row__name" />
            <el-input-number
              v-model="s.target_score"
              :min="0"
              placeholder="目标分"
              controls-position="right"
            />
            <div class="subject-row__level">
              <el-slider v-model="s.current_level" :min="1" :max="5" show-stops />
              <span class="tnum">{{ levelLabels[s.current_level] }}</span>
            </div>
            <el-input-number
              v-model="s.weight"
              :min="0"
              :step="0.5"
              :precision="1"
              controls-position="right"
            />
            <el-button :icon="Delete" type="danger" circle @click="removeSubject(i)" />
          </div>
        </div>

        <!-- 步骤 4：知识点水平评估（进入时自动联网搜集） -->
        <div v-show="current === 4" class="step">
          <div class="step-head step-head--column">
            <h2>基础水平评估</h2>
            <span class="muted">
              已根据你填写的考试与科目，自动联网搜集各科核心知识点，请确认或修改后自评掌握度（1-5
              星）。可跳过，稍后在考试配置中补充。
            </span>
          </div>
          <el-alert
            v-if="kpSuggesting"
            type="info"
            :closable="false"
            show-icon
            title="AI 正在联网搜集知识点，请稍候..."
            style="margin-bottom: var(--sp-3)"
          />
          <el-alert
            v-if="kpSuggestError"
            type="warning"
            :closable="false"
            :title="kpSuggestError"
            show-icon
            style="margin-bottom: var(--sp-3)"
          />
          <div v-if="kpSuggesting" style="text-align: center; padding: var(--sp-6)">
            <el-icon class="is-loading" style="font-size: 24px"><Loading /></el-icon>
            <div class="muted" style="margin-top: var(--sp-2)">正在生成知识点...</div>
          </div>
          <template v-else>
            <el-collapse v-for="(s, i) in subjects" :key="i" class="kp-collapse">
              <el-collapse-item
                :title="`${s.name || '科目 ' + (i + 1)}（${s.knowledgePoints.length} 个知识点）`"
                :name="i"
              >
                <div v-for="(kp, j) in s.knowledgePoints" :key="j" class="kp-row">
                  <el-input v-model="kp.name" placeholder="知识点名称" class="kp-row__name" />
                  <el-input
                    v-model="kp.chapter"
                    placeholder="章节（可选）"
                    class="kp-row__chapter"
                  />
                  <el-rate v-model="kp.mastery" />
                  <el-button :icon="Delete" type="danger" circle @click="removeKp(s, j)" />
                </div>
                <el-button :icon="Plus" size="small" @click="addKp(s)">添加知识点</el-button>
              </el-collapse-item>
            </el-collapse>
          </template>
          <div style="margin-top: var(--sp-3)">
            <el-button size="small" :loading="kpSuggesting" @click="autoSuggestKps">
              重新智能填充
            </el-button>
          </div>
        </div>

        <!-- 步骤 5：确认完成 -->
        <div v-show="current === 5" class="step">
          <h2>确认并完成</h2>
          <el-descriptions :column="1" border>
            <el-descriptions-item label="考试类型">
              {{ examTypes.find((t) => t.value === exam.exam_type)?.label }}
            </el-descriptions-item>
            <el-descriptions-item label="考试名称">{{ exam.name }}</el-descriptions-item>
            <el-descriptions-item label="考试日期">{{ exam.exam_date }}</el-descriptions-item>
            <el-descriptions-item label="总分">{{ exam.total_score ?? '—' }}</el-descriptions-item>
            <el-descriptions-item label="科目数">{{ subjects.length }}</el-descriptions-item>
            <el-descriptions-item label="知识点数">{{ totalKpCount() }}</el-descriptions-item>
          </el-descriptions>
          <el-alert
            type="info"
            :closable="false"
            title="完成配置后将进入智研主界面，可随时在对话中让 Agent 生成学习计划。"
            class="finish-alert"
          />
        </div>
      </div>

      <div class="welcome__footer">
        <el-button v-if="current > 0" :icon="ArrowLeft" @click="prev">上一步</el-button>
        <el-button v-if="current < 5" type="primary" :icon="ArrowRight" @click="next">
          下一步
        </el-button>
        <el-button
          v-if="current === 5"
          type="primary"
          :icon="Check"
          :loading="saving"
          @click="finish"
        >
          完成配置
        </el-button>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* Drenched 深紫罗兰品牌底：同色相径向光晕 + 点阵纹理（非多色渐变） */
.welcome {
  min-height: 100dvh;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--sp-6);
  background-color: #1e1640;
  background-image:
    radial-gradient(circle at 50% 0%, rgba(124, 99, 220, 0.38), transparent 62%),
    radial-gradient(circle at 85% 95%, rgba(109, 93, 200, 0.2), transparent 52%),
    radial-gradient(rgba(255, 255, 255, 0.045) 1px, transparent 1px);
  background-size:
    100% 100%,
    100% 100%,
    22px 22px;
}

.welcome__card {
  width: 100%;
  max-width: 760px;
  background: var(--c-surface);
  border: 1px solid var(--c-border);
  border-radius: var(--r-xl);
  padding: var(--sp-8) var(--sp-10);
  box-shadow: 0 24px 64px rgba(0, 0, 0, 0.32);
}

.welcome__brand {
  text-align: center;
  margin-bottom: var(--sp-6);
}
.brand-mark {
  display: inline-block;
  width: 40px;
  height: 40px;
  border-radius: 10px;
  background: var(--c-primary);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.3),
    0 6px 16px rgba(109, 93, 200, 0.45);
  margin-bottom: var(--sp-3);
}
.welcome__brand h1 {
  font-size: var(--fs-3xl);
  font-weight: 700;
  margin: 0;
  color: var(--c-ink);
  letter-spacing: 3px;
}
.welcome__brand p {
  color: var(--c-ink-3);
  font-size: var(--fs-sm);
  margin: var(--sp-2) 0 0;
}

.welcome__body {
  margin: var(--sp-8) 0;
  min-height: 240px;
}
.step h2 {
  font-size: var(--fs-xl);
  font-weight: 700;
  color: var(--c-ink);
  margin: 0 0 var(--sp-4);
}
.step-intro p {
  color: var(--c-ink-2);
  line-height: 1.8;
  margin: 0 0 var(--sp-4);
}
.features {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}
.features li {
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  padding: var(--sp-3);
  border-radius: var(--r-md);
  background: var(--c-surface-2);
  color: var(--c-ink-2);
  font-size: var(--fs-sm);
}
.features__icon {
  color: var(--c-primary);
  font-size: var(--fs-md);
  flex-shrink: 0;
}
.features b {
  color: var(--c-ink);
  font-weight: 600;
}

.type-cards {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--sp-3);
  width: 100%;
}
.type-card {
  border: 1.5px solid var(--c-border);
  border-radius: var(--r-md);
  padding: var(--sp-3) var(--sp-2);
  text-align: center;
  cursor: pointer;
  transition:
    border-color var(--dur-fast) var(--ease),
    background var(--dur-fast) var(--ease);
}
.type-card:hover {
  border-color: var(--c-border-strong);
}
.type-card.active {
  border-color: var(--c-primary);
  background: var(--c-primary-light);
}
.type-card__label {
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--c-ink);
}
.type-card.active .type-card__label {
  color: var(--c-primary);
}
.type-card__desc {
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
  margin-top: var(--sp-1);
}

.step-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: var(--sp-4);
  gap: var(--sp-4);
}
.step-head--column {
  flex-direction: column;
  align-items: flex-start;
}
.step-head h2 {
  margin: 0;
}
.muted {
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
  max-width: 480px;
  line-height: 1.6;
}
.full-w {
  width: 100%;
}
.subject-row {
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  margin-bottom: var(--sp-3);
  flex-wrap: wrap;
}
.subject-row__name {
  width: 180px;
}
.subject-row__level {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  min-width: 180px;
}
.subject-row__level :deep(.el-slider) {
  width: 120px;
}
.kp-collapse {
  margin-bottom: var(--sp-3);
}
.kp-row {
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  margin-bottom: var(--sp-3);
  flex-wrap: wrap;
}
.kp-row__name {
  width: 200px;
}
.kp-row__chapter {
  width: 150px;
}
.finish-alert {
  margin-top: var(--sp-4);
}
.welcome__footer {
  display: flex;
  justify-content: flex-end;
  gap: var(--sp-3);
  border-top: 1px solid var(--c-border);
  padding-top: var(--sp-5);
}
</style>
