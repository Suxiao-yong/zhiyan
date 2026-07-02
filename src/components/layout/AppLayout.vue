<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute } from 'vue-router'
import {
  Odometer,
  Document,
  EditPen,
  Calendar,
  TrendCharts,
  DataLine,
  Setting,
  Fold,
  Expand,
  Moon,
  Sunny,
} from '@element-plus/icons-vue'
import { useSettingsStore } from '@/stores/settings'

const isCollapse = ref(false)
const route = useRoute()
const settings = useSettingsStore()

interface MenuItem {
  index: string
  label: string
  icon: any
}

const menuItems: MenuItem[] = [
  { index: '/dashboard', label: '仪表盘', icon: Odometer },
  { index: '/exam-config', label: '考试配置', icon: Document },
  { index: '/study-record', label: '学习记录', icon: EditPen },
  { index: '/study-plan', label: '学习计划', icon: Calendar },
  { index: '/analysis', label: 'AI 分析', icon: TrendCharts },
  { index: '/visualization', label: '数据可视化', icon: DataLine },
  { index: '/settings', label: '设置', icon: Setting },
]

// /study-plan/:view? 下也高亮"学习计划"
const activeMenu = computed(() => {
  if (route.path.startsWith('/study-plan')) return '/study-plan'
  return route.path
})

async function toggleTheme() {
  await settings.setTheme(settings.theme === 'dark' ? 'light' : 'dark')
}
</script>

<template>
  <el-container class="app-layout">
    <el-aside :width="isCollapse ? '64px' : '220px'" class="app-aside">
      <div class="brand">
        <span class="brand-mark" />
        <span v-if="!isCollapse" class="brand-text">智研</span>
      </div>
      <el-menu
        :default-active="activeMenu"
        :collapse="isCollapse"
        :collapse-transition="false"
        router
        class="app-menu"
      >
        <el-menu-item v-for="item in menuItems" :key="item.index" :index="item.index">
          <el-icon><component :is="item.icon" /></el-icon>
          <template #title>{{ item.label }}</template>
        </el-menu-item>
      </el-menu>
    </el-aside>

    <el-container>
      <el-header class="app-header">
        <el-button text class="collapse-btn" @click="isCollapse = !isCollapse">
          <el-icon :size="18"><component :is="isCollapse ? Expand : Fold" /></el-icon>
        </el-button>
        <span class="header-tagline">AI 驱动的个性化学习规划</span>
        <div class="header-spacer" />
        <button
          class="icon-btn theme-toggle"
          :title="settings.theme === 'dark' ? '切换到浅色' : '切换到深色'"
          @click="toggleTheme"
        >
          <el-icon :size="18"><component :is="settings.theme === 'dark' ? Sunny : Moon" /></el-icon>
        </button>
      </el-header>
      <el-main class="app-main">
        <router-view v-slot="{ Component }">
          <transition name="fade" mode="out-in">
            <component :is="Component" />
          </transition>
        </router-view>
      </el-main>
    </el-container>
  </el-container>
</template>

<style scoped>
.app-layout {
  height: 100%;
}

/* 侧栏：第二中性层（略冷的浅灰；暗色为深面），非 Ant navy */
.app-aside {
  background: var(--c-surface-2);
  border-right: 1px solid var(--c-border);
  transition: width var(--dur) var(--ease);
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.brand {
  height: 56px;
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  padding: 0 var(--sp-4);
  flex-shrink: 0;
}
.brand-mark {
  width: 26px;
  height: 26px;
  border-radius: 7px;
  background: var(--c-primary);
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.25);
  flex-shrink: 0;
}
.brand-text {
  font-size: var(--fs-lg);
  font-weight: 700;
  letter-spacing: 1px;
  color: var(--c-ink);
}

.app-menu {
  border-right: none;
  background: transparent;
  flex: 1;
  padding: var(--sp-2) var(--sp-2);
}
.app-menu:not(.el-menu--collapse) {
  width: 220px;
}
:deep(.el-menu) {
  background: transparent;
}
:deep(.el-menu-item) {
  color: var(--c-ink-2);
  border-radius: var(--r-md);
  margin: 2px 0;
  height: 42px;
  line-height: 42px;
}
:deep(.el-menu-item:hover) {
  background: var(--c-surface-3);
  color: var(--c-ink);
}
:deep(.el-menu-item.is-active) {
  color: var(--c-primary);
  background: var(--c-primary-light);
  font-weight: 600;
}
:deep(.el-menu-item.is-active .el-icon) {
  color: var(--c-primary);
}

.app-header {
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  background: var(--c-surface);
  border-bottom: 1px solid var(--c-border);
  height: 56px;
  padding: 0 var(--sp-5);
}
.collapse-btn {
  color: var(--c-ink-3);
  padding: 6px;
}
.header-tagline {
  font-size: var(--fs-sm);
  color: var(--c-ink-3);
}
.header-spacer {
  flex: 1;
}
.theme-toggle {
  width: 36px;
  height: 36px;
  border: none;
  background: transparent;
  cursor: pointer;
}

.app-main {
  background: var(--c-bg);
  padding: var(--sp-6);
  overflow: auto;
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity var(--dur-fast) var(--ease);
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
