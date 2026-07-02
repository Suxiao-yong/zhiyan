<script setup lang="ts">
import { reactive, ref } from 'vue'
import type { FormInstance, FormRules } from 'element-plus'
import { ElMessage } from 'element-plus'
import type { Exam } from '@/types'
import type { ExamInput } from '@/services/exam-service'
import { validateExamDate } from '@/services/exam-service'

const props = defineProps<{ exam?: Exam }>()
const emit = defineEmits<{ submit: [data: ExamInput]; cancel: [] }>()

const formRef = ref<FormInstance>()
const form = reactive<ExamInput>({
  name: props.exam?.name ?? '',
  exam_type: props.exam?.exam_type ?? 'postgrad',
  exam_date: props.exam?.exam_date ?? '',
  total_score: props.exam?.total_score ?? null,
  description: props.exam?.description ?? '',
})

const examTypes = [
  { label: '考研', value: 'postgrad' },
  { label: '考公', value: 'civil' },
  { label: '考证', value: 'cert' },
  { label: '自定义', value: 'custom' },
]

// 禁用今天及之前（考试日期必须晚于今天）
const disabledDate = (date: Date) => {
  const today = new Date()
  today.setHours(0, 0, 0, 0)
  return date.getTime() <= today.getTime()
}

const rules: FormRules<ExamInput> = {
  name: [{ required: true, message: '请输入考试名称', trigger: 'blur' }],
  exam_date: [{ required: true, message: '请选择考试日期', trigger: 'change' }],
}

async function handleSubmit() {
  if (!formRef.value) return
  await formRef.value.validate(async (valid) => {
    if (!valid) return
    try {
      validateExamDate(form.exam_date)
      emit('submit', { ...form })
    } catch (e) {
      ElMessage.error((e as Error).message ?? '日期校验失败')
    }
  })
}
</script>

<template>
  <el-form ref="formRef" :model="form" :rules="rules" label-width="90px" @submit.prevent>
    <el-form-item label="考试类型" prop="exam_type">
      <el-radio-group v-model="form.exam_type">
        <el-radio-button v-for="t in examTypes" :key="t.value" :value="t.value">
          {{ t.label }}
        </el-radio-button>
      </el-radio-group>
    </el-form-item>
    <el-form-item label="考试名称" prop="name">
      <el-input
        v-model="form.name"
        placeholder="如 2027 管理类联考"
        maxlength="60"
        show-word-limit
      />
    </el-form-item>
    <el-form-item label="考试日期" prop="exam_date">
      <el-date-picker
        v-model="form.exam_date"
        type="date"
        placeholder="选择考试日期"
        value-format="YYYY-MM-DD"
        :disabled-date="disabledDate"
        style="width: 100%"
      />
    </el-form-item>
    <el-form-item label="总分">
      <el-input-number v-model="form.total_score" :min="0" :step="50" controls-position="right" />
    </el-form-item>
    <el-form-item label="描述">
      <el-input
        v-model="form.description"
        type="textarea"
        :rows="2"
        placeholder="可选"
        maxlength="200"
        show-word-limit
      />
    </el-form-item>
    <el-form-item>
      <el-button type="primary" @click="handleSubmit">{{ exam ? '保存' : '创建' }}</el-button>
      <el-button @click="emit('cancel')">取消</el-button>
    </el-form-item>
  </el-form>
</template>
