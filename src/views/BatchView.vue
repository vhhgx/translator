<script setup lang="ts">
/**
 * 批量翻译：一行一条 JSON 的 JSONL 文件，并发翻译后追加 `translation` 字段。
 *
 * 关键交互：
 *  - 目标语言用 LangSelect（和主界面同一套），不是自由文本框
 *  - 输入/输出路径可点选，也可直接把文件拖进来
 *  - 翻译中显示实时进度条 + 可取消
 *  - 「断点续跑」按记录身份跳过已译条目（后端已修：以前会重复输出）
 */
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { open, save } from '@tauri-apps/plugin-dialog'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { AlertTriangle, Check, FileJson, FolderOpen, LoaderCircle } from '@lucide/vue'
import LangSelect from '../components/LangSelect.vue'
import { api, err, toast } from '../api'
import { meta } from '../composables/useAppState'
import { useJobProgress } from '../composables/useJobProgress'
import type { BatchProgress } from '../types'

const LANG_KEY = 'hymt2:batch-target'

const inputPath = ref('')
const outputPath = ref('')
const targetLang = ref(localStorage.getItem(LANG_KEY) ?? '英语')
const textField = ref('text')
const concurrency = ref(3)
const resume = ref(true)
const running = ref(false)
const result = ref<BatchProgress | null>(null)
const dragging = ref(false)

const langs = computed<[string, string][]>(() => meta.value.languages)

const { done, total, failed, finished, begin, percent } = useJobProgress()
const currentJob = ref('')

function changeTarget(name: string) {
  targetLang.value = name
  localStorage.setItem(LANG_KEY, name)
}

/** 输入路径一变，输出路径跟着改（用户手动改过就别覆盖）。 */
function setInput(p: string) {
  inputPath.value = p
  if (!outputPath.value || outputPath.value === autoOutput.value) {
    outputPath.value = p.replace(/(\.[^.]+)?$/, '.translated.jsonl')
  }
}

const autoOutput = computed(() =>
  inputPath.value ? inputPath.value.replace(/(\.[^.]+)?$/, '.translated.jsonl') : ''
)

async function pickInput() {
  try {
    const p = await open({
      title: '选择 JSONL 文件',
      multiple: false,
      filters: [{ name: 'JSONL', extensions: ['jsonl', 'ndjson', 'json'] }]
    })
    if (p) setInput(p)
  } catch (e) {
    err(e)
  }
}

async function pickOutput() {
  try {
    const p = await save({
      title: '选择输出位置',
      defaultPath: outputPath.value || 'output.jsonl',
      filters: [{ name: 'JSONL', extensions: ['jsonl'] }]
    })
    if (p) outputPath.value = p
  } catch (e) {
    err(e)
  }
}

/** 输入文件拖进来。同 DocumentView：必须走 Tauri 原生拖放事件。 */
let unlistenDrag: (() => void) | null = null

onMounted(async () => {
  try {
    unlistenDrag = await getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload
      if (p.type === 'enter' || p.type === 'over') dragging.value = true
      else if (p.type === 'leave') dragging.value = false
      else if (p.type === 'drop') {
        dragging.value = false
        if (p.paths[0]) setInput(p.paths[0])
      }
    })
  } catch {
    /* 浏览器预览没有 IPC */
  }
})

onUnmounted(() => unlistenDrag?.())

async function run() {
  if (!inputPath.value || !outputPath.value) {
    toast('请先选择输入和输出路径')
    return
  }
  running.value = true
  result.value = null
  try {
    const jid = await begin()
    currentJob.value = jid
    const r = await api.batchTranslate(
      inputPath.value,
      outputPath.value,
      targetLang.value,
      textField.value,
      concurrency.value,
      resume.value,
      jid
    )
    result.value = r
    if (r.cancelled) toast(`已停止 · 已完成 ${r.done}/${r.total}，结果已保存`, 'info', 6000)
    else if (r.failed === 0) toast(`完成 ${r.done}/${r.total}`, 'success')
    else toast(`完成 ${r.done}/${r.total}，失败 ${r.failed}`, 'error', 6000)
  } catch (e) {
    err(e)
  } finally {
    running.value = false
  }
}

async function cancel() {
  if (!currentJob.value) return
  try {
    await api.cancelJob(currentJob.value)
    toast('正在停止…（当前条目译完即停）', 'info')
  } catch (e) {
    err(e)
  }
}

/** 输入路径变化时同步输出默认值。 */
watch(inputPath, () => {
  if (!outputPath.value) outputPath.value = autoOutput.value
})
</script>

