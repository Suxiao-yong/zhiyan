<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import type { FormInstance, FormRules } from 'element-plus'
import type { KnowledgePoint } from '@/types'
import type { KnowledgePointInput } from '@/services/exam-service'

const props = defineProps<{
  point?: KnowledgePoint
  parentId?: string | null
  subjectId: string
  existingPoints: KnowledgePoint[] // 同科目全部知识点（扁平），用于父节点选择与防环
}>()
const emit = defineEmits<{ submit: [data: Partial<KnowledgePointInput>]; cancel: [] }>()

const formRef = ref<FormInstance>()
const form = reactive({
  name: props.point?.name ?? '',
  parent_id: (props.point?.parent_id ?? props.parentId ?? null) as string | null,
  chapter: props.point?.chapter ?? '',
  difficulty_level: props.point?.difficulty_level ?? 3,
  current_mastery: props.point?.current_mastery ?? 3,
  weight: props.point?.weight ?? 1,
})

const masteryLabels = ['', '未掌握', '略懂', '了解', '熟悉', '掌握']

// 编辑时排除自身及其子孙，防止成环
const parentOptions = computed(() => {
  if (!props.point) return props.existingPoints
  const excluded = new Set<string>([props.point.id])
  const walk = (id: string) => {
    props.existingPoints
      .filter((p) => p.parent_id === id)
      .forEach((c) => {
        excluded.add(c.id)
        walk(c.id)
      })
  }
  walk(props.point.id)
  return props.existingPoints.filter((p) => !excluded.has(p.id))
})

const rules: FormRules = {
  name: [{ required: true, message: '请输入知识点名称', trigger: 'blur' }],
}

async function handleSubmit() {
  if (!formRef.value) return
  await formRef.value.validate((valid) => {
    if (!valid) return
    emit('submit', {
      name: form.name,
      parent_id: form.parent_id || null,
      chapter: form.chapter || null,
      difficulty_level: form.difficulty_level,
      current_mastery: form.current_mastery,
      weight: form.weight,
    })
  })
}
</script>

<template>
  <el-form ref="formRef" :model="form" :rules="rules" label-width="90px" @submit.prevent>
    <el-form-item label="名称" prop="name">
      <el-input v-model="form.name" placeholder="如 高等数学-极限" maxlength="50" />
    </el-form-item>
    <el-form-item label="父知识点">
      <el-select v-model="form.parent_id" clearable placeholder="留空为顶层知识点" style="width: 100%">
        <el-option v-for="p in parentOptions" :key="p.id" :label="p.name" :value="p.id" />
      </el-select>
    </el-form-item>
    <el-form-item label="章节">
      <el-input v-model="form.chapter" placeholder="可选" maxlength="30" />
    </el-form-item>
    <el-form-item label="难度">
      <div class="slider-row">
        <el-slider v-model="form.difficulty_level" :min="1" :max="5" show-stops class="slider-row__slider" />
        <span class="hint">{{ form.difficulty_level }} / 5</span>
      </div>
    </el-form-item>
    <el-form-item label="当前掌握">
      <div class="slider-row">
        <el-slider v-model="form.current_mastery" :min="1" :max="5" show-stops class="slider-row__slider" />
        <span class="hint mastery">{{ masteryLabels[form.current_mastery] }}</span>
      </div>
    </el-form-item>
    <el-form-item label="权重">
      <el-input-number v-model="form.weight" :min="0" :step="0.5" :precision="1" controls-position="right" />
    </el-form-item>
    <el-form-item>
      <el-button type="primary" @click="handleSubmit">{{ point ? '保存' : '添加' }}</el-button>
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
.hint {
  font-size: var(--fs-sm);
  color: var(--c-ink-3);
  white-space: nowrap;
  min-width: 60px;
  text-align: right;
}
.hint.mastery {
  color: var(--c-primary);
}
</style>
