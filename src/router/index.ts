import { createRouter, createWebHistory } from 'vue-router'
import { count, getSetting } from '@/services/db'

const routes = [
  {
    path: '/welcome',
    name: 'welcome',
    component: () => import('@/pages/Welcome.vue'),
    meta: { layout: 'full' },
  },
  // Task 2: legacy entry points redirect to the Agent home so old URLs and
  // bookmarks keep working during the migration; the pages stay for tests and
  // rollback, but are no longer user navigation.
  { path: '/dashboard', redirect: '/agent' },
  { path: '/exam-config', name: 'exam-config', component: () => import('@/pages/ExamConfig.vue') },
  {
    path: '/study-record',
    name: 'study-record',
    component: () => import('@/pages/StudyRecord.vue'),
  },
  {
    path: '/study-plan',
    name: 'study-plan',
    component: () => import('@/pages/StudyPlan.vue'),
  },
  {
    // 可选 view 参数：calendar|list|mind-map，用于 URL 直达某个 Tab（Phase 3 / Task 8）
    path: '/study-plan/:view?',
    name: 'study-plan-view',
    component: () => import('@/pages/StudyPlan.vue'),
  },
  { path: '/analysis', redirect: '/agent' },
  { path: '/visualization', redirect: '/agent' },
  { path: '/settings', name: 'settings', component: () => import('@/pages/Settings.vue') },
  // Task 13: the Agent debug page is registered in development builds only;
  // production bundles never contain it and the catch-all redirect absorbs
  // any stale /agent-debug bookmarks.
  ...(import.meta.env.DEV
    ? [
        {
          path: '/agent-debug',
          name: 'agent-debug',
          component: () => import('@/pages/AgentDebug.vue'),
          meta: { debugOnly: true },
        },
      ]
    : []),
  {
    path: '/agent',
    name: 'agent',
    component: () => import('@/pages/AgentHome.vue'),
    meta: { layout: 'full' },
  },
  { path: '/', name: 'home', component: { render: () => null } },
  { path: '/:pathMatch(.*)*', redirect: '/agent' },
]

const router = createRouter({
  history: createWebHistory(),
  routes,
})

// M6 Task 6 note: the `agent_os_enabled` settings key is kept for data
// compatibility and audit, but the production router no longer consults it —
// `/` always resolves to `/agent` (Task 2). Emergency rollback goes through a
// version downgrade, not a second home screen in the user path.

// 引导完成态缓存：避免每次导航都查库
let resolved = false
let onboardingOk = false

async function checkOnboarding(): Promise<boolean> {
  if (resolved) return onboardingOk
  // SAFETY: Tauri 运行时检测需访问未类型化的 window 全局，仅做特性探测，不做数据断言
  // Web 预览：非 Tauri 环境直接放行，便于浏览器 / Playwright 预览所有页面
  if (
    typeof window !== 'undefined' &&
    !(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
  ) {
    resolved = true
    onboardingOk = true
    return true
  }
  try {
    const done = await getSetting('onboarding_completed')
    onboardingOk = done === '1' && (await count('exams')) > 0
  } catch (e) {
    // db 未就绪等异常，安全降级到引导页
    console.error('引导态查询失败，降级到 /welcome', e)
    onboardingOk = false
  }
  resolved = true
  return onboardingOk
}

/** Welcome 完成时调用，刷新缓存使后续导航放行 */
export function markOnboardingDone(): void {
  onboardingOk = true
  resolved = true
}

router.beforeEach(async (to) => {
  if (to.path === '/welcome') return true
  if (await checkOnboarding()) {
    if (to.path === '/') {
      // Task 2: the Agent home is the single product entry.
      return { path: '/agent' }
    }
    return true
  }
  return { path: '/welcome' }
})

export default router
