<script setup lang="ts">
/**
 * 小窗模式：全局热键唤出的「输入 + 译文」轻量窗口。
 *
 * 它**不碰主窗口**——按热键就是想快速译一句，不该再弹出 960×640 的主界面。
 * 想进主窗口，用右上角那个按钮。
 *
 * 窗口是常驻的（关闭只隐藏），所以复位逻辑走 `hymt2:mini-shown` 事件，
 * 不能依赖 onMounted。
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { ArrowLeftRight, ClipboardPaste, LoaderCircle, PanelTop, X } from '@lucide/vue'
import { api, err } from '../api'
import { buildRequest, loadMeta, meta } from '../composables/useAppState'

const source = ref('')
const result = ref('')
const failed = ref('')
const busy = ref(false)
const input = ref<HTMLTextAreaElement | null>(null)

/** 和划词弹窗共用同一个目标语言选择，两个「快速翻译」入口方向保持一致。 */
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
  const jobId = `mini-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
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

/** 换方向：把当前目标语言和猜测的源方向对调，顺手重译一次。 */
function swapDirection() {
  const next = target.value === 'zh' ? 'en' : 'zh'
  changeTarget(next)
}

function changeTarget(v: string) {
  target.value = v
  localStorage.setItem(LANG_KEY, v)
  if (source.value.trim()) run()
}

/** Enter 翻译，Shift+Enter 换行——小窗里没有按钮可点，键盘就是主操作。 */
function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
    e.preventDefault()
    run()
  }
}

async function pasteFromClipboard() {
  try {
    const t = await api.readClipboardText()
    if (!t.trim()) return
    source.value = t
    focusInput()
    run()
  } catch (e) {
    err(e)
  }
}

function focusInput() {
  const el = input.value
  if (!el) return
  el.focus()
  // 选中已有内容：接着敲字直接覆盖，不用先手动清空
  el.select()
}

async function openMain() {
  try {
    await api.showWindow()
  } catch (e) {
    err(e)
  }
  await close()
}

async function close() {
  clearListeners()
  try {
    await api.hideMini()
  } catch {
    /* 浏览器预览没有 IPC */
  }
}

/** Esc 在整个窗口内都生效，不只在输入框里。 */
function onEsc(e: KeyboardEvent) {
  if (e.key === 'Escape') close()
}

onMounted(async () => {
  try {
    await loadMeta()
  } catch {
    /* meta 里已有静态兜底语言表 */
  }

  // 热键每次唤出都发这个事件——窗口是常驻的，不重启就没有 onMounted
  unlisten.push(
    await listen('hymt2:mini-shown', () => {
      focusInput()
    })
  )
  window.addEventListener('keydown', onEsc)
  focusInput()
})

onUnmounted(() => {
  clearListeners()
  window.removeEventListener('keydown', onEsc)
})
</script>

<template>
  <div class="mini">
    <header class="mini__bar" @dblclick="openMain">
      <span class="mini__title">快速翻译</span>
      <select
        class="mini__lang"
        :value="target"
        title="目标语言"
        @change="changeTarget(($event.target as HTMLSelectElement).value)"
      >
        <option v-for="[code, name] in langs" :key="code" :value="code">{{ name }}</option>
      </select>
      <button class="mini__btn" title="对调中英方向" @click="swapDirection">
        <ArrowLeftRight :size="14" />
      </button>

      <span class="mini__spacer" />

      <button class="mini__btn" title="从剪贴板填入" @click="pasteFromClipboard">
        <ClipboardPaste :size="14" />
      </button>
      <button class="mini__btn" title="在主窗口中打开" @click="openMain">
        <PanelTop :size="14" />
      </button>
      <button class="mini__btn" title="关闭（Esc）" @click="close">
        <X :size="14" />
      </button>
    </header>

    <textarea
      ref="input"
      v-model="source"
      class="mini__input"
      placeholder="输入要翻译的内容，Enter 翻译，Shift+Enter 换行"
      spellcheck="false"
      @keydown="onKeydown"
    />

    <div class="mini__out">
      <p v-if="failed" class="mini__err">{{ failed }}</p>
      <p v-else-if="!result && !busy" class="mini__hint">译文会显示在这里</p>
      <p v-else class="mini__text">
        {{ result }}<LoaderCircle v-if="busy" class="mini__spin" :size="13" />
      </p>
    </div>
  </div>
</template>

<style scoped>
.mini {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--c-bg);
}

.mini__bar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 8px 8px 12px;
  background: var(--c-surface-alt);
  border-bottom: 1px solid var(--c-border-soft);
  flex-shrink: 0;
  user-select: none;
}

.mini__title {
  font-size: 12px;
  font-weight: 600;
  color: var(--c-text-secondary);
}

.mini__lang {
  font-family: inherit;
  font-size: 12px;
  color: var(--c-text);
  background: var(--c-surface);
  border: 1px solid var(--c-border);
  border-radius: var(--r-sm);
  padding: 3px 6px;
  cursor: pointer;
}

.mini__spacer {
  flex: 1;
}

.mini__btn {
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
  flex-shrink: 0;
}

.mini__btn:hover {
  background: var(--c-surface-hover);
  color: var(--c-text);
}

.mini__input {
  flex: 0 0 auto;
  height: 34%;
  min-height: 68px;
  margin: 10px 12px 0;
  padding: 8px 10px;
  font-family: inherit;
  font-size: 14px;
  line-height: 1.6;
  color: var(--c-text);
  background: var(--c-surface);
  border: 1px solid var(--c-border);
  border-radius: var(--r-sm);
  resize: none;
  outline: none;
}

.mini__input:focus {
  border-color: var(--c-primary);
}

.mini__out {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  margin: 10px 12px 12px;
  padding: 8px 10px;
  background: var(--c-surface);
  border: 1px solid var(--c-border-soft);
  border-radius: var(--r-sm);
}

.mini__text {
  margin: 0;
  font-size: 14px;
  line-height: 1.65;
  white-space: pre-wrap;
  word-break: break-word;
}

.mini__hint,
.mini__err {
  margin: 0;
  font-size: 13px;
  line-height: 1.6;
  color: var(--c-text-tertiary);
}

.mini__err {
  color: var(--c-danger);
  word-break: break-word;
}

.mini__spin {
  margin-left: 4px;
  vertical-align: -2px;
  animation: mini-spin 0.9s linear infinite;
}

@keyframes mini-spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
