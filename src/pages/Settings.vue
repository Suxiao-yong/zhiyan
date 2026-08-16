<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useSettingsStore } from '@/stores/settings'
import { useExamStore } from '@/stores/exam'
import {
  cloudConsentStatus as fetchCloudConsent,
  confirmCloudConsent,
  testAgentProvider,
  type CloudConsentStatus,
} from '@/services/agent-client'
import * as exportSvc from '@/services/export'
import PageHeader from '@/components/common/PageHeader.vue'
import { Brush, Files } from '@element-plus/icons-vue'

const settingsStore = useSettingsStore()
const examStore = useExamStore()
const busy = ref(false)

// ---- LLM 配置 ----
// 表单只保存非敏感配置；API Key 是页面内的临时输入（apiKeyInput），
// 只在“保存”时一次性提交给 Rust 的 store_api_key，保存成功后立即清空。
const form = reactive({
  provider: settingsStore.llmConfig?.provider ?? 'deepseek',
  baseUrl: settingsStore.llmConfig?.baseUrl ?? 'https://api.deepseek.com',
  model: settingsStore.llmConfig?.model ?? 'deepseek-chat',
  temperature: settingsStore.llmConfig?.temperature ?? 0.7,
})
const apiKeyInput = ref('')
const showKey = ref(false)

// App.vue 异步加载设置；store 就绪后把已保存的非敏感配置同步进表单，
// 不能把初始 undefined 当成用户要覆盖的配置。
function syncFormFromStore() {
  const config = settingsStore.llmConfig
  if (!config) return
  form.provider = config.provider
  form.baseUrl = config.baseUrl
  form.model = config.model
  form.temperature = config.temperature
}
watch(
  () => settingsStore.llmConfig,
  (config) => {
    if (config) syncFormFromStore()
  },
  { immediate: true },
)

const ollamaDetected = computed(() => settingsStore.llmConfig?.provider === 'ollama')

// ---- 数据出境同意（Mandatory Task A）----
const consent = ref<CloudConsentStatus | null>(null)
const consentBusy = ref(false)

async function loadConsent() {
  try {
    consent.value = await fetchCloudConsent()
  } catch {
    consent.value = null
  }
}

async function onConfirmConsent() {
  consentBusy.value = true
  try {
    await ElMessageBox.confirm(
      '确认后，以下数据将在你提问时发送给所选云端模型：\n' +
        '· 你当前的提问和最近 12 条同一会话消息\n' +
        '· 当前考试的摘要信息\n' +
        '· 当天最多 20 条学习计划\n' +
        '· 最近 14 天最多 20 条学习记录摘要\n' +
        '· 最多 10 条待复习错题/薄弱项摘要\n\n' +
        'API Key 始终只保存在本机凭据管理器中，不会发送给模型。' +
        '任何 provider、API 地址或模型变化后，需要重新确认。',
      '确认数据出境范围',
      { type: 'warning', confirmButtonText: '同意并确认', cancelButtonText: '取消' },
    )
    consent.value = await confirmCloudConsent()
    ElMessage.success('已确认数据出境范围')
  } catch (e) {
    if ((e as { code?: string })?.code !== 'cancel') {
      ElMessage.error((e as Error).message ?? '确认失败')
    }
  } finally {
    consentBusy.value = false
  }
}

const testing = ref(false)

onMounted(async () => {
  // 1. 数据出境同意状态
  await loadConsent()
})

const providers = [
  { value: 'openai', label: 'OpenAI', baseUrl: 'https://api.openai.com', model: 'gpt-4o' },
  {
    value: 'deepseek',
    label: 'DeepSeek',
    baseUrl: 'https://api.deepseek.com',
    model: 'deepseek-chat',
  },
  {
    value: 'qwen',
    label: '通义千问',
    baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode',
    model: 'qwen-plus',
  },
  { value: 'kimi', label: 'Kimi', baseUrl: 'https://api.moonshot.cn', model: 'moonshot-v1-8k' },
  { value: 'custom', label: '自定义', baseUrl: '', model: '' },
]

function onProviderChange(v: string) {
  if (v === 'custom') return
  const p = providers.find((x) => x.value === v)
  if (p) {
    form.baseUrl = p.baseUrl
    form.model = p.model
  }
  // keyConfigured 反映的是旧 provider；立即按新 provider 刷新，
  // 避免 validateForm 把未配 key 的新 provider 误判为已配置。
  void settingsStore.refreshKeyState(v)
}

