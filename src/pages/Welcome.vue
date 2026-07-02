<script setup lang="ts">
import { reactive, ref } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessage } from 'element-plus'
import {
  ArrowRight,
  ArrowLeft,
  Check,
  Plus,
  Delete,
  Aim,
  Lock,
  ChatDotRound,
} from '@element-plus/icons-vue'
import { useExamStore } from '@/stores/exam'
import { setSetting } from '@/services/db'
import { validateExamDate } from '@/services/exam-service'
import { markOnboardingDone } from '@/router'

const router = useRouter()
const store = useExamStore()

const current = ref(0)
const saving = ref(false)

const examTypes = [
  { value: 'postgrad', label: '考研', desc: '全国硕士研究生招生考试' },
  { value: 'civil', label: '考公', desc: '公务员 / 事业单位' },
  { value: 'cert', label: '考证', desc: '职业资格 / 技能证书' },
  { value: 'custom', label: '自定义', desc: '其它考试目标' },
]

interface KpDraft {
  name: string
  chapter: string
  mastery: number
}
interface SubjectDraft {
  name: string
  target_score: number | null
  current_level: number
  weight: number
  knowledgePoints: KpDraft[]
}

const exam = reactive({
  name: '',
  exam_type: 'postgrad',
  exam_date: '',
  total_score: null as number | null,
})
const subjects = reactive<SubjectDraft[]>([
  { name: '', target_score: null, current_level: 3, weight: 1, knowledgePoints: [] },
])

const levelLabels = ['', '入门', '基础', '一般', '熟练', '精通']

const disabledDate = (date: Date) => {
  const t = new Date()
  t.setHours(0, 0, 0, 0)
  return date.getTime() <= t.getTime()
}

function addSubject() {
  subjects.push({ name: '', target_score: null, current_level: 3, weight: 1, knowledgePoints: [] })
}
function removeSubject(i: number) {
  subjects.splice(i, 1)
}
function addKp(s: SubjectDraft) {
  s.knowledgePoints.push({ name: '', chapter: '', mastery: 3 })
}
function removeKp(s: SubjectDraft, i: number) {
  s.knowledgePoints.splice(i, 1)
}

function next() {
  if (current.value === 1) {
    if (!exam.name.trim()) return ElMessage.warning('请输入考试名称')
    try {
      validateExamDate(exam.exam_date)
    } catch (e) {
      return ElMessage.error((e as Error).message)
    }
  }
  if (current.value === 2) {
    const valid = subjects.filter((s) => s.name.trim())
    if (!valid.length) return ElMessage.warning('至少添加一个科目')
    subjects.splice(0, subjects.length, ...valid)
  }
  current.value++
}
function prev() {
  if (current.value > 0) current.value--
}

const totalKpCount = () =>
  subjects.reduce((acc, s) => acc + s.knowledgePoints.filter((k) => k.name.trim()).length, 0)

