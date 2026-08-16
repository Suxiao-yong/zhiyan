// @vitest-environment jsdom
/// <reference lib="es2015" />

import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import ElementPlus from 'element-plus'
/// <reference lib="es2015" />

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ElMessage } from 'element-plus'

const client = vi.hoisted(() => ({
  cloudConsentStatus: vi.fn(),
  confirmCloudConsent: vi.fn(),
  testAgentProvider: vi.fn(),
}))
const exportSvc = vi.hoisted(() => ({
  exportToFile: vi.fn(),
  importFromFile: vi.fn(),
  backupDatabase: vi.fn(),
  restoreDatabase: vi.fn(),
}))
const db = vi.hoisted(() => ({
  getSetting: vi.fn(),
  setSetting: vi.fn(),
  deleteSetting: vi.fn(),
  hasSetting: vi.fn(),
}))
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))

vi.mock('@/services/agent-client', () => client)
vi.mock('@/services/export', () => exportSvc)
vi.mock('@/services/db', () => db)
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

import Settings from './Settings.vue'
import { useSettingsStore } from '@/stores/settings'

function mountPage() {
  const pinia = createPinia()
  setActivePinia(pinia)
  client.cloudConsentStatus.mockResolvedValue({
    configured: false,
    consented: false,
    fingerprint: null,
  })
  client.testAgentProvider.mockResolvedValue({
    model: 'deepseek-chat',
    latency_ms: 320,
    text_stream: true,
    tool_call: true,
    error_code: null,
  })
  const wrapper = mount(Settings, {
    global: { plugins: [pinia, ElementPlus] },
  })
  return { wrapper, pinia }
}

const keyInput = (wrapper: ReturnType<typeof mount>) =>
  wrapper.find('input[placeholder*="留空则沿用"]')

describe('Settings (API key stays inside Rust)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    invoke.mockResolvedValue(undefined)
    db.getSetting.mockResolvedValue(null)
    db.setSetting.mockResolvedValue(undefined)
    db.deleteSetting.mockResolvedValue(undefined)
    db.hasSetting.mockResolvedValue(false)
  })

  it('sends a newly typed key only to store_api_key and keeps no trace in state', async () => {
    const { wrapper, pinia } = mountPage()
    await flushPromises()
    const store = useSettingsStore(pinia)
    // No config saved yet and no key in keyring: a key is required.
    store.keyConfigured = false

    await keyInput(wrapper).setValue('sk-page-input')
    await wrapper
      .findAll('button')
      .find((b) => b.text().includes('保存配置'))!
      .trigger('click')
    await flushPromises()

    // The only key-carrying invoke is the save intent.
    expect(invoke).toHaveBeenCalledWith('store_api_key', {
      provider: 'deepseek',
      key: 'sk-page-input',
    })
    expect(invoke).not.toHaveBeenCalledWith('load_api_key', expect.anything())
    // Non-sensitive settings were persisted.
    expect(db.setSetting).toHaveBeenCalledWith('llm_provider', 'deepseek', expect.anything())
    // The key never lands in the store, component props, or messages.
    expect(JSON.stringify(store.$state)).not.toContain('sk-page-input')
    expect(wrapper.html()).not.toContain('sk-page-input')
    // The input is cleared after a successful save.
    expect((keyInput(wrapper).element as HTMLInputElement).value).toBe('')
  })

  it('rejects saving with an empty key when none is configured', async () => {
    const warn = vi.spyOn(ElMessage, 'warning').mockImplementation(() => undefined as never)
    const { wrapper, pinia } = mountPage()
    await flushPromises()
    const store = useSettingsStore(pinia)
    store.keyConfigured = false

    await wrapper
      .findAll('button')
      .find((b) => b.text().includes('保存配置'))!
      .trigger('click')
    await flushPromises()

    expect(invoke).not.toHaveBeenCalledWith('store_api_key', expect.anything())
    expect(db.setSetting).not.toHaveBeenCalledWith('llm_provider', 'deepseek', expect.anything())
    expect(warn).toHaveBeenCalledWith('云端模型需要填写 API Key')
    warn.mockRestore()
  })

  it('allows saving with an empty key when a key is already configured', async () => {
    const { wrapper, pinia } = mountPage()
    await flushPromises()
    const store = useSettingsStore(pinia)
    store.llmConfig = {
      provider: 'deepseek',
      baseUrl: 'https://api.deepseek.com',
      model: 'deepseek-chat',
      temperature: 0.7,
    }
    store.keyConfigured = true
    // Let the sync-from-store watch settle into the form.
    await flushPromises()

    await wrapper
      .findAll('button')
      .find((b) => b.text().includes('保存配置'))!
      .trigger('click')
    await flushPromises()

    // Empty input = keep the configured key; only settings are rewritten.
    expect(invoke).not.toHaveBeenCalledWith('store_api_key', expect.anything())
    expect(db.setSetting).toHaveBeenCalledWith('llm_provider', 'deepseek', expect.anything())
    expect(store.keyConfigured).toBe(true)
  })

  it('connection test auto-saves the config before probing', async () => {
    const { wrapper } = mountPage()
    await flushPromises()
    // Nothing saved yet: clicking 连接测试 saves the form first (store_api_key
    // with the typed key + settings), then probes via the Rust command.
    await keyInput(wrapper).setValue('sk-auto-save')
    await wrapper
      .findAll('button')
      .find((b) => b.text().includes('连接测试'))!
      .trigger('click')
    await flushPromises()

    expect(invoke).toHaveBeenCalledWith('store_api_key', {
      provider: 'deepseek',
      key: 'sk-auto-save',
    })
    expect(db.setSetting).toHaveBeenCalledWith('llm_provider', 'deepseek', expect.anything())
    expect(client.testAgentProvider).toHaveBeenCalledTimes(1)
    expect(client.testAgentProvider).toHaveBeenCalledWith()
  })

  it('connection test saves a newly typed key before probing', async () => {
    const { wrapper, pinia } = mountPage()
    await flushPromises()
    const store = useSettingsStore(pinia)
    store.llmConfig = {
      provider: 'deepseek',
      baseUrl: 'https://api.deepseek.com',
      model: 'deepseek-chat',
      temperature: 0.7,
    }
    store.keyConfigured = true
    await flushPromises()

    // The user typed a new key but did not save it yet: the test saves it to
    // the keyring first, then probes the saved config.
    await keyInput(wrapper).setValue('sk-unsaved')
    await wrapper
      .findAll('button')
      .find((b) => b.text().includes('连接测试'))!
      .trigger('click')
    await flushPromises()

    expect(invoke).toHaveBeenCalledWith('store_api_key', {
      provider: 'deepseek',
      key: 'sk-unsaved',
    })
    expect(client.testAgentProvider).toHaveBeenCalledTimes(1)
    expect(client.testAgentProvider).toHaveBeenCalledWith()
  })

  it('runs the provider test without arguments once the config is saved', async () => {
    const { wrapper, pinia } = mountPage()
    await flushPromises()
    const store = useSettingsStore(pinia)
    store.llmConfig = {
      provider: 'deepseek',
      baseUrl: 'https://api.deepseek.com',
      model: 'deepseek-chat',
      temperature: 0.7,
    }
    store.keyConfigured = true
    await flushPromises()

    await wrapper
      .findAll('button')
      .find((b) => b.text().includes('连接测试'))!
      .trigger('click')
    await flushPromises()

    // Rust reads settings + keyring itself; no key or config crosses the IPC.
    expect(client.testAgentProvider).toHaveBeenCalledTimes(1)
    expect(client.testAgentProvider).toHaveBeenCalledWith()
  })
})