function validateForm(): string | null {
  const url = form.baseUrl.trim()
  if (!/^https?:\/\/.+/i.test(url)) return 'API 地址必须是完整的 http/https 地址'
  if (!form.model.trim()) return '模型名不能为空'
  if (form.temperature < 0 || form.temperature > 2) return 'Temperature 必须在 0 到 2 之间'
  if (!settingsStore.keyConfigured && !apiKeyInput.value.trim()) {
    return '云端模型需要填写 API Key'
  }
  return null
}

async function save() {
  const problem = validateForm()
  if (problem) return ElMessage.warning(problem)
  try {
    // 用户刚输入的 key 一次性提交给 Rust；keyring 失败则整体不保存。
    await settingsStore.saveLlmConfig(
      {
        provider: form.provider,
        baseUrl: form.baseUrl,
        model: form.model,
        temperature: form.temperature,
      },
      apiKeyInput.value.trim(),
    )
    apiKeyInput.value = ''
    await loadConsent()
    ElMessage.success('LLM 配置已保存（apiKey 经 OS 凭据管理器加密存储）')
  } catch (e) {
    ElMessage.error((e as Error).message ?? '保存失败')
  }
}

/** 表单或 key 尚未保存时先保存（keyring 失败则中止）；返回是否已就绪。
 *  已与保存配置一致且无未保存 key 时直接返回 true（测试按钮用，不重写 settings）。 */
async function ensureSaved(): Promise<boolean> {
  if (formMatchesSaved() && !apiKeyInput.value.trim()) return true
  try {
    await settingsStore.saveLlmConfig(
      {
        provider: form.provider,
        baseUrl: form.baseUrl,
        model: form.model,
        temperature: form.temperature,
      },
      apiKeyInput.value.trim(),
    )
    apiKeyInput.value = ''
    return true
  } catch (e) {
    ElMessage.error((e as Error).message ?? '保存失败')
    return false
  }
}

/** 表单是否与最后保存的非敏感配置一致（连接测试的前置条件） */
function formMatchesSaved(): boolean {
  const saved = settingsStore.llmConfig
  if (!saved) return false
  return (
    saved.provider === form.provider &&
    saved.baseUrl === form.baseUrl &&
    saved.model === form.model &&
    saved.temperature === form.temperature
  )
}

async function test() {
  if (form.provider === 'ollama') return ElMessage.warning('本地模型暂不支持 Agent 工具')
  const problem = validateForm()
  if (problem) return ElMessage.warning(problem)
  // 测试基于“已保存”的配置：未保存的改动（含刚输入但未保存的 key）会先
  // 自动保存，测试结果由 Rust 从 settings + keyring 读取，key 绝不进入前端
  // 测试路径；keyring 保存失败则中止，不发送任何请求。
  if (!(await ensureSaved())) return
  testing.value = true
  try {
    const result = await testAgentProvider()
    if (result.error_code) {
      const labels: Record<string, string> = {
        provider_unavailable: '未配置云端模型或密钥缺失',
        provider_auth_failed: '认证失败（401/403），请检查 API Key',
        provider_rate_limited: '请求被限流（429），请稍后重试',
        provider_timeout: '连接超时，请检查网络或 baseUrl',
        provider_protocol_error: '响应格式不兼容（协议错误）',
        provider_request_failed: '网络或服务端错误：请检查 API 地址与模型名是否正确',
        consent_required: '数据出境同意未确认',
      }
      ElMessage.error(`连接失败：${labels[result.error_code] ?? result.error_code}`)
      return
    }
    ElMessage.success(
      `连接成功：${result.model}（流式 ${result.text_stream ? '✓' : '✗'}，` +
        `工具调用 ${result.tool_call ? '✓' : '✗'}，${result.latency_ms}ms）`,
    )
  } catch (e) {
    ElMessage.error((e as Error).message ?? '连接失败')
  } finally {
    testing.value = false
  }
}

// ---- 主题 ----
async function onThemeChange(v: boolean) {
  await settingsStore.setTheme(v ? 'dark' : 'light')
}

// ---- 导入导出 ----
const exportScope = ref<'all' | 'exam' | 'date'>('all')
const exportExamId = ref('')
const exportRange = ref<string[]>(['', ''])
const importMode = ref<'skip' | 'overwrite' | 'merge'>('skip')

