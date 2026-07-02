<script setup lang="ts">
import { reactive, ref } from 'vue'
import type { FormInstance, FormRules } from 'element-plus'
import type { Subject } from '@/types'
import type { SubjectInput } from '@/services/exam-service'

const props = defineProps<{ subject?: Subject; examId: string }>()
const emit = defineEmits<{ submit: [data: SubjectInput]; cancel: [] }>()

const formRef = ref<FormInstance>()
const form = reactive({
  name: props.subject?.name ?? '',
  target_score: props.subject?.target_score ?? null,
  current_level: props.subject?.current_level ?? 3,
  weight: props.subject?.weight ?? 1,
})

const levelLabels = ['', '入门', '基础', '一般', '熟练', '精通']

const rules: FormRules = {
  name: [{ required: true, message: '请输入科目名称', trigger: 'blur' }],
}

async function handleSubmit() {
  if (!formRef.value) return
  await formRef.value.validate((valid) => {
    if (!valid) return
    emit('submit', {
      exam_id: props.examId,
      name: form.name,
      target_score: form.target_score,
      current_level: form.current_level,
      weight: form.weight,
    })
  })
}
</script>

<template>
  <el-form ref="formRef" :model="form" :rules="rules" label-width="90px" @submit.prevent>
    <el-form-item label="科目名称" prop="name">
      <el-input v-model="form.name" placeholder="如 数学" maxlength="40" />
    </el-form-item>
    <el-form-item label="目标分数">
      <el-input-number v-model="form.target_score" :min="0" controls-position="right" />
    </el-form-item>
    <el-form-item label="当前水平">
      <div class="slider-row">
        <el-slider
          v-model="form.current_level"
          :min="1"
          :max="5"
          show-stops
          class="slider-row__slider"
        />
        <span class="level-hint">{{ levelLabels[form.current_level] }}</span>
      </div>
    </el-form-item>
    <el-form-item label="权重">
      <el-input-number
        v-model="form.weight"
        :min="0"
        :step="0.5"
        :precision="1"
        controls-position="right"
      />
    </el-form-item>
    <el-form-item>
      <el-button type="primary" @click="handleSubmit">{{ subject ? '保存' : '添加' }}</el-button>
      <el-button @click="emit('cancel')">取消</el-button>
    </el-form-item>
  </el-form>
</template>

<style scoped>
.slider-row {
  display: flex;
  align-items: center;
  gap: var(--sp-4);
  width: 100%;
}
.slider-row__slider {
  flex: 1;
}
.level-hint {
  font-size: var(--fs-sm);
  color: var(--c-primary);
  white-space: nowrap;
  min-width: 40px;
}
</style>