async function finish() {
  saving.value = true
  try {
    // 1. 创建考试
    const created = await store.createExam({
      name: exam.name.trim(),
      exam_type: exam.exam_type,
      exam_date: exam.exam_date,
      total_score: exam.total_score,
      description: null,
    })
    // 2. 逐科目创建，并写入其知识点（含自评掌握度 current_mastery）
    for (const s of subjects) {
      const subj = await store.createSubject({
        exam_id: created.id,
        name: s.name.trim(),
        target_score: s.target_score,
        current_level: s.current_level,
        weight: s.weight,
      })
      for (const kp of s.knowledgePoints) {
        if (!kp.name.trim()) continue
        await store.createKnowledgePoint({
          subject_id: subj.id,
          name: kp.name.trim(),
          parent_id: null,
          weight: 1,
          difficulty_level: 3,
          current_mastery: kp.mastery,
          chapter: kp.chapter.trim() || null,
        })
      }
    }
    // 3. 标记引导完成
    await setSetting('onboarding_completed', '1', '是否完成首次使用引导')
    markOnboardingDone()
    ElMessage.success('考试配置完成！')
    // 4. 进入学习计划（计划生成 Agent 在 Phase 3 实现）
    router.push('/study-plan')
  } catch (e) {
    ElMessage.error((e as Error).message ?? '保存失败，请重试')
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="welcome">
    <div class="welcome__card">
      <div class="welcome__brand">
        <span class="brand-mark" />
        <h1>智研</h1>
        <p>AI 驱动的个性化学习规划 · 半 Agent 模式：AI 提建议，你做决策</p>
      </div>

      <el-steps :active="current" finish-status="success" align-center>
        <el-step title="欢迎" />
        <el-step title="创建考试" />
        <el-step title="添加科目" />
        <el-step title="水平评估" />
        <el-step title="确认完成" />
      </el-steps>

      <div class="welcome__body">
        <!-- 步骤 0：欢迎 -->
        <div v-show="current === 0" class="step step-intro">
          <h2>欢迎使用智研</h2>
          <p>用 4 步完成你的考试配置：建立考试 → 添加科目 → 评估当前水平 → 生成学习计划。</p>
          <ul class="features">
            <li>
              <el-icon class="features__icon"><Aim /></el-icon>
              <span>
                <b>通用化</b>
                ：支持考研、考公、考证与自定义考试
              </span>
            </li>
            <li>
              <el-icon class="features__icon"><Lock /></el-icon>
              <span>
                <b>本地优先</b>
                ：所有数据存于本地 SQLite，你完全掌控
              </span>
            </li>
            <li>
              <el-icon class="features__icon"><ChatDotRound /></el-icon>
              <span>
                <b>半 Agent</b>
                ：AI 提建议，是否采纳由你决定
              </span>
            </li>
          </ul>
        </div>

        <!-- 步骤 1：创建考试 -->
        <div v-show="current === 1" class="step">
          <h2>创建你的考试</h2>
          <el-form label-width="90px">
            <el-form-item label="考试类型">
              <div class="type-cards">
                <div
                  v-for="t in examTypes"
                  :key="t.value"
                  class="type-card"
                  :class="{ active: exam.exam_type === t.value }"
                  @click="exam.exam_type = t.value"
                >
                  <div class="type-card__label">{{ t.label }}</div>
                  <div class="type-card__desc">{{ t.desc }}</div>
                </div>
              </div>
            </el-form-item>
            <el-form-item label="考试名称">
              <el-input
                v-model="exam.name"
                placeholder="如 2027 管理类联考"
                maxlength="60"
                show-word-limit
              />
            </el-form-item>
            <el-form-item label="考试日期">
              <el-date-picker
                v-model="exam.exam_date"
                type="date"
                placeholder="选择考试日期（须晚于今天）"
                value-format="YYYY-MM-DD"
                :disabled-date="disabledDate"
                class="full-w"
              />
            </el-form-item>
            <el-form-item label="总分">
              <el-input-number
                v-model="exam.total_score"
                :min="0"
                :step="50"
                controls-position="right"
              />
            </el-form-item>
          </el-form>
        </div>

        <!-- 步骤 2：添加科目 -->
        <div v-show="current === 2" class="step">
          <div class="step-head">
            <h2>添加考试科目</h2>
            <el-button :icon="Plus" @click="addSubject">添加科目</el-button>
          </div>
          <div v-for="(s, i) in subjects" :key="i" class="subject-row">
            <el-input v-model="s.name" placeholder="科目名称" class="subject-row__name" />
            <el-input-number
              v-model="s.target_score"
              :min="0"
              placeholder="目标分"
              controls-position="right"
            />
            <div class="subject-row__level">
              <el-slider v-model="s.current_level" :min="1" :max="5" show-stops />
              <span class="tnum">{{ levelLabels[s.current_level] }}</span>
            </div>
            <el-input-number
              v-model="s.weight"
              :min="0"
              :step="0.5"
              :precision="1"
              controls-position="right"
            />
            <el-button :icon="Delete" type="danger" circle @click="removeSubject(i)" />
          </div>
        </div>

        <!-- 步骤 3：知识点水平评估 -->
        <div v-show="current === 3" class="step">
          <div class="step-head step-head--column">
            <h2>基础水平评估</h2>
            <span class="muted">
              为各科添加知识点并自评掌握度（1-5 星），作为 AI
              诊断的初始基准。可跳过，稍后在考试配置中补充。
            </span>
          </div>
          <el-collapse v-for="(s, i) in subjects" :key="i" class="kp-collapse">
            <el-collapse-item
              :title="`${s.name || '科目 ' + (i + 1)}（${s.knowledgePoints.length} 个知识点）`"
              :name="i"
            >
              <div v-for="(kp, j) in s.knowledgePoints" :key="j" class="kp-row">
                <el-input v-model="kp.name" placeholder="知识点名称" class="kp-row__name" />
                <el-input v-model="kp.chapter" placeholder="章节（可选）" class="kp-row__chapter" />
                <el-rate v-model="kp.mastery" />
                <el-button :icon="Delete" type="danger" circle @click="removeKp(s, j)" />
              </div>
              <el-button :icon="Plus" size="small" @click="addKp(s)">添加知识点</el-button>
            </el-collapse-item>
          </el-collapse>
        </div>

        <!-- 步骤 4：确认完成 -->
        <div v-show="current === 4" class="step">
          <h2>确认并完成</h2>
          <el-descriptions :column="1" border>
            <el-descriptions-item label="考试类型">
              {{ examTypes.find((t) => t.value === exam.exam_type)?.label }}
            </el-descriptions-item>
            <el-descriptions-item label="考试名称">{{ exam.name }}</el-descriptions-item>
            <el-descriptions-item label="考试日期">{{ exam.exam_date }}</el-descriptions-item>
            <el-descriptions-item label="总分">{{ exam.total_score ?? '—' }}</el-descriptions-item>
            <el-descriptions-item label="科目数">{{ subjects.length }}</el-descriptions-item>
            <el-descriptions-item label="知识点数">{{ totalKpCount() }}</el-descriptions-item>
          </el-descriptions>
          <el-alert
            type="info"
            :closable="false"
            title="完成配置后将进入学习计划页面。AI 计划生成将在 Phase 3 实现。"
            class="finish-alert"
          />
        </div>
      </div>

      <div class="welcome__footer">
        <el-button v-if="current > 0" :icon="ArrowLeft" @click="prev">上一步</el-button>
        <el-button v-if="current < 4" type="primary" :icon="ArrowRight" @click="next">
          下一步
        </el-button>
        <el-button
          v-if="current === 4"
          type="primary"
          :icon="Check"
          :loading="saving"
          @click="finish"
        >
          完成配置
        </el-button>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* Drenched 深紫罗兰品牌底：同色相径向光晕 + 点阵纹理（非多色渐变） */
.welcome {
  min-height: 100dvh;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--sp-6);
  background-color: #1e1640;
  background-image:
    radial-gradient(circle at 50% 0%, rgba(124, 99, 220, 0.38), transparent 62%),
    radial-gradient(circle at 85% 95%, rgba(109, 93, 200, 0.2), transparent 52%),
    radial-gradient(rgba(255, 255, 255, 0.045) 1px, transparent 1px);
  background-size:
    100% 100%,
    100% 100%,
    22px 22px;
}

.welcome__card {
  width: 100%;
  max-width: 760px;
  background: var(--c-surface);
  border: 1px solid var(--c-border);
  border-radius: var(--r-xl);
  padding: var(--sp-8) var(--sp-10);
  box-shadow: 0 24px 64px rgba(0, 0, 0, 0.32);
}

.welcome__brand {
  text-align: center;
  margin-bottom: var(--sp-6);
}
.brand-mark {
  display: inline-block;
  width: 40px;
  height: 40px;
  border-radius: 10px;
  background: var(--c-primary);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.3),
    0 6px 16px rgba(109, 93, 200, 0.45);
  margin-bottom: var(--sp-3);
}
.welcome__brand h1 {
  font-size: var(--fs-3xl);
  font-weight: 700;
  margin: 0;
  color: var(--c-ink);
  letter-spacing: 3px;
}
.welcome__brand p {
  color: var(--c-ink-3);
  font-size: var(--fs-sm);
  margin: var(--sp-2) 0 0;
}

