<script setup lang="ts">
import { computed, onMounted, watch } from 'vue'
import { useRoute } from 'vue-router'
import AppLayout from '@/components/layout/AppLayout.vue'
import { useSettingsStore } from '@/stores/settings'
import { useExamStore } from '@/stores/exam'
import { runPendingAnalyses } from '@/services/agent-engine'
import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification'

const route = useRoute()
const isFullScreen = computed(() => route.meta.layout === 'full')

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
    runPendingAnalyses(examStore.activeExamId, settingsStore.llmConfig).catch((e) =>
      console.warn('启动补跑分析失败', e),
    )
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
        body: '今日学习任务完成了吗？打开应用查看计划与 AI 分析。',
      })
    }
  } catch (e) {
    console.warn('通知发送失败', e)
  }
}
</script>

<template>
  <router-view v-if="isFullScreen" />
  <AppLayout v-else>
    <router-view />
  </AppLayout>
</template>
