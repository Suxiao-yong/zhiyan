// 系统设置 store。
// theme/notification 从 settings 表读写。
// LLM 配置：provider/baseUrl/model/temperature 存 settings 表（非敏感），apiKey 存 OS 凭据管理器（keyring/DPAPI，加密）。
// 降级策略：keyring 失败时，apiKey 以 XOR+base64 混淆后存入 settings 表（{provider}_api_key_fallback），
//   防止明文落盘，同时保证 keyring 不可用时用户不必每次重填。

import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getSetting, setSetting, deleteSetting } from '@/services/db'
import type { LLMConfig } from '@/types'

// ---- API Key 混淆工具（XOR + base64，防止 SQLite 明文暴露） ----

const SALT = 'zhiyan-app-salt-2024'

/** 混淆：XOR 后 base64 编码（非加密，仅防明文暴露） */
export function obfuscate(key: string): string {
  if (!key) return ''
  const xored = Array.from(key)
    .map((c, i) => String.fromCharCode(c.charCodeAt(0) ^ SALT.charCodeAt(i % SALT.length)))
    .join('')
  return btoa(unescape(encodeURIComponent(xored)))
}

/** 解混淆：base64 解码后 XOR 还原 */
export function deobfuscate(encoded: string): string {
  if (!encoded) return ''
  try {
    const xored = decodeURIComponent(escape(atob(encoded)))
    return Array.from(xored)
      .map((c, i) => String.fromCharCode(c.charCodeAt(0) ^ SALT.charCodeAt(i % SALT.length)))
      .join('')
  } catch {
    return ''
  }
}

/** settings 表中 fallback key 的键名 */
function fallbackKey(provider: string): string {
  return `${provider}_api_key_fallback`
}

/** 从 keyring 或 fallback 加载 apiKey */
async function loadApiKey(provider: string): Promise<{ key: string; fromKeyring: boolean }> {
  // 1. 首选 keyring
  try {
    const k = await invoke<string | null>('load_api_key', { provider })
    if (k) return { key: k, fromKeyring: true }
  } catch (e) {
    console.warn('从凭据管理器加载 apiKey 失败，尝试 fallback', e)
  }
  // 2. 降级：SQLite fallback
  try {
    const fb = await getSetting(fallbackKey(provider))
    if (fb) {
      const key = deobfuscate(fb)
      if (key) return { key, fromKeyring: false }
    }
  } catch (e) {
    console.warn('从 fallback 加载 apiKey 也失败', e)
  }
  return { key: '', fromKeyring: false }
}

/** 保存 apiKey：keyring（首选）+ SQLite fallback（兜底） */
async function saveApiKey(provider: string, key: string): Promise<void> {
  if (!key) return
  // 1. 写 fallback（始终写，确保有备份）
  await setSetting(fallbackKey(provider), obfuscate(key), 'API Key fallback (obfuscated)')
  // 2. 尝试 keyring
  try {
    await invoke('store_api_key', { provider, key })
  } catch (e) {
    console.warn('保存 apiKey 到凭据管理器失败（已存入 fallback）', e)
  }
}

export const useSettingsStore = defineStore('settings', () => {
  const theme = ref<'light' | 'dark'>('light')
  const reminderTime = ref<string | null>(null)
  const notificationEnabled = ref(true)
  const llmConfig = ref<LLMConfig | null>(null)

  async function loadSettings() {
    const t = await getSetting('theme')
    theme.value = t === 'dark' ? 'dark' : 'light'
    reminderTime.value = await getSetting('reminder_time')
    notificationEnabled.value = (await getSetting('notification_enabled')) !== 'false'
    await loadLlmConfig()
  }

  /** 从 settings 表加载非敏感配置 + 从 keyring/fallback 加载 apiKey */
  async function loadLlmConfig() {
    const provider = await getSetting('llm_provider')
    if (!provider) {
      llmConfig.value = null
      return
    }
    const baseUrl = (await getSetting('llm_base_url')) ?? ''
    const model = (await getSetting('llm_model')) ?? ''
    const temperature = Number(await getSetting('llm_temperature')) || 0.7
    const { key: apiKey } = await loadApiKey(provider)
    llmConfig.value = { provider, baseUrl, model, temperature, apiKey }
  }

  /** 保存 LLM 配置：非敏感入 settings 表，apiKey 入 keyring + fallback */
  async function saveLlmConfig(form: LLMConfig) {
    await setSetting('llm_provider', form.provider, 'LLM Provider')
    await setSetting('llm_base_url', form.baseUrl, 'LLM baseUrl')
    await setSetting('llm_model', form.model, 'LLM model')
    await setSetting('llm_temperature', String(form.temperature), 'LLM temperature')
    if (form.apiKey) {
      await saveApiKey(form.provider, form.apiKey)
    }
    llmConfig.value = { ...form }
  }

  async function clearLlmConfig() {
    if (llmConfig.value) {
      const provider = llmConfig.value.provider
      try {
        await invoke('delete_api_key', { provider })
      } catch {
        /* ignore */
      }
      // 同时清除 fallback
      try {
        await deleteSetting(fallbackKey(provider))
      } catch {
        /* ignore */
      }
    }
    llmConfig.value = null
  }

  async function setTheme(t: 'light' | 'dark') {
    theme.value = t
    await setSetting('theme', t)
  }
  async function setReminderTime(v: string | null) {
    reminderTime.value = v
    await setSetting('reminder_time', v ?? '')
  }
  async function setNotificationEnabled(v: boolean) {
    notificationEnabled.value = v
    await setSetting('notification_enabled', v ? 'true' : 'false')
  }

  return {
    theme,
    reminderTime,
    notificationEnabled,
    llmConfig,
    loadSettings,
    loadLlmConfig,
    saveLlmConfig,
    clearLlmConfig,
    setTheme,
    setReminderTime,
    setNotificationEnabled,
  }
})
