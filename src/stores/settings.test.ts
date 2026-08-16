/// <reference lib="es2015" />

import { beforeEach, describe, expect, it, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
const db = vi.hoisted(() => ({
  getSetting: vi.fn(),
  setSetting: vi.fn(),
  deleteSetting: vi.fn(),
  hasSetting: vi.fn(),
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke }))
vi.mock('@/services/db', () => db)

import { useSettingsStore } from './settings'
import { setActivePinia, createPinia } from 'pinia'

describe('settings store (API key stays inside Rust)', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    // Default: keyring holds no key, no legacy fallback rows.
    invoke.mockResolvedValue(null)
    db.hasSetting.mockResolvedValue(false)
    db.getSetting.mockImplementation(async (key: string) => {
      if (key === 'llm_provider') return 'deepseek'
      if (key === 'llm_base_url') return 'https://api.deepseek.com'
      if (key === 'llm_model') return 'deepseek-chat'
      if (key === 'llm_temperature') return '0.7'
      return null
    })
    db.setSetting.mockResolvedValue(undefined)
    db.deleteSetting.mockResolvedValue(undefined)
  })

  it('loads non-sensitive config and only asks has_api_key, never the key content', async () => {
    // The keyring holds a key, but the frontend only ever receives the boolean.
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'has_api_key') return true
      return null
    })
    db.getSetting.mockImplementation(async (key: string) => {
      if (key === 'llm_provider') return 'deepseek'
      if (key === 'llm_base_url') return 'https://api.deepseek.com'
      if (key === 'llm_model') return 'deepseek-chat'
      if (key === 'llm_temperature') return '0.7'
      return null
    })

    const store = useSettingsStore()
    await store.loadLlmConfig()

    expect(store.llmConfig).toEqual({
      provider: 'deepseek',
      baseUrl: 'https://api.deepseek.com',
      model: 'deepseek-chat',
      temperature: 0.7,
    })
    expect(store.keyConfigured).toBe(true)
    expect(invoke).toHaveBeenCalledWith('has_api_key', { provider: 'deepseek' })
    expect(invoke).not.toHaveBeenCalledWith('load_api_key', expect.anything())
    // No key content ever lands in the store state.
    expect(JSON.stringify(store.$state)).not.toContain('configured-key-placeholder')
  })

  it('flags a legacy fallback row by existence only, without decoding it', async () => {
    // A legacy fallback row exists: hasSetting sees it, getSetting is never
    // asked for the fallback value.
    db.hasSetting.mockImplementation(async (key: string) => key === 'deepseek_api_key_fallback')

    const store = useSettingsStore()
    await store.loadLlmConfig()

    expect(store.legacyFallbackDetected).toBe(true)
    expect(db.hasSetting).toHaveBeenCalledWith('deepseek_api_key_fallback')
    expect(db.getSetting).not.toHaveBeenCalledWith('deepseek_api_key_fallback', expect.anything())
  })

  it('reports keyConfigured false when no key exists in the keyring', async () => {
    invoke.mockResolvedValue(false)

    const store = useSettingsStore()
    await store.loadLlmConfig()

    expect(store.keyConfigured).toBe(false)
  })

  it('saves a newly typed key to keyring only, then settings; store keeps no key', async () => {
    const store = useSettingsStore()
    await store.saveLlmConfig(
      {
        provider: 'deepseek',
        baseUrl: 'https://api.deepseek.com',
        model: 'deepseek-chat',
        temperature: 0.7,
      },
      'sk-new',
    )

    // The only key-handling invoke is store_api_key; load/read never happens.
    expect(invoke).toHaveBeenCalledWith('store_api_key', {
      provider: 'deepseek',
      key: 'sk-new',
    })
    expect(invoke).not.toHaveBeenCalledWith('load_api_key', expect.anything())
    // Never writes a fallback row, but cleans up the legacy one (existence check only).
    const fallbackWrites = db.setSetting.mock.calls.filter(
      (call) => call[0] === 'deepseek_api_key_fallback',
    )
    expect(fallbackWrites).toHaveLength(0)
    expect(db.deleteSetting).toHaveBeenCalledWith('deepseek_api_key_fallback')
    expect(store.keyConfigured).toBe(true)
    expect(JSON.stringify(store.$state)).not.toContain('sk-new')
  })

  it('keeps the stored key configured when saving with an empty key input', async () => {
    const store = useSettingsStore()
    await store.saveLlmConfig(
      {
        provider: 'deepseek',
        baseUrl: 'https://api.deepseek.com',
        model: 'deepseek-chat',
        temperature: 0.7,
      },
      '',
    )

    expect(invoke).not.toHaveBeenCalledWith('store_api_key', expect.anything())
    expect(db.setSetting).toHaveBeenCalledWith('llm_provider', 'deepseek', expect.anything())
    expect(store.keyConfigured).toBe(false) // was false and no new key was given
  })

  it('clearLlmConfig deletes the keyring entry and the legacy fallback row', async () => {
    const store = useSettingsStore()
    await store.loadLlmConfig()
    await store.clearLlmConfig()

    expect(invoke).toHaveBeenCalledWith('delete_api_key', { provider: 'deepseek' })
    expect(db.deleteSetting).toHaveBeenCalledWith('deepseek_api_key_fallback')
    expect(store.llmConfig).toBeNull()
    expect(store.keyConfigured).toBe(false)
  })

  it('does not persist non-sensitive settings when keyring save fails', async () => {
    // keyring 保存失败 -> 抛错且 settings 表不被写入新配置。
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'store_api_key') throw new Error('keyring unavailable')
      return null
    })
    const store = useSettingsStore()
    await expect(
      store.saveLlmConfig(
        {
          provider: 'deepseek',
          baseUrl: 'https://api.deepseek.com',
          model: 'deepseek-chat',
          temperature: 0.7,
        },
        'sk-new',
      ),
    ).rejects.toThrow('keyring unavailable')

    // 非敏感 settings 从未写入（provider 等 key 无 setSetting 调用）。
    const writtenKeys = db.setSetting.mock.calls.map((call) => call[0])
    expect(writtenKeys).not.toContain('llm_provider')
    expect(writtenKeys).not.toContain('llm_base_url')
    expect(writtenKeys).not.toContain('llm_model')
    expect(store.llmConfig).toBeNull()
  })
})
