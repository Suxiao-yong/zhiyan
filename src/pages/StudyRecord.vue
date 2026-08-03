<script setup lang="ts">
import { ref } from 'vue'
import { useRoute } from 'vue-router'
import { Plus } from '@element-plus/icons-vue'
import PageHeader from '@/components/common/PageHeader.vue'
import RecordCalendar from '@/components/record/RecordCalendar.vue'
import RecordList from '@/components/record/RecordList.vue'
import WrongQuestionList from '@/components/record/WrongQuestionList.vue'
import QuickRecordDialog from '@/components/record/QuickRecordDialog.vue'
import PlanCheckinBoard from '@/components/record/PlanCheckinBoard.vue'
import { initialStudyRecordTab } from '@/components/record/checkin-ui'

// M6 Task 4: the workbench host mounts this page with initial-tab to land on
// the records list; the full page still supports ?tab= and ?date= routing.
const props = defineProps<{ initialTab?: 'checkin' | 'records' | 'wrong' }>()

const route = useRoute()
const activeTab = ref(props.initialTab ?? initialStudyRecordTab(route.query.date))
const quickVisible = ref(false)
const preselectDate = ref<string | undefined>(undefined)
const calendarRef = ref<InstanceType<typeof RecordCalendar> | null>(null)
const listRef = ref<InstanceType<typeof RecordList> | null>(null)
const boardRef = ref<InstanceType<typeof PlanCheckinBoard> | null>(null)

function onSelectDate(date: string) {
  preselectDate.value = date
  activeTab.value = 'records'
}
function onSaved() {
  preselectDate.value = undefined
  calendarRef.value?.refresh()
  listRef.value?.refresh()
  boardRef.value?.refresh()
}
function onHistoryChanged() {
  calendarRef.value?.refresh()
  boardRef.value?.refresh()
}
</script>

<template>
  <div>
    <PageHeader title="学习记录" subtitle="按学习计划逐项打卡，也可记录计划外的临时学习">
      <template #actions>
        <el-button :icon="Plus" @click="quickVisible = true">自由记录</el-button>
      </template>
    </PageHeader>

    <el-tabs v-model="activeTab">
      <el-tab-pane label="计划打卡" name="checkin">
        <PlanCheckinBoard ref="boardRef" @changed="onSaved" />
      </el-tab-pane>
      <el-tab-pane label="历史记录" name="records">
        <RecordCalendar ref="calendarRef" @select-date="onSelectDate" />
        <RecordList ref="listRef" :preselect-date="preselectDate" @changed="onHistoryChanged" />
      </el-tab-pane>
      <el-tab-pane label="错题库" name="wrong">
        <WrongQuestionList />
      </el-tab-pane>
    </el-tabs>

    <QuickRecordDialog v-model="quickVisible" @saved="onSaved" />
  </div>
</template>
