<script setup lang="ts">
/**
 * 划词翻译弹窗。
 *
 * 它是常驻的透明无边框窗口（`label: "popup"`），由 Rust 侧的全局鼠标监听
 * 在 `hymt2:selection` 事件里带着选中文本唤出。翻译走和主窗口完全相同的
 * `translate_stream`，靠 job_id 过滤事件，两边互不串台。
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { Check, Copy, LoaderCircle, X } from '@lucide/vue'
import { api, err } from '../api'
import { buildRequest, loadMeta, meta } from '../composables/useAppState'

const source = ref('')
const result = ref('')
const failed = ref('')
const busy = ref(false)
const copied = ref(false)

const LANG_KEY = 'hymt2:popup-target'
const target = ref(localStorage.getItem(LANG_KEY) ?? '')

/** 中日韩文字，用来猜翻译方向。 */
const CJK = /[\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]/
const guessTarget = (t: string) => (CJK.test(t) ? 'en' : 'zh')

const langs = computed(() => meta.value.languages)

const unlisten: Array<() => void> = []
let currentJob = ''

function clearListeners() {
  unlisten.forEach((f) => f())
  unlisten.length = 0
}

async function run() {
  const text = source.value.trim()
  if (!text) return
  if (!target.value) {
    target.value = guessTarget(text)
    localStorage.setItem(LANG_KEY, target.value)
  }

  clearListeners()
  const jobId = `sel-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
  currentJob = jobId
  result.value = ''
  failed.value = ''
  busy.value = true

  try {
    unlisten.push(
      await listen<{ job_id: string; text: string }>('hymt2:delta', (e) => {
        if (e.payload.job_id !== currentJob) return
        result.value += e.payload.text
      })
    )
    unlisten.push(
      await listen<{ job_id: string; text: string }>('hymt2:done', (e) => {
        if (e.payload.job_id !== currentJob) return
        result.value = e.payload.text
        busy.value = false
      })
    )
    unlisten.push(
      await listen<{ job_id: string; message: string }>('hymt2:error', (e) => {
        if (e.payload.job_id !== currentJob) return
        failed.value = e.payload.message
        busy.value = false
      })
    )

    const req = buildRequest(
      {
        text,
        mode: 'basic',
        terms: [],
        auto_filter_terms: false,
        style: '',
        format_type: '',
        context: '',
        ignore_glossary: true
      },
      target.value
    )
    await api.translateStream(req, jobId)
  } catch (e) {
    failed.value = String(e)
    busy.value = false
  }
}

function changeTarget(v: string) {
  target.value = v
  localStorage.setItem(LANG_KEY, v)
  run()
}

async function copyResult() {
  if (!result.value) return
  try {
    await api.copyText(result.value)
    copied.value = true
    setTimeout(() => (copied.value = false), 1200)
  } catch (e) {
    err(e)
  }
}

async function close() {
  clearListeners()
  try {
    await api.hideSelectionPopup()
  } catch {
    /* 浏览器预览没有 IPC，忽略 */
  }
}

onMounted(async () => {
  try {
    await loadMeta()
  } catch {
    /* meta 里已有静态兜底语言表 */
  }

  await listen<{ text: string }>('hymt2:selection', (e) => {
    source.value = e.payload.text
    run()
  })

  // 首次弹出时这个窗口才刚被 realize、webview 才刚开始加载，上面那条
  // listen 来不及注册，触发它的那次事件必然收不到——主动取一次。
  // currentJob 为空说明事件确实没收到，避免同一次划词翻两遍。
  try {
    const pending = await api.takePendingSelection()
    if (pending && !currentJob) {
      source.value = pending
      run()
    }
  } catch {
    /* 浏览器预览没有 IPC */
  }
})

onUnmounted(clearListeners)
</script>

<template>
  <div class="pop">
    <header class="pop__bar">
      <span class="pop__title">划词翻译</span>

      <select
        class="pop__lang"
        :value="target"
        title="目标语言"
        @change="changeTarget(($event.target as HTMLSelectElement).value)"
      >
        <option v-for="[code, name] in langs" :key="code" :value="code">{{ name }}</option>
      </select>

      <span class="pop__spacer" />

      <button class="pop__btn" :disabled="!result" title="复制译文" @click="copyResult">
        <component :is="copied ? Check : Copy" :size="14" />
      </button>
      <button class="pop__btn" title="关闭" @click="close">
        <X :size="14" />
      </button>
    </header>

    <p class="pop__src" :title="source">{{ source }}</p>

    <div class="pop__body">
      <p v-if="failed" class="pop__err">{{ failed }}</p>
      <p v-else class="pop__out">
        {{ result }}<LoaderCircle v-if="busy" class="pop__spin" :size="13" />
      </p>
    </div>
  </div>
</template>

<style scoped>
/* 外层留 8px 给投影——透明窗口里圆角外侧要能看见阴影 */
.pop {
  display: flex;
  flex-direction: column;
  height: calc(100% - 16px);
  margin: 8px;
  background: var(--c-surface);
  border: 1px solid var(--c-border-soft);
  border-radius: var(--r-md);
  box-shadow: var(--shadow-float);
  overflow: hidden;
}

.pop__bar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 8px 8px 12px;
  background: var(--c-surface-alt);
  border-bottom: 1px solid var(--c-border-soft);
  flex-shrink: 0;
}

.pop__title {
  font-size: 12px;
  font-weight: 600;
  color: var(--c-text-secondary);
}

.pop__lang {
  font-family: inherit;
  font-size: 12px;
  color: var(--c-text);
  background: var(--c-surface);
  border: 1px solid var(--c-border);
  border-radius: var(--r-sm);
  padding: 3px 6px;
  cursor: pointer;
}

.pop__spacer {
  flex: 1;
}

.pop__btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  color: var(--c-text-secondary);
  background: none;
  border: none;
  border-radius: var(--r-sm);
  cursor: pointer;
}

.pop__btn:hover:not(:disabled) {
  background: var(--c-surface-hover);
  color: var(--c-text);
}

.pop__btn:disabled {
  opacity: 0.35;
  cursor: default;
}

/* 原文压成两行，把空间留给译文 */
.pop__src {
  margin: 0;
  padding: 8px 12px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--c-text-tertiary);
  border-bottom: 1px solid var(--c-border-soft);
  display: -webkit-box;
  -webkit-line-clamp: 2;
  line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
  flex-shrink: 0;
}

.pop__body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 10px 12px;
}

.pop__out {
  margin: 0;
  font-size: 14px;
  line-height: 1.65;
  white-space: pre-wrap;
  word-break: break-word;
}

.pop__err {
  margin: 0;
  font-size: 13px;
  line-height: 1.6;
  color: var(--c-danger);
  word-break: break-word;
}

.pop__spin {
  margin-left: 4px;
  vertical-align: -2px;
  animation: pop-spin 0.9s linear infinite;
}

@keyframes pop-spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
