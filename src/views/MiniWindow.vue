<script setup lang="ts">
/**
 * 小窗模式：全局热键唤出的「输入 + 译文」轻量窗口。
 *
 * 它**不碰主窗口**——按热键就是想快速译一句，不该再弹出主界面。
 * 想进主窗口，用右上角那个按钮。
 *
 * 翻译行为和主窗口完全一致：输入即自动翻译，长文只改末尾时走增量翻译
 * （见 useAutoTranslate）。Enter 不再触发翻译，只用来换行。
 *
 * 窗口是常驻的（关闭只隐藏），所以复位逻辑走 `hymt2:mini-shown` 事件，
 * 不能依赖 onMounted。
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { emit, listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import {
  ArrowLeftRight,
  Check,
  ClipboardPaste,
  Copy,
  LoaderCircle,
  PanelTop,
  Pin,
  PinOff,
  X
} from '@lucide/vue'
import LangSelect from '../components/LangSelect.vue'
import { api, err } from '../api'
import { activeBackend, buildRequest, loadMeta, meta } from '../composables/useAppState'
import { useAutoTranslate } from '../composables/useAutoTranslate'

const source = ref('')
const copied = ref(false)
const input = ref<HTMLTextAreaElement | null>(null)
const outEl = ref<HTMLElement | null>(null)
/** 钉住时失焦不收起，和划词弹窗同一套交互 */
const pinned = ref(false)

/** 和划词弹窗共用同一个目标语言选择，两个「快速翻译」入口方向保持一致。 */
const LANG_KEY = 'hymt2:popup-target'
const target = ref(localStorage.getItem(LANG_KEY) ?? '')

/** 中日韩文字，用来猜翻译方向。 */
const CJK = /[\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]/
const guessTarget = (t: string) => (CJK.test(t) ? 'en' : 'zh')

const langs = computed(() => meta.value.languages)

/**
 * LangSelect 以中文名收发，而 localStorage 里存的是语言代码（与划词弹窗共用）。
 * 这里做一层双向换算，存储格式保持不变——两个「快速翻译」入口仍共享同一方向。
 */
const targetName = computed<string>({
  get: () => langs.value.find(([code]) => code === target.value)?.[1] ?? '',
  set: (name) => {
    const code = langs.value.find(([, n]) => n === name)?.[0]
    if (code) changeTarget(code)
  }
})

// ---------- 翻译：与主窗口共用的自动 + 增量翻译 ----------
const {
  result,
  busy,
  failed,
  elapsed,
  usedTerms,
  abort,
  clearAll,
  onBackspace,
  schedule,
  watchAuto
} = useAutoTranslate({
    source,
    jobPrefix: 'mini-',
    outEl,
    buildRequest: () => {
      // 首次输入时还没有明确方向，先按内容猜一个
      if (!target.value && source.value.trim()) {
        const g = guessTarget(source.value)
        target.value = g
        localStorage.setItem(LANG_KEY, g)
      }
      return buildRequest(
        {
          text: source.value,
          mode: 'basic',
          terms: [],
          auto_filter_terms: false,
          style: '',
          format_type: '',
          context: '',
          ignore_glossary: true
        },
        target.value || 'zh'
      )
    }
  })

// 输入即翻译
watchAuto()

const usedCount = computed(() => usedTerms.value.length)

/** 换方向：把当前目标语言和猜测的源方向对调，顺手重译一次。 */
function swapDirection() {
  changeTarget(target.value === 'zh' ? 'en' : 'zh')
}

function changeTarget(v: string) {
  target.value = v
  localStorage.setItem(LANG_KEY, v)
  // 换语言 = 另一次翻译：中断当前流立刻重译
  if (source.value.trim()) {
    abort()
    schedule()
  }
}

async function pasteFromClipboard() {
  try {
    const t = await api.readClipboardText()
    if (!t.trim()) return
    source.value = t
    focusInput()
    schedule()
  } catch (e) {
    err(e)
  }
}

async function copyResult() {
  if (!result.value) return
  try {
    await api.copyText(result.value)
    copied.value = true
    window.setTimeout(() => (copied.value = false), 1300)
  } catch (e) {
    err(e)
  }
}

