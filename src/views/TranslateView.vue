<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { listen } from '@tauri-apps/api/event'
import LangSelect from '../components/LangSelect.vue'
import BackendPicker from '../components/BackendPicker.vue'
import { api, err, toast } from '../api'
import { glossary, meta } from '../composables/useAppState'
import type { Mode, TermPair } from '../types'

/** 「自动检测」在 UI 里的名字。左右两栏都用中文名比对，故只存这一个值。 */
const AUTO_LANG = '检测语言'

// ---------- state ----------
const source = ref('')
const result = ref('')
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
const busy = ref(false)
const usedTerms = ref<string[]>([])
const elapsed = ref(0)
const showAdvanced = ref(false)
const currentJob = ref('')
const failed = ref('')
const inputEl = ref<HTMLTextAreaElement | null>(null)
/** 当前译文是否已收藏，持久化在 localStorage */
const starred = ref(false)

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

/** 最近一次完成翻译的原文，用来识别「只是改了内容」还是「原样重跑」。 */
let lastTranslated = ''
let debounceTimer: number | undefined
/** 翻译进行中触发的自动翻译不丢弃，等这一轮结束补跑。 */
let autoPending = false

// ---------- derived ----------
const modeOptions = computed(() => meta.value.modes.map(([k, label]) => ({ value: k as Mode, label })))
const styleOptions = computed(() => meta.value.styles)
const formatOptions = computed(() => meta.value.format_types)
const allLangs = computed<[string, string][]>(() => meta.value.languages)

/** 源语言下拉要比目标语言多一项「自动检测」，用同样的中文名参与比对。 */
const sourceLangOptions = computed<[string, string][]>(() => [['auto', AUTO_LANG], ...allLangs.value])

/** 收藏以「原文+译文」为键，换一段文本就是另一条收藏。 */
const starKey = computed(() => `${source.value}\u0000${result.value}`)

function syncStar() {
  try {
    const raw = localStorage.getItem('hymt2:starred')
    starred.value = raw ? (JSON.parse(raw) as string[]).includes(starKey.value) : false
  } catch {
    starred.value = false
  }
}

function toggleStar() {
  let list: string[] = []
  try {
    list = JSON.parse(localStorage.getItem('hymt2:starred') ?? '[]')
  } catch {}
  if (starred.value) {
    list = list.filter((k) => k !== starKey.value)
  } else {
    list.push(starKey.value)
  }
  localStorage.setItem('hymt2:starred', JSON.stringify(list))
  starred.value = !starred.value
  toast(starred.value ? '已收藏这条译文' : '已取消收藏', 'success')
}

onMounted(() => {
  loadGlossaryTerms()

  window.addEventListener('hymt2:reuse', onReuse)
  const pending = localStorage.getItem('hymt2:reuse')
  if (pending) {
    localStorage.removeItem('hymt2:reuse')
    onReuse(new CustomEvent('hymt2:reuse', { detail: JSON.parse(pending) }) as CustomEvent)
  }
})

watch(starKey, syncStar)

