// 智研 · ECharts 共享主题
// 注册 zhiyan-light / zhiyan-dark，统一所有图表的坐标轴、文字、tooltip、图例、默认系列色。
// 各图表组件只需：:theme="theme"（来自 useChartTheme），并移除 option 内硬编码的轴/文字色。
// 数据驱动的颜色（状态色、科目分类色）仍用 theme.ts 的 hex 常量。

import { registerTheme } from 'echarts/core'
import { SUBJECT_PALETTE, SEMANTIC } from './theme'

interface AxisTokens {
  text: string
  sub: string
  axis: string
  split: string
  border: string
  bg: string
  shadow: string
}

function buildTheme(t: AxisTokens) {
  return {
    backgroundColor: 'transparent',
    color: [...SUBJECT_PALETTE],
    textStyle: {
      color: t.text,
      fontFamily:
        'Inter, "SF Pro Text", "PingFang SC", "Microsoft YaHei", system-ui, sans-serif',
    },
    title: {
      textStyle: { color: t.text, fontWeight: 600 },
      subtextStyle: { color: t.sub },
    },
    legend: {
      textStyle: { color: t.sub },
      pageTextStyle: { color: t.sub },
    },
    tooltip: {
      backgroundColor: t.bg,
      borderColor: t.border,
      borderWidth: 1,
      textStyle: { color: t.text },
      extraCssText: `border-radius: 8px; box-shadow: ${t.shadow};`,
    },
    grid: { left: 48, right: 16, top: 24, bottom: 32 },
    categoryAxis: {
      axisLine: { show: true, lineStyle: { color: t.axis } },
      axisTick: { lineStyle: { color: t.axis } },
      axisLabel: { color: t.sub },
      splitLine: { show: false, lineStyle: { color: t.split } },
      splitArea: { show: false },
    },
    valueAxis: {
      axisLine: { show: false, lineStyle: { color: t.axis } },
      axisTick: { show: false },
      axisLabel: { color: t.sub },
      splitLine: { show: true, lineStyle: { color: t.split, type: 'dashed' } },
      splitArea: { show: false },
    },
    logAxis: {
      axisLine: { show: false },
      axisLabel: { color: t.sub },
      splitLine: { lineStyle: { color: t.split, type: 'dashed' } },
    },
    timeAxis: {
      axisLine: { lineStyle: { color: t.axis } },
      axisTick: { lineStyle: { color: t.axis } },
      axisLabel: { color: t.sub },
      splitLine: { lineStyle: { color: t.split, type: 'dashed' } },
    },
    radar: {
      axisName: { color: t.sub },
      axisLine: { lineStyle: { color: t.split } },
      splitLine: { lineStyle: { color: t.split } },
      splitArea: { show: false },
    },
    visualMap: {
      textStyle: { color: t.sub },
      inRange: { color: [SEMANTIC.danger, SEMANTIC.warning, SEMANTIC.success] },
    },
    line: {
      smooth: false,
      symbol: 'circle',
      symbolSize: 6,
      lineStyle: { width: 2 },
      itemStyle: { borderWidth: 2, borderColor: t.bg },
    },
    bar: {
      itemStyle: { borderRadius: [6, 6, 0, 0] },
    },
    pie: {
      itemStyle: { borderColor: t.bg, borderWidth: 2 },
      label: { color: t.sub },
    },
    gauge: {
      title: { color: t.sub },
      detail: { color: t.text },
    },
    candlestick: {},
    graph: {},
    sankey: {},
    scatter: {},
  }
}

registerTheme('zhiyan-light', buildTheme({
  text: '#1a1b1e',
  sub: '#6e7178',
  axis: '#d3d5db',
  split: '#f0f2f5',
  border: '#e6e7eb',
  bg: '#ffffff',
  shadow: '0 4px 12px rgba(20,21,25,0.08)',
}))

registerTheme('zhiyan-dark', buildTheme({
  text: '#ececef',
  sub: '#8e9098',
  axis: '#3a3c44',
  split: '#2d2f36',
  border: '#2d2f36',
  bg: '#1b1c20',
  shadow: '0 8px 24px rgba(0,0,0,0.5)',
}))

export {}
