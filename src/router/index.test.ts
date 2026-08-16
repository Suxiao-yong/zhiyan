// @vitest-environment jsdom

import { beforeEach, describe, expect, it, vi } from 'vitest'

const db = vi.hoisted(() => ({
  count: vi.fn().mockResolvedValue(1),
  getSetting: vi.fn(),
}))

vi.mock('@/services/db', () => db)

describe('router single-entry contract (Task 2)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.resetModules()
    db.count.mockResolvedValue(1)
    db.getSetting.mockImplementation(async (key: string) => {
      if (key === 'onboarding_completed') return '1'
      return null
    })
  })

  it('redirects / to /agent', async () => {
    const { default: router } = await import('./index')
    await router.push('/')
    expect(router.currentRoute.value.path).toBe('/agent')
  })

  it('ignores a legacy agent_os_enabled=0 setting and still enters /agent', async () => {
    db.getSetting.mockImplementation(async (key: string) => {
      if (key === 'onboarding_completed') return '1'
      if (key === 'agent_os_enabled') return '0'
      return null
    })
    const { default: router } = await import('./index')
    await router.push('/')
    expect(router.currentRoute.value.path).toBe('/agent')
  })

  it('redirects legacy entry points /dashboard /analysis /visualization to /agent', async () => {
    const { default: router } = await import('./index')
    for (const legacy of ['/dashboard', '/analysis', '/visualization']) {
      await router.push(legacy)
      expect(router.currentRoute.value.path).toBe('/agent')
    }
  })

  it('redirects unknown old paths to /agent instead of 404', async () => {
    const { default: router } = await import('./index')
    await router.push('/some-unknown-old-path')
    expect(router.currentRoute.value.path).toBe('/agent')
  })

  it('redirects / to /welcome when onboarding is not finished', async () => {
    db.getSetting.mockImplementation(async (key: string) => {
      if (key === 'onboarding_completed') return null
      return null
    })
    db.count.mockResolvedValue(0)
    const { default: router } = await import('./index')
    await router.push('/')
    expect(router.currentRoute.value.path).toBe('/welcome')
  })

  it('keeps the four user navigation areas reachable', async () => {
    const { default: router } = await import('./index')
    for (const path of ['/agent', '/study-plan', '/study-record', '/exam-config', '/settings']) {
      await router.push(path)
      expect(router.currentRoute.value.path).toBe(path)
    }
  })

  it('does not expose /dashboard /analysis /visualization as ordinary routes', async () => {
    const { default: router } = await import('./index')
    const names = new Set(router.getRoutes().map((route) => route.name))
    expect(names.has('dashboard')).toBe(false)
    expect(names.has('analysis')).toBe(false)
    expect(names.has('visualization')).toBe(false)
  })
})
