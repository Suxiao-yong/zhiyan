import { createApp } from 'vue'
import { createPinia } from 'pinia'
import piniaPluginPersistedstate from 'pinia-plugin-persistedstate'
import ElementPlus from 'element-plus'
import 'element-plus/dist/index.css'
// 暗色主题变量（Phase 5 启用切换，此处预先引入，无害）
import 'element-plus/theme-chalk/dark/css-vars.css'
import * as ElementPlusIconsVue from '@element-plus/icons-vue'

// Task 16: ECharts 不再在应用启动时全局注册。图表组件各自通过
// src/services/echarts-setup 惰性注册,保证 Today/Agent 首屏不加载 ECharts。

import App from './App.vue'
import router from './router'
import './assets/main.css'

const app = createApp(App)

// Pinia + persistedstate（仅 UI 状态 store 持久化到 localStorage；业务 store 不持久化）
const pinia = createPinia()
pinia.use(piniaPluginPersistedstate)
app.use(pinia)

app.use(router)
app.use(ElementPlus)

// 注册所有 Element Plus 图标为全局组件
for (const [key, component] of Object.entries(ElementPlusIconsVue)) {
  app.component(key, component as never)
}

app.mount('#app')
