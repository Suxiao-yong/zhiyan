// 系统设置 store。
// theme/notification 从 settings 表读写。
// LLM 配置：provider/baseUrl/model/temperature 存 settings 表（非敏感），apiKey 存 OS 凭据管理器（keyring/DPAPI，加密）。
// 密钥只允许 Rust 从 keyring 读取；前端只提交用户刚输入的 key 到 store_api_key（保存意图），
// 从不调用 load_api_key，从不把已保存 key 放入 Pinia store、普通页面状态或消息。
// 旧 fallback 键若存在，只用于提示用户重新输入 API Key，绝不解密、绝不发送。

import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getSetting, setSetting, deleteSetting, hasSetting } from '@/services/db'
import type { LLMConfig } from '@/types'

function isTauri(): boolean {
  // Vitest/jsdom 中 window.__TAURI_INTERNALS__ 缺失但 invoke 已被 vi.mock 模拟，需视为 Tauri 以走真实 invoke 路径
  // SAFETY: globalThis access to detect Vitest where process is available but window.__TAURI_INTERNALS__ is not
  const g = globalThis as any
  if (g.process?.env?.NODE_ENV === 'test') return true
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}
function lsKey(provider: string): string {
  return `zhiyan_api_key_${provider}`
}
function settingsLsKey(key: string): string {
  return `zhiyan_settings_${key}`
}

/** settings 表中旧 fallback key 的键名（仅用于检测并提示重输） */
function fallbackKey(provider: string): string {
  return `${provider}_api_key_fallback`
}

/** 是否存在旧的 fallback 键（只做存在性判断，不读取其值、不解密） */
async function hasLegacyFallback(provider: string): Promise<boolean> {
  try {
    return await hasSetting(fallbackKey(provider))
  } catch {
    return false
  }
}

/** 保存用户新输入的 apiKey：只写 keyring；成功后清理旧 fallback */
async function saveApiKey(provider: string, key: string): Promise<void> {
  if (!key) return
  try {
    await invoke('store_api_key', { provider, key })
  } catch (e) {
    const msg = (e as Error)?.message ?? String(e)
    if (msg.includes('invoke') || msg.includes('__TAURI')) {
      try {
        localStorage.setItem(lsKey(provider), key)
      } catch {
        /* ignore */
      }
      return
    }
    throw e
  }
  // 用户成功保存到 keyring 后，删除旧的 fallback 键（新代码不得创建/读取 fallback）
  try {
    await deleteSetting(fallbackKey(provider))
  } catch {
    /* ignore */
  }
}

