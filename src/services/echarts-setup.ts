// Task 16: 惰性 ECharts 注册入口。
// main.ts 不再在启动时注册 ECharts;只有真正渲染图表的组件才 import 本模块,
// 从而保证 Today/Agent 首屏不加载 ECharts。use() 幂等,重复 import 无害。
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
import './echarts-theme'

export function ensureECharts(): void {
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
}
