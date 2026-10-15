<script setup lang="ts">
/**
 * 文档翻译：选一个 .txt / .md，整篇翻掉。
 *
 * 关键交互：
 *  - 目标语言用 LangSelect（和主界面同一套 38 种语言），不是自由文本框
 *  - 支持拖拽文件到空白区，比点两次按钮快
 *  - 翻译中显示实时进度条 + 可取消（后端每 120ms 报一次）
 *  - 段落预览只显示前若干段，避免上千段的文档把 DOM 撑爆
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { open, save } from '@tauri-apps/plugin-dialog'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { AlertTriangle, Check, FileText, FolderOpen, LoaderCircle } from '@lucide/vue'
import LangSelect from '../components/LangSelect.vue'
import { api, err, toast } from '../api'
import { meta } from '../composables/useAppState'
import { useJobProgress } from '../composables/useJobProgress'
import type { DocProgress, LoadedDocument } from '../types'

const LANG_KEY = 'hymt2:doc-target'

const doc = ref<LoadedDocument | null>(null)
const targetLang = ref(localStorage.getItem(LANG_KEY) ?? '英语')
const concurrency = ref(3)
const running = ref(false)
const progress = ref<DocProgress | null>(null)
const dragging = ref(false)
const showAll = ref(false)
/** 输出路径。默认由后端按「原文件名 + 目标语言」推导，用户可另存为。 */
const outPath = ref('')
/** 用户是否手动指定过输出路径。指定过就不再被语言切换覆盖。 */
const outPathTouched = ref(false)

const langs = computed<[string, string][]>(() => meta.value.languages)

const { done, total, failed, finished, begin, percent } = useJobProgress()
const currentJob = ref('')

/** LangSelect 以中文名收发；这里顺手落盘，下次打开还是同一方向。 */
function changeTarget(name: string) {
  targetLang.value = name
  localStorage.setItem(LANG_KEY, name)
  // 输出文件名含目标语言，换了语言要重新推导——但用户手动改过就别覆盖。
  void syncOutPath()
}

/** 让后端算出默认输出路径（它知道各语言该用什么后缀）。 */
async function syncOutPath() {
  if (!doc.value || outPathTouched.value) return
  try {
    outPath.value = await api.docOutputPath(doc.value.path, targetLang.value)
  } catch {
    /* 拿不到就让用户自己填 */
  }
}

async function pickOut() {
  try {
    const p = await save({
      title: '选择输出位置',
      defaultPath: outPath.value || 'translated.txt'
    })
    if (p) {
      outPath.value = p
      outPathTouched.value = true
    }
  } catch (e) {
    err(e)
  }
}

// ---------- 载入 ----------

async function load(path: string) {
  try {
    doc.value = await api.docLoad(path)
    progress.value = null
    showAll.value = false
    // 换了文件，上一个文件手动指定的输出路径不再适用。
    outPathTouched.value = false
    await syncOutPath()
    toast(`已载入 ${doc.value.segments.length} 个段落 · ${doc.value.encoding}`, 'success')
  } catch (e) {
    err(e)
  }
}

async function pick() {
  try {
    const path = await open({
      title: '选择文档',
      multiple: false,
      filters: [{ name: '文本文档 / Markdown', extensions: ['txt', 'md', 'markdown'] }]
    })
    if (path) await load(path)
  } catch (e) {
    err(e)
  }
}

/**
 * 拖拽落文件。
 *
 * **不能用 HTML5 的 dragover/drop**：Tauri 默认开启 dragDropEnabled，会拦下
 * 原生拖放，浏览器那套事件根本不触发，而且 dataTransfer.files 里也拿不到
 * 真实文件路径（安全限制）。必须走 Tauri 自己的 onDragDropEvent，它的
 * payload 里是真实路径。
 */
const DRAG_EXT = ['txt', 'md', 'markdown']
let unlistenDrag: (() => void) | null = null