function onReuse(e: Event) {
  const item = (e as CustomEvent).detail
  source.value = item.source
  result.value = item.translation
  targetLang.value = item.target_lang
  mode.value = item.mode as Mode
  lastTranslated = item.source
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

// ---------- streaming ----------
const unlisten: Array<() => void> = []

function newJobId(): string {
  const c = globalThis.crypto
  if (c && typeof c.randomUUID === 'function') return c.randomUUID()
  return `j-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`
}

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

async function run() {
  if (!source.value.trim()) {
    toast('请输入要翻译的内容')
    return
  }
  busy.value = true
  failed.value = ''
  result.value = ''
  usedTerms.value = []
  elapsed.value = 0

  const jobId = newJobId()
  currentJob.value = jobId

  unlisten.forEach((f) => f())
  unlisten.length = 0

  try {
    unlisten.push(
      await listen<{ job_id: string; text: string }>('hymt2:delta', (e) => {
        if (e.payload.job_id !== currentJob.value) return
        result.value += e.payload.text
        const out = document.querySelector('.card-pane__body--out')
        if (out) out.scrollTop = out.scrollHeight
      })
    )
    unlisten.push(
      await listen<{ job_id: string; text: string; ms: number; used_terms: string[] }>(
        'hymt2:done',
        (e) => {
          if (e.payload.job_id !== currentJob.value) return
          result.value = e.payload.text
          elapsed.value = e.payload.ms
          usedTerms.value = e.payload.used_terms ?? []
          lastTranslated = source.value
          busy.value = false
        }
      )
    )
    unlisten.push(
      await listen<{ job_id: string; message: string }>('hymt2:error', (e) => {
        if (e.payload.job_id !== currentJob.value) return
        busy.value = false
        failed.value = e.payload.message
        err(e.payload.message)
      })
    )

    await api.translateStream(buildRequest(), jobId)

    window.setTimeout(() => {
      if (busy.value && currentJob.value === jobId) {
        busy.value = false
        failed.value = '翻译超时或未收到结果，请重试'
      }
    }, 180000)
  } catch (e) {
    busy.value = false
    failed.value = String(e)
    err(e)
  }
}

onUnmounted(() => {
  unlisten.forEach((f) => f())
  window.clearTimeout(debounceTimer)
})

function clearAll() {
  source.value = ''
  result.value = ''
  failed.value = ''
  usedTerms.value = []
  elapsed.value = 0
  lastTranslated = ''
  autoPending = false
  window.clearTimeout(debounceTimer)
  inputEl.value?.focus()
}

function scheduleAuto() {
  window.clearTimeout(debounceTimer)
  if (!source.value.trim()) return
  debounceTimer = window.setTimeout(() => {
    const text = source.value
    if (text.trim() === lastTranslated.trim()) return
    if (busy.value) {
      autoPending = true
      return
    }
    run()
  }, 1000)
}

// 自动翻译是默认且唯一的行为，不再提供开关
watch(source, scheduleAuto)

// 无论从药丸还是从下拉切换语言，都把它记进表头快捷项
watch(sourceLang, (v) => rememberPick('src', v))
watch(targetLang, (v) => rememberPick('tgt', v))

watch(busy, (isBusy) => {
  if (isBusy || !autoPending) return
  autoPending = false
  scheduleAuto()
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
  lastTranslated = source.value
}

async function copy(text: string) {
  await navigator.clipboard.writeText(text)
  toast('已复制到剪贴板', 'success')
}
</script>

<template>
  <div class="gt-main">
    <!-- 主翻译双卡片容器 -->
    <div class="workspace">
      <!-- 左侧：输入卡片（纯白卡片，微阴影，大圆角） -->
      <section class="card-pane card-pane--in">
        <!-- 语言选择胶囊行 -->
        <header class="card-pane__header">
          <div class="lang-group">
            <div class="lang-pills">
              <button
                v-for="l in sourcePills"
                :key="l"
                class="lang-pill"
                :class="{ 'lang-pill--active': sourceLang === l }"
                @click="sourceLang = l"
              >
                {{ l }}
              </button>
            </div>
            <LangSelect v-model="sourceLang" :options="sourceLangOptions" />
          </div>
        </header>

        <!-- 输入区域 -->
        <div class="card-pane__body">
          <textarea
            ref="inputEl"
            v-model="source"
            class="editor-input"
            placeholder="输入或粘贴文本…"
            spellcheck="false"
            @keydown.ctrl.enter="run"
            @keydown.esc="clearAll"
          />

          <!-- 极简清空 ✕ 按钮（只在有内容时显现） -->
          <button
            v-if="source"
            class="action-btn action-btn--clear"
            title="清空所有内容 (Esc)"
            @click="clearAll"
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        </div>

        <!-- 左卡片底栏 -->
        <footer class="card-pane__footer">
          <div class="footer-left">
            <BackendPicker />
            <button class="footer-icon-btn" title="提示词参数配置" @click="showAdvanced = !showAdvanced">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <circle cx="12" cy="12" r="3" />
                <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
              </svg>
            </button>
          </div>

          <div class="footer-right">
            <span v-if="busy" class="spinner" />
            <span class="char-count">{{ source.length }} / 5000</span>
          </div>
        </footer>
      </section>

      <!-- 中央正中浮动圆形交换按钮 -->
      <div class="swap-dock">
        <button
          class="floating-swap-btn"
          :class="{ 'floating-swap-btn--dim': sourceLang === AUTO_LANG }"
          :title="swapHint"
          @click="swap"
        >
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M8 3L4 7l4 4" />
            <path d="M4 7h16" />
            <path d="M16 21l4-4-4-4" />
            <path d="M20 17H4" />
          </svg>
        </button>
      </div>

      <!-- 右侧：译文卡片（微浅蓝底色 #f0f4f9，雅致反差） -->
      <section class="card-pane card-pane--out">
        <!-- 目标语言选择胶囊行 -->
        <header class="card-pane__header">
            <div class="lang-group">
              <div class="lang-pills">
                <button
                  v-for="l in targetPills"
                  :key="l"
                  class="lang-pill"
                  :class="{ 'lang-pill--active': targetLang === l }"
                  @click="targetLang = l"
                >
                  {{ l }}
                </button>
              </div>
              <LangSelect v-model="targetLang" :options="allLangs" align="right" />
            </div>

          <!-- 右上角收藏星标 -->
          <button
            class="star-btn"
            :class="{ 'star-btn--on': starred }"
            :disabled="!result"
            :title="starred ? '取消收藏' : '收藏本条翻译'"
            @click="toggleStar"
          >
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
            </svg>
          </button>
        </header>

        <!-- 译文输出区域 -->
        <div class="card-pane__body card-pane__body--out">
          <p v-if="failed" class="error-msg">⚠️ {{ failed }}</p>
          <div v-else-if="result" class="output-text">
            {{ result }}<span v-if="busy" class="stream-caret" />
          </div>
          <p v-else-if="busy" class="placeholder-msg">模型思考翻译中…</p>
          <p v-else class="placeholder-msg">译文将实时显示于此</p>
        </div>

        <!-- 右卡片底栏 -->
        <footer class="card-pane__footer">
          <div class="footer-left">
            <span v-if="elapsed && result" class="time-tag">{{ (elapsed / 1000).toFixed(1) }}s</span>
            <!-- 命中术语展示 -->
            <div v-if="usedTerms.length" class="terms-tag-group">
              <span v-for="t in usedTerms" :key="t" class="term-pill">{{ t }}</span>
            </div>
          </div>

          <div class="footer-right">
            <!-- 复制按钮 -->
            <button
              class="footer-icon-btn"
              :disabled="!result"
              title="复制译文"
              @click="copy(result)"
            >
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <rect x="9" y="9" width="13" height="13" rx="2" />
                <path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1" />
              </svg>
            </button>
            <!-- 反向翻译按钮 -->
            <button
              class="footer-icon-btn"
              :disabled="!result"
              :title="swapHint"
              @click="swap"
            >
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M17 1l4 4-4 4" />
                <path d="M3 11V9a4 4 0 0 1 4-4h14" />
                <path d="M7 23l-4-4 4-4" />
                <path d="M21 13v2a4 4 0 0 1-4 4H3" />
              </svg>
            </button>
          </div>
        </footer>
      </section>
    </div>

    <!-- 高级配置抽屉（折叠状态） -->
    <transition name="drop">
      <div v-if="showAdvanced" class="adv-drawer">
        <div class="adv-grid">
          <label class="adv-field">
            <span class="adv-field__label">翻译模式</span>
            <select v-model="mode" class="select">
              <option v-for="m in modeOptions" :key="m.value" :value="m.value">{{ m.label }}</option>
            </select>
          </label>
          <label v-if="mode === 'style'" class="adv-field">
            <span class="adv-field__label">目标风格</span>
            <select v-model="style" class="select">
              <option v-for="s in styleOptions" :key="s" :value="s">{{ s }}</option>
            </select>
          </label>
          <label v-if="mode === 'structured'" class="adv-field">
            <span class="adv-field__label">数据格式</span>
            <select v-model="formatType" class="select">
              <option v-for="f in formatOptions" :key="f" :value="f">{{ f }}</option>
            </select>
          </label>
          <label v-if="mode === 'context'" class="adv-field adv-field--grow">
            <span class="adv-field__label">背景信息</span>
            <input v-model="context" class="input" placeholder="例如：医学报告或工程机械展会场景" />
          </label>
        </div>

        <div v-if="mode === 'terminology'" class="adv-term-box">
          <span class="adv-field__label">专有术语（源词 =&gt; 译词，每行一条）· 激活词库「{{ glossary.active }}」</span>
          <textarea v-model="termsEditor" class="input" rows="3" spellcheck="false" />
          <div class="adv-term-opts">
            <label class="check"><input v-model="autoFilterTerms" type="checkbox" /> 仅使用原文命中的词</label>
            <label class="check"><input v-model="ignoreGlossary" type="checkbox" /> 未命中时退回基础翻译</label>
          </div>
        </div>
      </div>
    </transition>
  </div>
</template>

<style scoped>
.gt-main {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: 0;
  box-sizing: border-box;
  background: var(--c-bg);
}

/* 核心工作区：两张卡片无缝相邻，通栏铺满 */
.workspace {
  position: relative;
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 0;
  flex: 1;
  min-height: 0;
}

.card-pane {
  display: flex;
  flex-direction: column;
  border-radius: var(--r-xl);
  overflow: hidden;
  box-sizing: border-box;
}

/* 左侧输入卡片：纯白底，靠底色与右侧淡蓝卡区分，不加边框与阴影 */
.card-pane--in {
  background: var(--c-surface);
  border: none;
  box-shadow: none;
}

/* 右侧输出卡片：纯淡蓝底色，不要边框、阴影与圆角 */
.card-pane--out {
  background: var(--c-pane-out-bg);
  border: none;
  border-radius: 0;
  box-shadow: none;
}

/* 卡片顶部语言药丸栏。--header-h / --pill-h 供选中下划线定位用 */
.card-pane__header {
  --header-h: 52px;
  --pill-h: 36px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: var(--header-h);
  padding: 0 16px;
  border-bottom: 1px solid rgba(0, 0, 0, 0.05);
}

.lang-group {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
  min-width: 0;
}

/* 语种多到放不下时只让这一段横向滚动，下拉箭头始终留在可视区 */
.lang-pills {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  overflow-x: auto;
  scrollbar-width: none;
}

.lang-pills::-webkit-scrollbar {
  display: none;
}

/* 选中态用底部横线表示，不再铺药丸底色 */
.lang-pill {
  position: relative;
  display: inline-flex;
  align-items: center;
  flex-shrink: 0;
  height: var(--pill-h);
  padding: 0 16px;
  font-family: inherit;
  font-size: 14px;
  font-weight: 500;
  color: var(--c-text-secondary);
  background: none;
  border: none;
  cursor: pointer;
  transition: color 0.15s ease;
  white-space: nowrap;
}

.lang-pill:hover {
  color: var(--c-text);
}

.lang-pill--active {
  color: var(--c-primary);
  font-weight: 600;
}

/* 药丸在表头里垂直居中，它到底部边框的距离是 (header-h - pill-h) / 2。
   取负值让蓝线正好压在 border 上，而不是浮在药丸底下。 */
.lang-pill--active::after {
  content: '';
  position: absolute;
  left: 12px;
  right: 12px;
  bottom: calc((var(--pill-h) - var(--header-h)) / 2);
  height: 2px;
  background: var(--c-primary);
  border-radius: 2px;
}

.star-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  width: 36px;
  height: 36px;
  background: none;
  border: none;
  border-radius: 50%;
  color: var(--c-text-tertiary);
  cursor: pointer;
  transition: all 0.15s;
}

.star-btn:hover {
  background: rgba(0, 0, 0, 0.05);
  color: var(--c-text);
}

.star-btn--on {
  color: #fbbc04;
}

.star-btn--on svg {
  fill: #fbbc04;
}

/* 卡片主输入/输出区：大字号 20px，极度清晰舒适 */
.card-pane__body {
  position: relative;
  display: flex;
  flex: 1;
  min-height: 0;
  padding: 20px 24px;
  overflow-y: auto;
}

/* 作为 flex item 被块级化，不再产生 inline-block 的基线间隙，
   否则 height 填满时仍会溢出 4px 而显示空滚动条。 */
.editor-input {
  flex: 1;
  min-height: 0;
  width: 100%;
  font-family: var(--font-ui);
  font-size: 20px;
  line-height: 1.6;
  color: var(--c-text);
  background: transparent;
  border: none;
  outline: none;
  resize: none;
  box-sizing: border-box;
}

.editor-input::placeholder {
  color: var(--c-text-tertiary);
}

.action-btn--clear {
  position: absolute;
  top: 18px;
  right: 18px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  background: none;
  border: none;
  border-radius: 50%;
  color: var(--c-text-secondary);
  cursor: pointer;
  transition: background 0.15s;
}

.action-btn--clear:hover {
  background: var(--c-surface-hover);
  color: var(--c-text);
}

.output-text {
  font-size: 20px;
  line-height: 1.6;
  color: var(--c-text);
  white-space: pre-wrap;
  word-break: break-word;
}

.placeholder-msg {
  margin: 0;
  font-size: 20px;
  color: var(--c-text-tertiary);
}

.error-msg {
  color: var(--c-danger);
  font-size: 15px;
}

.stream-caret {
  display: inline-block;
  width: 2px;
  height: 1.1em;
  margin-left: 2px;
  vertical-align: text-bottom;
  background: var(--c-primary);
  animation: blink 1s step-end infinite;
}

@keyframes blink {
  50% { opacity: 0; }
}

/* 卡片底栏 */
.card-pane__footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 52px;
  padding: 0 16px 8px;
}

