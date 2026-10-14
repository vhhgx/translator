<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { listen } from '@tauri-apps/api/event'
import type { Component } from 'vue'
import {
  ArrowLeftRight,
  BookOpen,
  Braces,
  Check,
  Copy,
  Globe,
  Link2,
  Palette,
  Star,
  X,
  Zap
} from '@lucide/vue'
import LangSelect from '../components/LangSelect.vue'
import { api, toast } from '../api'
import { glossary, glossaryTermCount, meta, refreshCounts, tab } from '../composables/useAppState'
import { useAutoTranslate } from '../composables/useAutoTranslate'
import type { Mode, TermPair } from '../types'

/** 「自动检测」在 UI 里的名字。左右两栏都用中文名比对，故只存这一个值。 */
const AUTO_LANG = '检测语言'

// ---------- state ----------
const source = ref('')
/** 源语言：统一用中文名存储。检测语言是保留选项，可能出错，用户可手动改。 */
const sourceLang = ref(AUTO_LANG)
const targetLang = ref('英语')
const mode = ref<Mode>('basic')
const style = ref('日常口语化风格')
const context = ref('')
const formatType = ref('JSON')
const autoFilterTerms = ref(true)
const ignoreGlossary = ref(false)
const termsEditor = ref('')
const showModePanel = ref(false)
const inputEl = ref<HTMLTextAreaElement | null>(null)
const outEl = ref<HTMLElement | null>(null)
// result / busy / failed / elapsed / usedTerms / historyId / starred
// 由下方 useAutoTranslate 提供——与小窗共用同一份状态和流式逻辑。

// ---------- 模式元数据（图标 + 触发胶囊摘要） ----------
const MODE_ICONS: Record<Mode, Component> = {
  basic: Zap,
  terminology: BookOpen,
  style: Palette,
  context: Globe,
  delimiter: Link2,
  structured: Braces
}

/** 左下角模式胶囊右侧的子参数摘要；basic 没有参数则隐藏摘要并弱化胶囊。 */
const modeSummary = computed(() => {
  switch (mode.value) {
    case 'terminology':
      return `词库「${glossary.value.active || '—'}」`
    case 'style':
      return style.value
    case 'context':
      return context.value.trim() || '未填写'
    case 'delimiter':
      return '已开启'
    case 'structured':
      return formatType.value
    default:
      return ''
  }
})

const modeLabel = computed(
  () => meta.value.modes.find(([k]) => k === mode.value)?.[1] ?? '基础翻译'
)

/**
 * 表头快捷药丸：固定保留中英文，其余语言要用户从下拉里选过才会出现。
 * 用户选过的语种记在 localStorage，重开应用仍然在；超出上限则淘汰最早的。
 */
const SEED_SOURCE = [AUTO_LANG, '中文', '英语']
const SEED_TARGET = ['英语', '中文']
const MAX_USER_PICKS = 3

type PickSide = 'src' | 'tgt'

function loadPicks(side: PickSide): string[] {
  try {
    const raw = localStorage.getItem(`hymt2:picks:${side}`)
    const list = raw ? (JSON.parse(raw) as string[]) : []
    return list.filter((x) => typeof x === 'string' && x).slice(0, MAX_USER_PICKS)
  } catch {
    return []
  }
}

const sourcePicks = ref<string[]>(loadPicks('src'))
const targetPicks = ref<string[]>(loadPicks('tgt'))

const sourcePills = computed(() => [...new Set([...SEED_SOURCE, ...sourcePicks.value])])
const targetPills = computed(() => [...new Set([...SEED_TARGET, ...targetPicks.value])])

/** 选了非固定语种才记住；种子语种本来就在表头上，不必记。 */
function rememberPick(side: PickSide, lang: string) {
  const seed = side === 'src' ? SEED_SOURCE : SEED_TARGET
  if (seed.includes(lang)) return
  const current = side === 'src' ? sourcePicks.value : targetPicks.value
  const next = [lang, ...current.filter((x) => x !== lang)].slice(0, MAX_USER_PICKS)
  localStorage.setItem(`hymt2:picks:${side}`, JSON.stringify(next))
  if (side === 'src') sourcePicks.value = next
  else targetPicks.value = next
}

