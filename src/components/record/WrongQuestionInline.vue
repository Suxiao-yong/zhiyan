<script setup lang="ts">
import { computed } from 'vue'
import { Plus, Delete, Warning } from '@element-plus/icons-vue'

// 错题草稿（不含 subject_id/kp_id，由 QuickRecordDialog 在保存时补全）
interface WrongDraft {
  question_source: string
  question_desc: string
  correct_answer: string
  my_answer: string
  error_type: string
  error_reason: string
}

const props = defineProps<{ modelValue: WrongDraft[] }>()
const emit = defineEmits<{ 'update:modelValue': [val: WrongDraft[]] }>()

const errorTypes = [
  { label: '概念不清', value: '概念不清' },
  { label: '计算错误', value: '计算错误' },
  { label: '粗心', value: '粗心' },
  { label: '其他', value: '其他' },
]

const list = computed({
  get: () => props.modelValue,
  set: (v: WrongDraft[]) => emit('update:modelValue', v),
})

function add() {
  list.value = [
    ...list.value,
    {
      question_source: '',
      question_desc: '',
      correct_answer: '',
      my_answer: '',
      error_type: '概念不清',
      error_reason: '',
    },
  ]
}
function remove(i: number) {
  const arr = [...list.value]
  arr.splice(i, 1)
  list.value = arr
}
</script>

<template>
  <div class="wrong-inline">
    <div class="wrong-inline__head">
      <span class="head-label">
        <el-icon class="head-icon"><Warning /></el-icon>
        <span class="head-title">错题录入（做题联动）</span>
      </span>
      <el-button size="small" :icon="Plus" @click="add">添加错题</el-button>
    </div>
    <div v-if="!list.length" class="wrong-empty">暂无错题，有做错的题可点击「添加错题」录入</div>
    <div v-for="(w, i) in list" :key="i" class="wrong-card">
      <div class="wrong-card__head">
        <span>错题 {{ i + 1 }}</span>
        <el-button link type="danger" :icon="Delete" @click="remove(i)">删除</el-button>
      </div>
      <el-row :gutter="10">
        <el-col :span="12">
          <el-input v-model="w.question_source" placeholder="题目来源（可选）" />
        </el-col>
        <el-col :span="12">
          <el-select v-model="w.error_type" class="full">
            <el-option v-for="t in errorTypes" :key="t.value" :label="t.label" :value="t.value" />
          </el-select>
        </el-col>
        <el-col :span="24">
          <el-input v-model="w.question_desc" type="textarea" :rows="2" placeholder="题目描述" />
        </el-col>
        <el-col :span="12">
          <el-input v-model="w.correct_answer" placeholder="正确答案" />
        </el-col>
        <el-col :span="12">
          <el-input v-model="w.my_answer" placeholder="我的答案" />
        </el-col>
        <el-col :span="24">
          <el-input v-model="w.error_reason" type="textarea" :rows="1" placeholder="错误原因" />
        </el-col>
      </el-row>
    </div>
  </div>
</template>

<style scoped>
.wrong-inline {
  background: var(--c-warning-light);
  border: 1px solid color-mix(in srgb, var(--c-warning) 22%, var(--c-border));
  border-radius: var(--r-md);
  padding: var(--sp-3);
  margin-top: var(--sp-2);
}
.wrong-inline__head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: var(--sp-2);
}
.head-label {
  display: inline-flex;
  align-items: center;
  gap: var(--sp-2);
}
.head-icon {
  color: var(--c-warning);
  font-size: var(--fs-md);
}
.head-title {
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--c-warning);
}
.wrong-empty {
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
  text-align: center;
  padding: var(--sp-2);
}
.wrong-card {
  border: 1px dashed color-mix(in srgb, var(--c-warning) 30%, var(--c-border));
  border-radius: var(--r-sm);
  padding: var(--sp-3);
  margin-bottom: var(--sp-2);
  background: var(--c-surface);
}
.wrong-card:last-child {
  margin-bottom: 0;
}
.wrong-card__head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: var(--sp-2);
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
}
.full {
  width: 100%;
}
</style>
