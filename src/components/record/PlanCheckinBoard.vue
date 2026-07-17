<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { Calendar, CircleCheck, Clock } from '@element-plus/icons-vue'
import { useExamStore } from '@/stores/exam'
import { useRecordStore } from '@/stores/record'
import { businessToday } from '@/services/record-service'
import { getPlansByDate } from '@/services/plan-service'
import { colorForSubject } from '@/services/theme'
import { checkinAction } from './checkin-ui'
import PlanCheckinDialog from './PlanCheckinDialog.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import type { PlanWithNames } from '@/services/plan-service'

const examStore = useExamStore()
const recordStore = useRecordStore()
const selectedDate = ref(businessToday())
const tasks = ref<PlanWithNames[]>([])
const loading = ref(false)
const dialogVisible = ref(false)
const selectedPlan = ref<PlanWithNames | null>(null)

const isFuture = computed(() => selectedDate.value > businessToday())
const completedCount = computed(
  () => tasks.value.filter((task) => task.status === 'completed').length,
)

async function load() {
  if (!examStore.activeExamId) {
    tasks.value = []
    return
  }
  loading.value = true
  try {
    tasks.value = await getPlansByDate(examStore.activeExamId, selectedDate.value)
  } finally {
    loading.value = false
  }
}

onMounted(async () => {
  await examStore.loadExams()
  if (examStore.activeExamId) await examStore.loadSubjects()
  await load()
})
watch(selectedDate, load)
watch(() => examStore.activeExamId, load)

async function act(task: PlanWithNames) {
  if (task.status === 'skipped') {
    await recordStore.restorePlan(task.id)
    ElMessage.success('任务已恢复')
    await load()
    return
  }
  selectedPlan.value = task
  dialogVisible.value = true
}

async function onSaved() {
  await load()
  emit('changed')
}

const emit = defineEmits<{ changed: [] }>()
defineExpose({ refresh: load })
</script>

<template>
  <el-card v-loading="loading" shadow="never" class="checkin-board">
    <template #header>
      <div class="board-head">
        <div>
          <div class="board-title">计划任务打卡</div>
          <div class="board-subtitle">按实际学习进度分次记录，完成任务时生成真实学习记录</div>
        </div>
        <div class="board-tools">
          <span v-if="tasks.length" class="progress tnum">
            {{ completedCount }}/{{ tasks.length }}
          </span>
          <el-date-picker
            v-model="selectedDate"
            type="date"
            value-format="YYYY-MM-DD"
            :clearable="false"
            class="date-picker"
          />
        </div>
      </div>
    </template>

    <el-alert
      v-if="isFuture"
      title="未来计划仅供查看，到计划日期后才能打卡"
      type="info"
      show-icon
      :closable="false"
      class="future-alert"
    />

    <EmptyState
      v-if="!tasks.length"
      title="当日暂无计划"
      description="可切换日期查看其他任务，或前往学习计划页生成计划。"
      :icon="Calendar"
    />
    <div v-else class="task-grid">
      <article
        v-for="task in tasks"
        :key="task.id"
        class="task-card"
        :class="`task-card--${task.status}`"
      >
        <div class="task-card__head">
          <span class="tag-tinted" :style="{ '--tag-color': colorForSubject(task.subject_name) }">
            {{ task.subject_name ?? '未知科目' }}
          </span>
          <el-tag
            size="small"
            effect="light"
            :type="
              task.status === 'completed'
                ? 'success'
                : task.status === 'skipped'
                  ? 'danger'
                  : 'info'
            "
          >
            {{
              task.status === 'pending'
                ? '未开始'
                : task.status === 'in_progress'
                  ? '进行中'
                  : task.status === 'completed'
                    ? '已完成'
                    : '已跳过'
            }}
          </el-tag>
        </div>
        <div v-if="task.knowledge_point_name" class="knowledge">
          {{ task.knowledge_point_name }}
        </div>
        <div class="task-content">{{ task.planned_tasks || '未填写任务内容' }}</div>
        <div class="task-metrics">
          <span>
            <el-icon><Clock /></el-icon>
            计划 {{ task.planned_duration ?? 0 }} 分
          </span>
          <span>
            <el-icon><CircleCheck /></el-icon>
            已记录 {{ task.actual_duration ?? 0 }} 分
          </span>
          <span v-if="task.record_count" class="tnum">{{ task.record_count }} 次打卡</span>
        </div>
        <el-progress
          :percentage="
            task.planned_duration
              ? Math.min(
                  100,
                  Math.round(((task.actual_duration ?? 0) / task.planned_duration) * 100),
                )
              : 0
          "
          :status="task.status === 'completed' ? 'success' : undefined"
          :stroke-width="6"
        />
        <div class="task-card__actions">
          <el-button
            :type="checkinAction(task.status).type"
            :disabled="isFuture"
            @click="act(task)"
          >
            {{ checkinAction(task.status).label }}
          </el-button>
        </div>
      </article>
    </div>

    <PlanCheckinDialog v-model="dialogVisible" :plan="selectedPlan" @saved="onSaved" />
  </el-card>
</template>

<style scoped>
.board-head,
.board-tools,
.task-card__head,
.task-metrics {
  display: flex;
  align-items: center;
}
.board-head {
  justify-content: space-between;
  gap: var(--sp-4);
}
.board-title {
  color: var(--c-ink);
  font-size: var(--fs-lg);
  font-weight: 700;
}
.board-subtitle,
.knowledge,
.task-metrics {
  color: var(--c-ink-3);
  font-size: var(--fs-xs);
}
.board-subtitle {
  margin-top: var(--sp-1);
}
.board-tools {
  gap: var(--sp-3);
}
.progress {
  color: var(--c-primary);
  font-weight: 700;
}
.date-picker {
  width: 150px;
}
.future-alert {
  margin-bottom: var(--sp-3);
}
.task-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
  gap: var(--sp-3);
}
.task-card {
  padding: var(--sp-3);
  border: 1px solid var(--c-border);
  border-radius: var(--r-lg);
  background: var(--c-surface);
}
.task-card--completed {
  border-color: var(--c-success);
  background: var(--c-success-light);
}
.task-card--skipped {
  opacity: 0.72;
}
.task-card__head {
  justify-content: space-between;
  gap: var(--sp-2);
}
.knowledge {
  margin-top: var(--sp-2);
}
.task-content {
  min-height: 44px;
  margin: var(--sp-2) 0;
  color: var(--c-ink);
  font-size: var(--fs-sm);
  font-weight: 600;
}
.task-metrics {
  gap: var(--sp-3);
  flex-wrap: wrap;
  margin-bottom: var(--sp-2);
}
.task-metrics span {
  display: inline-flex;
  align-items: center;
  gap: 3px;
}
.task-card__actions {
  display: flex;
  justify-content: flex-end;
  margin-top: var(--sp-3);
}
@media (max-width: 640px) {
  .board-head {
    align-items: flex-start;
    flex-direction: column;
  }
}
</style>
