<script setup lang="ts">
import { computed } from 'vue'
import { Calendar, Setting, Edit, Delete } from '@element-plus/icons-vue'
import type { Exam } from '@/types'

const props = defineProps<{ exam: Exam }>()
const emit = defineEmits<{ edit: []; delete: []; manageSubjects: [] }>()

const examTypeLabel = computed(() => {
  const m: Record<string, string> = {
    postgrad: '考研',
    civil: '考公',
    cert: '考证',
    custom: '自定义',
  }
  return m[props.exam.exam_type ?? ''] ?? props.exam.exam_type ?? '未分类'
})

const daysLeft = computed(() => {
  const d = new Date(props.exam.exam_date + 'T00:00:00')
  const now = new Date()
  now.setHours(0, 0, 0, 0)
  return Math.ceil((d.getTime() - now.getTime()) / 86400000)
})
</script>

<template>
  <el-card class="exam-card" shadow="never" @click="emit('manageSubjects')">
    <div class="exam-card__top">
      <el-tag size="small" type="primary" effect="light">{{ examTypeLabel }}</el-tag>
      <span class="exam-card__date">
        <el-icon><Calendar /></el-icon>
        {{ exam.exam_date }}
      </span>
    </div>
    <h3 class="exam-card__name">{{ exam.name }}</h3>
    <div class="exam-card__meta">
      <span v-if="exam.total_score != null" class="tnum">总分 {{ exam.total_score }}</span>
      <span>
        剩余
        <b class="tnum" :class="['days', { warn: daysLeft < 30, urgent: daysLeft < 7 }]">
          {{ daysLeft }}
        </b>
        天
      </span>
    </div>
    <p v-if="exam.description" class="exam-card__desc">{{ exam.description }}</p>
    <div class="exam-card__actions" @click.stop>
      <el-button size="small" :icon="Setting" @click="emit('manageSubjects')">科目管理</el-button>
      <el-button size="small" :icon="Edit" @click="emit('edit')">编辑</el-button>
      <el-button size="small" type="danger" :icon="Delete" @click="emit('delete')">删除</el-button>
    </div>
  </el-card>
</template>

<style scoped>
.exam-card {
  cursor: pointer;
  transition:
    transform var(--dur-fast) var(--ease),
    box-shadow var(--dur-fast) var(--ease);
}
.exam-card:hover {
  transform: translateY(-2px);
  box-shadow: var(--shadow-md);
}
.exam-card__top {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: var(--sp-2);
}
.exam-card__date {
  display: inline-flex;
  align-items: center;
  gap: var(--sp-1);
  font-size: var(--fs-sm);
  color: var(--c-ink-3);
}
.exam-card__name {
  font-size: var(--fs-lg);
  font-weight: 600;
  color: var(--c-ink);
  margin: 0 0 var(--sp-2);
}
.exam-card__meta {
  display: flex;
  gap: var(--sp-5);
  font-size: var(--fs-sm);
  color: var(--c-ink-2);
  margin-bottom: var(--sp-2);
}
.days.warn {
  color: var(--c-warning);
}
.days.urgent {
  color: var(--c-danger);
}
.exam-card__desc {
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
  margin: 0 0 var(--sp-3);
  line-height: 1.5;
}
.exam-card__actions {
  display: flex;
  gap: var(--sp-2);
  border-top: 1px solid var(--c-border);
  padding-top: var(--sp-3);
}
</style>
