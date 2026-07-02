// 联网搜索服务（基于 anysearch API：https://api.anysearch.com/mcp，JSON-RPC 2.0）。
// 匿名可用（低限额），可选 key（经 keyring 存，provider='anysearch'；keyring 失败时走 fallback）。
// 用 @tauri-apps/plugin-http（Rust 侧，绕 CSP）。结果截断防爆。

import { fetch } from '@tauri-apps/plugin-http'
import { invoke } from '@tauri-apps/api/core'
import { getSetting } from '@/services/db'
import { deobfuscate } from '@/stores/settings'

const ENDPOINT = 'https://api.anysearch.com/mcp'

async function getApiKey(): Promise<string> {
  // 1. 首选 keyring
  try {
    const k = await invoke<string | null>('load_api_key', { provider: 'anysearch' })
    if (k) return k
  } catch {
    /* keyring 不可用，走 fallback */
  }
  // 2. 降级：SQLite fallback
  try {
    const fb = await getSetting('anysearch_api_key_fallback')
    if (fb) {
      const key = deobfuscate(fb)
      if (key) return key
    }
  } catch {
    /* ignore */
  }
  return ''
}

/** 调 anysearch 的 tools/call */
async function callAnysearch(tool: string, args: Record<string, unknown>): Promise<string> {
  const key = await getApiKey()
  const resp = await fetch(ENDPOINT, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...(key ? { Authorization: `Bearer ${key}` } : {}),
    },
    body: JSON.stringify({
      jsonrpc: '2.0',
      id: 1,
      method: 'tools/call',
      params: { name: tool, arguments: args },
    }),
  })
  if (!resp.ok) {
    const t = await resp.text().catch(() => '')
    throw new Error(`搜索服务异常 (${resp.status}): ${t.slice(0, 120)}`)
  }
  const json: any = await resp.json()
  if (json.error) throw new Error(json.error.message ?? '搜索失败')
  const content = json.result?.content
  if (Array.isArray(content)) {
    const text = content.find((c: any) => c.type === 'text')?.text
    if (text) return text
  }
  return JSON.stringify(json.result ?? json)
}

/** 网页搜索，返回结果文本（截断到 2000 字符） */
export async function searchWeb(query: string, maxResults = 5): Promise<string> {
  const text = await callAnysearch('search', { query, max_results: maxResults })
  return text.slice(0, 2000)
}

/** 提取网页正文（截断到 4000 字符） */
export async function extractUrl(url: string): Promise<string> {
  const text = await callAnysearch('extract', { url })
  return text.slice(0, 4000)
}
