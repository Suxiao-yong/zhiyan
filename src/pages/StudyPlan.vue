<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { Plus } from '@element-plus/icons-vue'
import PageHeader from '@/components/common/PageHeader.vue'
import PlanCalendar from '@/components/plan/PlanCalendar.vue'
import PlanGantt from '@/components/plan/PlanGantt.vue'
import PlanList from '@/components/plan/PlanList.vue'
import PlanCompare from '@/components/plan/PlanCompare.vue'
import PlanChatAssistant from '@/components/plan/PlanChatAssistant.vue'
import { useExamStore } from '@/stores/exam'

const route = useRoute()
const router = useRouter()
const examStore = useExamStore()

const activeTab = ref('calendar')
const assistantVisible = ref(false)
const assistantExam = ref<{ id: string; name: string } | null>(null)
const refreshKey = ref(0)

const validViews = ['calendar', 'gantt', 'list', 'compare']

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

function openAssistant() {
  const ex = examStore.exams.find((e) => e.id === examStore.activeExamId)
  assistantExam.value = ex ? { id: ex.id, name: ex.name } : null
  assistantVisible.value = true
}

function onGenerated() {
  refreshKey.value++
}
</script>

<template>
  <div>
    <PageHeader title="学习计划" subtitle="AI 联网研究考试 + 交互式讨论生成个性化计划">
      <template #actions>
        <el-button type="primary" :icon="Plus" @click="openAssistant">
          AI 生成 / 重新生成计划
        </el-button>
      </template>
    </PageHeader>

    <!-- lazy: 只挂载当前 Tab，避免 4 视图同时加载/渲染导致卡顿 -->
    <el-tabs v-model="activeTab" class="plan-tabs">
      <el-tab-pane label="日历" name="calendar" lazy>
        <PlanCalendar :key="'cal-' + refreshKey" />
      </el-tab-pane>
      <el-tab-pane label="甘特图" name="gantt" lazy>
        <PlanGantt :key="'gnt-' + refreshKey" />
      </el-tab-pane>
      <el-tab-pane label="列表" name="list" lazy>
        <PlanList :key="'lst-' + refreshKey" />
      </el-tab-pane>
      <el-tab-pane label="计划 vs 实际" name="compare" lazy>
        <PlanCompare :key="'cmp-' + refreshKey" />
      </el-tab-pane>
    </el-tabs>

    <PlanChatAssistant
      v-model="assistantVisible"
      :exam-id="assistantExam?.id ?? ''"
      :exam-name="assistantExam?.name ?? ''"
      @done="onGenerated"
    />
  </div>
</template>

<style scoped>
.plan-tabs {
  margin-top: var(--sp-4);
}
</style>