async function togglePin() {
  pinned.value = !pinned.value
  try {
    await api.setMiniPinned(pinned.value)
  } catch (e) {
    pinned.value = !pinned.value
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

/**
 * 在主窗口中打开，并把当前原文/译文带过去。
 *
 * 必须走 Tauri 事件：三个窗口是各自独立的 webview，`window.dispatchEvent`
 * 只在本窗口有效，主窗口根本收不到（localStorage 那条路要等主窗口重新
 * 挂载才读，而它是常驻的）。
 */
async function openMain() {
  if (source.value.trim() || result.value) {
    try {
      await emit('hymt2:reuse', {
        source: source.value,
        translation: result.value,
        target_lang: target.value || 'zh',
        mode: 'basic'
      })
    } catch {
      /* 浏览器预览没有 IPC */
    }
  }
  try {
    await api.showWindow()
  } catch (e) {
    err(e)
  }
  await close()
}

async function close() {
  // 收起即解除钉住，否则下次热键唤出还是钉住态
  pinned.value = false
  abort()
  try {
    await api.hideMini()
  } catch {
    /* 浏览器预览没有 IPC */
  }
}

/** 头部空白处拖拽移动小窗。 */
function startDrag(e: MouseEvent) {
  if ((e.target as HTMLElement).closest('button, select')) return
  try {
    getCurrentWindow().startDragging()
  } catch {
    /* 浏览器预览没有 IPC */
  }
}

/** Esc 在整个窗口内都生效，不只在输入框里。 */
function onEsc(e: KeyboardEvent) {
  if (e.key === 'Escape') close()
}

const unlisten: Array<() => void> = []

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
  unlisten.forEach((f) => f())
  window.removeEventListener('keydown', onEsc)
})
</script>

<template>
  <div class="mini">
    <!-- 头部：标题 + 语言胶囊 + 钉住/粘贴/主窗/关闭 -->
    <header class="mini__header" @mousedown="startDrag">
      <div class="mini__header-left">
        <span class="mini__title">快速翻译</span>
        <div class="mini__lang-capsule" title="切换目标语言">
          <span class="mini__lang-hint">自动检测</span>
          <span class="mini__lang-arrow">→</span>
          <LangSelect v-model="targetName" :options="langs" show-label />
        </div>
        <button class="mini__btn" title="对调中英方向" @click="swapDirection">
          <ArrowLeftRight :size="13" />
        </button>
      </div>

      <span class="mini__spacer" />

      <button
        class="mini__btn"
        :class="{ 'mini__btn--on': pinned }"
        :title="pinned ? '取消钉住：点击别处将自动收起' : '钉住小窗：失焦时不自动收起'"
        @click="togglePin"
      >
        <PinOff v-if="pinned" :size="13" />
        <Pin v-else :size="13" />
      </button>
      <button class="mini__btn" title="从剪贴板填入" @click="pasteFromClipboard">
        <ClipboardPaste :size="14" />
      </button>
      <button class="mini__btn" title="在主窗口中打开" @click="openMain">
        <PanelTop :size="14" />
      </button>
      <button class="mini__btn mini__btn--close" title="关闭（Esc）" @click="close">
        <X :size="14" />
      </button>
    </header>

    <!-- 主体：输入卡 + 输出卡 -->
    <div class="mini__body">
      <div class="mini-input-card">
        <textarea
          ref="input"
          v-model="source"
          class="mini__textarea"
          placeholder="输入或粘贴文本，输入后自动翻译…"
          spellcheck="false"
          @keydown.backspace="onBackspace"
        />
        <div class="mini-input-hints">
          <button v-if="source" class="mini__clear" title="清空原文与译文" @click="clearAll">
            <X :size="11" /> 清空
          </button>
          <span v-else><kbd class="mini-kbd">↵</kbd> 换行</span>
          <span v-if="busy" class="mini-translating">翻译中…</span>
          <span v-else-if="elapsed && result">{{ (elapsed / 1000).toFixed(1) }}s</span>
        </div>
      </div>

      <div class="mini-output-card">
        <p v-if="failed" class="mini__err">{{ failed }}</p>
        <p v-else-if="!result && !busy" class="mini__hint">译文会显示在这里</p>
        <p v-else ref="outEl" class="mini__text">
          {{ result }}<LoaderCircle v-if="busy" class="mini__spin" :size="13" />
        </p>

        <div class="mini-output-footer">
          <span class="mini__meta">
            <span class="status-dot" />
            {{ activeBackend?.name ?? '本地引擎' }}<template v-if="usedCount"> · 命中术语 {{ usedCount }}</template>
          </span>
          <button class="mini__btn" :disabled="!result" title="复制译文" @click="copyResult">
            <Check v-if="copied" :size="14" />
            <Copy v-else :size="14" />
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.mini {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--surface-primary);
  overflow: hidden;
}

