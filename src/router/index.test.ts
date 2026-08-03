// @vitest-environment jsdom

import { beforeEach, describe, expect, it, vi } from 'vitest'

const db = vi.hoisted(() => ({
  count: vi.fn().mockResolvedValue(1),
  getSetting: vi.fn(),
}))

vi.mock('@/services/db', () => db)

describe('router agent_os_enabled fallback', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.resetModules()
    db.count.mockResolvedValue(1)
    db.getSetting.mockImplementation(async (key: string) => {
      if (key === 'onboarding_completed') return '1'
      return null
    })
  })

  it('redirects / to /agent when agent_os_enabled is unset (default)', async () => {
    const { default: router } = await import('./index')
    await router.push('/')
    expect(router.currentRoute.value.path).toBe('/agent')
  })

  it('redirects / to /agent when agent_os_enabled is 1', async () => {
    db.getSetting.mockImplementation(async (key: string) => {
      if (key === 'onboarding_completed') return '1'
      if (key === 'agent_os_enabled') return '1'
      return null
    })
    const { default: router } = await import('./index')
    await router.push('/')
    expect(router.currentRoute.value.path).toBe('/agent')
  })

  it('redirects / to /dashboard when agent_os_enabled is 0 (fallback)', async () => {
    db.getSetting.mockImplementation(async (key: string) => {
      if (key === 'onboarding_completed') return '1'
      if (key === 'agent_os_enabled') return '0'
      return null
    })
    const { default: router } = await import('./index')
    await router.push('/')
    expect(router.currentRoute.value.path).toBe('/dashboard')
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
})
