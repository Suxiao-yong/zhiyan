<script setup lang="ts">
import { computed, ref } from 'vue'
import { usePlanStore } from '@/stores/plan'
import { useRecordStore } from '@/stores/record'
import { colorForSubject } from '@/services/theme'
import { checkinAction } from '@/components/record/checkin-ui'
import PlanCheckinDialog from '@/components/record/PlanCheckinDialog.vue'
import type { PlanWithNames } from '@/services/plan-service'

const props = defineProps<{ modelValue: boolean; date: string | null }>()
const emit = defineEmits<{ 'update:modelValue': [v: boolean]; changed: [] }>()

const planStore = usePlanStore()
const recordStore = useRecordStore()
const checkinVisible = ref(false)
const selectedPlan = ref<PlanWithNames | null>(null)

const visible = computed({
  get: () => props.modelValue,
  set: (v) => emit('update:modelValue', v),
})

const tasks = computed(() =>
  props.date ? planStore.plans.filter((p) => p.date === props.date) : [],
)

async function act(plan: PlanWithNames) {
  if (plan.status === 'skipped') {
    await recordStore.restorePlan(plan.id)
    emit('changed')
    return
  }
  selectedPlan.value = plan
  checkinVisible.value = true
}

async function skip(plan: PlanWithNames) {
  await planStore.updatePlanStatus(plan.id, 'skipped')
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
          <span v-if="t.knowledge_point_name" class="kp">· {{ t.knowledge_point_name }}</span>
          <div class="task-text">{{ t.planned_tasks }}</div>
          <div class="meta tnum">
            计划 {{ t.planned_duration ?? 0 }} 分
            <span v-if="t.actual_duration != null">· 实际 {{ t.actual_duration }} 分</span>
            <el-tag v-if="t.user_modified" size="small" type="warning" effect="light">
              已手动调整
            </el-tag>
          </div>
        </div>
        <div class="task-actions">
          <el-button size="small" :type="checkinAction(t.status).type" @click="act(t)">
            {{ checkinAction(t.status).label }}
          </el-button>
          <el-button
            v-if="t.status !== 'completed' && t.status !== 'skipped'"
            size="small"
            link
            type="danger"
            @click="skip(t)"
          >
            跳过
          </el-button>
        </div>
      </div>
    </div>
    <PlanCheckinDialog v-model="checkinVisible" :plan="selectedPlan" @saved="emit('changed')" />
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
.task-actions {
  display: flex;
  align-items: center;
  flex-shrink: 0;
}
</style>
