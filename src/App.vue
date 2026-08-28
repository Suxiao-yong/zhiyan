<script setup lang="ts">
import { computed, onMounted, watch } from 'vue'
import { useRoute } from 'vue-router'
import zhCn from 'element-plus/es/locale/lang/zh-cn'
import en from 'element-plus/es/locale/lang/en'
import AppLayout from '@/components/layout/AppLayout.vue'
import { useSettingsStore } from '@/stores/settings'
import { useExamStore } from '@/stores/exam'
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from '@tauri-apps/plugin-notification'

const route = useRoute()
const isFullScreen = computed(() => route.meta.layout === 'full')

// 日历/日期选择器语言跟随系统：系统为中文时用 zhCn，否则默认英语（与截图要求一致）
const locale = computed(() => {
  if (typeof navigator === 'undefined') return zhCn
  const lang = (navigator.language || '').toLowerCase()
  return lang.startsWith('zh') ? zhCn : en
})

const settingsStore = useSettingsStore()
const examStore = useExamStore()

function applyTheme(t: 'light' | 'dark') {
  document.documentElement.classList.toggle('dark', t === 'dark')
}

onMounted(async () => {
  await settingsStore.loadSettings()
  applyTheme(settingsStore.theme)
  watch(() => settingsStore.theme, applyTheme)
  await examStore.loadExams()
  if (examStore.activeExamId) {
    // Task 9: 启动补跑分析已删除——本地统计与文案由 Agent 侧 ContextSnapshot
    // / Daily Brief（Rust）负责,不再有前端自动的 LLM 分析路径。
    sendStartupReminder()
  }
})

/** 启动通知补发（桌面应用关闭时错过提醒，启动补一条汇总） */
async function sendStartupReminder() {
  if (!settingsStore.notificationEnabled) return
  try {
    let granted = await isPermissionGranted()
    if (!granted) {
      const perm = await requestPermission()
      granted = perm === 'granted'
    }
    if (granted) {
      sendNotification({
        title: '智研',
        body: '今日学习任务完成了吗？打开应用查看今日计划与智能复盘。',
      })
    }
  } catch (e) {
    console.warn('通知发送失败', e)
  }
}
</script>

<template>
  <el-config-provider :locale="locale">
    <router-view v-if="isFullScreen" />
    <AppLayout v-else>
      <router-view />
    </AppLayout>
  </el-config-provider>
</template>
