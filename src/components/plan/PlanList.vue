<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import draggable from 'vuedraggable'
import { ElMessage } from 'element-plus'
import { Edit, List } from '@element-plus/icons-vue'
import { usePlanStore } from '@/stores/plan'
import { useExamStore } from '@/stores/exam'
import { useRecordStore } from '@/stores/record'
import { colorForSubject } from '@/services/theme'
import { checkinAction } from '@/components/record/checkin-ui'
import PlanCheckinDialog from '@/components/record/PlanCheckinDialog.vue'
import type { PlanWithNames } from '@/services/plan-service'

const planStore = usePlanStore()
const examStore = useExamStore()
const recordStore = useRecordStore()

const groups = ref<Record<string, PlanWithNames[]>>({})
const activeDates = ref<string[]>([])
const loading = ref(false)

function fmtDate(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(
    d.getDate(),
  ).padStart(2, '0')}`
}
function defaultRange(): string[] {
  const t = new Date()
  t.setHours(0, 0, 0, 0)
  const end = new Date(t)
  end.setDate(end.getDate() + 60)
  return [fmtDate(t), fmtDate(end)]
}
const filterRange = ref<string[]>(defaultRange())
const showAll = ref(false)

const editVisible = ref(false)
const editId = ref('')
const checkinVisible = ref(false)
const selectedPlan = ref<PlanWithNames | null>(null)
const editForm = ref({
  date: '',
  planned_duration: 0,
  planned_tasks: '',
})

const sortedDates = computed(() => Object.keys(groups.value).sort())

async function load() {
  if (!examStore.activeExamId) return
  loading.value = true
  try {
    const from = showAll.value ? '1900-01-01' : filterRange.value[0] || '1900-01-01'
    const to = showAll.value ? '2100-12-31' : filterRange.value[1] || '2100-12-31'
    await planStore.loadPlansByDateRange(examStore.activeExamId, from, to)
    const g: Record<string, PlanWithNames[]> = {}
    for (const p of planStore.plans) {
      ;(g[p.date] ??= []).push(p)
    }
    // 组内按 sort_order 排序
    for (const d of Object.keys(g)) g[d].sort((a, b) => a.sort_order - b.sort_order)
    groups.value = g
    // 默认展开最近 5 天
    activeDates.value = sortedDates.value.slice(0, 5)
  } finally {
    loading.value = false
  }
}
onMounted(async () => {
  await examStore.loadExams()
  if (!examStore.activeExamId && examStore.exams.length)
    examStore.setActiveExam(examStore.exams[0].id)
  await load()
})
watch(() => examStore.activeExamId, load)
watch([filterRange, showAll], load, { deep: true })

async function onDragEnd(date: string) {
  const ids = (groups.value[date] ?? []).map((p) => p.id)
  await planStore.reorderPlans(ids)
  ElMessage.success('已重排')
}

async function act(plan: PlanWithNames) {
  if (plan.status === 'skipped') {
    await recordStore.restorePlan(plan.id)
    await load()
    return
  }
  selectedPlan.value = plan
  checkinVisible.value = true
}

async function skip(plan: PlanWithNames) {
  await planStore.updatePlanStatus(plan.id, 'skipped')
  await load()
}

function openEdit(p: PlanWithNames) {
  editId.value = p.id
  editForm.value = {
    date: p.date,
    planned_duration: p.planned_duration ?? 0,
    planned_tasks: p.planned_tasks ?? '',
  }
  editVisible.value = true
}

async function saveEdit() {
  await planStore.updatePlan(editId.value, { ...editForm.value })
  ElMessage.success('已保存（标记为手动调整）')
  editVisible.value = false
  await load()
}
</script>

<template>
  <el-card v-loading="loading" shadow="never">
    <template #header>
      <div class="card-head">
        <el-icon class="card-head__icon"><List /></el-icon>
        <span class="card-head__title">任务列表</span>
        <span class="card-head__hint">拖拽 ⋮⋮ 排序，可编辑日期 / 时长 / 内容</span>
      </div>
      <div class="filter-row">
        <el-date-picker
          v-model="filterRange"
          type="daterange"
          value-format="YYYY-MM-DD"
          start-placeholder="开始"
          end-placeholder="结束"
          size="small"
          class="range-picker"
          :disabled="showAll"
        />
        <el-checkbox v-model="showAll">显示全部</el-checkbox>
      </div>
    </template>
    <el-empty v-if="!sortedDates.length" description="尚未生成计划" :image-size="60" />
    <el-collapse v-else v-model="activeDates">
      <el-collapse-item
        v-for="date in sortedDates"
        :key="date"
        :name="date"
        :title="`${date}（${groups[date].length} 项）`"
      >
        <draggable
          v-model="groups[date]"
          item-key="id"
          handle=".drag-handle"
          @end="onDragEnd(date)"
        >
          <template #item="{ element }">
            <div class="task-row">
              <span class="drag-handle" title="拖拽排序">⋮⋮</span>
              <span
                class="tag-tinted subj-tag"
                :style="{ '--tag-color': colorForSubject(element.subject_name) }"
              >
                {{ element.subject_name ?? '-' }}
              </span>
              <span class="task">{{ element.planned_tasks }}</span>
              <span class="dur tnum">{{ element.planned_duration ?? 0 }}分</span>
              <el-tag
                size="small"
                effect="light"
                :type="
                  element.status === 'completed'
                    ? 'success'
                    : element.status === 'skipped'
                      ? 'danger'
                      : 'info'
                "
              >
                {{
                  element.status === 'pending'
                    ? '未开始'
                    : element.status === 'in_progress'
                      ? '进行中'
                      : element.status === 'completed'
                        ? '已完成'
                        : '已跳过'
                }}
              </el-tag>
              <el-button
                size="small"
                :type="checkinAction(element.status).type"
                plain
                @click="act(element)"
              >
                {{ checkinAction(element.status).label }}
              </el-button>
              <el-button
                v-if="element.status !== 'completed' && element.status !== 'skipped'"
                size="small"
                link
                type="danger"
                @click="skip(element)"
              >
                跳过
              </el-button>
              <el-button link :icon="Edit" @click="openEdit(element)">编辑</el-button>
            </div>
          </template>
        </draggable>
      </el-collapse-item>
    </el-collapse>

    <el-dialog v-model="editVisible" title="编辑任务" width="460px">
      <el-form label-width="72px" @submit.prevent>
        <el-form-item label="日期">
          <el-date-picker
            v-model="editForm.date"
            type="date"
            value-format="YYYY-MM-DD"
            class="full-width"
          />
        </el-form-item>
        <el-form-item label="计划时长">
          <el-input-number
            v-model="editForm.planned_duration"
            :min="0"
            :step="15"
            controls-position="right"
          />
        </el-form-item>
        <el-form-item label="任务内容">
          <el-input v-model="editForm.planned_tasks" type="textarea" :rows="2" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="editVisible = false">取消</el-button>
        <el-button type="primary" @click="saveEdit">保存</el-button>
      </template>
    </el-dialog>
    <PlanCheckinDialog v-model="checkinVisible" :plan="selectedPlan" @saved="load" />
  </el-card>
</template>

<style scoped>
.card-head {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
}
.card-head__icon {
  color: var(--c-primary);
  font-size: var(--fs-md);
}
.card-head__title {
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--c-ink);
}
.card-head__hint {
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
  margin-left: auto;
}
.filter-row {
  margin-top: var(--sp-2);
  display: flex;
  align-items: center;
  gap: var(--sp-3);
}
.range-picker {
  width: 240px;
}
.task-row {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  padding: var(--sp-2);
  border-bottom: 1px solid var(--c-border);
  transition: background var(--dur-fast) var(--ease);
}
.task-row:hover {
  background: var(--c-surface-2);
}
.drag-handle {
  cursor: grab;
  color: var(--c-ink-muted);
  user-select: none;
}
.subj-tag {
  flex-shrink: 0;
  min-width: 60px;
  justify-content: center;
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
.full-width {
  width: 100%;
}
</style>