onMounted(async () => {
  try {
    unlistenDrag = await getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload
      if (p.type === 'enter' || p.type === 'over') {
        dragging.value = true
      } else if (p.type === 'leave') {
        dragging.value = false
      } else if (p.type === 'drop') {
        dragging.value = false
        const path = p.paths[0]
        if (!path) return
        const ext = path.split('.').pop()?.toLowerCase() ?? ''
        if (!DRAG_EXT.includes(ext)) {
          toast(`只支持 .txt / .md，收到 .${ext}`, 'error')
          return
        }
        void load(path)
      }
    })
  } catch {
    /* 浏览器预览没有 IPC */
  }
})

onUnmounted(() => unlistenDrag?.())

// ---------- 翻译 ----------

async function run() {
  if (!doc.value) return
  running.value = true
  progress.value = null
  try {
    const jid = await begin(doc.value.segments.filter((s) => s.translatable).length)
    currentJob.value = jid
    const r = await api.docTranslate(
      doc.value.path,
      targetLang.value,
      [],
      concurrency.value,
      jid,
      outPath.value
    )
    progress.value = r
    if (r.cancelled) toast(`已停止 · 已完成 ${r.done}/${r.total} 段，结果已保存`, 'info', 6000)
    else if (r.failed === 0) toast(`翻译完成 → ${r.out_path}`, 'success')
    else toast(`完成 ${r.done}/${r.total}，失败 ${r.failed} 段`, 'error', 6000)
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
    toast('正在停止…（当前段落译完即停）', 'info')
  } catch (e) {
    err(e)
  }
}

function reset() {
  doc.value = null
  progress.value = null
  showAll.value = false
}

const KIND_LABEL: Record<string, string> = {
  prose: '正文',
  heading: '标题',
  list: '列表',
  table: '表格',
  quote: '引用',
  code: '代码',
  frontmatter: '前言'
}

const kindCounts = computed(() => {
  const out: Record<string, number> = {}
  for (const s of doc.value?.segments ?? []) out[s.kind] = (out[s.kind] ?? 0) + 1
  return out
})

const translatable = computed(() => doc.value?.segments.filter((s) => s.translatable).length ?? 0)

/** 预览只渲染前 N 段：大文档全渲染会卡死。 */
const PREVIEW_LIMIT = 60
const previewSegments = computed(() => {
  const segs = doc.value?.segments ?? []
  return showAll.value ? segs : segs.slice(0, PREVIEW_LIMIT)
})

const fileName = computed(() => doc.value?.path.split(/[\\/]/).pop() ?? '')
</script>