/** 「检测语言」下后端识别到的源语言中文名（如「英语」），用于徽标展示。 */
const detectedLangName = ref('')
let detectTimer: number | undefined

// ---------- 翻译：与快速翻译小窗共用同一套自动 + 增量逻辑 ----------
const {
  result,
  busy,
  failed,
  elapsed,
  historyId,
  starred,
  abort,
  setBaseline,
  clearAll: clearTranslation,
  onBackspace,
  translateNow,
  watchAuto,
  toggleStar
} = useAutoTranslate({
  source,
  outEl,
  buildRequest,
  onDone: () => void refreshCounts()
})

/** Ctrl+Enter 手动跑一次。自动翻译是默认行为，这里只是给键盘一个入口。 */
function run() {
  if (!source.value.trim()) {
    toast('请输入要翻译的内容')
    return
  }
  translateNow()
}

/** 清空原文与译文，并把焦点还给输入框。 */
function clearAll() {
  clearTranslation()
  detectedLangName.value = ''
  inputEl.value?.focus()
}

// ---------- derived ----------
const modeOptions = computed(() =>
  meta.value.modes.map(([k, label]) => ({ value: k as Mode, label }))
)
const styleOptions = computed(() => meta.value.styles)
const formatOptions = computed(() => meta.value.format_types)
const allLangs = computed<[string, string][]>(() => meta.value.languages)

/** 源语言下拉要比目标语言多一项「自动检测」，用同样的中文名参与比对。 */
const sourceLangOptions = computed<[string, string][]>(() => [
  ['auto', AUTO_LANG],
  ...allLangs.value
])

onMounted(() => {
  loadGlossaryTerms()

  // 两条来源都要接：
  //  - window CustomEvent：历史页复用（同一窗口内 App.vue 派发）
  //  - Tauri 事件：小窗/划词弹窗「在主窗口打开」（跨 webview，只有 Tauri 事件能到）
  window.addEventListener('hymt2:reuse', (e) => applyReuse((e as CustomEvent).detail))
  listen<{ source: string; translation: string; target_lang: string; mode: string }>(
    'hymt2:reuse',
    (e) => applyReuse(e.payload)
  ).catch(() => {
    /* 浏览器预览没有 IPC */
  })

  const pending = localStorage.getItem('hymt2:reuse')
  if (pending) {
    localStorage.removeItem('hymt2:reuse')
    applyReuse(JSON.parse(pending))
  }

  document.addEventListener('click', closePanelOnOutside)
  document.addEventListener('keydown', onPanelKey)
})

/** 把一份可复用的翻译灌进主界面。 */
function applyReuse(item: {
  source: string
  translation: string
  target_lang: string
  mode: string
}) {
  source.value = item.source
  result.value = item.translation
  targetLang.value = item.target_lang
  mode.value = item.mode as Mode
  // 标记为「这段刚译过」，避免复用后又触发一次自动翻译
  setBaseline(item.source)
}

// ---------- glossary ----------
watch(
  () => glossary.value.terms,
  (terms) => {
    termsEditor.value = terms.map((t: TermPair) => t.join(' => ')).join('\n')
  },
  { deep: true }
)

function loadGlossaryTerms() {
  termsEditor.value = glossary.value.terms.map((t: TermPair) => t.join(' => ')).join('\n')
}

function parseTerms(): [string, string][] {
  return termsEditor.value
    .split('\n')
    .map((line) => {
      const m = line.match(/^\s*(.+?)\s*(?:=>|→|\t|翻译成|:)\s*(.+?)\s*$/)
      return m ? [m[1], m[2]] : null
    })
    .filter((x): x is [string, string] => !!x)
}

// ---------- 模式悬浮面板 ----------
function closePanelOnOutside(e: MouseEvent) {
  if (!showModePanel.value) return
  const target = e.target as HTMLElement
  // 面板和触发胶囊自身的点击不算「外部」
  if (target.closest('.mode-popover') || target.closest('.mode-trigger-btn')) return
  showModePanel.value = false
}

