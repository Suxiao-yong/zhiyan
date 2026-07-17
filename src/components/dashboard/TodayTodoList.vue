<script setup lang="ts">
import { storeToRefs } from 'pinia'
import { usePlanStore } from '@/stores/plan'
import EmptyState from '@/components/common/EmptyState.vue'
import { Calendar } from '@element-plus/icons-vue'
import { colorForSubject } from '@/services/theme'
import { checkinAction } from '@/components/record/checkin-ui'
import type { PlanWithNames } from '@/services/plan-service'

const planStore = usePlanStore()
const { todayTasks } = storeToRefs(planStore)

const emit = defineEmits<{
  checkin: [plan: PlanWithNames]
  restore: [plan: PlanWithNames]
}>()

function act(plan: PlanWithNames) {
  if (plan.status === 'skipped') emit('restore', plan)
  else emit('checkin', plan)
}
</script>

<template>
  <el-card shadow="never" class="todo-card">
    <template #header>
      <div class="card-head">
        <span class="card-head__title">今日待办</span>
        <span class="count tnum">
          {{ todayTasks.filter((t) => t.status === 'completed').length }}/{{ todayTasks.length }}
        </span>
      </div>
    </template>
    <EmptyState
      v-if="!todayTasks.length"
      title="今日暂无计划"
      description="尚未生成学习计划，前往「学习计划」页生成；或今日为休息日。"
      :icon="Calendar"
    />
    <div v-else class="todo-list">
      <div
        v-for="t in todayTasks"
        :key="t.id"
        class="todo-item"
        :class="{ done: t.status === 'completed' }"
      >
        <span class="status-dot" :class="`status-dot--${t.status}`" />
        <span
          class="tag-tinted subj-tag"
          :style="{ '--tag-color': colorForSubject(t.subject_name) }"
        >
          {{ t.subject_name }}
        </span>
        <span class="task">{{ t.planned_tasks }}</span>
        <span class="dur tnum">{{ t.planned_duration }}分</span>
        <el-button size="small" :type="checkinAction(t.status).type" plain @click="act(t)">
          {{ checkinAction(t.status).label }}
        </el-button>
      </div>
    </div>
  </el-card>
</template>

<style scoped>
.todo-card {
  height: 100%;
}
.card-head {
  display: flex;
  align-items: center;
}
.card-head__title {
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--c-ink);
}
.count {
  margin-left: auto;
  font-size: var(--fs-sm);
  color: var(--c-primary);
  font-weight: 600;
}
.todo-list {
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}
.todo-item {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  padding: var(--sp-2) var(--sp-3);
  border-radius: var(--r-md);
  background: var(--c-surface-2);
  transition: background var(--dur-fast) var(--ease);
}
.todo-item:hover {
  background: var(--c-surface-3);
}
.todo-item.done {
  opacity: 0.5;
}
.todo-item.done .task {
  text-decoration: line-through;
}
.subj-tag {
  font-size: var(--fs-xs);
  flex-shrink: 0;
}
.task {
  flex: 1;
  font-size: var(--fs-sm);
  color: var(--c-ink);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.dur {
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
  white-space: nowrap;
}
.status-dot {
  width: 8px;
  height: 8px;
  border-radius: var(--r-pill);
  background: var(--c-ink-muted);
  flex-shrink: 0;
}
.status-dot--in_progress {
  background: var(--c-primary);
}
.status-dot--completed {
  background: var(--c-success);
}
.status-dot--skipped {
  background: var(--c-danger);
}
</style>
