import { describe, it, expect, vi } from 'vitest'

// mock 掉 Tauri HTTP 插件（node 环境无 Tauri 运行时）
vi.mock('@tauri-apps/plugin-http', () => ({ fetch: vi.fn() }))

import { parseJsonResponse } from './llm-adapter'

describe('parseJsonResponse', () => {
  it('解析纯 JSON', () => {
    expect(parseJsonResponse('{"a":1}').a).toBe(1)
  })
  it('去除 ```json 代码块标记', () => {
    expect(parseJsonResponse('```json\n{"a":2}\n```').a).toBe(2)
  })
  it('从前后文字中提取 JSON', () => {
    expect(parseJsonResponse('好的，结果如下：\n{"a":3}\n以上是分析').a).toBe(3)
  })
  it('修复末尾多余逗号', () => {
    expect(parseJsonResponse('{"a":4,}').a).toBe(4)
    expect(parseJsonResponse('{"b":[1,2,]}').b).toEqual([1, 2])
  })
  it('解析嵌套对象', () => {
    const r = parseJsonResponse('{"x":{"y":[1,2,3]}}')
    expect(r.x.y).toEqual([1, 2, 3])
  })
})
