import { createRouter, createWebHistory } from 'vue-router'
import { count, getSetting } from '@/services/db'

const routes = [
  {
    path: '/welcome',
    name: 'welcome',
    component: () => import('@/pages/Welcome.vue'),
    meta: { layout: 'full' },
  },
  { path: '/dashboard', name: 'dashboard', component: () => import('@/pages/Dashboard.vue') },
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
    // 可选 view 参数：calendar|gantt|list|compare，用于 URL 直达某个 Tab（Phase 3）
    path: '/study-plan/:view?',
    name: 'study-plan-view',
    component: () => import('@/pages/StudyPlan.vue'),
  },
  { path: '/analysis', name: 'analysis', component: () => import('@/pages/Analysis.vue') },
  {
    path: '/visualization',
    name: 'visualization',
    component: () => import('@/pages/Visualization.vue'),
  },
  { path: '/settings', name: 'settings', component: () => import('@/pages/Settings.vue') },
  { path: '/', redirect: '/dashboard' },
  { path: '/:pathMatch(.*)*', redirect: '/dashboard' },
]

const router = createRouter({
  history: createWebHistory(),
  routes,
})

// 引导完成态缓存：避免每次导航都查库
let resolved = false
let onboardingOk = false

async function checkOnboarding(): Promise<boolean> {
  if (resolved) return onboardingOk
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
  if (await checkOnboarding()) return true
  return { path: '/welcome' }
})

export default router