<template>
  <div class="doc">
    <!-- 工具栏 -->
    <div class="page-toolbar">
      <div class="toolbar-left">
        <button class="btn btn--primary btn--sm" @click="pick">
          <FolderOpen :size="13" />
          选择文档
        </button>
        <template v-if="doc">
          <span class="file-chip">
            <FileText :size="12" />
            {{ fileName }}
          </span>
          <span class="doc__info">
            {{ doc.kind === 'markdown' ? 'Markdown' : '纯文本' }} · {{ doc.lines }} 行 ·
            {{ doc.encoding }}
          </span>
        </template>
      </div>

      <div class="toolbar-right">
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
          并发
          <input
            v-model.number="concurrency"
            class="input input--num"
            type="number"
            min="1"
            max="8"
            :disabled="running"
            title="同时翻译的段落数。云端 API 可调高，本地推理建议 2–4"
          />
        </label>

        <button v-if="!running" class="btn btn--primary btn--sm" :disabled="!doc" @click="run">
          开始翻译
        </button>
        <template v-else>
          <span class="running-label"><LoaderCircle :size="13" class="spin" /> 翻译中…</span>
          <button class="btn btn--danger btn--sm" @click="cancel">停止</button>
        </template>
      </div>
    </div>

    <!-- 输出路径：开跑前就能看到、能改，而不是翻完才知道文件写哪去了 -->
    <div v-if="doc" class="outrow">
      <label class="outrow__label">输出到</label>
      <input
        v-model="outPath"
        class="input outrow__input"
        :disabled="running"
        placeholder="输出文件路径…"
        @input="outPathTouched = true"
      />
      <button class="btn btn--sm" :disabled="running" @click="pickOut">另存为</button>
    </div>

    <!-- 进度条：仅在跑或刚跑完时出现 -->
    <div v-if="running || finished" class="progress-wrap">
      <div class="progress-bar">
        <div class="progress-fill" :style="{ width: `${percent()}%` }" />
      </div>
      <span class="progress-text">
        {{ done }} / {{ total || '…' }} 段
        <template v-if="failed"> · 失败 {{ failed }}</template>
      </span>
    </div>

    <!-- 结果 -->
    <div v-if="progress" class="doc__result" :class="{ 'doc__result--err': progress.failed > 0 }">
      <p class="doc__result-line">
        <Check v-if="progress.failed === 0 && !progress.cancelled" :size="14" />
        <AlertTriangle v-else :size="14" />
        <strong>{{ progress.done }} / {{ progress.total }}</strong> 段完成
        <span v-if="progress.cancelled"> · 已手动停止</span>
        <span v-if="progress.failed"> · {{ progress.failed }} 段失败</span>
      </p>
      <div class="doc__out">
        输出：<code>{{ progress.out_path }}</code>
      </div>
      <details v-if="progress.errors.length" class="doc__errors">
        <summary>查看失败详情（{{ progress.errors.length }}）</summary>
        <p v-for="[i, msg] in progress.errors" :key="i">段落 {{ i }}：{{ msg }}</p>
      </details>
    </div>

    <!-- 空状态 / 拖拽区 -->
    <div v-if="!doc" class="dropzone" :class="{ 'dropzone--over': dragging }" @click="pick">
      <FileText :size="34" class="dropzone__icon" />
      <span class="dropzone__title">点击选择，或把文件拖到这里</span>
      <span class="dropzone__hint">
        支持 .txt / .md。自动识别 GBK / GB18030 / Big5 / UTF-16；Markdown 会保留标题、列表和代码块
      </span>
    </div>

    <!-- 段落预览 -->
    <template v-else>
      <div class="segments">
        <div class="segments__head">
          <h4 class="segments__title">
            段落预览
            <span class="segments__count"
              >{{ doc.segments.length }} 段 · {{ translatable }} 段待译</span
            >
          </h4>
          <button class="btn btn--ghost btn--sm" @click="reset">换一个文件</button>
        </div>
        <div class="chiprow">
          <span v-for="(v, k) in kindCounts" :key="k" class="tag">
            {{ KIND_LABEL[k] ?? k }} × {{ v }}
          </span>
        </div>
      </div>

      <div class="preview">
        <div
          v-for="s in previewSegments"
          :key="s.index"
          class="preview__seg"
          :class="{ 'preview__seg--skip': !s.translatable }"
        >
          <span class="preview__kind">{{ KIND_LABEL[s.kind] ?? s.kind }}</span>
          <pre class="preview__text">{{ s.text }}</pre>
        </div>
        <button
          v-if="!showAll && (doc.segments.length ?? 0) > PREVIEW_LIMIT"
          class="btn btn--sm preview__more"
          @click="showAll = true"
        >
          显示全部 {{ doc.segments.length }} 段
        </button>
      </div>
    </template>
  </div>
</template>

<style scoped>
.doc {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  background: var(--surface-secondary);
  overflow: hidden;
}

/* ---------- 工具栏 ---------- */
.page-toolbar {
  padding: 12px 24px;
  background: var(--surface-primary);
  border-bottom: 1px solid var(--border-subtle);
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  flex-wrap: wrap;
  flex-shrink: 0;
}

.toolbar-left,
.toolbar-right {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
  flex-wrap: wrap;
}

.file-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  max-width: 260px;
  padding: 3px 10px;
  font-size: 12px;
  font-weight: 500;
  color: var(--text-primary);
  background: var(--surface-tertiary);
  border: 1px solid var(--border-subtle);
  border-radius: var(--r-pill);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.doc__info {
  font-size: 12px;
  color: var(--text-muted);
}

.inline {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12.5px;
  color: var(--text-secondary);
  white-space: nowrap;
}

