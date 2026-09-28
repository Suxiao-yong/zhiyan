<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessage } from 'element-plus'
import { invoke } from '@tauri-apps/api/core'

function isTauri(): boolean {
  return (
    typeof window !== 'undefined' &&
    !!(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
  )
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
  VideoPlay,
  DataAnalysis,
  Connection,
  MagicStick,
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
  llmForm.provider = v
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
    if (!isTauri() || msg.includes('invoke') || msg.includes('__TAURI')) {
      settingsStore.llmConfig = {
        provider: llmForm.provider,
        baseUrl: llmForm.baseUrl,
        model: llmForm.model,
        temperature: llmForm.temperature,
      } as never
      if (llmApiKey.value.trim()) {
        ;(settingsStore as unknown as { keyConfigured: boolean }).keyConfigured = true as never
      }
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
  if (!isTauri()) return ElMessage.warning('浏览器预览不支持连接测试，请在桌面端测试')
  const problem = validateLlmForm()
  if (problem) return ElMessage.warning(problem)
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

// ---- 知识点自动搜集 ----
const kpSuggesting = ref(false)
const kpSuggestError = ref('')
async function autoSuggestKps(force = false) {
  const validSubjects = subjects.filter((s) => s.name.trim())
  if (!validSubjects.length) return
  if (kpSuggesting.value) return
  const targets = force
    ? validSubjects
    : validSubjects.filter((s) => s.knowledgePoints.length === 0)
  if (!targets.length) return
  kpSuggesting.value = true
  kpSuggestError.value = ''
  try {
    let suggestions: Array<Array<{ name: string; chapter: string | null }>>
    if (!isTauri()) {
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
    const targetIndexMap = new Map(targets.map((t) => [t, validSubjects.indexOf(t)]))
    targets.forEach((s) => {
      const idx = targetIndexMap.get(s)!
      const list = suggestions[idx] ?? heuristicKps(s.name.trim())
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

const welcomeSteps = ['欢迎', '配置大模型', '创建考试', '添加科目', '水平评估', '确认完成'] as const
const welcomeStepCount = welcomeSteps.length - 1
const welcomeIntro = computed(
  () => `用 ${welcomeStepCount} 步完成你的考试配置：${welcomeSteps.slice(1).join(' → ')}。`,
)
const brandIntro = computed(() => {
  const examN = examTypes.length
  const cap = '材料导入 · 概念拆解 · 闪卡记忆 · 思维导图'
  return `AI 驱动 · ${welcomeStepCount} 步完成配置 · 支持 ${examN} 类考试 · 本地 SQLite · AI 辅助决策 · ${cap}`
})
// 深度重构：AI 应用定位 — Agent 是内部可审计的实现，不是产品本身
// 智研是 AI 学习规划应用（package.json: AI-driven personalized study planner），
// “半 Agent 模式”是工程实现：AI 只通过 18 个工具与数据交互，写操作需你确认、可撤销
const introFeatures = computed(() => [
  {
    icon: Aim,
    title: '全科适配',
    desc: `一处搞定 ${examTypes.map((t) => t.label).join(' / ')}，自定义考试无需另起应用，开箱即用`,
    accent: '#6d5dc8',
  },
  {
    icon: Lock,
    title: '本地可信',
    desc: 'SQLite 单文件就在你电脑，材料/闪卡/知识点全归你。离线可用，无订阅，可迁移可审计',
    accent: '#2f8a57',
  },
  {
    icon: ChatDotRound,
    title: 'AI 规划',
    desc: `配好模型后，AI 按需搜集知识点、生成周计划，你一键确认才落库。AI 做重活，你做决策`,
    accent: '#4a7bb5',
  },
  {
    icon: Notebook,
    title: '学练记闭环',
    desc: '材料导入 → 知识点拆解 → 闪卡记忆 → 思维导图 → SM-2 间隔复习，从学到记一条链路',
    accent: '#b07d1f',
  },
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
  if (saving.value || kpSuggesting.value || llmSaving.value) return
  if (current.value === 1) {
    const isConfigured = !!settingsStore.llmConfig && settingsStore.keyConfigured
    const isDirty =
      !isConfigured ||
      llmApiKey.value.trim().length > 0 ||
      settingsStore.llmConfig?.provider !== llmForm.provider ||
      settingsStore.llmConfig?.baseUrl !== llmForm.baseUrl ||
      settingsStore.llmConfig?.model !== llmForm.model ||
      settingsStore.llmConfig?.temperature !== llmForm.temperature
    const hasUnsavedInput = isDirty
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
    const names = valid.map((s) => s.name.trim())
    if (new Set(names).size !== names.length) return ElMessage.warning('科目名不能重复')
    subjects.splice(0, subjects.length, ...valid)
  }
  current.value++
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
  if (!isTauri()) return ElMessage.warning('浏览器预览不支持持久化，请在桌面应用完成引导')
  saving.value = true
  try {
    const created = await store.createExam({
      name: exam.name.trim(),
      exam_type: exam.exam_type,
      exam_date: exam.exam_date,
      total_score: exam.total_score,
      description: null,
    })
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
    await setSetting('onboarding_completed', '1', '是否完成首次使用引导')
    markOnboardingDone()
    ElMessage.success('考试配置完成！')
    router.push('/agent')
  } catch (e) {
    ElMessage.error((e as Error).message ?? '保存失败，请重试')
  } finally {
    saving.value = false
  }
}

const progressPct = computed(() => ((current.value + 1) / welcomeSteps.length) * 100)
const isLlmReady = computed(() => !!settingsStore.llmConfig && settingsStore.keyConfigured)
</script>

<template>
  <div class="welcome-shell">
    <!-- 背景层：深紫罗兰 + 双光晕 + 点阵 + 细网格 -->
    <div class="welcome-bg" aria-hidden="true">
      <div class="welcome-bg__halo welcome-bg__halo--top" />
      <div class="welcome-bg__halo welcome-bg__halo--bottom" />
      <div class="welcome-bg__dots" />
      <div class="welcome-bg__vignette" />
    </div>

    <div class="welcome-frame">
      <!-- 顶部进度与品牌 -->
      <div class="welcome-top">
        <div class="brand">
          <div class="brand__mark" aria-hidden="true">
            <img :src="'/icon-new.png'" alt="" />
          </div>
          <div class="brand__text">
            <div class="brand__name">
              智研
              <span class="brand__dot">· Zhiyan</span>
            </div>
            <div class="brand__sub">{{ brandIntro }}</div>
          </div>
        </div>

        <div class="stepper-wrap">
          <div class="stepper">
            <template v-for="(title, idx) in welcomeSteps" :key="title">
              <div
                class="stepper__node"
                :class="{ active: idx === current, done: idx < current, todo: idx > current }"
              >
                <div class="stepper__dot">
                  <el-icon v-if="idx < current" :size="12"><Check /></el-icon>
                  <span v-else class="stepper__num">{{ idx === 0 ? '✓' : idx }}</span>
                </div>
                <span class="stepper__label">{{ title }}</span>
              </div>
              <div
                v-if="idx < welcomeSteps.length - 1"
                class="stepper__line"
                :class="{ filled: idx < current }"
              />
            </template>
          </div>
          <div class="progress-rail" aria-hidden="true">
            <div class="progress-rail__fill" :style="{ width: progressPct + '%' }" />
          </div>
        </div>
      </div>

      <!-- 主卡片 -->
      <div class="welcome-card">
        <!-- 装饰线 -->
        <div class="welcome-card__accent" />

        <div class="welcome-card__body">
          <!-- 步骤 0：欢迎 -->
          <div v-show="current === 0" class="step step-intro">
            <div class="intro-hero">
              <div class="intro-hero__left">
                <div class="eyebrow">
                  <el-icon :size="14"><MagicStick /></el-icon>
                  <span>Thinking Workspace · 为备考而生的工作台</span>
                </div>
                <h2 class="intro-title">欢迎使用智研</h2>
                <p class="intro-lead">
                  {{ welcomeIntro }} 一次配好，之后专注学习本身。本地存储、Agent
                  辅助决策、内容智能贯通。
                </p>
                <div class="intro-meta">
                  <span class="intro-meta__pill">
                    <el-icon><VideoPlay /></el-icon>
                    6 步 · 约 3 分钟
                  </span>
                  <span class="intro-meta__pill">
                    <el-icon><Lock /></el-icon>
                    本地 SQLite 加密
                  </span>
                  <span class="intro-meta__pill">
                    <el-icon><DataAnalysis /></el-icon>
                    材料 → 闪卡 → 计划
                  </span>
                </div>
              </div>
              <div class="intro-hero__right">
                <div class="mini-preview">
                  <div class="mini-preview__head">
                    <span class="mini-dot" />
                    <span class="mini-dot" />
                    <span class="mini-dot" />
                    <span class="mini-preview__title">今日学习 · 预览</span>
                  </div>
                  <div class="mini-preview__grid">
                    <div class="mini-card">
                      <div class="mini-card__k">待学</div>
                      <div class="mini-card__v">3</div>
                      <div class="mini-card__sub">计划任务</div>
                    </div>
                    <div class="mini-card">
                      <div class="mini-card__k">待复习</div>
                      <div class="mini-card__v">12</div>
                      <div class="mini-card__sub">闪卡 · 错题</div>
                    </div>
                    <div class="mini-card mini-card--wide">
                      <div class="mini-card__k">下一次考试</div>
                      <div class="mini-card__v mini-card__v--sm">{{ exam.exam_date || '—' }}</div>
                      <div class="mini-card__sub">
                        {{ exam.name || '未命名考试' }} · {{ subjects.length }} 科 ·
                        {{ totalKpCount() }} 知识点
                      </div>
                    </div>
                  </div>
                  <div class="mini-preview__foot">
                    <span class="mini-tag">
                      <el-icon><Connection /></el-icon>
                      Agent 半自动
                    </span>
                    <span class="mini-tag mini-tag--primary">本地优先</span>
                  </div>
                </div>
              </div>
            </div>

            <div class="feature-grid">
              <div v-for="f in introFeatures" :key="f.title" class="feature-card">
                <div class="feature-card__icon" :style="{ '--accent': f.accent } as any">
                  <el-icon :size="18"><component :is="f.icon" /></el-icon>
                </div>
                <div class="feature-card__body">
                  <div class="feature-card__title">{{ f.title }}</div>
                  <div class="feature-card__desc">{{ f.desc }}</div>
                </div>
              </div>
            </div>

            <div class="intro-note">
              <el-icon :size="14"><ChatDotRound /></el-icon>
              <span>
                下一步将强制配置大模型：这是 Agent 规划与知识点智能填充的前提。未配置无法继续。
              </span>
            </div>
          </div>

          <!-- 步骤 1：配置大模型 -->
          <div v-show="current === 1" class="step">
            <div class="step-header">
              <div>
                <h2 class="step-title">配置大模型</h2>
                <p class="step-desc">
                  进入应用的第一步。配置后才能使用 AI 规划、知识点智能填充与 Agent
                  对话。密钥仅存于系统凭据管理器，不落地明文。
                </p>
              </div>
              <div class="llm-status" :class="{ ready: isLlmReady }">
                <span class="llm-status__dot" />
                {{ isLlmReady ? '已就绪' : '未配置' }}
              </div>
            </div>

            <div v-if="isLlmReady" class="callout callout--success">
              <el-icon><Check /></el-icon>
              <span>已配置大模型，可直接继续或修改后保存覆盖。</span>
            </div>
            <div class="callout callout--muted">
              <el-icon><Lock /></el-icon>
              <span>
                API Key 加密存于 OS Keychain / Credential
                Manager；前端仅保存布尔态，不回显、不落盘。
              </span>
            </div>

            <div class="provider-grid">
              <button
                v-for="p in llmProviders"
                :key="p.value"
                type="button"
                class="provider-card"
                :class="{ active: llmForm.provider === p.value }"
                @click="onLlmProviderChange(p.value)"
              >
                <span class="provider-card__name">{{ p.label }}</span>
                <span class="provider-card__model">{{ p.model || '自定义模型' }}</span>
                <span v-if="llmForm.provider === p.value" class="provider-card__check">
                  <el-icon :size="12"><Check /></el-icon>
                </span>
              </button>
            </div>

            <el-form label-width="88px" class="llm-form" @submit.prevent>
              <el-form-item label="API 地址">
                <el-input v-model="llmForm.baseUrl" placeholder="https://api.deepseek.com" />
              </el-form-item>
              <el-form-item label="模型">
                <el-input
                  v-model="llmForm.model"
                  placeholder="deepseek-chat / gpt-4o / qwen-plus"
                />
              </el-form-item>
              <el-form-item label="API Key">
                <el-input
                  v-model="llmApiKey"
                  :type="llmShowKey ? 'text' : 'password'"
                  :placeholder="
                    isLlmReady ? '已配置，留空则保留原 Key' : '必填 · 仅存于系统凭据管理器'
                  "
                >
                  <template #append>
                    <el-button @click="llmShowKey = !llmShowKey">
                      {{ llmShowKey ? '隐藏' : '显示' }}
                    </el-button>
                  </template>
                </el-input>
              </el-form-item>
              <el-form-item label="Temperature">
                <div class="temp-row">
                  <el-slider
                    v-model="llmForm.temperature"
                    :min="0"
                    :max="2"
                    :step="0.1"
                    show-input
                    style="flex: 1; max-width: 420px"
                  />
                  <span class="temp-hint">0 更确定 · 2 更发散</span>
                </div>
              </el-form-item>
              <el-form-item>
                <div class="llm-actions">
                  <el-button type="primary" :loading="llmSaving" @click="saveLlmForOnboarding">
                    保存配置
                  </el-button>
                  <el-button :loading="llmTesting" @click="testLlmForOnboarding">
                    连接测试
                  </el-button>
                  <span class="muted muted--inline">保存后自动校验；测试失败不阻塞可稍后重试</span>
                </div>
              </el-form-item>
            </el-form>
          </div>

          <!-- 步骤 2：创建考试 -->
          <div v-show="current === 2" class="step">
            <div class="step-header">
              <div>
                <h2 class="step-title">创建你的考试</h2>
                <p class="step-desc">
                  选择考试类型，填写名称与日期。日期校验与总分仅为引导期轻校验，完整规则在考试配置页。
                </p>
              </div>
            </div>

            <div class="type-grid">
              <button
                v-for="t in examTypes"
                :key="t.value"
                type="button"
                class="type-card"
                :class="{ active: exam.exam_type === t.value }"
                @click="exam.exam_type = t.value"
              >
                <div class="type-card__top">
                  <span class="type-card__label">{{ t.label }}</span>
                  <span v-if="exam.exam_type === t.value" class="type-card__badge">
                    <el-icon :size="12"><Check /></el-icon>
                  </span>
                </div>
                <div class="type-card__desc">{{ t.desc }}</div>
              </button>
            </div>

            <el-form label-width="88px" class="form-stack">
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
                <div class="total-row">
                  <el-input-number
                    v-model="exam.total_score"
                    :min="0"
                    :step="50"
                    controls-position="right"
                  />
                  <span class="muted muted--inline">可选，用于目标分与达成度计算</span>
                </div>
              </el-form-item>
            </el-form>
          </div>

          <!-- 步骤 3：添加科目 -->
          <div v-show="current === 3" class="step">
            <div class="step-header step-header--row">
              <div>
                <h2 class="step-title">添加考试科目</h2>
                <p class="step-desc">
                  至少 1 科，科目名不可重复。目标分、水平与权重用于后续计划与薄弱分析。
                </p>
              </div>
              <el-button :icon="Plus" @click="addSubject">添加科目</el-button>
            </div>

            <div class="subject-list">
              <div v-for="(s, i) in subjects" :key="i" class="subject-card">
                <div class="subject-card__head">
                  <span class="subject-card__index">科目 {{ i + 1 }}</span>
                  <span class="subject-card__level">{{ levelLabels[s.current_level] }}</span>
                  <el-button
                    :icon="Delete"
                    type="danger"
                    circle
                    size="small"
                    :disabled="subjects.length === 1"
                    @click="removeSubject(i)"
                  />
                </div>
                <div class="subject-card__grid">
                  <el-input v-model="s.name" placeholder="科目名称，如 数学 / 英语" />
                  <el-input-number
                    v-model="s.target_score"
                    :min="0"
                    placeholder="目标分"
                    controls-position="right"
                    class="w-full"
                  />
                </div>
                <div class="subject-card__meta">
                  <div class="meta-field">
                    <span class="meta-label">当前水平 · {{ levelLabels[s.current_level] }}</span>
                    <el-slider v-model="s.current_level" :min="1" :max="5" show-stops />
                  </div>
                  <div class="meta-field meta-field--sm">
                    <span class="meta-label">权重</span>
                    <el-input-number
                      v-model="s.weight"
                      :min="0"
                      :step="0.5"
                      :precision="1"
                      controls-position="right"
                      class="w-full"
                    />
                  </div>
                </div>
              </div>
            </div>
          </div>

          <!-- 步骤 4：知识点水平评估 -->
          <div v-show="current === 4" class="step">
            <div class="step-header">
              <div>
                <h2 class="step-title">基础水平评估</h2>
                <p class="step-desc">
                  已根据考试与科目自动联网搜集核心知识点，请确认或修改后自评掌握度（1–5
                  星）。可跳过，稍后在考试配置中补充。
                </p>
              </div>
              <el-button size="small" :loading="kpSuggesting" @click="autoSuggestKps(true)">
                重新智能填充
              </el-button>
            </div>

            <div v-if="kpSuggesting" class="callout callout--info">
              <el-icon class="is-loading"><Loading /></el-icon>
              <span>AI 正在联网搜集知识点，请稍候…</span>
            </div>
            <div v-if="kpSuggestError" class="callout callout--warn">{{ kpSuggestError }}</div>

            <div v-if="kpSuggesting" class="kp-loading">
              <el-icon class="is-loading" :size="28"><Loading /></el-icon>
              <div class="muted">正在生成知识点…</div>
            </div>
            <template v-else>
              <div class="kp-panels">
                <div v-for="(s, i) in subjects" :key="i" class="kp-panel">
                  <div class="kp-panel__head">
                    <span class="kp-panel__title">{{ s.name || '科目 ' + (i + 1) }}</span>
                    <span class="kp-panel__count">{{ s.knowledgePoints.length }} 个知识点</span>
                  </div>
                  <div v-if="!s.knowledgePoints.length" class="kp-empty">
                    暂无知识点，点击下方添加或重新智能填充
                  </div>
                  <div v-else class="kp-rows">
                    <div v-for="(kp, j) in s.knowledgePoints" :key="j" class="kp-row">
                      <el-input v-model="kp.name" placeholder="知识点名称" class="kp-row__name" />
                      <el-input
                        v-model="kp.chapter"
                        placeholder="章节（可选）"
                        class="kp-row__chapter"
                      />
                      <el-rate v-model="kp.mastery" :max="5" />
                      <el-button
                        :icon="Delete"
                        type="danger"
                        circle
                        size="small"
                        @click="removeKp(s, j)"
                      />
                    </div>
                  </div>
                  <el-button :icon="Plus" size="small" class="kp-add" @click="addKp(s)">
                    添加知识点
                  </el-button>
                </div>
              </div>
            </template>
          </div>

          <!-- 步骤 5：确认完成 -->
          <div v-show="current === 5" class="step">
            <h2 class="step-title">确认并完成</h2>
            <p class="step-desc">
              检查将写入 SQLite 的数据概览，确认后进入智研主界面（Agent · 计划 ·
              记录）。引导完成后可在「考试配置」中继续细化。
            </p>

            <div class="confirm-grid">
              <div class="confirm-card">
                <div class="confirm-card__label">考试</div>
                <div class="confirm-card__value">{{ exam.name || '—' }}</div>
                <div class="confirm-card__sub">
                  {{ examTypes.find((t) => t.value === exam.exam_type)?.label }} ·
                  {{ exam.exam_date || '未选日期' }}
                </div>
              </div>
              <div class="confirm-card">
                <div class="confirm-card__label">总分 / 科目 / 知识点</div>
                <div class="confirm-card__value">
                  {{ exam.total_score ?? '—' }}
                  <span class="confirm-card__sep">/</span>
                  {{ subjects.length }} 科
                  <span class="confirm-card__sep">/</span>
                  {{ totalKpCount() }}
                </div>
                <div class="confirm-card__sub">权重与掌握度已一并记录</div>
              </div>
            </div>

            <el-descriptions :column="1" border class="confirm-table">
              <el-descriptions-item label="考试类型">
                {{ examTypes.find((t) => t.value === exam.exam_type)?.label }}
              </el-descriptions-item>
              <el-descriptions-item label="考试名称">{{ exam.name || '—' }}</el-descriptions-item>
              <el-descriptions-item label="考试日期">
                {{ exam.exam_date || '—' }}
              </el-descriptions-item>
              <el-descriptions-item label="总分">
                {{ exam.total_score ?? '—' }}
              </el-descriptions-item>
              <el-descriptions-item label="科目">
                {{ subjects.map((s) => s.name || '未命名').join('、') }}
              </el-descriptions-item>
            </el-descriptions>

            <div class="callout callout--info">
              <el-icon><ChatDotRound /></el-icon>
              <span>
                完成配置后将进入主界面，可随时在对话中让 Agent 生成学习计划、导入材料并生成闪卡。
              </span>
            </div>
          </div>
        </div>

        <!-- 底部操作区 -->
        <div class="welcome-card__footer">
          <div class="footer-hint">
            <span class="footer-hint__step">
              步骤 {{ current + 1 }} / {{ welcomeSteps.length }} · {{ welcomeSteps[current] }}
            </span>
            <span v-if="current === 1 && !isLlmReady" class="footer-hint__warn">
              需完成大模型配置才能继续
            </span>
          </div>
          <div class="footer-actions">
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
              完成配置 · 进入智研
            </el-button>
          </div>
        </div>
      </div>

      <div class="welcome-footnote">
        本地优先 · 密钥存于系统凭据管理器 · 数据不出境除非你显式授权 Agent 联网
      </div>
    </div>
  </div>
</template>

<style scoped>
/* ========== 外壳与背景 ========== */
.welcome-shell {
  min-height: 100dvh;
  display: flex;
  align-items: flex-start;
  justify-content: center;
  padding: 28px 20px 32px;
  position: relative;
  overflow-x: hidden;
  overflow-y: auto;
  background: #17133a;
}
.welcome-bg {
  position: absolute;
  inset: 0;
  background: #1b1640;
  overflow: hidden;
}
.welcome-bg__halo--top {
  position: absolute;
  inset: -20% -10% auto -10%;
  height: 68%;
  background:
    radial-gradient(ellipse 900px 520px at 50% 0%, rgba(124, 99, 220, 0.45), transparent 62%),
    radial-gradient(ellipse 700px 400px at 28% 18%, rgba(109, 93, 200, 0.22), transparent 60%);
}
.welcome-bg__halo--bottom {
  position: absolute;
  inset: auto -12% -18% -12%;
  height: 42%;
  background: radial-gradient(
    ellipse 820px 360px at 82% 100%,
    rgba(109, 93, 200, 0.18),
    transparent 58%
  );
}
.welcome-bg__dots {
  position: absolute;
  inset: 0;
  background-image: radial-gradient(rgba(255, 255, 255, 0.05) 1px, transparent 1px);
  background-size: 22px 22px;
  mask-image: radial-gradient(ellipse 120% 85% at 50% 18%, black 58%, transparent 92%);
}
.welcome-bg__vignette {
  position: absolute;
  inset: 0;
  background: radial-gradient(
    ellipse 140% 115% at 50% 50%,
    transparent 62%,
    rgba(0, 0, 0, 0.22) 100%
  );
}

.welcome-frame {
  position: relative;
  width: 100%;
  max-width: 980px;
}

/* ========== 顶部品牌 + 步进 ========== */
.welcome-top {
  display: flex;
  flex-direction: column;
  gap: 16px;
  margin-bottom: 18px;
}
.brand {
  display: flex;
  align-items: center;
  gap: 14px;
}
.brand__mark {
  width: 42px;
  height: 42px;
  border-radius: 12px;
  overflow: hidden;
  background: transparent;
  box-shadow:
    0 10px 24px rgba(109, 93, 200, 0.45),
    0 2px 8px rgba(0, 0, 0, 0.18);
  border: 1px solid rgba(255, 255, 255, 0.08);
  flex-shrink: 0;
}
.brand__mark img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.brand__name {
  font-size: 18px;
  font-weight: 800;
  color: #fff;
  letter-spacing: 2px;
  line-height: 1;
}
.brand__dot {
  font-weight: 500;
  color: rgba(255, 255, 255, 0.72);
  letter-spacing: 0.2px;
}
.brand__sub {
  margin-top: 6px;
  font-size: 12px;
  line-height: 1.5;
  color: rgba(255, 255, 255, 0.62);
  max-width: 760px;
}

.stepper-wrap {
  background: rgba(255, 255, 255, 0.92);
  border: 1px solid rgba(255, 255, 255, 0.18);
  border-radius: 14px;
  padding: 14px 14px 10px;
  backdrop-filter: blur(8px);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.16);
}
.stepper {
  display: flex;
  align-items: center;
  gap: 8px;
  overflow-x: auto;
  scrollbar-width: none;
}
.stepper::-webkit-scrollbar {
  display: none;
}
.stepper__node {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  flex-shrink: 0;
}
.stepper__dot {
  width: 26px;
  height: 26px;
  border-radius: 999px;
  display: grid;
  place-items: center;
  font-size: 12px;
  font-weight: 700;
  border: 1.5px solid var(--c-border);
  background: var(--c-surface);
  color: var(--c-ink-3);
}
.stepper__node.active .stepper__dot {
  background: var(--c-primary);
  border-color: var(--c-primary);
  color: #fff;
  box-shadow: 0 4px 12px rgba(109, 93, 200, 0.35);
}
.stepper__node.done .stepper__dot {
  background: var(--c-primary-light);
  border-color: var(--c-primary);
  color: var(--c-primary);
}
.stepper__label {
  font-size: 13px;
  font-weight: 600;
  color: var(--c-ink-3);
  white-space: nowrap;
}
.stepper__node.active .stepper__label {
  color: var(--c-ink);
}
.stepper__line {
  width: 28px;
  height: 2px;
  border-radius: 999px;
  background: var(--c-border);
  flex-shrink: 0;
}
.stepper__line.filled {
  background: var(--c-primary);
}
.progress-rail {
  margin-top: 12px;
  height: 4px;
  border-radius: 999px;
  background: var(--c-surface-3);
  overflow: hidden;
}
.progress-rail__fill {
  height: 100%;
  background: linear-gradient(90deg, #6d5dc8, #8b7fe0);
  border-radius: 999px;
  transition: width 280ms cubic-bezier(0.16, 1, 0.3, 1);
}

/* ========== 主卡片 ========== */
.welcome-card {
  background: var(--c-surface);
  border: 1px solid rgba(255, 255, 255, 0.9);
  border-radius: 18px;
  overflow: hidden;
  box-shadow:
    0 24px 64px rgba(0, 0, 0, 0.28),
    0 1px 0 rgba(255, 255, 255, 0.9) inset;
}
.welcome-card__accent {
  height: 3px;
  background: linear-gradient(90deg, #6d5dc8 0%, #9a8de0 45%, #2f8a57 100%);
  opacity: 0.95;
}
.welcome-card__body {
  padding: 26px 28px 18px;
  min-height: 360px;
}
.welcome-card__footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 16px 28px;
  border-top: 1px solid var(--c-border);
  background: color-mix(in srgb, var(--c-surface-2) 92%, white);
}

.step-title {
  font-size: 20px;
  font-weight: 800;
  letter-spacing: -0.02em;
  color: var(--c-ink);
  margin: 0 0 8px;
}
.step-desc {
  font-size: 13px;
  line-height: 1.6;
  color: var(--c-ink-3);
  margin: 0;
}
.step-header {
  display: flex;
  gap: 16px;
  justify-content: space-between;
  align-items: flex-start;
  margin-bottom: 18px;
}
.step-header--row {
  align-items: center;
}
.muted {
  font-size: 12px;
  color: var(--c-ink-3);
  line-height: 1.6;
}
.muted--inline {
  margin-left: 8px;
}
.full-w {
  width: 100%;
}
.w-full {
  width: 100%;
}
:deep(.el-input__wrapper),
:deep(.el-textarea__inner),
:deep(.el-input-number),
:deep(.el-date-editor) {
  border-radius: 10px;
}

/* ========== Intro ========== */
.intro-hero {
  display: grid;
  grid-template-columns: 1.25fr 0.9fr;
  gap: 18px;
  align-items: start;
  margin-bottom: 18px;
}
.eyebrow {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--c-primary);
  background: var(--c-primary-light);
  border: 1px solid color-mix(in srgb, var(--c-primary) 18%, transparent);
  padding: 6px 10px;
  border-radius: 999px;
}
.intro-title {
  margin: 12px 0 10px;
  font-size: 28px;
  font-weight: 900;
  letter-spacing: -0.03em;
  line-height: 1.1;
  color: var(--c-ink);
}
.intro-lead {
  margin: 0;
  font-size: 13.5px;
  line-height: 1.7;
  color: var(--c-ink-2);
}
.intro-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 14px;
}
.intro-meta__pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  font-weight: 600;
  color: var(--c-ink-2);
  background: var(--c-surface-2);
  border: 1px solid var(--c-border);
  padding: 6px 10px;
  border-radius: 999px;
}
.mini-preview {
  border: 1px solid var(--c-border);
  border-radius: 14px;
  overflow: hidden;
  background: linear-gradient(180deg, var(--c-surface) 0%, var(--c-surface-2) 100%);
  box-shadow: var(--shadow-sm);
}
.mini-preview__head {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 10px 12px;
  border-bottom: 1px solid var(--c-border);
  background: color-mix(in srgb, var(--c-surface-2) 85%, white);
}
.mini-dot {
  width: 8px;
  height: 8px;
  border-radius: 999px;
  background: var(--c-border-strong);
}
.mini-dot:nth-child(1) {
  background: #ff5f57;
}
.mini-dot:nth-child(2) {
  background: #ffbd2e;
}
.mini-dot:nth-child(3) {
  background: #28c840;
}
.mini-preview__title {
  margin-left: 8px;
  font-size: 12px;
  font-weight: 700;
  color: var(--c-ink-2);
}
.mini-preview__grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
  padding: 12px;
}
.mini-card {
  border: 1px solid var(--c-border);
  border-radius: 12px;
  padding: 12px;
  background: var(--c-surface);
}
.mini-card--wide {
  grid-column: 1 / -1;
}
.mini-card__k {
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--c-ink-3);
}
.mini-card__v {
  margin-top: 6px;
  font-size: 22px;
  font-weight: 900;
  letter-spacing: -0.02em;
  color: var(--c-ink);
}
.mini-card__v--sm {
  font-size: 16px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.mini-card__sub {
  margin-top: 4px;
  font-size: 11px;
  color: var(--c-ink-3);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.mini-preview__foot {
  display: flex;
  justify-content: space-between;
  padding: 10px 12px;
  border-top: 1px solid var(--c-border);
  background: var(--c-surface);
}
.mini-tag {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  font-weight: 700;
  color: var(--c-ink-2);
  background: var(--c-surface-2);
  border: 1px solid var(--c-border);
  padding: 4px 8px;
  border-radius: 999px;
}
.mini-tag--primary {
  background: var(--c-primary-light);
  border-color: color-mix(in srgb, var(--c-primary) 18%, transparent);
  color: var(--c-primary);
}

.feature-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
}
.feature-card {
  display: flex;
  gap: 12px;
  padding: 14px;
  border-radius: 14px;
  border: 1px solid var(--c-border);
  background: var(--c-surface-2);
}
.feature-card__icon {
  width: 38px;
  height: 38px;
  border-radius: 11px;
  display: grid;
  place-items: center;
  flex-shrink: 0;
  background: color-mix(in srgb, var(--accent) 14%, white);
  color: var(--accent);
  border: 1px solid color-mix(in srgb, var(--accent) 18%, transparent);
}
.feature-card__title {
  font-size: 13px;
  font-weight: 800;
  color: var(--c-ink);
}
.feature-card__desc {
  margin-top: 4px;
  font-size: 12px;
  line-height: 1.6;
  color: var(--c-ink-2);
}
.intro-note {
  display: flex;
  gap: 8px;
  align-items: flex-start;
  margin-top: 14px;
  padding: 12px 14px;
  border-radius: 12px;
  background: color-mix(in srgb, var(--c-primary) 8%, var(--c-surface-2));
  border: 1px dashed color-mix(in srgb, var(--c-primary) 18%, transparent);
  color: var(--c-ink-2);
  font-size: 12px;
  line-height: 1.6;
}

/* ========== 通用 callout ========== */
.callout {
  display: flex;
  gap: 10px;
  align-items: flex-start;
  padding: 12px 14px;
  border-radius: 12px;
  border: 1px solid var(--c-border);
  font-size: 12px;
  line-height: 1.6;
  margin-bottom: 14px;
}
.callout--success {
  background: var(--c-success-light);
  border-color: color-mix(in srgb, var(--c-success) 18%, transparent);
  color: var(--c-ink);
}
.callout--muted {
  background: var(--c-surface-2);
  color: var(--c-ink-2);
}
.callout--info {
  background: var(--c-info-light);
  border-color: color-mix(in srgb, var(--c-info) 18%, transparent);
  color: var(--c-ink);
}
.callout--warn {
  background: var(--c-warning-light);
  border-color: color-mix(in srgb, var(--c-warning) 18%, transparent);
  color: var(--c-ink);
}

/* ========== LLM provider 卡片 ========== */
.llm-status {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border-radius: 999px;
  font-size: 12px;
  font-weight: 800;
  border: 1px solid var(--c-border);
  background: var(--c-surface-2);
  color: var(--c-ink-3);
  white-space: nowrap;
}
.llm-status.ready {
  background: var(--c-success-light);
  border-color: color-mix(in srgb, var(--c-success) 18%, transparent);
  color: var(--c-success);
}
.llm-status__dot {
  width: 8px;
  height: 8px;
  border-radius: 999px;
  background: currentColor;
  box-shadow: 0 0 0 4px color-mix(in srgb, currentColor 18%, transparent);
}
.provider-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 10px;
  margin-bottom: 16px;
}
.provider-card {
  position: relative;
  text-align: left;
  padding: 12px 12px;
  border-radius: 12px;
  border: 1.5px solid var(--c-border);
  background: var(--c-surface);
  cursor: pointer;
  transition:
    border-color 150ms var(--ease),
    background 150ms var(--ease),
    transform 150ms var(--ease);
}
.provider-card:hover {
  border-color: var(--c-border-strong);
  transform: translateY(-1px);
}
.provider-card.active {
  border-color: var(--c-primary);
  background: var(--c-primary-light);
  box-shadow: 0 6px 16px rgba(109, 93, 200, 0.14);
}
.provider-card__name {
  display: block;
  font-size: 13px;
  font-weight: 800;
  color: var(--c-ink);
}
.provider-card.active .provider-card__name {
  color: var(--c-primary);
}
.provider-card__model {
  display: block;
  margin-top: 4px;
  font-size: 11px;
  color: var(--c-ink-3);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.provider-card__check {
  position: absolute;
  top: 10px;
  right: 10px;
  width: 18px;
  height: 18px;
  border-radius: 999px;
  display: grid;
  place-items: center;
  background: var(--c-primary);
  color: #fff;
}
.llm-form {
  margin-top: 4px;
}
.temp-row {
  display: flex;
  align-items: center;
  gap: 12px;
  width: 100%;
}
.temp-hint {
  font-size: 11px;
  color: var(--c-ink-3);
  white-space: nowrap;
}
.llm-actions {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}

/* ========== 考试类型 ========== */
.type-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 12px;
  margin-bottom: 18px;
}
.type-card {
  text-align: left;
  padding: 14px 14px;
  border-radius: 14px;
  border: 1.5px solid var(--c-border);
  background: var(--c-surface);
  cursor: pointer;
  transition:
    border-color 150ms var(--ease),
    background 150ms var(--ease),
    transform 150ms var(--ease);
}
.type-card:hover {
  border-color: var(--c-border-strong);
  transform: translateY(-1px);
}
.type-card.active {
  border-color: var(--c-primary);
  background: var(--c-primary-light);
  box-shadow: 0 8px 18px rgba(109, 93, 200, 0.14);
}
.type-card__top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.type-card__label {
  font-size: 14px;
  font-weight: 800;
  color: var(--c-ink);
}
.type-card.active .type-card__label {
  color: var(--c-primary);
}
.type-card__desc {
  margin-top: 6px;
  font-size: 11px;
  line-height: 1.5;
  color: var(--c-ink-3);
}
.type-card__badge {
  width: 18px;
  height: 18px;
  border-radius: 999px;
  display: grid;
  place-items: center;
  background: var(--c-primary);
  color: #fff;
  flex-shrink: 0;
}
.form-stack :deep(.el-form-item) {
  margin-bottom: 16px;
}
.total-row {
  display: flex;
  align-items: center;
  gap: 12px;
}

/* ========== 科目 ========== */
.subject-list {
  display: grid;
  gap: 14px;
}
.subject-card {
  border: 1px solid var(--c-border);
  border-radius: 14px;
  background: var(--c-surface-2);
  padding: 14px;
}
.subject-card__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  margin-bottom: 12px;
}
.subject-card__index {
  font-size: 12px;
  font-weight: 800;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--c-ink-2);
  background: var(--c-surface);
  border: 1px solid var(--c-border);
  padding: 4px 8px;
  border-radius: 999px;
}
.subject-card__level {
  margin-left: auto;
  font-size: 12px;
  font-weight: 700;
  color: var(--c-primary);
  background: var(--c-primary-light);
  border: 1px solid color-mix(in srgb, var(--c-primary) 16%, transparent);
  padding: 4px 10px;
  border-radius: 999px;
}
.subject-card__grid {
  display: grid;
  grid-template-columns: 1.5fr 0.9fr;
  gap: 10px;
}
.subject-card__meta {
  display: grid;
  grid-template-columns: 1.35fr 0.75fr;
  gap: 14px;
  margin-top: 12px;
}
.meta-label {
  display: block;
  font-size: 11px;
  font-weight: 700;
  color: var(--c-ink-3);
  margin-bottom: 8px;
}

/* ========== 知识点 ========== */
.kp-loading {
  text-align: center;
  padding: 28px 0;
}
.kp-panels {
  display: grid;
  gap: 14px;
}
.kp-panel {
  border: 1px solid var(--c-border);
  border-radius: 14px;
  background: var(--c-surface-2);
  padding: 14px;
}
.kp-panel__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  margin-bottom: 12px;
}
.kp-panel__title {
  font-size: 13px;
  font-weight: 800;
  color: var(--c-ink);
}
.kp-panel__count {
  font-size: 11px;
  font-weight: 700;
  color: var(--c-ink-3);
  background: var(--c-surface);
  border: 1px solid var(--c-border);
  padding: 4px 8px;
  border-radius: 999px;
}
.kp-empty {
  font-size: 12px;
  color: var(--c-ink-3);
  padding: 10px 0 6px;
}
.kp-rows {
  display: grid;
  gap: 10px;
}
.kp-row {
  display: grid;
  grid-template-columns: 1.4fr 0.9fr auto auto;
  gap: 10px;
  align-items: center;
}
.kp-row__name,
.kp-row__chapter {
  min-width: 0;
}
.kp-add {
  margin-top: 12px;
}

/* ========== 确认 ========== */
.confirm-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
  margin-bottom: 16px;
}
.confirm-card {
  border: 1px solid var(--c-border);
  border-radius: 14px;
  padding: 16px;
  background: linear-gradient(180deg, var(--c-surface) 0%, var(--c-surface-2) 100%);
}
.confirm-card__label {
  font-size: 11px;
  font-weight: 800;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--c-ink-3);
}
.confirm-card__value {
  margin-top: 8px;
  font-size: 16px;
  font-weight: 900;
  letter-spacing: -0.02em;
  color: var(--c-ink);
}
.confirm-card__sep {
  color: var(--c-border-strong);
  margin: 0 4px;
}
.confirm-card__sub {
  margin-top: 6px;
  font-size: 11px;
  color: var(--c-ink-3);
}
.confirm-table {
  margin-top: 6px;
}

