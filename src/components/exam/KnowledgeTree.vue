<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useExamStore } from '@/stores/exam'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Plus, Edit, Delete } from '@element-plus/icons-vue'
import KnowledgePointForm from './KnowledgePointForm.vue'
import type { KnowledgePoint } from '@/types'
import type { KnowledgePointInput } from '@/services/exam-service'

const props = defineProps<{ subjectId: string }>()
const store = useExamStore()

const kpDialogVisible = ref(false)
const editingPoint = ref<KnowledgePoint | null>(null)
const parentId = ref<string | null>(null)

const treeProps = { label: 'name', children: 'children' }

onMounted(() => store.loadKnowledgeTree(props.subjectId))

// 扁平化知识点（供 KnowledgePointForm 选父节点 + 防环）
const flatPoints = computed<KnowledgePoint[]>(() => {
  const out: KnowledgePoint[] = []
  const walk = (nodes: KnowledgePoint[]) => {
    nodes.forEach((n) => {
      out.push(n)
      if (n.children) walk(n.children)
    })
  }
  walk(store.knowledgeTree)
  return out
})

function openAdd(pid: string | null = null) {
  editingPoint.value = null
  parentId.value = pid
  kpDialogVisible.value = true
}
function openEdit(node: KnowledgePoint) {
  editingPoint.value = node
  parentId.value = node.parent_id
  kpDialogVisible.value = true
}

async function handleSubmit(data: Partial<KnowledgePointInput>) {
  try {
    if (editingPoint.value) {
      await store.updateKnowledgePoint(editingPoint.value.id, data, props.subjectId)
      ElMessage.success('知识点已更新')
    } else {
      await store.createKnowledgePoint(
        { ...(data as KnowledgePointInput), subject_id: props.subjectId },
        props.subjectId,
      )
      ElMessage.success('知识点已添加')
    }
    kpDialogVisible.value = false
  } catch (e) {
    ElMessage.error((e as Error).message ?? '保存失败')
  }
}

async function handleDelete(node: KnowledgePoint) {
  try {
    await ElMessageBox.confirm(
      `确认删除知识点「${node.name}」？其子知识点将上浮为顶层（不会被连带删除）。`,
      '删除确认',
      { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' },
    )
    await store.deleteKnowledgePoint(node.id, props.subjectId)
    ElMessage.success('已删除')
  } catch (e) {
    if (e !== 'cancel' && e !== 'close') ElMessage.error((e as Error).message ?? '删除失败')
  }
}

// 拖拽排序：收集拖拽后的扁平顺序，持久化 sort_order
function collectIds(tree: KnowledgePoint[]): string[] {
  const out: string[] = []
  const walk = (nodes: KnowledgePoint[]) => {
    nodes.forEach((n) => {
      out.push(n.id)
      if (n.children) walk(n.children)
    })
  }
  walk(tree)
  return out
}
async function onDrop() {
  const ids = collectIds(store.knowledgeTree)
  await store.reorderKnowledgePoints(ids, props.subjectId)
}
</script>

<template>
  <div class="knowledge-tree">
    <div class="knowledge-tree__header">
      <el-button size="small" :icon="Plus" @click="openAdd(null)">添加顶层知识点</el-button>
      <span class="hint">支持拖拽节点排序</span>
    </div>

    <el-empty v-if="!store.knowledgeTree.length" description="还没有知识点，点击左上角添加" />
    <el-tree
      v-else
      :data="store.knowledgeTree"
      :props="treeProps"
      node-key="id"
      draggable
      default-expand-all
      @node-drop="onDrop"
    >
      <template #default="{ data }">
        <div class="tree-node">
          <span class="tree-node__label">{{ data.name }}</span>
          <span v-if="data.chapter" class="tag-tinted chapter-tag">{{ data.chapter }}</span>
          <el-rate :model-value="data.current_mastery" disabled size="small" />
          <span class="tree-node__actions">
            <el-button link :icon="Plus" @click.stop="openAdd(data.id)">加子项</el-button>
            <el-button link :icon="Edit" @click.stop="openEdit(data)">编辑</el-button>
            <el-button link type="danger" :icon="Delete" @click.stop="handleDelete(data)">
              删除
            </el-button>
          </span>
        </div>
      </template>
    </el-tree>

    <el-dialog
      v-model="kpDialogVisible"
      :title="editingPoint ? '编辑知识点' : '添加知识点'"
      width="480px"
      append-to-body
    >
      <KnowledgePointForm
        :point="editingPoint ?? undefined"
        :parent-id="parentId"
        :subject-id="props.subjectId"
        :existing-points="flatPoints"
        @submit="handleSubmit"
        @cancel="kpDialogVisible = false"
      />
    </el-dialog>
  </div>
</template>

<style scoped>
.knowledge-tree__header {
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  margin-bottom: var(--sp-3);
}
.hint {
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
}
.tree-node {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  flex: 1;
  padding-right: var(--sp-2);
}
.tree-node__label {
  font-size: var(--fs-base);
  color: var(--c-ink);
}
.chapter-tag {
  --tag-color: var(--c-info);
}
.tree-node__actions {
  margin-left: auto;
  display: flex;
  gap: var(--sp-1);
}
</style>
