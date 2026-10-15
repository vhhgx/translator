<script setup lang="ts">
/**
 * 划词翻译弹窗。
 *
 * 它是常驻的透明无边框窗口（`label: "popup"`），由 Rust 侧的全局鼠标监听
 * 在 `hymt2:selection` 事件里带着选中文本唤出。翻译走和主窗口完全相同的
 * `translate_stream`，靠 job_id 过滤事件，两边互不串台。
 *
 * 窗口本身 focusable: false（不抢焦点）——点别处时由 Rust 的全局鼠标监听
 * 隐藏它；「钉住」时 Rust 会跳过隐藏逻辑（见 selection.rs 的 PINNED）。
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { emit, listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { Check, Copy, ExternalLink, LoaderCircle, Pin, PinOff, Star, X } from '@lucide/vue'
import { api, err, toast, toasts } from '../api'
import LangSelect from '../components/LangSelect.vue'
import { activeBackend, buildRequest, loadMeta, meta } from '../composables/useAppState'

const source = ref('')
const result = ref('')
const failed = ref('')
const busy = ref(false)
const copied = ref(false)
/** 钉住状态：钉住时点击别处不收起，新划词也不打扰 */
const pinned = ref(false)
/** 本条翻译在历史库里的 id（done 事件回报），收藏挂在它上面 */
const historyId = ref('')
const starred = ref(false)
/** 本条命中的术语数（done 事件回报） */
const usedCount = ref(0)

const LANG_KEY = 'hymt2:popup-target'
const target = ref(localStorage.getItem(LANG_KEY) ?? '')

/** 中日韩文字，用来猜翻译方向。 */
const CJK = /[\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]/
const guessTarget = (t: string) => (CJK.test(t) ? 'en' : 'zh')

const langs = computed(() => meta.value.languages)