function onPanelKey(e: KeyboardEvent) {
  if (e.key === 'Escape') showModePanel.value = false
}

function pickMode(m: Mode) {
  mode.value = m
  // 无参数的模式选完即走，干脆利落
  if (m === 'basic' || m === 'delimiter') {
    window.setTimeout(() => (showModePanel.value = false), 220)
  }
}

// ---------- streaming ----------
function buildRequest() {
  return {
    text: source.value,
    target_lang: targetLang.value,
    mode: mode.value,
    terms: parseTerms(),
    auto_filter_terms: autoFilterTerms.value,
    style: style.value,
    format_type: formatType.value,
    context: context.value,
    ignore_glossary: ignoreGlossary.value
  }
}

// ---------- 检测语言徽标 ----------
/** 触发一次语种检测（300ms 防抖）。检测语言模式下才需要。 */
function scheduleDetect() {
  window.clearTimeout(detectTimer)
  if (sourceLang.value !== AUTO_LANG) return
  const text = source.value.trim()
  if (!text) {
    detectedLangName.value = ''
    return
  }
  detectTimer = window.setTimeout(async () => {
    try {
      const code = await api.detectLanguage(text)
      // 模型的回答偶尔会带标点或解释，保守地只取第一个词
      const name = langNameOf(code.split(/[\s,。.\n]/)[0] ?? '')
      // 与「检测语言」的占位值一致时才显示徽标
      if (sourceLang.value === AUTO_LANG) detectedLangName.value = name
    } catch {
      // 后端没起来时不显示徽标，静默即可
    }
  }, 300)
}

/** 语言代码 → 中文名。识别不出来时保持空串（不显示徽标）。 */
function langNameOf(code: string): string {
  const hit = meta.value.languages.find(([c]) => c === code || c.startsWith(`${code}-`))
  return hit?.[1] ?? ''
}


onUnmounted(() => {
  window.clearTimeout(detectTimer)
  document.removeEventListener('click', closePanelOnOutside)
  document.removeEventListener('keydown', onPanelKey)
})

// 输入即翻译（增量逻辑在 useAutoTranslate 里），这里只顺带刷新语种徽标
watchAuto()
watch(source, () => scheduleDetect())

// 无论从药丸还是从下拉切换语言，都把它记进表头快捷项
watch(sourceLang, (v) => {
  rememberPick('src', v)
  // 手动指定源语言后不再显示检测徽标；改回「检测语言」则重新检测
  detectedLangName.value = ''
  scheduleDetect()
})
watch(targetLang, (v) => {
  rememberPick('tgt', v)
  // 换目标语言 = 另一次翻译：中断当前流、立刻按新语言重译
  abort()
  translateNow()
})

/**
 * 源语言为「自动检测」时无法对称交换——反方向该译成什么语言是未知的，
 * 此时只允许交换语言方向，不搬动文本。
 */
const swapHint = computed(() =>
  sourceLang.value === AUTO_LANG
    ? '请先手动指定源语言，再交换方向'
    : result.value.trim()
      ? '交换语言与内容'
      : '交换翻译方向'
)

function swap() {
  if (sourceLang.value === AUTO_LANG) {
    toast('源语言是「检测语言」，请先手动指定后再交换', 'error')
    return
  }
  const ls = sourceLang.value
  sourceLang.value = targetLang.value
  targetLang.value = ls
  // 没有译文时只换语言方向，不能把用户刚输入的原文挪进只读的译文区。
  if (!result.value.trim()) return
  const text = source.value
  source.value = result.value
  result.value = text
  // 交换后这一对原文/译文是对得上的，登记为增量基线，继续编辑可以只译增量
  setBaseline(source.value, result.value)
}

async function copy(text: string) {
  await navigator.clipboard.writeText(text)
  toast('已复制到剪贴板', 'success')
}

/** 复制按钮的瞬时反馈状态 */
const copied = ref(false)
async function copyResult() {
  if (!result.value) return
  await copy(result.value)
  copied.value = true
  window.setTimeout(() => (copied.value = false), 1400)
}
</script>

