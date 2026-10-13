<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { listen } from '@tauri-apps/api/event'
import LangSelect from '../components/LangSelect.vue'
import { api, err, toast } from '../api'
import { glossary, meta } from '../composables/useAppState'
import type { Mode, TermPair } from '../types'

// ---------- state ----------
const source = ref('')
const result = ref('')
const sourceLang = ref('auto')
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
const autoTranslate = ref(true)
/** 当前译文是否已收藏，持久化在 localStorage */
const starred = ref(false)

// 设计图常用语言快捷药丸（用户可一键切换，也可点下拉选全部 38 种）
const quickSourceLangs = [
  { code: 'auto', name: '检测语言' },
  { code: 'zh', name: '中文' },
  { code: 'en', name: '英语' },
  { code: 'ja', name: '日语' }
]

const quickTargetLangs = [
  { code: 'en', name: '英语' },
  { code: 'zh', name: '中文' },
  { code: 'ja', name: '日语' },
  { code: 'ko', name: '韩语' }
]

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

watch(source, () => {
  if (autoTranslate.value) scheduleAuto()
})

watch(busy, (isBusy) => {
  if (isBusy || !autoPending) return
  autoPending = false
  scheduleAuto()
})

function swap() {
  const t = source.value
  source.value = result.value
  result.value = t
  targetLang.value = targetLang.value === '中文' ? '英语' : '中文'
  lastTranslated = source.value
}

async function copy(text: string) {
  await navigator.clipboard.writeText(text)
  toast('已复制到剪贴板', 'success')
}

async function showPrompt() {
  try {
    const p = await api.previewPrompt(buildRequest())
    console.log(p)
    toast('提示词已输出到控制台', 'success')
  } catch (e) {
    err(e)
  }
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
            <button
              v-for="l in quickSourceLangs"
              :key="l.code"
              class="lang-pill"
              :class="{ 'lang-pill--active': sourceLang === l.code }"
              @click="sourceLang = l.code"
            >
              {{ l.name }}
            </button>
            <LangSelect v-model="sourceLang" :options="allLangs" />
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
            <!-- 语音/查看提示词图标（对齐设计图左下角微标） -->
            <button class="footer-icon-btn" title="查看发送的完整模型提示词" @click="showPrompt">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
                <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
                <line x1="12" y1="19" x2="12" y2="22" />
              </svg>
            </button>
            <button class="footer-icon-btn" title="提示词参数配置" @click="showAdvanced = !showAdvanced">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <circle cx="12" cy="12" r="3" />
                <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
              </svg>
            </button>
          </div>

          <div class="footer-right">
            <span class="char-count">{{ source.length }} / 5000</span>
            <label class="auto-switch" title="停止输入 1 秒后自动执行翻译">
              <input v-model="autoTranslate" type="checkbox" />
              <span class="auto-switch__label">自动翻译</span>
            </label>
            <button
              class="primary-go-btn"
              :disabled="busy || !source.trim()"
              title="立即执行翻译 (Ctrl+Enter)"
              @click="run"
            >
              <span v-if="busy" class="spinner" />
              <svg v-else width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
                <line x1="5" y1="12" x2="19" y2="12" />
                <polyline points="12 5 19 12 12 19" />
              </svg>
            </button>
          </div>
        </footer>
      </section>

      <!-- 中央正中浮动圆形交换按钮 -->
      <div class="swap-dock">
        <button class="floating-swap-btn" title="交换语言与内容" @click="swap">
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
            <button
              v-for="l in quickTargetLangs"
              :key="l.code"
              class="lang-pill"
              :class="{ 'lang-pill--active': targetLang === l.name }"
              @click="targetLang = l.name"
            >
              {{ l.name }}
            </button>
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
              title="反向翻译"
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
  padding: 20px 24px;
  box-sizing: border-box;
  background: var(--c-bg);
}

/* 核心工作区：占满除边距外的全部高度，双卡片各自撑满 */
.workspace {
  position: relative;
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 16px;
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

/* 右侧输出卡片：纯淡蓝底色，不要边框与阴影 */
.card-pane--out {
  background: var(--c-pane-out-bg);
  border: none;
  box-shadow: none;
}

/* 卡片顶部语言药丸栏 */
.card-pane__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 52px;
  padding: 0 16px;
  border-bottom: 1px solid rgba(0, 0, 0, 0.05);
}

.lang-group {
  display: flex;
  align-items: center;
  gap: 8px;
}

.lang-pill {
  padding: 8px 16px;
  font-family: inherit;
  font-size: 14px;
  font-weight: 500;
  color: var(--c-text-secondary);
  background: none;
  border: none;
  border-radius: var(--r-pill);
  cursor: pointer;
  transition: all 0.15s ease;
  white-space: nowrap;
}

.lang-pill:hover {
  background: var(--c-surface-hover);
  color: var(--c-text);
}

.lang-pill--active {
  background: var(--c-pill-bg);
  color: var(--c-pill-text);
  font-weight: 600;
}

.star-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
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

.auto-switch {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  color: var(--c-text-secondary);
  cursor: pointer;
  user-select: none;
}

.auto-switch input {
  cursor: pointer;
}

.primary-go-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 38px;
  height: 38px;
  background: var(--c-primary);
  color: #fff;
  border: none;
  border-radius: 50%;
  cursor: pointer;
  box-shadow: 0 2px 6px rgba(26, 115, 232, 0.3);
  transition: all 0.15s;
}

.primary-go-btn:hover:not(:disabled) {
  background: var(--c-primary-hover);
  transform: scale(1.05);
}

.primary-go-btn:disabled {
  background: #dadce0;
  box-shadow: none;
  cursor: not-allowed;
}

.spinner {
  width: 16px;
  height: 16px;
  border: 2px solid rgba(255, 255, 255, 0.4);
  border-top-color: #fff;
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
