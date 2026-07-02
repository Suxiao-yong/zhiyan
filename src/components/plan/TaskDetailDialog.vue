<script setup lang="ts">
import { computed } from 'vue'
import { usePlanStore } from '@/stores/plan'
import { colorForSubject } from '@/services/theme'
import type { PlanStatus } from '@/types'

const props = defineProps<{ modelValue: boolean; date: string | null }>()
const emit = defineEmits<{ 'update:modelValue': [v: boolean]; changed: [] }>()

const planStore = usePlanStore()

const visible = computed({
  get: () => props.modelValue,
  set: (v) => emit('update:modelValue', v),
})

const tasks = computed(() =>
  props.date ? planStore.plans.filter((p) => p.date === props.date) : [],
)

const statusOptions: { label: string; value: PlanStatus; type: string }[] = [
  { label: '未开始', value: 'pending', type: 'info' },
  { label: '进行中', value: 'in_progress', type: '' },
  { label: '已完成', value: 'completed', type: 'success' },
  { label: '已跳过', value: 'skipped', type: 'danger' },
]

async function onStatus(id: string, status: PlanStatus) {
  await planStore.updatePlanStatus(id, status)
  emit('changed')
}
</script>

<template>
  <el-dialog v-model="visible" :title="`${date ?? ''} 任务清单`" width="560px">
    <el-empty v-if="!tasks.length" description="当日无任务" :image-size="60" />
    <div v-else class="task-list">
      <div v-for="t in tasks" :key="t.id" class="task-item">
        <div class="task-item__main">
          <span
            class="tag-tinted subj-tag"
            :style="{ '--tag-color': colorForSubject(t.subject_name) }"
          >
            {{ t.subject_name ?? '-' }}
          </span>
          <span class="kp" v-if="t.knowledge_point_name">· {{ t.knowledge_point_name }}</span>
          <div class="task-text">{{ t.planned_tasks }}</div>
          <div class="meta tnum">
            计划 {{ t.planned_duration ?? 0 }} 分
            <span v-if="t.actual_duration != null">· 实际 {{ t.actual_duration }} 分</span>
            <el-tag v-if="t.user_modified" size="small" type="warning" effect="light">
              已手动调整
            </el-tag>
          </div>
        </div>
        <el-select
          :model-value="t.status"
          size="small"
          class="status-select"
          @change="(v: any) => onStatus(t.id, v as PlanStatus)"
        >
          <el-option v-for="o in statusOptions" :key="o.value" :label="o.label" :value="o.value" />
        </el-select>
      </div>
    </div>
  </el-dialog>
</template>

<style scoped>
.task-list {
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}
.task-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: var(--sp-3);
  padding: var(--sp-2);
  background: var(--c-surface-2);
  border-radius: var(--r-md);
}
.task-item__main {
  flex: 1;
  min-width: 0;
}
.subj-tag {
  margin-right: var(--sp-1);
}
.kp {
  color: var(--c-ink-3);
  font-size: var(--fs-xs);
  margin-left: var(--sp-1);
}
.task-text {
  font-size: var(--fs-sm);
  color: var(--c-ink);
  margin: var(--sp-1) 0;
}
.meta {
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
}
.status-select {
  width: 110px;
  flex-shrink: 0;
}
</style>
