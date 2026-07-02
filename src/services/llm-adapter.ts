// 最小 LLM 适配层（Phase 3）。
// Phase 4 将扩展：Stronghold 加密存储 apiKey + 连接测试 + 完整错误处理/重试退避 + Ollama 完善。
// HTTP 用 @tauri-apps/plugin-http（Rust 侧，绕前端 CSP；用户运行时配置的 baseUrl 任意），不用前端 fetch。

import { fetch } from '@tauri-apps/plugin-http'
import type { LLMConfig, LLMMessage } from '@/types'

export interface LLMResponse {
  content: string
  usage: { prompt_tokens: number; completion_tokens: number }
}

const DEFAULT_TIMEOUT_MS = 30000

function withTimeout<T>(p: Promise<T>, ms: number): Promise<T> {
  let timer: ReturnType<typeof setTimeout>
  return Promise.race([
    p,
    new Promise<T>((_, reject) => {
      timer = setTimeout(() => reject(new Error('AI 请求超时，请稍后重试')), ms)
    }),
  ]).finally(() => clearTimeout(timer))
}

function httpErrorMsg(status: number, body: string): string {
  if (status === 401 || status === 403) return 'API Key 无效或无权限'
  if (status === 429) return '请求过于频繁，请稍后重试'
  if (status >= 500) return `AI 服务异常 (${status})`
  return `AI 调用失败 (${status}): ${body.slice(0, 120)}`
}

function friendlyError(e: unknown): string {
  const msg = (e as Error)?.message ?? String(e)
  if (msg.includes('Failed to fetch') || msg.includes('Network') || msg.includes('connect')) {
    return '无法连接 AI 服务，请检查 baseUrl 与网络（Ollama 需本机运行）'
  }
  return msg
}

function sleep(ms: number) {
  return new Promise((r) => setTimeout(r, ms))
}

/** 3 次重试 + 指数退避（1s, 2s）；401/403（Key 无效）不重试 */
async function callWithRetry<T>(fn: () => Promise<T>, retries = 3): Promise<T> {
  let lastErr: unknown
  for (let i = 0; i < retries; i++) {
    try {
      return await fn()
    } catch (e) {
      lastErr = e
      const msg = (e as Error)?.message ?? ''
      if (msg.includes('API Key 无效') || msg.includes('无权限')) throw e
      if (i < retries - 1) await sleep(1000 * 2 ** i)
    }
  }
  throw lastErr
}

/** 统一调用：Ollama 走 /api/generate，其余走 OpenAI 兼容 /chat/completions；含重试 */
export async function callLLM(
  config: LLMConfig,
  messages: LLMMessage[],
  opts?: { timeoutMs?: number; retries?: number },
): Promise<LLMResponse> {
  const timeoutMs = opts?.timeoutMs ?? DEFAULT_TIMEOUT_MS
  const retries = opts?.retries ?? 3
  return callWithRetry(
    () =>
      config.provider === 'ollama'
        ? callOllama(config, messages, timeoutMs)
        : callOpenAICompatible(config, messages, timeoutMs),
    retries,
  )
}

// ============ Function Calling（工具调用，供聊天 Agent 用） ============

export interface LLMToolCall {
  id: string
  function: { name: string; arguments: string }
}

export interface LLMFullResponse {
  content: string
  tool_calls: LLMToolCall[]
  usage: { prompt_tokens: number; completion_tokens: number }
}

/**
 * 带工具的 LLM 调用（OpenAI 兼容 tools + tool_calls）。
 * Ollama 工具支持不一，统一抛错让上层降级本地。
 */
export async function callLLMWithTools(
  config: LLMConfig,
  messages: any[],
  tools: any[],
  opts?: { timeoutMs?: number; retries?: number },
): Promise<LLMFullResponse> {
  if (config.provider === 'ollama') {
    throw new Error('Ollama 暂不支持工具调用，请用 OpenAI 兼容 Provider（如 DeepSeek/OpenAI/通义）')
  }
  const timeoutMs = opts?.timeoutMs ?? 60000
  const retries = opts?.retries ?? 1
  const url = `${config.baseUrl.replace(/\/$/, '')}/chat/completions`
  const fn = async (): Promise<LLMFullResponse> => {
    const resp = await withTimeout(
      fetch(url, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${config.apiKey}`,
        },
        body: JSON.stringify({
          model: config.model,
          messages,
          temperature: config.temperature,
          stream: false,
          tools,
        }),
      }),
      timeoutMs,
    )
    if (!resp.ok) {
      const text = await resp.text().catch(() => '')
      throw new Error(httpErrorMsg(resp.status, text))
    }
    const data: any = await resp.json()
    const msg = data?.choices?.[0]?.message ?? {}
    return {
      content: msg.content ?? '',
      tool_calls: msg.tool_calls ?? [],
      usage: {
        prompt_tokens: data?.usage?.prompt_tokens ?? 0,
        completion_tokens: data?.usage?.completion_tokens ?? 0,
      },
    }
  }
  return callWithRetry(fn, retries)
}

async function callOpenAICompatible(
  config: LLMConfig,
  messages: LLMMessage[],
  timeoutMs: number,
): Promise<LLMResponse> {
  const url = `${config.baseUrl.replace(/\/$/, '')}/chat/completions`
  try {
    const resp = await withTimeout(
      fetch(url, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${config.apiKey}`,
        },
        body: JSON.stringify({
          model: config.model,
          messages,
          temperature: config.temperature,
          stream: false,
        }),
      }),
      timeoutMs,
    )
    if (!resp.ok) {
      const text = await resp.text().catch(() => '')
      throw new Error(httpErrorMsg(resp.status, text))
    }
    const data = await resp.json()
    return {
      content: data?.choices?.[0]?.message?.content ?? '',
      usage: {
        prompt_tokens: data?.usage?.prompt_tokens ?? 0,
        completion_tokens: data?.usage?.completion_tokens ?? 0,
      },
    }
  } catch (e) {
    throw new Error(friendlyError(e))
  }
}

async function callOllama(
  config: LLMConfig,
  messages: LLMMessage[],
  timeoutMs: number,
): Promise<LLMResponse> {
  const url = `${config.baseUrl.replace(/\/$/, '')}/api/generate`
  const prompt = messages.map((m) => `${m.role}: ${m.content}`).join('\n\n')
  try {
    const resp = await withTimeout(
      fetch(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          model: config.model,
          prompt,
          stream: false,
          options: { temperature: config.temperature },
        }),
      }),
      timeoutMs,
    )
    if (!resp.ok) {
      const text = await resp.text().catch(() => '')
      throw new Error(httpErrorMsg(resp.status, text))
    }
    const data = await resp.json()
    return {
      content: data?.response ?? '',
      usage: {
        prompt_tokens: data?.prompt_eval_count ?? 0,
        completion_tokens: data?.eval_count ?? 0,
      },
    }
  } catch (e) {
    throw new Error(friendlyError(e))
  }
}

/** 容错解析 LLM 返回的 JSON：trim、去 ```json``` 包裹、截取首尾 {}、修末尾多余逗号 */
export function parseJsonResponse<T = any>(content: string): T {
  let s = (content ?? '').trim()
  const fence = s.match(/```(?:json)?\s*([\s\S]*?)```/i)
  if (fence) s = fence[1].trim()
  const start = s.indexOf('{')
  const end = s.lastIndexOf('}')
  if (start >= 0 && end > start) s = s.slice(start, end + 1)
  s = s.replace(/,(\s*[}\]])/g, '$1')
  return JSON.parse(s)
}
