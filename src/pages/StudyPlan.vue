<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import PageHeader from '@/components/common/PageHeader.vue'
import PlanCalendar from '@/components/plan/PlanCalendar.vue'
import PlanList from '@/components/plan/PlanList.vue'
import KnowledgeMindMap from '@/components/plan/KnowledgeMindMap.vue'
import { useExamStore } from '@/stores/exam'

// Task 4: the plan page only views and edits plans. Generation and adjustment
// go through the Agent (plan.preview_generate / plan.apply_preview). The
// Gantt and Compare views were removed as second products.
const route = useRoute()
const router = useRouter()
const examStore = useExamStore()

const activeTab = ref('calendar')
const refreshKey = ref(0)

const validViews = ['calendar', 'list', 'mind-map']

onMounted(async () => {
  await examStore.loadExams()
  const v = route.params.view
  if (typeof v === 'string' && validViews.includes(v)) {
    activeTab.value = v
  }
})

watch(activeTab, (v) => {
  if (route.params.view !== v) {
    router.replace({ name: 'study-plan-view', params: { view: v } })
  }
})
</script>

<template>
  <div>
    <PageHeader title="学习计划" subtitle="查看与编辑计划；生成与调整通过 Agent 完成">
      <template #actions>
        <el-button type="primary" @click="router.push('/agent')">在 Agent 中调整计划</el-button>
      </template>
    </PageHeader>

    <!-- lazy: 只挂载当前 Tab，避免两视图同时加载/渲染导致卡顿 -->
    <el-tabs v-model="activeTab" class="plan-tabs">
      <el-tab-pane label="日历" name="calendar" lazy>
        <PlanCalendar :key="'cal-' + refreshKey" />
      </el-tab-pane>
      <el-tab-pane label="列表" name="list" lazy>
        <PlanList :key="'lst-' + refreshKey" />
      </el-tab-pane>
      <!-- Task 8: 只读知识点思维导图；点击节点在组件内弹溯源 dialog，页面只透传 select -->
      <el-tab-pane label="思维导图" name="mind-map" lazy>
        <KnowledgeMindMap />
      </el-tab-pane>
    </el-tabs>
  </div>
</template>

<style scoped>
.plan-tabs {
  margin-top: var(--sp-4);
}
</style>