/** LangSelect 以中文名收发，存储用的是语言代码——两层换算，存储格式不变。 */
const targetName = computed<string>({
  get: () => langs.value.find(([code]) => code === target.value)?.[1] ?? '',
  set: (name) => {
    const code = langs.value.find(([, n]) => n === name)?.[0]
    if (code) changeTarget(code)
  }
})

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
  historyId.value = ''
  starred.value = false
  usedCount.value = 0

  try {
    unlisten.push(
      await listen<{ job_id: string; text: string }>('hymt2:delta', (e) => {
        if (e.payload.job_id !== currentJob) return
        result.value += e.payload.text
      })
    )
    unlisten.push(
      await listen<{ job_id: string; text: string; used_terms?: string[]; history_id?: string }>(
        'hymt2:done',
        (e) => {
          if (e.payload.job_id !== currentJob) return
          result.value = e.payload.text
          usedCount.value = e.payload.used_terms?.length ?? 0
          historyId.value = e.payload.history_id ?? ''
          busy.value = false
        }
      )
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

async function togglePin() {
  pinned.value = !pinned.value
  try {
    await api.setPopupPinned(pinned.value)
  } catch (e) {
    pinned.value = !pinned.value
    err(e)
  }
}

async function toggleStar() {
  if (!historyId.value) return
  starred.value = !starred.value
  try {
    await api.historySetStarred(historyId.value, starred.value)
    toast(starred.value ? '已收藏' : '已取消收藏', 'success')
  } catch (e) {
    starred.value = !starred.value
    err(e)
  }
}

/**
 * 把当前原文送进主窗口深挖。
 *
 * 走 Tauri 事件而不是 `window.dispatchEvent`：三个窗口是各自独立的
 * webview，CustomEvent 只在本窗口内派发，主窗口收不到。
 */
async function openInMain() {
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
  try {
    await api.showWindow()
  } catch {
    /* 浏览器预览没有 IPC */
  }
  await close()
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
  // 关闭即解除钉住，避免隐藏后钉住状态残留
  if (pinned.value) {
    pinned.value = false
    try {
      await api.setPopupPinned(false)
    } catch {
      /* ignore */
    }
  }
  try {
    await api.hideSelectionPopup()
  } catch {
    /* 浏览器预览没有 IPC，忽略 */
  }
}

/** 头部空白处拖拽移动弹窗（默认位置由 Rust 按光标计算）。 */
function startDrag(e: MouseEvent) {
  if ((e.target as HTMLElement).closest('button, select')) return
  try {
    getCurrentWindow().startDragging()
  } catch {
    /* 浏览器预览没有 IPC */
  }
}

onMounted(async () => {
  try {
    await loadMeta()
  } catch {
    /* meta 里已有静态兜底语言表 */
  }

  await listen<{ text: string }>('hymt2:selection', (e) => {
    // 钉住期间不打扰当前内容（Rust 侧也拦了一层，这里双保险）
    if (pinned.value) return
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
  <!-- 外层留 8px 给投影——透明窗口里圆角外侧要能看见阴影。用 padding 而非
       margin：margin 会与父容器折叠，留白就白留了（见样式里的说明）。 -->
  <div class="pop-wrap">
    <div class="pop">
    <!-- 顶部：徽标 + 语言流向 + 钉住/送主窗/关闭（可拖拽区域） -->
    <header class="pop__header" @mousedown="startDrag">
      <div class="pop__header-left">
        <span class="pop__badge">
          <span class="pop__badge-dot" />
          划词翻译
        </span>
        <div class="lang-direction-pill" title="切换目标语言">
          <span>自动</span>
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <line x1="5" y1="12" x2="19" y2="12" />
            <polyline points="12 5 19 12 12 19" />
          </svg>
          <LangSelect v-model="targetName" :options="langs" show-label />
        </div>
      </div>

      <div class="pop__controls">
        <button
          class="pop__btn"
          :class="{ 'pop__btn--on': pinned }"
          :title="pinned ? '取消钉住：点击别处将自动收起' : '钉住弹窗：失焦时不自动关闭'"
          @click="togglePin"
        >
          <PinOff v-if="pinned" :size="13" />
          <Pin v-else :size="13" />
        </button>
        <button class="pop__btn" title="在主翻译窗口中打开" @click="openInMain">
          <ExternalLink :size="13" />
        </button>
        <button class="pop__btn pop__btn--close" title="关闭 (点击别处也可收起)" @click="close">
          <X :size="14" />
        </button>
      </div>
    </header>

    <!-- 原文不再展示：划词场景下原文就在用户眼前，弹窗里重复一遍是噪音。
         source 状态、翻译流程、送主窗的握手都保留，只是不渲染这一块。 -->

    <!-- 译文区 -->
    <div class="pop__body">
      <p v-if="failed" class="pop__err">{{ failed }}</p>
      <p v-else class="pop__out">
        {{ result }}<LoaderCircle v-if="busy" class="pop__spin" :size="13" />
      </p>
    </div>

    <!-- 底栏：引擎与术语命中 + 收藏/复制 -->
    <footer class="pop__footer">
      <div class="pop__meta">
        <span class="status-dot" />
        <span>{{ activeBackend?.name ?? '本地引擎' }}<template v-if="usedCount"> · 命中术语 {{ usedCount }}</template></span>
      </div>
      <div class="pop__actions">
        <button
          class="pop__mini-btn"
          :class="{ 'pop__mini-btn--starred': starred }"
          :disabled="!historyId"
          :title="starred ? '取消收藏' : '加入收藏'"
          @click="toggleStar"
        >
          <Star :size="13" :fill="starred ? 'currentColor' : 'none'" />
        </button>
        <button class="pop__copy" :disabled="!result" @click="copyResult">
          <Check v-if="copied" :size="12" />
          <Copy v-else :size="12" />
          <span>{{ copied ? '已复制' : '复制' }}</span>
        </button>
      </div>
    </footer>

    <!-- 独立窗口也要有自己的 toast 容器（App.vue 的那份挂不到这里） -->
    <div v-for="t in toasts" :key="t.id" class="toast" :class="`toast--${t.kind}`">
      {{ t.text }}
    </div>
    </div>
  </div>
</template>

<style scoped>
/* 留白容器：padding 不与父级折叠，margin 会 */
.pop-wrap {
  height: 100%;
  padding: 8px;
  box-sizing: border-box;
}

/* 外层用 padding 而不是给卡片 margin。
 *
 margin 会和父容器**折叠**：`.pop` 的 margin:8px 顶部直接被 #app 吃掉，
 * 实测卡片 top=0、上边完全没有留白，而下边多出 16px —— 投影在顶部被窗口
 * 边界齐刷刷切断，就成了你看到的那圈错位硬边。padding 不参与折叠，
 * 留白是实打实的。 */
.pop {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--surface-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 13px;
  /* 向下最远约 5px，刚好塞进外层 8px 留白里，不会被裁。
     悬浮小卡片也不该用很重的投影——它压在别人的窗口上，阴影太浓反而脏。 */
  box-shadow:
    0 1px 2px rgba(15, 23, 42, 0.08),
    0 3px 8px -2px rgba(15, 23, 42, 0.16);
  overflow: hidden;
}

/* 深色下纯黑投影几乎看不见，用一圈稍重的黑托出层次。 */
:global(html[data-theme='dark']) .pop {
  box-shadow:
    0 1px 2px rgba(0, 0, 0, 0.5),
    0 3px 8px -2px rgba(0, 0, 0, 0.6);
}

/* ---------- 顶部 ---------- */
.pop__header {
  height: 38px;
  padding: 0 8px 0 12px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: var(--surface-primary);
  border-bottom: 1px solid var(--border-subtle);
  flex-shrink: 0;
  cursor: grab;
  user-select: none;
}

.pop__header:active {
  cursor: grabbing;
}

.pop__header-left {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.pop__badge {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-primary);
  white-space: nowrap;
}

.pop__badge-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--accent-primary);
  box-shadow: 0 0 8px var(--accent-primary);
  flex-shrink: 0;
}

/* 语言流向胶囊：自动 → 目标语 ▾ */
.lang-direction-pill {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 2px 8px;
  border-radius: 6px;
  background: var(--surface-tertiary);
  border: 1px solid var(--border-subtle);
  font-size: 11.5px;
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.15s;
}

.lang-direction-pill:hover {
  border-color: var(--border-focus);
  color: var(--text-primary);
}

/* 语言流向胶囊里的 LangSelect 触发器：去掉它自带的底色，避免和胶囊两层皮 */
.lang-direction-pill :deep(.langpick__trigger--labeled) {
  background: transparent;
  border: none;
  padding: 0 2px;
}

.pop__controls {
  display: flex;
  align-items: center;
  gap: 2px;
}

.pop__btn {
  width: 26px;
  height: 26px;
  border-radius: 6px;
  border: 1px solid transparent;
  color: var(--text-secondary);
  background: transparent;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: all 0.15s;
}

.pop__btn:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.pop__btn--on {
  color: var(--accent-primary);
  background: var(--surface-active);
  border-color: var(--border-focus);
}

.pop__btn--close:hover {
  background: #ef4444;
  color: #fff;
}

/* ---------- 迷你按钮（收藏/原文区共用） ---------- */
.pop__mini-btn {
  width: 22px;
  height: 22px;
  border-radius: 5px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  flex-shrink: 0;
  transition: all 0.15s;
}

.pop__mini-btn:hover:not(:disabled) {
  color: var(--text-primary);
  background: var(--surface-hover);
}

.pop__mini-btn:disabled {
  opacity: 0.35;
  cursor: default;
}

.pop__mini-btn--starred {
  color: var(--star-active);
}

/* ---------- 译文区 ---------- */
.pop__body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 12px 14px;
  background: var(--surface-primary);
}

.pop__out {
  margin: 0;
  font-size: 14.5px;
  line-height: 1.65;
  color: var(--text-primary);
  white-space: pre-wrap;
  word-break: break-word;
  user-select: text;
}

.pop__err {
  margin: 0;
  font-size: 13px;
  line-height: 1.6;
  color: var(--status-danger);
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

/* ---------- 底栏 ---------- */
.pop__footer {
  padding: 6px 10px 6px 12px;
  background: var(--surface-primary);
  border-top: 1px dashed var(--border-subtle);
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-shrink: 0;
}

.pop__meta {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: var(--text-muted);
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
}

.pop__actions {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}

.pop__copy {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 3px 9px;
  border-radius: 6px;
  background: var(--surface-tertiary);
  border: 1px solid var(--border-subtle);
  color: var(--text-secondary);
  font-family: inherit;
  font-size: 11.5px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s;
}

.pop__copy:hover:not(:disabled) {
  color: var(--text-primary);
  border-color: var(--border-focus);
  background: var(--surface-hover);
}

.pop__copy:disabled {
  opacity: 0.4;
  cursor: default;
}

.pop__copy.copied,
.pop__copy:has(+ .copied) {
  color: var(--status-online);
  border-color: color-mix(in srgb, var(--status-online) 35%, transparent);
  background: color-mix(in srgb, var(--status-online) 10%, transparent);
}
</style>