.input--num {
  width: 62px;
  padding: 5px 8px;
  font-size: 12.5px;
  text-align: center;
}

.running-label {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12.5px;
  color: var(--accent-primary);
}

.spin {
  animation: doc-spin 0.9s linear infinite;
}

@keyframes doc-spin {
  to {
    transform: rotate(360deg);
  }
}

/* ---------- 输出路径 ---------- */
.outrow {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 24px;
  background: var(--surface-primary);
  border-bottom: 1px solid var(--border-subtle);
  flex-shrink: 0;
}

.outrow__label {
  flex-shrink: 0;
  font-size: 12.5px;
  color: var(--text-secondary);
}

.outrow__input {
  flex: 1;
  font-family: var(--font-mono);
  font-size: 11.5px;
}

/* ---------- 进度条 ---------- */
.progress-wrap {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 24px;
  background: var(--surface-primary);
  border-bottom: 1px solid var(--border-subtle);
  flex-shrink: 0;
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

/* ---------- 结果 ---------- */
.doc__result {
  margin: 12px 24px 0;
  padding: 12px 14px;
  font-size: 13px;
  background: color-mix(in srgb, var(--status-online) 8%, transparent);
  border: 1px solid color-mix(in srgb, var(--status-online) 28%, transparent);
  border-radius: var(--r-md);
  flex-shrink: 0;
}

.doc__result--err {
  background: var(--status-danger-bg);
  border-color: color-mix(in srgb, var(--status-danger) 28%, transparent);
}

.doc__result-line {
  display: flex;
  align-items: center;
  gap: 7px;
  margin: 0 0 4px;
}

.doc__out {
  font-size: 12px;
  color: var(--text-secondary);
  word-break: break-all;
}

.doc__out code {
  font-family: var(--font-mono);
  font-size: 11.5px;
}

.doc__errors {
  margin-top: 8px;
  font-size: 12px;
}

/* ---------- 空状态 / 拖拽 ---------- */
.dropzone {
  flex: 1;
  margin: 24px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 32px;
  text-align: center;
  color: var(--text-muted);
  background: var(--surface-primary);
  border: 2px dashed var(--border-subtle);
  border-radius: var(--r-lg);
  cursor: pointer;
  transition: all 0.18s;
}

.dropzone:hover {
  border-color: var(--border-focus);
}

.dropzone--over {
  border-color: var(--accent-primary);
  background: var(--surface-active);
}

.dropzone__icon {
  opacity: 0.45;
}

.dropzone__title {
  font-size: 13.5px;
  font-weight: 500;
  color: var(--text-secondary);
}

.dropzone__hint {
  max-width: 460px;
  font-size: 12px;
  line-height: 1.6;
}

/* ---------- 段落预览 ---------- */
.segments {
  padding: 14px 24px 0;
  flex-shrink: 0;
}

.segments__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 8px;
}

.segments__title {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
}

.segments__count {
  margin-left: 8px;
  font-weight: 400;
  font-size: 12px;
  color: var(--text-muted);
}

.chiprow {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}

.preview {
  flex: 1;
  min-height: 0;
  margin: 12px 24px 20px;
  padding: 6px;
  overflow-y: auto;
  background: var(--surface-primary);
  border: 1px solid var(--border-subtle);
  border-radius: var(--r-lg);
}

.preview__seg {
  display: flex;
  gap: 10px;
  padding: 8px 10px;
  border-radius: var(--r-sm);
}

.preview__seg:hover {
  background: var(--surface-hover);
}

.preview__seg--skip {
  opacity: 0.55;
}

.preview__kind {
  flex-shrink: 0;
  width: 46px;
  padding-top: 2px;
  font-size: 10.5px;
  color: var(--text-muted);
  text-align: right;
}

.preview__text {
  flex: 1;
  min-width: 0;
  margin: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.65;
  color: var(--text-primary);
  white-space: pre-wrap;
  word-break: break-word;
}

.preview__more {
  width: 100%;
  margin-top: 6px;
}
</style>
