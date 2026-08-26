<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import VueECharts from 'vue-echarts'
import { ensureECharts } from '@/services/echarts-setup'
import { getById } from '@/services/db'
import { knowledgeTree, type KnowledgePointNode, type KnowledgeTreeSubject } from '@/services/agent-client'
import { buildTreeOption, masteryColor, parseSourceRef, splitMaterialParagraphs } from './knowledge-map'

ensureECharts()

const emit = defineEmits<{ (e: 'select', node: KnowledgePointNode): void }>()

const loading = ref(true)
const failed = ref(false)
const subjects = ref<KnowledgeTreeSubject[]>([])
const chartData = computed(() => ({ exam_id: null as string | null, subjects: subjects.value }))
const option = computed(() => buildTreeOption(subjects.value))

onMounted(async () => {
  try {
    const out = await knowledgeTree()
    subjects.value = out.subjects
  } catch {
    failed.value = true
  } finally {
    loading.value = false
  }
})

// ---- 溯源 dialog ----
const selected = ref<KnowledgePointNode | null>(null)
const materialTitle = ref('')
const paragraphs = ref<string[]>([])
const highlighted = ref<Set<number>>(new Set())
const materialLoading = ref(false)
const dialogVisible = ref(false)

function isHighlighted(index: number): boolean {
  return highlighted.value.has(index + 1)
}

async function openEvidence(node: KnowledgePointNode) {
  emit('select', node)
  selected.value = node
  dialogVisible.value = true
  paragraphs.value = []
  highlighted.value = new Set()
  materialTitle.value = ''
  if (!node.source_ref || !node.material_id) return

  materialLoading.value = true
  try {
    // materials 经 db.ts 白名单直查（Task 8 溯源闭环）。
    const material = await getById<{ title: string; content: string }>('materials', node.material_id)
    if (!material) return
    materialTitle.value = material.title
    paragraphs.value = splitMaterialParagraphs(material.content)
    highlighted.value = new Set(parseSourceRef(node.source_ref) ?? [])
  } finally {
    materialLoading.value = false
  }
}

const rangeText = computed(() => {
  if (!selected.value?.source_ref) return ''
  return selected.value.source_ref
})
</script>

<template>
  <div class="mind-map">
    <el-card shadow="never">
      <template #header>
        <div class="map-head">
          <span>知识点思维导图（颜色 = 掌握度：红 1-2 / 黄 3 / 绿 4-5）</span>
          <span v-if="chartData.exam_id" class="legend">
            <i class="dot" :style="{ background: masteryColor(1) }" />薄弱
            <i class="dot" :style="{ background: masteryColor(3) }" />一般
            <i class="dot" :style="{ background: masteryColor(5) }" />扎实
          </span>
        </div>
      </template>

      <div v-loading="loading" class="map-body">
        <VueECharts
          v-if="!loading && subjects.length"
          class="chart"
          :option="option"
          autoresize
          @click="(params: any) => params.data?.raw && openEvidence(params.data.raw)"
        />
        <el-empty
          v-else-if="!loading"
          :description="failed ? '加载失败，请重试' : '请先完成考试配置'"
        />
      </div>
    </el-card>

    <el-dialog v-model="dialogVisible" title="知识点溯源" width="640px">
      <template v-if="selected">
        <div class="evidence-meta">
          <p><strong>{{ selected.name }}</strong></p>
          <p>
            掌握度：<el-tag :color="masteryColor(selected.mastery)" effect="dark" size="small">
              {{ selected.mastery }}/5
            </el-tag>
            错题数：{{ selected.wrong_count }}
            引用：<code>{{ rangeText || '无' }}</code>
          </p>
        </div>

        <div v-loading="materialLoading" class="material">
          <template v-if="paragraphs.length">
            <h4>{{ materialTitle }}</h4>
            <ol class="paragraphs">
              <li
                v-for="(para, index) in paragraphs"
                :key="index"
                :class="{ highlighted: isHighlighted(index) }"
              >
                {{ para }}
              </li>
            </ol>
          </template>
          <el-empty
            v-else-if="!materialLoading && selected.source_ref"
            description="未找到对应材料或段落"
            :image-size="50"
          />
        </div>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped>
.map-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-weight: 600;
}
.legend .dot {
  display: inline-block;
  width: 10px;
  height: 10px;
  border-radius: 50%;
  margin: 0 4px 0 12px;
}
.chart {
  height: 480px;
}
.evidence-meta p {
  margin: 4px 0;
}
.paragraphs li {
  padding: 6px 8px;
  line-height: 1.6;
}
.paragraphs li.highlighted {
  background: var(--el-color-warning-light-9);
  outline: 1px solid var(--el-color-warning-light-5);
  border-radius: 4px;
}
</style>