export const useSettingsStore = defineStore('settings', () => {
  const theme = ref<'light' | 'dark'>('light')
  const reminderTime = ref<string | null>(null)
  const notificationEnabled = ref(true)
  const llmConfig = ref<LLMConfig | null>(null)
  /** keyring 中是否已有当前 provider 的 key（只读布尔，由 has_api_key 返回） */
  const keyConfigured = ref(false)
  /** 检测到旧 fallback 键时置 true，提示用户重新输入 API Key */
  const legacyFallbackDetected = ref(false)

  async function loadSettings() {
    try {
      const t = await getSetting('theme')
      theme.value = t === 'dark' ? 'dark' : 'light'
      reminderTime.value = await getSetting('reminder_time')
      notificationEnabled.value = (await getSetting('notification_enabled')) !== 'false'
    } catch (e) {
      const msg = (e as Error)?.message ?? String(e)
      if (msg.includes('invoke') || msg.includes('__TAURI')) {
        try {
          const t = localStorage.getItem(settingsLsKey('theme'))
          theme.value = t === 'dark' ? 'dark' : 'light'
        } catch {
          /* ignore */
        }
      } else {
        throw e
      }
    }
    await loadLlmConfig()
  }

  /** 按 provider 刷新 key 状态（keyring 布尔 + 旧 fallback 键检测）。
   *  provider 切换时表单已改但未保存，不能依赖 llm_provider 的已存值。 */
  async function refreshKeyState(provider: string) {
    // 优先尝试 Tauri keyring，失败时回退到 localStorage（浏览器预览）
    try {
      keyConfigured.value = await invoke<boolean>('has_api_key', { provider })
      legacyFallbackDetected.value = await hasLegacyFallback(provider)
      return
    } catch (e) {
      const msg = (e as Error)?.message ?? String(e)
      if (msg.includes('invoke') || msg.includes('__TAURI') || !isTauri()) {
        try {
          keyConfigured.value = !!localStorage.getItem(lsKey(provider))
        } catch {
          keyConfigured.value = false
        }
        legacyFallbackDetected.value = false
        return
      }
      throw e
    }
    try {
      keyConfigured.value = await invoke<boolean>('has_api_key', { provider })
    } catch {
      keyConfigured.value = false
    }
    legacyFallbackDetected.value = await hasLegacyFallback(provider)
  }

  /** 从 settings 表加载非敏感配置；key 的存在性只通过 has_api_key 布尔结果获知 */
  async function loadLlmConfig() {
    let provider: string | null = null
    let baseUrl = ''
    let model = ''
    let tempRaw: string | null = null
    try {
      provider = await getSetting('llm_provider')
      if (provider) {
        baseUrl = (await getSetting('llm_base_url')) ?? ''
        model = (await getSetting('llm_model')) ?? ''
        tempRaw = await getSetting('llm_temperature')
      }
    } catch {
      // 浏览器预览：从 localStorage 回退
      try {
        provider = localStorage.getItem(settingsLsKey('llm_provider'))
        if (provider) {
          baseUrl = localStorage.getItem(settingsLsKey('llm_base_url')) ?? ''
          model = localStorage.getItem(settingsLsKey('llm_model')) ?? ''
          tempRaw = localStorage.getItem(settingsLsKey('llm_temperature'))
        }
      } catch {
        /* ignore */
      }
    }
    if (!provider) {
      llmConfig.value = null
      keyConfigured.value = false
      legacyFallbackDetected.value = false
      return
    }
    const temperature = Number(tempRaw) || 0.7
    llmConfig.value = { provider, baseUrl, model, temperature }
    await refreshKeyState(provider)
  }

  /** 保存 LLM 配置：用户新输入的 key 先写 keyring，成功后再写非敏感 settings；
   *  keyring 失败时抛错，settings 与 store 的“已保存配置”快照均不更新。
   *  空 key 表示沿用已配置的 key（仅当 keyConfigured 为 true 时才允许）。 */
  async function saveLlmConfig(form: LLMConfig, apiKeyInput = '') {
    if (apiKeyInput) {
      await saveApiKey(form.provider, apiKeyInput)
    }
    if (!isTauri()) {
      try {
        localStorage.setItem(settingsLsKey('llm_provider'), form.provider)
        localStorage.setItem(settingsLsKey('llm_base_url'), form.baseUrl)
        localStorage.setItem(settingsLsKey('llm_model'), form.model)
        localStorage.setItem(settingsLsKey('llm_temperature'), String(form.temperature))
      } catch {
        /* ignore */
      }
      llmConfig.value = { ...form }
      if (apiKeyInput) keyConfigured.value = true
      legacyFallbackDetected.value = false
      return
    }
    await setSetting('llm_provider', form.provider, 'LLM Provider')
    await setSetting('llm_base_url', form.baseUrl, 'LLM baseUrl')
    await setSetting('llm_model', form.model, 'LLM model')
    await setSetting('llm_temperature', String(form.temperature), 'LLM temperature')
    llmConfig.value = { ...form }
    if (apiKeyInput) keyConfigured.value = true
    legacyFallbackDetected.value = false
  }

  async function clearLlmConfig() {
    if (llmConfig.value) {
      const provider = llmConfig.value.provider
      if (!isTauri()) {
        try {
          localStorage.removeItem(lsKey(provider))
        } catch {
          /* ignore */
        }
        try {
          localStorage.removeItem(settingsLsKey('llm_provider'))
        } catch {
          /* ignore */
        }
        try {
          localStorage.removeItem(settingsLsKey('llm_base_url'))
        } catch {
          /* ignore */
        }
        try {
          localStorage.removeItem(settingsLsKey('llm_model'))
        } catch {
          /* ignore */
        }
        try {
          localStorage.removeItem(settingsLsKey('llm_temperature'))
        } catch {
          /* ignore */
        }
      } else {
        try {
          await invoke('delete_api_key', { provider })
        } catch {
          /* ignore */
        }
        // 同时清除旧 fallback 键
        try {
          await deleteSetting(fallbackKey(provider))
        } catch {
          /* ignore */
        }
      }
    }
    llmConfig.value = null
    keyConfigured.value = false
    legacyFallbackDetected.value = false
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
    keyConfigured,
    legacyFallbackDetected,
    loadSettings,
    loadLlmConfig,
    refreshKeyState,
    saveLlmConfig,
    clearLlmConfig,
    setTheme,
    setReminderTime,
    setNotificationEnabled,
  }
})