.footer-left {
  display: flex;
  align-items: center;
  gap: 8px;
}

.footer-right {
  display: flex;
  align-items: center;
  gap: 12px;
}

.footer-icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 36px;
  height: 36px;
  background: none;
  border: none;
  border-radius: 50%;
  color: var(--c-text-secondary);
  cursor: pointer;
  transition: all 0.15s;
}

.footer-icon-btn:hover:not(:disabled) {
  background: rgba(0, 0, 0, 0.06);
  color: var(--c-text);
}

.footer-icon-btn:disabled {
  opacity: 0.3;
  cursor: not-allowed;
}

.char-count {
  font-size: 13px;
  color: var(--c-text-tertiary);
}

/* 翻译中的轻量指示（原先挂在发送按钮上，按钮移除后留在底栏） */
.spinner {
  width: 13px;
  height: 13px;
  border: 2px solid var(--c-border);
  border-top-color: var(--c-primary);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

.time-tag {
  font-size: 12px;
  color: var(--c-text-tertiary);
}

.terms-tag-group {
  display: flex;
  align-items: center;
  gap: 4px;
}

.term-pill {
  padding: 2px 8px;
  font-size: 11px;
  color: var(--c-pill-text);
  background: var(--c-pill-bg);
  border-radius: var(--r-pill);
}

/* 中央悬浮交换按钮：浮于两张卡片正中央 */
.swap-dock {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  z-index: 10;
  pointer-events: none;
}

.floating-swap-btn {
  pointer-events: auto;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 44px;
  height: 44px;
  background: #ffffff;
  color: var(--c-text-secondary);
  border: 1px solid var(--c-border);
  border-radius: 50%;
  box-shadow: 0 3px 12px rgba(60, 64, 67, 0.12);
  cursor: pointer;
  transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
}

.floating-swap-btn:hover {
  background: var(--c-surface-hover);
  color: var(--c-primary);
  transform: scale(1.1);
  box-shadow: 0 4px 16px rgba(26, 115, 232, 0.2);
}

/* 源语言为自动检测时反方向未知，置灰提示 */
.floating-swap-btn--dim {
  color: var(--c-text-tertiary);
  opacity: 0.55;
}

.floating-swap-btn--dim:hover {
  color: var(--c-text-tertiary);
  background: #ffffff;
  transform: scale(1.05);
  box-shadow: 0 3px 12px rgba(60, 64, 67, 0.12);
}

/* 高级配置抽屉 */
.adv-drawer {
  margin-top: 16px;
  padding: 16px 20px;
  background: var(--c-surface-alt);
  border-radius: var(--r-lg);
  border: 1px solid var(--c-border-soft);
}

.adv-grid {
  display: flex;
  flex-wrap: wrap;
  gap: 16px;
}

.adv-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.adv-field--grow {
  flex: 1;
}

.adv-field__label {
  font-size: 12px;
  font-weight: 500;
  color: var(--c-text-secondary);
}

.adv-term-box {
  margin-top: 14px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.adv-term-opts {
  display: flex;
  gap: 16px;
  font-size: 13px;
}

.grow {
  flex: 1;
}
</style>