/* ---------- 头部 ---------- */
.mini__header {
  height: 42px;
  padding: 0 8px 0 12px;
  display: flex;
  align-items: center;
  gap: 8px;
  background: var(--surface-primary);
  border-bottom: 1px solid var(--border-subtle);
  flex-shrink: 0;
  user-select: none;
  cursor: grab;
}

.mini__header:active {
  cursor: grabbing;
}

.mini__header-left {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.mini__title {
  font-size: 12.5px;
  font-weight: 600;
  color: var(--text-primary);
  white-space: nowrap;
}

/* 胶囊内部：LangSelect 触发器自带的底色/边框和胶囊重复，去掉 */
.mini__lang-capsule {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 0;
  border-radius: 8px;
  font-size: 12px;
}

.mini__lang-capsule :deep(.langpick__trigger--labeled) {
  background: transparent;
  border: none;
  padding: 1px 2px;
}

.mini__lang-hint,
.mini__lang-arrow {
  color: var(--text-muted);
  white-space: nowrap;
}

.mini__spacer {
  flex: 1;
  min-width: 0;
}

.mini__btn {
  width: 24px;
  height: 24px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid transparent;
  border-radius: 6px;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.15s;
}

.mini__btn:hover:not(:disabled) {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.mini__btn:disabled {
  opacity: 0.35;
  cursor: default;
}

.mini__btn--on {
  color: var(--accent-primary);
  background: var(--surface-active);
  border-color: var(--border-focus);
}

.mini__btn--close:hover {
  background: #ef4444;
  color: #fff;
}

/* ---------- 主体 ---------- */
.mini__body {
  flex: 1;
  min-height: 0;
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  background: var(--surface-secondary);
}

/* 输入卡：focus-within 光环 */
.mini-input-card {
  flex: 0 0 auto;
  height: 40%;
  min-height: 92px;
  display: flex;
  flex-direction: column;
  padding: 7px 10px 5px;
  background: var(--surface-secondary);
  border: 1px solid var(--border-subtle);
  border-radius: 10px;
  transition: all 0.2s;
}

.mini-input-card:focus-within {
  background: var(--surface-primary);
  border-color: var(--border-focus);
  box-shadow: 0 0 0 3px var(--accent-glow);
}

.mini__textarea {
  flex: 1;
  min-height: 0;
  width: 100%;
  border: none;
  background: transparent;
  color: var(--text-primary);
  font-family: inherit;
  font-size: 13.5px;
  line-height: 1.55;
  resize: none;
  outline: none;
}

.mini__textarea::placeholder {
  color: var(--text-muted);
}

.mini-input-hints {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
  padding-top: 3px;
  font-size: 10.5px;
  color: var(--text-muted);
  flex-shrink: 0;
}

.mini-translating {
  color: var(--accent-primary);
}

/* 清空按钮：与键帽提示同一行，低调处理 */
.mini__clear {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 1px 5px;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: var(--text-muted);
  font-family: inherit;
  font-size: 10.5px;
  cursor: pointer;
  transition: all 0.15s;
}

.mini__clear:hover {
  background: var(--surface-hover);
  color: var(--status-danger);
}

/* 输出卡 */
.mini-output-card {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 10px;
  background: var(--surface-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 10px;
  overflow: hidden;
}

.mini__text {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  margin: 0;
  font-size: 13.5px;
  line-height: 1.6;
  color: var(--text-primary);
  white-space: pre-wrap;
  word-break: break-word;
  user-select: text;
}

.mini__hint,
.mini__err {
  flex: 1;
  margin: 0;
  font-size: 12.5px;
  line-height: 1.6;
  color: var(--text-muted);
}

.mini__err {
  color: var(--status-danger);
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

.mini-output-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-top: 8px;
  padding-top: 7px;
  border-top: 1px dashed var(--border-subtle);
  flex-shrink: 0;
}

.mini__meta {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  overflow: hidden;
  font-size: 10.5px;
  color: var(--text-muted);
  white-space: nowrap;
}
</style>