<template>
  <div class="gt-main">
    <!-- 语言栏：源语言药丸 | 交换 | 目标语言药丸 + 词库胶囊 -->
    <div class="lang-bar">
      <div class="lang-pills">
        <button
          v-for="l in sourcePills"
          :key="l"
          class="lang-item"
          :class="{ 'lang-item--active': sourceLang === l }"
          @click="sourceLang = l"
        >
          {{ l }}
          <span v-if="l === AUTO_LANG && detectedLangName" class="lang-badge">{{ detectedLangName }}</span>
        </button>
        <LangSelect v-model="sourceLang" :options="sourceLangOptions" />
      </div>

      <button class="swap-btn" :class="{ 'swap-btn--dim': sourceLang === AUTO_LANG }" :title="swapHint" @click="swap">
        <ArrowLeftRight :size="14" />
      </button>

      <div class="lang-bar__right">
        <div class="lang-pills lang-pills--right">
          <button
            v-for="l in targetPills"
            :key="l"
            class="lang-item"
            :class="{ 'lang-item--active': targetLang === l }"
            @click="targetLang = l"
          >
            {{ l }}
          </button>
          <LangSelect v-model="targetLang" :options="allLangs" align="right" />
        </div>
        <!-- 词库快捷入口：显示当前生效词库与词条数，点击直达术语库管理 -->
        <button class="glossary-chip" title="点击配置生效的术语词库" @click="tab = 'glossary'">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
            <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
          </svg>
          <span>词库: <strong>{{ glossary.active || '—' }}（{{ glossaryTermCount }}）</strong></span>
        </button>
      </div>
    </div>

    <!-- 双栏工作区 -->
    <div class="workspace">
      <!-- 左侧：输入栏 -->
      <section class="pane pane-source">
        <div class="pane-body">
          <textarea
            ref="inputEl"
            v-model="source"
            class="editor-textarea"
            placeholder="输入或粘贴文本…"
            spellcheck="false"
            @keydown.ctrl.enter="run"
            @keydown.backspace="onBackspace"
            @keydown.esc="clearAll"
          />
          <button v-if="source" class="clear-btn" title="清空所有内容 (Esc)" @click="clearAll">
            <X :size="16" />
          </button>
        </div>

        <!-- 左下角控制区：模式触发胶囊 + 字数 -->
        <div class="pane-bottom-bar">
          <div class="bottom-left-pills">
            <button
              class="mode-trigger-btn"
              :class="{
                'mode-trigger-btn--active': mode !== 'basic' || !!modeSummary,
                'mode-trigger-btn--open': showModePanel
              }"
              @click.stop="showModePanel = !showModePanel"
            >
              <component :is="MODE_ICONS[mode]" :size="12" class="mode-trigger-icon" />
              <span>{{ modeLabel }}</span>
              <template v-if="modeSummary">
                <span class="mode-trigger-dot">·</span>
                <span class="mode-trigger-sub">{{ modeSummary }}</span>
              </template>
              <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
                <polyline points="18 15 12 9 6 15" />
              </svg>
            </button>
          </div>
          <span class="char-counter">{{ source.length }} 字</span>
        </div>

        <!-- 向上弹出的模式悬浮面板 -->
        <transition name="pop">
          <div v-if="showModePanel" class="mode-popover" @click.stop>
            <div class="popover-header">
              <span class="popover-title">选择翻译模式</span>
              <button class="popover-close" @click="showModePanel = false">
                收起 <X :size="11" />
              </button>
            </div>

            <!-- 6 模式宫格 -->
            <div class="mode-grid-6">
              <button
                v-for="m in modeOptions"
                :key="m.value"
                class="mode-card-btn"
                :class="{ 'mode-card-btn--selected': mode === m.value }"
                @click="pickMode(m.value)"
              >
                <component :is="MODE_ICONS[m.value]" :size="13" />
                <span>{{ m.label }}</span>
              </button>
            </div>

            <!-- 按模式显示的子参数区 -->
            <div v-if="mode === 'basic'" class="popover-param-box">
              <p class="minimal-info">
                <Check :size="12" /> 标准直译模式，无附加规则约束。
              </p>
            </div>

            <div v-else-if="mode === 'terminology'" class="popover-param-box">
              <div class="param-label">
                <span>专有术语（源词 =&gt; 译词，每行一条）</span>
                <span class="param-label__accent">激活词库「{{ glossary.active }}」</span>
              </div>
              <textarea v-model="termsEditor" class="pop-textarea" rows="3" spellcheck="false" />
              <div class="pop-checks">
                <button
                  class="pop-check-pill"
                  :class="{ 'pop-check-pill--on': autoFilterTerms }"
                  @click="autoFilterTerms = !autoFilterTerms"
                >
                  <Check v-if="autoFilterTerms" :size="11" />
                  <span v-else class="pop-check-box" />
                  仅使用原文命中的词
                </button>
                <button
                  class="pop-check-pill"
                  :class="{ 'pop-check-pill--on': ignoreGlossary }"
                  @click="ignoreGlossary = !ignoreGlossary"
                >
                  <Check v-if="ignoreGlossary" :size="11" />
                  <span v-else class="pop-check-box" />
                  未命中时退回基础翻译
                </button>
              </div>
            </div>

            <div v-else-if="mode === 'style'" class="popover-param-box">
              <div class="param-label"><span>目标风格（点击即生效）：</span></div>
              <div class="style-compact-grid">
                <button
                  v-for="s in styleOptions"
                  :key="s"
                  class="style-tag"
                  :class="{ 'style-tag--active': style === s }"
                  @click="style = s"
                >
                  {{ s }}
                </button>
              </div>
            </div>

            <div v-else-if="mode === 'context'" class="popover-param-box">
              <div class="param-label"><span>背景信息说明：</span></div>
              <input
                v-model="context"
                class="pop-input"
                placeholder="例如：医学报告或工程机械展会场景…"
              />
            </div>

            <div v-else-if="mode === 'delimiter'" class="popover-param-box">
              <p class="minimal-info">
                <Link2 :size="12" /> 自动识别并保留原文中的 ||、|、Tab 制表符及换行结构。
              </p>
            </div>

            <div v-else-if="mode === 'structured'" class="popover-param-box">
              <div class="param-label"><span>数据格式：</span></div>
              <div class="fmt-row">
                <button
                  v-for="f in formatOptions"
                  :key="f"
                  class="fmt-pill"
                  :class="{ 'fmt-pill--active': formatType === f }"
                  @click="formatType = f"
                >
                  {{ f }}
                </button>
              </div>
            </div>
          </div>
        </transition>
      </section>

      <!-- 右侧：译文栏 -->
      <section class="pane pane-target">
        <div ref="outEl" class="pane-body pane-body--out">
          <p v-if="failed" class="error-msg">⚠️ {{ failed }}</p>
          <div v-else-if="result" class="output-text">
            {{ result }}<span v-if="busy" class="stream-caret" />
          </div>
          <p v-else-if="busy" class="placeholder-msg">模型思考翻译中…</p>
          <p v-else class="placeholder-msg">译文将实时显示于此</p>
        </div>

        <div class="pane-bottom-bar pane-bottom-bar--right">
          <div class="bottom-left-pills">
            <span v-if="busy" class="spinner" />
            <span v-if="elapsed && result" class="time-tag">{{ (elapsed / 1000).toFixed(1) }}s</span>
            <!-- 命中术语数存在 useAutoTranslate 里（后续可以做高亮提示），界面上不显示药丸 -->
          </div>
          <div class="bottom-right-pills">
            <button
              class="icon-btn"
              :class="{ 'icon-btn--starred': starred }"
              :disabled="!historyId"
              :title="starred ? '取消收藏' : '收藏本条翻译'"
              @click="toggleStar"
            >
              <Star :size="15" :fill="starred ? 'currentColor' : 'none'" />
            </button>
            <button class="icon-btn" :disabled="!result" title="复制译文" @click="copyResult">
              <Check v-if="copied" :size="15" />
              <Copy v-else :size="15" />
            </button>
            <button class="icon-btn" :disabled="!result" :title="swapHint" @click="swap">
              <ArrowLeftRight :size="15" />
            </button>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.gt-main {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  background: var(--surface-primary);
}

