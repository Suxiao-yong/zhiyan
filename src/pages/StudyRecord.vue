<script setup lang="ts">
import { ref } from 'vue'
import { Plus } from '@element-plus/icons-vue'
import PageHeader from '@/components/common/PageHeader.vue'
import RecordCalendar from '@/components/record/RecordCalendar.vue'
import RecordList from '@/components/record/RecordList.vue'
import WrongQuestionList from '@/components/record/WrongQuestionList.vue'
import QuickRecordDialog from '@/components/record/QuickRecordDialog.vue'

const activeTab = ref('records')
const quickVisible = ref(false)
const preselectDate = ref<string | undefined>(undefined)
const calendarRef = ref<InstanceType<typeof RecordCalendar> | null>(null)
const listRef = ref<InstanceType<typeof RecordList> | null>(null)

function onSelectDate(date: string) {
  preselectDate.value = date
  activeTab.value = 'records'
}
function onSaved() {
  preselectDate.value = undefined
  calendarRef.value?.refresh()
  listRef.value?.refresh()
}
</script>

<template>
  <div>
    <PageHeader title="学习记录" subtitle="记录每日学习，做题联动错题；跨天 04:00 自动归一化">
      <template #actions>
        <el-button type="primary" :icon="Plus" @click="quickVisible = true">快速记录</el-button>
      </template>
    </PageHeader>

    <el-tabs v-model="activeTab">
      <el-tab-pane label="学习记录" name="records">
        <RecordCalendar ref="calendarRef" @select-date="onSelectDate" />
        <RecordList ref="listRef" :preselect-date="preselectDate" />
      </el-tab-pane>
      <el-tab-pane label="错题库" name="wrong">
        <WrongQuestionList />
      </el-tab-pane>
    </el-tabs>

    <QuickRecordDialog v-model="quickVisible" @saved="onSaved" />
  </div>
</template>