<template>
  <div class="batch">
    <div class="page-toolbar">
      <div class="toolbar-left">
        <h3 class="page-title">
          <FileJson :size="15" />
          批量翻译
        </h3>
        <span class="page-sub">每行一个 JSON，输出追加 translation 字段</span>
      </div>
    </div>

    <div class="batch__body">
      <div class="card">
        <div class="field">
          <label class="label">输入文件</label>
          <div class="row">
            <input
              v-model="inputPath"
              class="input"
              placeholder="选择 .jsonl 文件，或把文件拖到这里…"
            />
            <button class="btn btn--sm" @click="pickInput">
              <FolderOpen :size="13" />
              选择
            </button>
          </div>
        </div>

        <div class="field">
          <label class="label">输出文件</label>
          <div class="row">
            <input v-model="outputPath" class="input" placeholder="输出路径…" />
            <button class="btn btn--sm" @click="pickOutput">另存为</button>
          </div>
        </div>

        <div class="options">
          <label class="inline">
            目标语言
            <LangSelect
              :model-value="targetLang"
              :options="langs"
              show-label
              @update:model-value="changeTarget"
            />
          </label>
          <label class="inline">
            文本字段
            <input
              v-model="textField"
              class="input input--field"
              placeholder="text"
              title="从每行 JSON 的哪个字段取待译文本"
            />
          </label>
          <label class="inline">
            并发
            <input
              v-model.number="concurrency"
              class="input input--num"
              type="number"
              min="1"
              max="16"
              :disabled="running"
            />
          </label>
          <label class="inline inline--check">
            <input v-model="resume" type="checkbox" :disabled="running" />
            断点续跑
          </label>
        </div>

        <div class="actions">
          <button v-if="!running" class="btn btn--primary" :disabled="!inputPath" @click="run">
            开始批量翻译
          </button>
          <template v-else>
            <span class="running-label"><LoaderCircle :size="14" class="spin" /> 翻译中…</span>
            <button class="btn btn--danger" @click="cancel">停止</button>
          </template>
        </div>
      </div>

      <!-- 进度 -->
      <div v-if="running || finished" class="progress-wrap">
        <div class="progress-bar">
          <div class="progress-fill" :style="{ width: `${percent()}%` }" />
        </div>
        <span class="progress-text">
          {{ done }} / {{ total || '…' }} 条
          <template v-if="failed"> · 失败 {{ failed }}</template>
        </span>
      </div>

      <!-- 结果 -->
      <div v-if="result" class="card result" :class="{ 'result--err': result.failed > 0 }">
        <p class="result__line">
          <Check v-if="result.failed === 0 && !result.cancelled" :size="14" />
          <AlertTriangle v-else :size="14" />
          共 <strong>{{ result.total }}</strong> 条，成功
          <strong>{{ result.done }}</strong> 条<template v-if="result.failed"
            >，失败 <strong>{{ result.failed }}</strong> 条</template
          >
          <span v-if="result.cancelled"> · 已手动停止</span>
        </p>
        <p class="muted result__out"
          >输出：<code>{{ result.output }}</code></p
        >
      </div>

      <div class="card">
        <h3 class="card__title">说明</h3>
        <ul class="muted">
          <li>输入示例：<code>{'{"id":1,"text":"今天天气真好"}'}</code></li>
          <li>输出会在每行追加 <code>translation</code> 字段，其余字段原样保留。</li>
          <li>「断点续跑」会按记录内容跳过已译过的条目，不会重复输出。</li>
          <li>本地 CPU 推理较慢，建议并发 2–4；云端 API 可调高。</li>
        </ul>
      </div>
    </div>
  </div>
</template>

<style scoped>
.batch {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  background: var(--surface-secondary);
  overflow: hidden;
}

.page-toolbar {
  padding: 12px 24px;
  background: var(--surface-primary);
  border-bottom: 1px solid var(--border-subtle);
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  flex-shrink: 0;
}

.toolbar-left {
  display: flex;
  align-items: baseline;
  gap: 10px;
  min-width: 0;
}

.page-title {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  margin: 0;
  font-size: 13.5px;
  font-weight: 600;
  color: var(--text-primary);
}

.page-sub {
  font-size: 12px;
  color: var(--text-muted);
}

.batch__body {
  flex: 1;
  min-height: 0;
  padding: 16px 24px 24px;
  overflow-y: auto;
}

.card {
  margin-bottom: 12px;
}

.row {
  display: flex;
  gap: 8px;
}

.options {
  display: flex;
  align-items: center;
  gap: 18px;
  flex-wrap: wrap;
  margin-bottom: 14px;
}

.inline {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12.5px;
  color: var(--text-secondary);
  white-space: nowrap;
}

.inline--check {
  cursor: pointer;
}

.input--num {
  width: 62px;
  padding: 5px 8px;
  font-size: 12.5px;
  text-align: center;
}

.input--field {
  width: 96px;
  padding: 5px 8px;
  font-size: 12.5px;
}

.actions {
  display: flex;
  align-items: center;
  gap: 12px;
}

.running-label {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12.5px;
  color: var(--accent-primary);
}

.spin {
  animation: batch-spin 0.9s linear infinite;
}

@keyframes batch-spin {
  to {
    transform: rotate(360deg);
  }
}

/* 进度条 */
.progress-wrap {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 12px;
  padding: 10px 14px;
  background: var(--surface-primary);
  border: 1px solid var(--border-subtle);
  border-radius: var(--r-md);
}

.progress-bar {
  flex: 1;
  height: 5px;
  border-radius: var(--r-pill);
  background: var(--surface-tertiary);
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  background: var(--accent-primary);
  border-radius: var(--r-pill);
  transition: width 0.25s ease;
}

.progress-text {
  font-size: 12px;
  color: var(--text-secondary);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

/* 结果 */
.result {
  border-color: color-mix(in srgb, var(--status-online) 28%, transparent);
  background: color-mix(in srgb, var(--status-online) 8%, transparent);
}

.result--err {
  background: var(--status-danger-bg);
  border-color: color-mix(in srgb, var(--status-danger) 28%, transparent);
}

.result__line {
  display: flex;
  align-items: center;
  gap: 7px;
  margin: 0 0 4px;
  font-size: 13px;
}

.result__out {
  margin: 0;
  word-break: break-all;
}

code {
  padding: 1px 5px;
  font-family: var(--font-mono);
  font-size: 11.5px;
  background: var(--surface-tertiary);
  border-radius: var(--r-sm);
}

.muted {
  font-size: 12.5px;
  color: var(--text-secondary);
  line-height: 1.8;
}

.muted ul {
  margin: 0;
  padding-left: 20px;
}
</style>