/* ---------- 语言栏 ---------- */
.lang-bar {
  display: grid;
  grid-template-columns: 1fr auto 1fr;
  align-items: center;
  height: 44px;
  padding: 0 16px;
  background: var(--surface-primary);
  border-bottom: 1px solid var(--border-subtle);
  flex-shrink: 0;
  gap: 8px;
}

.lang-pills {
  display: flex;
  align-items: center;
  gap: 2px;
  min-width: 0;
  overflow-x: auto;
  scrollbar-width: none;
}

.lang-pills::-webkit-scrollbar {
  display: none;
}

.lang-bar__right {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  min-width: 0;
}

.lang-pills--right {
  flex: 1;
}

.lang-item {
  position: relative;
  flex-shrink: 0;
  padding: 0 10px;
  height: 44px; /* 撑满语言栏，让选中下划线正好压在栏底边框上 */
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-family: inherit;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-secondary);
  background: transparent;
  border: none;
  cursor: pointer;
  white-space: nowrap;
  transition: color 0.15s;
}

.lang-item:hover {
  color: var(--text-primary);
}

.lang-item--active {
  color: var(--accent-primary);
  font-weight: 600;
}

/* 选中下划线压在语言栏底边上 */
.lang-item--active::after {
  content: '';
  position: absolute;
  bottom: 0;
  left: 10px;
  right: 10px;
  height: 2px;
  background: var(--accent-primary);
  border-radius: 2px;
  box-shadow: 0 -2px 8px var(--accent-glow);
}