async function onExport() {
  busy.value = true
  try {
    const range: any = { scope: exportScope.value }
    if (exportScope.value === 'exam') range.examId = exportExamId.value || undefined
    if (exportScope.value === 'date') {
      range.from = exportRange.value[0] || undefined
      range.to = exportRange.value[1] || undefined
    }
    const p = await exportSvc.exportToFile(range)
    if (p) ElMessage.success('已导出到：' + p)
  } catch (e) {
    ElMessage.error('导出失败：' + (e as Error).message)
  } finally {
    busy.value = false
  }
}

async function onImport() {
  busy.value = true
  try {
    const r = await exportSvc.importFromFile(importMode.value)
    if (!r) return
    ElMessage.success(`导入完成：成功 ${r.ok}，跳过 ${r.skipped}，失败 ${r.failed}`)
    if (r.errors.length) {
      ElMessageBox.alert(r.errors.slice(0, 20).join('\n'), '部分失败原因', {
        confirmButtonText: '知道了',
      })
    }
  } catch (e) {
    ElMessage.error((e as Error).message ?? '导入失败')
  } finally {
    busy.value = false
  }
}

async function onBackup() {
  busy.value = true
  try {
    const p = await exportSvc.backupDatabase()
    if (p) ElMessage.success('已备份到：' + p)
  } catch (e) {
    ElMessage.error('备份失败：' + (e as Error).message)
  } finally {
    busy.value = false
  }
}

async function onRestore() {
  try {
    await ElMessageBox.confirm(
      '恢复将覆盖当前所有数据，应用会重启。强烈建议先备份。确认继续？',
      '恢复数据库',
      { type: 'warning', confirmButtonText: '恢复并重启', cancelButtonText: '取消' },
    )
    await exportSvc.restoreDatabase()
  } catch {
    /* canceled */
  }
}
</script>

