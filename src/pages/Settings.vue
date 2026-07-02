<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useSettingsStore, obfuscate, deobfuscate } from '@/stores/settings'
import { useExamStore } from '@/stores/exam'
import { callLLM } from '@/services/llm-adapter'
import { getSetting, setSetting } from '@/services/db'
import * as exportSvc from '@/services/export'
import PageHeader from '@/components/common/PageHeader.vue'
import { Brush, Connection, Files, Search } from '@element-plus/icons-vue'

const settingsStore = useSettingsStore()
const examStore = useExamStore()
const busy = ref(false)

// ---- LLM 配置 ----
const form = reactive({
  provider: settingsStore.llmConfig?.provider ?? 'deepseek',
  baseUrl: settingsStore.llmConfig?.baseUrl ?? 'https://api.deepseek.com',
  model: settingsStore.llmConfig?.model ?? 'deepseek-chat',
  apiKey: settingsStore.llmConfig?.apiKey ?? '',
  temperature: settingsStore.llmConfig?.temperature ?? 0.7,
})
const showKey = ref(false)
const testing = ref(false)

// ---- anysearch 联网搜索（可选 key，匿名可用）----
const anysearchKey = ref('')
onMounted(async () => {
  // 1. 首选 keyring
  try {
    const k = await invoke<string | null>('load_api_key', { provider: 'anysearch' })
    if (k) {
      anysearchKey.value = k
      return
    }
  } catch {
    /* keyring 不可用，走 fallback */
  }
  // 2. 降级：SQLite fallback
  try {
    const fb = await getSetting('anysearch_api_key_fallback')
    if (fb) {
      const key = deobfuscate(fb)
      if (key) anysearchKey.value = key
    }
  } catch {
    /* ignore */
  }
})
async function saveAnysearch() {
  try {
    const key = anysearchKey.value
    if (key) {
      // 写 fallback（始终写，确保有备份）
      await setSetting('anysearch_api_key_fallback', obfuscate(key), 'AnySearch API Key fallback (obfuscated)')
      // 尝试 keyring
      try {
        await invoke('store_api_key', { provider: 'anysearch', key })
      } catch {
        console.warn('AnySearch Key 保存到凭据管理器失败（已存入 fallback）')
      }
    }
    ElMessage.success('AnySearch Key 已保存')
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

const providers = [
  { value: 'openai', label: 'OpenAI', baseUrl: 'https://api.openai.com', model: 'gpt-4o' },
  { value: 'deepseek', label: 'DeepSeek', baseUrl: 'https://api.deepseek.com', model: 'deepseek-chat' },
  { value: 'qwen', label: '通义千问', baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode', model: 'qwen-plus' },
  { value: 'kimi', label: 'Kimi', baseUrl: 'https://api.moonshot.cn', model: 'moonshot-v1-8k' },
  { value: 'ollama', label: 'Ollama（本地）', baseUrl: 'http://localhost:11434', model: 'llama3' },
  { value: 'custom', label: '自定义', baseUrl: '', model: '' },
]

function onProviderChange(v: string) {
  if (v === 'custom') return
  const p = providers.find((x) => x.value === v)
  if (p) {
    form.baseUrl = p.baseUrl
    form.model = p.model
  }
}

async function save() {
  try {
    await settingsStore.saveLlmConfig({ ...form })
    ElMessage.success('LLM 配置已保存（apiKey 经 OS 凭据管理器加密存储）')
  } catch (e) {
    ElMessage.error((e as Error).message ?? '保存失败')
  }
}

async function test() {
  if (form.provider !== 'ollama' && !form.apiKey)
    return ElMessage.warning('请先填写 API Key')
  settingsStore.llmConfig = { ...form }
  testing.value = true
  const t0 = Date.now()
  try {
    await callLLM(
      settingsStore.llmConfig!,
      [{ role: 'user', content: '请回复 OK' }],
      { timeoutMs: 15000 },
    )
    ElMessage.success(`连接成功（${Date.now() - t0}ms）`)
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
          <el-select v-model="form.provider" @change="onProviderChange" class="field-w">
            <el-option v-for="p in providers" :key="p.value" :label="p.label" :value="p.value" />
          </el-select>
        </el-form-item>
        <el-form-item label="API 地址">
          <el-input v-model="form.baseUrl" placeholder="如 https://api.deepseek.com" />
        </el-form-item>
        <el-form-item label="API Key">
          <el-input v-model="form.apiKey" :type="showKey ? 'text' : 'password'" placeholder="Ollama 无需 Key">
            <template #append>
              <el-button @click="showKey = !showKey">{{ showKey ? '隐藏' : '显示' }}</el-button>
            </template>
          </el-input>
          <span class="hint">经 OS 凭据管理器加密存储，不明文落盘</span>
        </el-form-item>
        <el-form-item label="模型">
          <el-input v-model="form.model" placeholder="如 deepseek-chat" />
        </el-form-item>
        <el-form-item label="Temperature">
          <el-slider v-model="form.temperature" :min="0" :max="2" :step="0.1" show-input class="slider-w" />
        </el-form-item>
        <el-form-item>
          <el-button type="primary" @click="save">保存配置</el-button>
          <el-button :loading="testing" @click="test">连接测试</el-button>
        </el-form-item>
      </el-form>
    </el-card>

    <el-card shadow="never" class="card">
      <template #header>
        <div class="card-head">
          <el-icon class="card-head__icon" :size="18"><Search /></el-icon>
          <span class="card-head__title">联网搜索（AnySearch）</span>
        </div>
      </template>
      <el-form label-width="100px" @submit.prevent>
        <el-form-item label="AnySearch Key">
          <el-input v-model="anysearchKey" placeholder="留空 = 匿名访问（低限额，够用）" />
          <span class="hint">AI 规划助手联网研究考试用；匿名可用，key 可选（更高限额）</span>
        </el-form-item>
        <el-form-item>
          <el-button @click="saveAnysearch">保存</el-button>
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
            @change="onThemeChange"
            active-text="暗色"
            inactive-text="亮色"
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
          <el-date-picker v-model="exportRange" type="daterange" value-format="YYYY-MM-DD" class="field-w-lg" />
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
  font-feature-settings: "tnum";
}
.hint {
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
  margin-left: var(--sp-2);
}
</style>