/* 「检测语言」下后端识别出的具体语种徽标 */
.lang-badge {
  font-size: 11px;
  padding: 1px 6px;
  border-radius: 4px;
  background: var(--surface-active);
  color: var(--accent-primary);
  font-weight: 500;
  white-space: nowrap;
}

.swap-btn {
  width: 30px;
  height: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border-subtle);
  border-radius: 8px;
  background: var(--surface-tertiary);
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.2s;
  flex-shrink: 0;
}

.swap-btn:hover {
  color: var(--accent-primary);
  border-color: var(--border-focus);
  transform: rotate(180deg);
}

.swap-btn--dim {
  opacity: 0.55;
}

/* 词库快捷胶囊 */
.glossary-chip {
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
  cursor: pointer;
  transition: all 0.15s;
  white-space: nowrap;
  flex-shrink: 0;
}

.glossary-chip:hover {
  border-color: var(--border-focus);
  color: var(--text-primary);
}

.glossary-chip strong {
  color: var(--accent-primary);
  font-weight: 500;
}

/* ---------- 工作区 ---------- */
.workspace {
  flex: 1;
  display: grid;
  grid-template-columns: 1fr 1fr;
  min-height: 0;
  overflow: hidden;
  position: relative;
}

.pane {
  display: flex;
  flex-direction: column;
  min-width: 0;
  position: relative;
  overflow: hidden;
}

.pane-source {
  background: var(--surface-primary);
  border-right: 1px solid var(--border-subtle);
}

.pane-target {
  background: var(--surface-secondary);
}

.pane-body {
  position: relative;
  flex: 1;
  min-height: 0;
  display: flex;
  padding: 16px 18px 8px;
}

.pane-body--out {
  overflow-y: auto;
  display: block;
}

.editor-textarea {
  flex: 1;
  min-height: 0;
  width: 100%;
  font-family: var(--font-ui);
  font-size: 16px;
  line-height: 1.65;
  color: var(--text-primary);
  background: transparent;
  border: none;
  outline: none;
  resize: none;
}

.editor-textarea::placeholder {
  color: var(--text-muted);
}

.clear-btn {
  position: absolute;
  top: 12px;
  right: 12px;
  width: 26px;
  height: 26px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.15s;
}

.clear-btn:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.output-text {
  font-size: 16px;
  line-height: 1.65;
  color: var(--text-primary);
  white-space: pre-wrap;
  word-break: break-word;
  width: 100%;
}