.welcome__body {
  margin: var(--sp-8) 0;
  min-height: 240px;
}
.step h2 {
  font-size: var(--fs-xl);
  font-weight: 700;
  color: var(--c-ink);
  margin: 0 0 var(--sp-4);
}
.step-intro p {
  color: var(--c-ink-2);
  line-height: 1.8;
  margin: 0 0 var(--sp-4);
}
.features {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}
.features li {
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  padding: var(--sp-3);
  border-radius: var(--r-md);
  background: var(--c-surface-2);
  color: var(--c-ink-2);
  font-size: var(--fs-sm);
}
.features__icon {
  color: var(--c-primary);
  font-size: var(--fs-md);
  flex-shrink: 0;
}
.features b {
  color: var(--c-ink);
  font-weight: 600;
}

.type-cards {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--sp-3);
  width: 100%;
}
.type-card {
  border: 1.5px solid var(--c-border);
  border-radius: var(--r-md);
  padding: var(--sp-3) var(--sp-2);
  text-align: center;
  cursor: pointer;
  transition:
    border-color var(--dur-fast) var(--ease),
    background var(--dur-fast) var(--ease);
}
.type-card:hover {
  border-color: var(--c-border-strong);
}
.type-card.active {
  border-color: var(--c-primary);
  background: var(--c-primary-light);
}
.type-card__label {
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--c-ink);
}
.type-card.active .type-card__label {
  color: var(--c-primary);
}
.type-card__desc {
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
  margin-top: var(--sp-1);
}

.step-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: var(--sp-4);
  gap: var(--sp-4);
}
.step-head--column {
  flex-direction: column;
  align-items: flex-start;
}
.step-head h2 {
  margin: 0;
}
.muted {
  font-size: var(--fs-xs);
  color: var(--c-ink-3);
  max-width: 480px;
  line-height: 1.6;
}
.full-w {
  width: 100%;
}
.subject-row {
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  margin-bottom: var(--sp-3);
  flex-wrap: wrap;
}
.subject-row__name {
  width: 180px;
}
.subject-row__level {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  min-width: 180px;
}
.subject-row__level :deep(.el-slider) {
  width: 120px;
}
.kp-collapse {
  margin-bottom: var(--sp-3);
}
.kp-row {
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  margin-bottom: var(--sp-3);
  flex-wrap: wrap;
}
.kp-row__name {
  width: 200px;
}
.kp-row__chapter {
  width: 150px;
}
.finish-alert {
  margin-top: var(--sp-4);
}
.welcome__footer {
  display: flex;
  justify-content: flex-end;
  gap: var(--sp-3);
  border-top: 1px solid var(--c-border);
  padding-top: var(--sp-5);
}
</style>
