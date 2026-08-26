<script setup lang="ts">
import { computed } from 'vue'
import { Warning, RefreshLeft } from '@element-plus/icons-vue'
import type { AgentActionPreview } from '@/types'

/**
 * 标准化审批预览（Task 10）:只渲染 Rust 生成的脱敏预览字段(动作名称、
 * 影响对象数量、日期范围、摘要、冲突、undo 可用性),绝不显示原始请求体
 * 或模型输出。按钮由父级(ApprovalCard)负责。
 */
const props = defineProps<{ preview: AgentActionPreview | null }>()

const riskLabel = computed(() => {
  const risk = props.preview?.risk ?? 3
  return risk >= 3 ? `R${risk} 高风险` : `R${risk}`
})

const conflictText = computed(() =>
  (props.preview?.conflicts ?? []).map((c) => c.detail).join('；'),
)

/** Task 5: the sanitized draft rows the apply would write (from fields.rows). */
interface PreviewRow {
  date: string
  subject_name: string
  planned_tasks: string
  planned_duration: number
  /** Task 7：确定性依据（知识点任务才有）；只展示 Rust 生成的 reason 文本。 */
  evidence?: {
    mastery?: number
    wrong_question_count?: number
    days_to_exam?: number | null
    reason?: string
    source_ref?: string | null
  } | null
}

const draftRows = computed<PreviewRow[]>(() => {
  const rows = props.preview?.fields?.rows
  if (!Array.isArray(rows)) return []
  return rows
    .filter(
      (row): row is PreviewRow =>
        typeof row === 'object' && row !== null && typeof (row as PreviewRow).date === 'string',
    )
    .slice(0, 20) // compact: show the first rows, the summary states the total
})

const draftRowCount = computed(() => {
  const count = props.preview?.fields?.draft_row_count
  return typeof count === 'number' ? count : undefined
})

const hasPrecondition = computed(() => {
  const hash = props.preview?.fields?.precondition_hash
  return typeof hash === 'string' && hash.length > 0
})
</script>

<template>
  <div class="action-preview" data-test="action-preview">
    <div v-if="preview" class="action-preview__body">
      <div class="action-preview__head">
        <span class="action-preview__action" data-test="action-preview-action">
          {{ preview.action }}
        </span>
        <span class="action-preview__risk" data-test="action-preview-risk">{{ riskLabel }}</span>
        <span
          v-if="preview.affected_count > 0"
          class="action-preview__count"
          data-test="action-preview-count"
        >
          影响 {{ preview.affected_count }} 项
        </span>
      </div>
      <p class="action-preview__summary" data-test="action-preview-summary">
        {{ preview.summary }}
      </p>
      <p v-if="preview.date_range" class="action-preview__range" data-test="action-preview-range">
        日期范围：{{ preview.date_range }}
      </p>
      <div v-if="draftRows.length" class="action-preview__rows" data-test="action-preview-rows">
        <div
          v-for="(row, index) in draftRows"
          :key="`${row.date}-${row.subject_name}-${index}`"
          class="action-preview__row"
        >
          <span class="action-preview__row-date">{{ row.date }}</span>
          <span class="action-preview__row-subject">{{ row.subject_name }}</span>
          <span class="action-preview__row-task">{{ row.planned_tasks }}</span>
          <span class="action-preview__row-min">{{ row.planned_duration }} 分钟</span>
          <span
            v-if="row.evidence?.reason"
            class="action-preview__row-evidence"
            data-test="action-preview-row-evidence"
          >
            依据：{{ row.evidence.reason }}
          </span>
          <span
            v-if="row.evidence?.source_ref"
            class="action-preview__row-evidence"
            data-test="action-preview-row-source-ref"
          >
            出处：{{ row.evidence.source_ref }}
          </span>
        </div>
        <p
          v-if="draftRowCount !== undefined && draftRowCount > draftRows.length"
          class="action-preview__more"
        >
          共 {{ draftRowCount }} 行（仅显示前 {{ draftRows.length }} 行）
        </p>
      </div>
      <p
        v-if="preview.tool === 'plan.apply_preview'"
        class="action-preview__precondition"
        data-test="action-preview-precondition"
      >
        {{
          hasPrecondition
            ? '已校验计划前置条件（应用前重新校验，数据变化将中止）'
            : '前置条件校验信息缺失'
        }}
      </p>
      <p v-if="conflictText" class="action-preview__conflicts" data-test="action-preview-conflicts">
        <el-icon><Warning /></el-icon>
        {{ conflictText }}
      </p>
      <p v-if="preview.undo_available" class="action-preview__undo" data-test="action-preview-undo">
        <el-icon><RefreshLeft /></el-icon>
        执行后可以撤销
      </p>
    </div>
    <p v-else class="action-preview__missing" data-test="action-preview-missing">（预览不可用）</p>
  </div>
</template>

<style scoped>
.action-preview__body {
  display: flex;
  flex-direction: column;
  gap: var(--sp-1);
}
.action-preview__head {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  flex-wrap: wrap;
}
.action-preview__action {
  font-weight: 600;
}
.action-preview__risk {
  font-size: var(--fs-xs);
  color: #b45309;
  background: #fef3c7;
  border-radius: 4px;
  padding: 1px 6px;
}
.action-preview__count {
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
}
.action-preview__summary {
  margin: 0;
  color: var(--c-ink-2);
  font-size: var(--fs-sm);
}
.action-preview__range {
  margin: 0;
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
}
.action-preview__rows {
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-height: 180px;
  overflow-y: auto;
  border: 1px solid var(--el-border-color-lighter);
  border-radius: 6px;
  padding: 6px 8px;
}
.action-preview__row {
  display: flex;
  gap: 8px;
  font-size: 12px;
  color: var(--c-ink-2);
  flex-wrap: wrap;
}
.action-preview__row-date {
  font-variant-numeric: tabular-nums;
  color: var(--c-ink-3);
  min-width: 88px;
}
.action-preview__row-subject {
  min-width: 48px;
  font-weight: 600;
}
.action-preview__row-task {
  flex: 1;
}
.action-preview__row-min {
  color: var(--c-ink-3);
}
.action-preview__row-evidence {
  width: 100%;
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
}
.action-preview__more,
.action-preview__precondition {
  margin: 4px 0 0;
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
}
.action-preview__precondition {
  color: var(--c-primary);
}
.action-preview__conflicts,
.action-preview__undo {
  margin: 0;
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: var(--fs-xs);
  color: #b45309;
}
.action-preview__undo {
  color: var(--c-ink-3);
}
.action-preview__missing {
  margin: 0;
  color: var(--c-ink-3);
  font-size: var(--fs-sm);
}
</style>
