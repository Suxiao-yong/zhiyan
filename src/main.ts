import { createApp } from 'vue'
import { createPinia } from 'pinia'
import piniaPluginPersistedstate from 'pinia-plugin-persistedstate'
import ElementPlus from 'element-plus'
import 'element-plus/dist/index.css'
// 暗色主题变量（Phase 5 启用切换，此处预先引入，无害）
import 'element-plus/theme-chalk/dark/css-vars.css'
import * as ElementPlusIconsVue from '@element-plus/icons-vue'

// ECharts 按需引入（覆盖后续各阶段所需图表类型与组件）
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { BarChart, LineChart, PieChart, GaugeChart, RadarChart, HeatmapChart } from 'echarts/charts'
import {
  GridComponent,
  TooltipComponent,
  LegendComponent,
  TitleComponent,
  DataZoomComponent,
  VisualMapComponent,
  CalendarComponent,
  MarkLineComponent,
  MarkPointComponent,
} from 'echarts/components'
import VueECharts from 'vue-echarts'

// ECharts 共享主题（zhiyan-light / zhiyan-dark），必须在图表渲染前注册
import './services/echarts-theme'

import App from './App.vue'
import router from './router'
import './assets/main.css'

use([
  CanvasRenderer,
  BarChart,
  LineChart,
  PieChart,
  GaugeChart,
  RadarChart,
  HeatmapChart,
  GridComponent,
  TooltipComponent,
  LegendComponent,
  TitleComponent,
  DataZoomComponent,
  VisualMapComponent,
  CalendarComponent,
  MarkLineComponent,
  MarkPointComponent,
])

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

// ECharts 全局组件 <v-chart>
app.component('VChart', VueECharts)

app.mount('#app')