.placeholder-msg {
  margin: 0;
  font-size: 16px;
  color: var(--text-muted);
}

.error-msg {
  margin: 0;
  color: var(--status-danger);
  font-size: 14px;
}

/* ---------- 底部控制条 ---------- */
.pane-bottom-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 14px 10px;
  min-height: 38px;
  flex-shrink: 0;
}

.pane-bottom-bar--right {
  padding-bottom: 10px;
}

.bottom-left-pills {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.bottom-right-pills {
  display: flex;
  align-items: center;
  gap: 2px;
}

.char-counter {
  font-size: 11.5px;
  color: var(--text-muted);
  font-variant-numeric: tabular-nums;
}

/* 模式触发胶囊 */
.mode-trigger-btn {
  height: 28px;
  max-width: 100%;
  padding: 0 11px;
  border-radius: var(--r-pill);
  background: var(--surface-tertiary);
  border: 1px solid var(--border-subtle);
  color: var(--text-secondary);
  font-family: inherit;
  font-size: 12px;
  font-weight: 500;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
  transition: all 0.15s;
  white-space: nowrap;
  overflow: hidden;
}

.mode-trigger-btn:hover {
  color: var(--text-primary);
  border-color: var(--border-focus);
}

.mode-trigger-btn--active {
  background: var(--surface-active);
  color: var(--accent-primary);
  border-color: var(--border-focus);
}

.mode-trigger-btn--open {
  background: var(--surface-active);
  color: var(--accent-primary);
  border-color: var(--border-focus);
}

.mode-trigger-icon {
  font-size: 12px;
  line-height: 1;
}

.mode-trigger-dot {
  opacity: 0.4;
}

.mode-trigger-sub {
  color: var(--text-primary);
  font-weight: 600;
  max-width: 110px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.mode-trigger-btn--active .mode-trigger-sub,
.mode-trigger-btn--open .mode-trigger-sub {
  color: var(--accent-primary);
}

.time-tag {
  font-size: 11.5px;
  color: var(--text-muted);
  font-variant-numeric: tabular-nums;
}

.terms-tag-group {
  display: flex;
  align-items: center;
  gap: 4px;
  overflow: hidden;
}

.term-pill {
  padding: 1px 7px;
  font-size: 11px;
  color: var(--accent-primary);
  background: var(--surface-active);
  border-radius: 5px;
  white-space: nowrap;
}

.icon-btn {
  width: 28px;
  height: 28px;
  border-radius: 6px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: all 0.15s;
  flex-shrink: 0;
}

.icon-btn:hover:not(:disabled) {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.icon-btn:disabled {
  opacity: 0.3;
  cursor: not-allowed;
}

.icon-btn--starred {
  color: var(--star-active);
}

.spinner {
  width: 12px;
  height: 12px;
  border: 2px solid var(--border-subtle);
  border-top-color: var(--accent-primary);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
  flex-shrink: 0;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

/* ---------- 模式悬浮面板（锚定左下角向上弹出，不占工作区空间） ---------- */
.mode-popover {
  position: absolute;
  left: 16px;
  bottom: 46px;
  width: min(460px, calc(100% - 32px));
  background: var(--popover-bg);
  backdrop-filter: blur(24px);
  border: 1px solid var(--border-subtle);
  border-radius: var(--r-md);
  box-shadow: var(--popover-shadow);
  padding: 12px;
  z-index: 50;
  display: flex;
  flex-direction: column;
  gap: 12px;
  transform-origin: bottom left;
}

.popover-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 2px;
}

.popover-title {
  font-size: 11.5px;
  font-weight: 600;
  color: var(--text-muted);
  letter-spacing: 0.02em;
}

.popover-close {
  font-family: inherit;
  font-size: 11px;
  color: var(--text-muted);
  background: none;
  border: none;
  cursor: pointer;
  padding: 2px 4px;
  border-radius: 4px;
  display: inline-flex;
  align-items: center;
  gap: 3px;
}

.popover-close:hover {
  color: var(--text-primary);
  background: var(--surface-hover);
}

.mode-grid-6 {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 6px;
}

.mode-card-btn {
  padding: 7px 10px;
  border-radius: 8px;
  background: var(--surface-tertiary);
  border: 1px solid var(--border-subtle);
  color: var(--text-secondary);
  font-family: inherit;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 6px;
  transition: all 0.14s;
  text-align: left;
}

.mode-card-btn:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
  border-color: var(--border-focus);
}

.mode-card-btn--selected {
  background: var(--surface-active);
  color: var(--accent-primary);
  border-color: var(--border-focus);
  font-weight: 600;
}

.popover-param-box {
  padding-top: 10px;
  border-top: 1px solid var(--border-subtle);
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.param-label {
  font-size: 11.5px;
  color: var(--text-secondary);
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.param-label__accent {
  color: var(--accent-primary);
  font-weight: 500;
}

.pop-textarea {
  width: 100%;
  padding: 7px 10px;
  border-radius: 7px;
  background: var(--surface-secondary);
  border: 1px solid var(--border-subtle);
  color: var(--text-primary);
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.45;
  resize: none;
  outline: none;
}

.pop-textarea:focus {
  border-color: var(--border-focus);
}

.pop-checks {
  display: flex;
  gap: 6px;
}

.pop-check-pill {
  flex: 1;
  padding: 5px 8px;
  border-radius: 6px;
  background: var(--surface-tertiary);
  border: 1px solid var(--border-subtle);
  color: var(--text-secondary);
  font-family: inherit;
  font-size: 11.5px;
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 5px;
  transition: all 0.14s;
}

.pop-check-pill--on {
  background: var(--surface-active);
  border-color: var(--border-focus);
  color: var(--accent-primary);
}

/* 未选中时的小方框占位（和 Check 图标同位置） */
.pop-check-box {
  display: inline-block;
  width: 10px;
  height: 10px;
  border: 1.5px solid var(--text-muted);
  border-radius: 2.5px;
  flex-shrink: 0;
}

.minimal-info {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.minimal-info svg {
  flex-shrink: 0;
  color: var(--accent-primary);
}

.style-compact-grid {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
}

.style-tag {
  padding: 4px 9px;
  border-radius: 6px;
  background: var(--surface-tertiary);
  border: 1px solid var(--border-subtle);
  color: var(--text-secondary);
  font-family: inherit;
  font-size: 11.5px;
  cursor: pointer;
  transition: all 0.12s;
}

.style-tag:hover {
  color: var(--text-primary);
  border-color: var(--border-focus);
}

.style-tag--active {
  background: var(--accent-primary);
  border-color: var(--accent-primary);
  color: #090a0f;
  font-weight: 600;
}

:root[data-theme='light'] .style-tag--active {
  color: #ffffff;
}

.pop-input {
  width: 100%;
  padding: 7px 10px;
  border-radius: 7px;
  background: var(--surface-secondary);
  border: 1px solid var(--border-subtle);
  color: var(--text-primary);
  font-family: inherit;
  font-size: 12px;
  outline: none;
}

.pop-input:focus {
  border-color: var(--border-focus);
}

.fmt-row {
  display: grid;
  grid-template-columns: repeat(5, 1fr);
  gap: 5px;
}

.fmt-pill {
  padding: 5px 0;
  text-align: center;
  border-radius: 6px;
  background: var(--surface-tertiary);
  border: 1px solid var(--border-subtle);
  color: var(--text-secondary);
  font-family: var(--font-mono);
  font-size: 11.5px;
  font-weight: 600;
  cursor: pointer;
  transition: all 0.13s;
}

.fmt-pill--active {
  background: var(--surface-active);
  color: var(--accent-primary);
  border-color: var(--border-focus);
}

.minimal-info {
  margin: 0;
  font-size: 11.5px;
  color: var(--text-muted);
  padding: 2px 0;
}

/* 面板弹出/收起动画 */
.pop-enter-active {
  animation: pop-up 0.16s cubic-bezier(0.16, 1, 0.3, 1);
}

.pop-leave-active {
  transition:
    opacity 0.12s,
    transform 0.12s;
}

.pop-leave-to {
  opacity: 0;
  transform: translateY(8px) scale(0.97);
}
</style>