/* ========== Footer ========== */
.footer-hint {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.footer-hint__step {
  font-size: 12px;
  font-weight: 700;
  color: var(--c-ink-2);
}
.footer-hint__warn {
  font-size: 11px;
  color: var(--c-warning);
  font-weight: 600;
}
.footer-actions {
  display: flex;
  gap: 10px;
  align-items: center;
}
.welcome-footnote {
  margin-top: 14px;
  text-align: center;
  font-size: 11px;
  color: rgba(255, 255, 255, 0.52);
}

/* ========== 响应式 ========== */
@media (max-width: 860px) {
  .intro-hero {
    grid-template-columns: 1fr;
  }
  .feature-grid,
  .confirm-grid {
    grid-template-columns: 1fr;
  }
  .provider-grid {
    grid-template-columns: 1fr 1fr;
  }
  .type-grid {
    grid-template-columns: 1fr 1fr;
  }
  .subject-card__grid,
  .subject-card__meta {
    grid-template-columns: 1fr;
  }
  .kp-row {
    grid-template-columns: 1fr;
  }
  .welcome-card__body {
    padding: 20px 16px 16px;
  }
  .welcome-card__footer {
    padding: 14px 16px;
    flex-direction: column;
    align-items: stretch;
  }
  .footer-actions {
    justify-content: flex-end;
  }
}
</style>