<template>
  <div v-loading="busy" element-loading-text="正在处理数据，请稍候…">
    <PageHeader title="系统设置" subtitle="LLM 配置 / 主题 / 数据导入导出" />

    <el-card shadow="never" class="card">
      <template #header>
        <div class="card-head">
          <el-icon class="card-head__icon" :size="18"><Connection /></el-icon>
          <span class="card-head__title">大模型 API 配置</span>
        </div>
      </template>
      <el-form label-width="100px" @submit.prevent>
        <el-form-item label="Provider">
          <el-select v-model="form.provider" class="field-w" @change="onProviderChange">
            <el-option v-for="p in providers" :key="p.value" :label="p.label" :value="p.value" />
          </el-select>
        </el-form-item>
        <el-form-item label="API 地址">
          <el-input v-model="form.baseUrl" placeholder="如 https://api.deepseek.com" />
        </el-form-item>
        <el-form-item label="API Key">
          <el-input
            v-model="apiKeyInput"
            :type="showKey ? 'text' : 'password'"
            placeholder="留空则沿用已配置的 Key；未配置时必填"
          >
            <template #append>
              <el-button @click="showKey = !showKey">{{ showKey ? '隐藏' : '显示' }}</el-button>
            </template>
          </el-input>
          <span class="hint">经 OS 凭据管理器加密存储，不明文落盘</span>
          <el-tag v-if="settingsStore.keyConfigured" type="success" size="small" class="key-state">
            已配置
          </el-tag>
          <el-tag v-else type="warning" size="small" class="key-state">需要重新输入</el-tag>
        </el-form-item>
        <el-form-item v-if="ollamaDetected" label="本地模型">
          <el-alert
            type="info"
            :closable="false"
            show-icon
            title="当前保存的是本地 Ollama 配置，本地模型暂不支持 Agent 工具。请切换到云端 provider 以使用 Agent。"
          />
        </el-form-item>
        <el-form-item label="模型">
          <el-input v-model="form.model" placeholder="如 deepseek-chat" />
        </el-form-item>
        <el-form-item label="Temperature">
          <el-slider
            v-model="form.temperature"
            :min="0"
            :max="2"
            :step="0.1"
            show-input
            class="slider-w"
          />
        </el-form-item>
        <el-form-item>
          <el-button type="primary" @click="save">保存配置</el-button>
          <el-button :loading="testing" @click="test">连接测试</el-button>
        </el-form-item>
        <el-form-item v-if="settingsStore.legacyFallbackDetected" label="API Key">
          <el-alert
            type="warning"
            :closable="false"
            show-icon
            title="检测到旧版本地保存的 API Key，出于安全考虑已不再读取。请重新输入并保存。"
          />
        </el-form-item>
        <el-form-item label="数据出境">
          <div class="consent-box">
            <template v-if="!consent || !consent.configured">
              <span class="hint">未配置云端模型，无需数据出境。</span>
            </template>
            <template v-else-if="consent.consented">
              <el-tag type="success" size="small">已确认数据范围</el-tag>
              <span class="hint">当前 provider / API 地址 / 模型组合已获你确认</span>
            </template>
            <template v-else>
              <el-tag type="warning" size="small">需要确认数据出境范围</el-tag>
              <span class="hint">
                发送给云端模型的数据包括：你的提问、最近会话消息、考试摘要、当日计划、近期记录和错题摘要
              </span>
              <el-button
                size="small"
                type="primary"
                :loading="consentBusy"
                @click="onConfirmConsent"
              >
                查看并确认
              </el-button>
            </template>
          </div>
        </el-form-item>
      </el-form>
    </el-card>

    <el-card shadow="never" class="card">
      <template #header>
        <div class="card-head">
          <el-icon class="card-head__icon" :size="18"><Brush /></el-icon>
          <span class="card-head__title">主题</span>
        </div>
      </template>
      <el-form label-width="100px">
        <el-form-item label="暗色模式">
          <el-switch
            :model-value="settingsStore.theme === 'dark'"
            active-text="暗色"
            inactive-text="亮色"
            @change="onThemeChange"
          />
        </el-form-item>
      </el-form>
    </el-card>

    <el-card shadow="never" class="card">
      <template #header>
        <div class="card-head">
          <el-icon class="card-head__icon" :size="18"><Files /></el-icon>
          <span class="card-head__title">数据导入导出 / 备份恢复</span>
        </div>
      </template>
      <el-form label-width="100px">
        <el-divider content-position="left">导出</el-divider>
        <el-form-item label="导出范围">
          <el-radio-group v-model="exportScope">
            <el-radio-button value="all">全部</el-radio-button>
            <el-radio-button value="exam">指定考试</el-radio-button>
            <el-radio-button value="date">日期范围</el-radio-button>
          </el-radio-group>
        </el-form-item>
        <el-form-item v-if="exportScope === 'exam'" label="考试">
          <el-select v-model="exportExamId" placeholder="选择考试" class="field-w-md">
            <el-option v-for="e in examStore.exams" :key="e.id" :label="e.name" :value="e.id" />
          </el-select>
        </el-form-item>
        <el-form-item v-if="exportScope === 'date'" label="日期范围">
          <el-date-picker
            v-model="exportRange"
            type="daterange"
            value-format="YYYY-MM-DD"
            class="field-w-lg"
          />
        </el-form-item>
        <el-form-item>
          <el-button type="primary" @click="onExport">导出 JSON</el-button>
        </el-form-item>

        <el-divider content-position="left">导入</el-divider>
        <el-form-item label="冲突处理">
          <el-radio-group v-model="importMode">
            <el-radio value="skip">跳过已存在</el-radio>
            <el-radio value="overwrite">覆盖</el-radio>
            <el-radio value="merge">合并（仅填空字段）</el-radio>
          </el-radio-group>
        </el-form-item>
        <el-form-item>
          <el-button type="primary" @click="onImport">从 JSON 导入</el-button>
          <span class="hint">导入前会校验结构，非法整批拒绝</span>
        </el-form-item>

        <el-divider content-position="left">数据库备份/恢复</el-divider>
        <el-form-item>
          <el-button @click="onBackup">备份数据库（.db）</el-button>
          <el-button type="danger" @click="onRestore">恢复数据库（覆盖+重启）</el-button>
        </el-form-item>
      </el-form>
    </el-card>
  </div>
</template>

<style scoped>
.card {
  margin-bottom: var(--sp-4);
}
.card :deep(.el-card__header) {
  padding: var(--sp-4) var(--sp-5);
  border-bottom: 1px solid var(--c-border);
}
.card :deep(.el-card__body) {
  padding: var(--sp-5);
}
.card-head {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
}
.card-head__icon {
  color: var(--c-primary);
}
.card-head__title {
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--c-ink);
}
.field-hint {
  font-size: 12px;
  color: var(--c-gray);
  margin-left: 12px;
}
.field-w {
  width: 240px;
}
.field-w-md {
  width: 280px;
}
.field-w-lg {
  width: 320px;
}
.slider-w {
  width: 100%;
  max-width: 400px;
}
.slider-w :deep(.el-input-number .el-input__inner) {
  font-variant-numeric: tabular-nums;
  font-feature-settings: 'tnum';
}
.hint {
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
  margin-left: var(--sp-2);
}
.key-state {
  margin-left: var(--sp-2);
}
.consent-box {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  flex-wrap: wrap;
}
</style>
