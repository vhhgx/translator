<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { api, err, toast } from '../api'
import { glossary, meta, pickBackend, settings } from '../composables/useAppState'
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
const showAllLangs = ref(false)
const currentJob = ref<string>('')
const failed = ref('')

// ---------- derived ----------
const modeOptions = computed(() => meta.value.modes.map(([k, label]) => ({ value: k as Mode, label })))
const styleOptions = computed(() => meta.value.styles)
const formatOptions = computed(() => meta.value.format_types)

const QUICK = ['英语', '中文', '日语', '韩语', '法语', '德语', '西班牙语']
const allLangs = computed(() => meta.value.languages.map(([code, name]) => ({ value: name, code })))

/** Tabs show a few common languages; the rest live in the dropdown. */
function quickLangs(exclude: string) {
  return QUICK.filter((l) => l !== exclude)
}

const tabsForSource = computed(() => ['自动检测', ...quickLangs(targetLang.value)])
const tabsForTarget = computed(() => quickLangs('自动检测').filter((l) => l !== sourceLang.value || sourceLang.value === '自动检测'))

function isActiveTab(tab: string, current: string) {
  return tab === current
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

function onReuse(e: Event) {
  const item = (e as CustomEvent).detail
  source.value = item.source
  result.value = item.translation
  targetLang.value = item.target_lang
  mode.value = item.mode as Mode
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

function insertGlossary() {
  mode.value = 'terminology'
  showAdvanced.value = true
}

// ---------- streaming ----------
const unlisten: Array<() => void> = []

// `crypto.randomUUID` is unavailable in non-secure contexts; fall back so a
// job id always exists rather than throwing before the request is even sent.
function newJobId(): string {
  const c = globalThis.crypto
  if (c && typeof c.randomUUID === 'function') return c.randomUUID()
  return `j-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`
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
  showAllLangs.value = false

  const jobId = newJobId()
  currentJob.value = jobId

  // Detach any previous listeners so a stale job can't append.
  unlisten.forEach((f) => f())
  unlisten.length = 0

  try {
    unlisten.push(
      await listen<{ job_id: string; text: string }>('hymt2:delta', (e) => {
        if (e.payload.job_id !== currentJob.value) return
        result.value += e.payload.text
        const out = document.querySelector('.t-card__body--out')
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

    await api.translateStream(
      {
        text: source.value,
        target_lang: targetLang.value,
        mode: mode.value,
        terms: parseTerms(),
        auto_filter_terms: autoFilterTerms.value,
        style: style.value,
        format_type: formatType.value,
        context: context.value,
        ignore_glossary: ignoreGlossary.value
      },
      jobId
    )
    // Safety net: never leave the spinner running if no terminal event lands.
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

onUnmounted(() => unlisten.forEach((f) => f()))

// ---------- misc ----------
function swap() {
  const t = source.value
  source.value = result.value
  result.value = t
  const l = targetLang.value
  targetLang.value = sourceLang.value === '自动检测' ? '英语' : sourceLang.value
  sourceLang.value = l
}

async function copy(text: string) {
  await navigator.clipboard.writeText(text)
  toast('已复制', 'success')
}

async function showPrompt() {
  try {
    const p = await api.previewPrompt({
      text: source.value,
      target_lang: targetLang.value,
      mode: mode.value,
      terms: parseTerms(),
      auto_filter_terms: autoFilterTerms.value,
      style: style.value,
      format_type: formatType.value,
      context: context.value,
      ignore_glossary: ignoreGlossary.value
    })
    console.log(p)
    toast('提示词已输出到控制台', 'success')
  } catch (e) {
    err(e)
  }
}
</script>

<template>
  <div class="gt">
    <!-- 顶部：后端 + 高级 -->
    <div class="gt__top">
      <div class="chip-row">
        <select
          class="chip-select"
          :value="settings.active_id"
          @change="pickBackend(($event.target as HTMLSelectElement).value)"
        >
          <option v-for="b in settings.backends" :key="b.id" :value="b.id">
            {{ b.name }}{{ b.has_api_key ? ' 🔑' : '' }}
          </option>
        </select>
        <span class="chip">{{ modeOptions.find((m) => m.value === mode)?.label }}</span>
      </div>
      <div class="gt__top-right">
        <button class="icon-btn" title="查看提示词" @click="showPrompt">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M4 19.5A2.5 2.5 0 016.5 17H20" />
            <path d="M6.5 2H20v20H6.5A2.5 2.5 0 014 19.5v-15A2.5 2.5 0 016.5 2z" />
          </svg>
        </button>
      </div>
    </div>

    <!-- 语言 Tab 行 -->
    <div class="lang-row">
      <div class="lang-tabs">
        <button
          v-for="t in tabsForSource"
          :key="t"
          class="lang-tab"
          :class="{ 'lang-tab--active': isActiveTab(t, sourceLang) }"
          @click="sourceLang = t"
        >
          {{ t }}
        </button>
        <button class="lang-tab lang-tab--chev" title="更多语言" @click="showAllLangs = !showAllLangs">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M6 9l6 6 6-6" />
          </svg>
        </button>
      </div>

      <button class="swap-btn" title="交换语言" @click="swap">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M16 3l4 4-4 4" />
          <path d="M20 7H4" />
          <path d="M8 21l-4-4 4-4" />
          <path d="M4 17h16" />
        </svg>
      </button>

      <div class="lang-tabs">
        <button
          v-for="t in tabsForTarget"
          :key="t"
          class="lang-tab"
          :class="{ 'lang-tab--active': t === targetLang }"
          @click="targetLang = t"
        >
          {{ t }}
        </button>
        <button class="lang-tab lang-tab--chev" title="更多语言" @click="showAllLangs = !showAllLangs">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M6 9l6 6 6-6" />
          </svg>
        </button>
      </div>
    </div>

    <!-- 更多语言面板 -->
    <div v-if="showAllLangs" class="lang-panel">
      <button
        v-for="l in allLangs"
        :key="l.code"
        class="lang-opt"
        :class="{ 'lang-opt--active': l.value === targetLang }"
        @click="targetLang = l.value; showAllLangs = false"
      >
        {{ l.value }}
      </button>
    </div>

    <!-- 双卡片 -->
    <div class="cards">
      <div class="t-card">
        <div class="t-card__body">
          <textarea
            v-model="source"
            class="plain-area"
            placeholder="输入或粘贴文本…"
            spellcheck="false"
            @keydown.ctrl.enter="run"
          />
        </div>
        <div class="t-card__foot">
          <span class="count">{{ source.length }} 字</span>
          <span class="grow" />
          <button class="text-btn" @click="showAdvanced = !showAdvanced">
            高级
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
              <path d="M6 9l6 6 6-6" />
            </svg>
          </button>
          <button
            class="round-btn round-btn--primary"
            :disabled="busy || !source.trim()"
            title="翻译 (Ctrl+Enter)"
            @click="run"
          >
            <span v-if="busy" class="spin" />
            <svg v-else width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M21 12a9 9 0 11-3-6.7" />
              <path d="M21 3v6h-6" />
            </svg>
          </button>
        </div>
      </div>

      <div class="t-card t-card--output">
        <div class="t-card__body t-card__body--out">
          <p v-if="failed" class="result result--error">⚠️ {{ failed }}</p>
          <p v-else-if="result" class="result">
            {{ result }}<span v-if="busy" class="stream-caret" />
          </p>
          <p v-else-if="busy" class="placeholder">翻译中…</p>
          <p v-else class="placeholder">译文</p>
        </div>
        <div class="t-card__foot">
          <span v-if="result && !busy" class="count">{{ (elapsed / 1000).toFixed(1) }}s</span>
          <span class="grow" />
          <button v-if="result" class="icon-btn" title="复制" @click="copy(result)">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <rect x="9" y="9" width="13" height="13" rx="2" />
              <path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1" />
            </svg>
          </button>
        </div>
      </div>
    </div>

    <!-- 命中术语 -->
    <div v-if="usedTerms.length" class="hits">
      命中术语：<span v-for="t in usedTerms" :key="t" class="tag tag--primary">{{ t }}</span>
    </div>

    <!-- 高级面板 -->
    <div v-if="showAdvanced" class="advanced">
      <div class="adv-grid">
        <div class="field">
          <label class="label">翻译模式</label>
          <select v-model="mode" class="input">
            <option v-for="m in modeOptions" :key="m.value" :value="m.value">{{ m.label }}</option>
          </select>
        </div>
        <div v-if="mode === 'style'" class="field">
          <label class="label">目标风格</label>
          <select v-model="style" class="input">
            <option v-for="s in styleOptions" :key="s" :value="s">{{ s }}</option>
          </select>
        </div>
        <div v-if="mode === 'structured'" class="field">
          <label class="label">数据格式</label>
          <select v-model="formatType" class="input">
            <option v-for="f in formatOptions" :key="f" :value="f">{{ f }}</option>
          </select>
        </div>
        <div v-if="mode === 'context'" class="field field--grow">
          <label class="label">背景信息</label>
          <input v-model="context" class="input" placeholder="例如：医学检测报告场景" />
        </div>
      </div>

      <div v-if="mode === 'terminology'" class="field">
        <label class="label">术语（每行一条：源词 => 译词）· 词库「{{ glossary.active }}」</label>
        <textarea v-model="termsEditor" class="input mono" rows="4" spellcheck="false" />
        <div class="checks">
          <label class="check"><input v-model="autoFilterTerms" type="checkbox" /> 仅使用原文命中的词</label>
          <label class="check"><input v-model="ignoreGlossary" type="checkbox" /> 未命中时退回基础翻译</label>
          <button class="text-btn" @click="insertGlossary">打开词库</button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.gt {
  display: flex;
  flex-direction: column;
  gap: var(--sp-3);
  height: 100%;
  padding: var(--sp-4) var(--sp-5) var(--sp-5);
  overflow-y: auto;
}

/* 顶部 */
.gt__top {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.chip-row {
  display: flex;
  gap: var(--sp-2);
  align-items: center;
}

.chip {
  padding: 4px 12px;
  font-size: 12px;
  color: var(--c-text-secondary);
  background: var(--c-surface-alt);
  border: 1px solid var(--c-border-soft);
  border-radius: var(--r-full);
}

.chip-select {
  padding: 4px 26px 4px 12px;
  font-family: inherit;
  font-size: 12px;
  color: var(--c-primary);
  background-color: var(--c-primary-soft);
  border: 1px solid transparent;
  border-radius: var(--r-full);
  appearance: none;
  cursor: pointer;
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='10' viewBox='0 0 24 24' fill='none' stroke='%231a73e8' stroke-width='3'%3E%3Cpath d='M6 9l6 6 6-6'/%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right 10px center;
}

.icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  color: var(--c-text-secondary);
  background: none;
  border: none;
  border-radius: 50%;
  cursor: pointer;
}

.icon-btn:hover {
  background: var(--c-surface-hover);
  color: var(--c-text);
}

/* 语言行 */
.lang-row {
  display: grid;
  grid-template-columns: 1fr auto 1fr;
  align-items: center;
  gap: var(--sp-2);
}

.swap-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  color: var(--c-text-secondary);
  background: var(--c-surface-alt);
  border: 1px solid var(--c-border-soft);
  border-radius: 50%;
  cursor: pointer;
  transition:
    transform 0.2s,
    color 0.15s;
}

.swap-btn:hover {
  color: var(--c-primary);
  transform: scale(1.06);
}

.lang-tab--chev {
  display: inline-flex;
  align-items: center;
}

/* 更多语言 */
.lang-panel {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  max-height: 180px;
  overflow-y: auto;
  padding: var(--sp-3);
  background: var(--c-surface-alt);
  border: 1px solid var(--c-border-soft);
  border-radius: var(--r-md);
}

.lang-opt {
  padding: 5px 12px;
  font-family: inherit;
  font-size: 13px;
  color: var(--c-text);
  background: var(--c-surface);
  border: 1px solid var(--c-border-soft);
  border-radius: var(--r-full);
  cursor: pointer;
}

.lang-opt--active {
  background: var(--c-primary-soft);
  border-color: var(--c-primary);
  color: var(--c-primary);
}

/* 卡片 */
.cards {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--sp-4);
  flex: 1;
  min-height: 300px;
}

.plain-area {
  width: 100%;
  height: 100%;
  min-height: 240px;
  padding: 0;
  font-family: var(--font-ui);
  font-size: 16px;
  line-height: 1.75;
  color: var(--c-text);
  background: none;
  border: none;
  outline: none;
  resize: none;
}

.plain-area::placeholder {
  color: var(--c-text-tertiary);
}

.placeholder {
  margin: 0;
  color: var(--c-text-tertiary);
}

.result {
  margin: 0;
  white-space: pre-wrap;
  word-break: break-word;
}

.result--error {
  color: var(--c-danger);
  font-size: 14px;
  line-height: 1.6;
}

.count {
  font-size: 12px;
  color: var(--c-text-tertiary);
}

.grow {
  flex: 1;
}

.text-btn {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 5px 10px;
  font-family: inherit;
  font-size: 13px;
  color: var(--c-text-secondary);
  background: none;
  border: none;
  border-radius: var(--r-sm);
  cursor: pointer;
}

.text-btn:hover {
  background: var(--c-surface-hover);
}

.round-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  color: var(--c-text-secondary);
  background: none;
  border: 1px solid var(--c-border);
  border-radius: 50%;
  cursor: pointer;
  transition: background 0.15s;
}

.round-btn:hover:not(:disabled) {
  background: var(--c-surface-hover);
}

.round-btn--primary {
  color: var(--c-primary);
  border-color: var(--c-primary);
}

.round-btn--primary:hover:not(:disabled) {
  background: var(--c-primary-soft);
}

.round-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.spin {
  width: 14px;
  height: 14px;
  border: 2px solid rgba(26, 115, 232, 0.3);
  border-top-color: var(--c-primary);
  border-radius: 50%;
  animation: spin 0.7s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.hits {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
  font-size: 12px;
  color: var(--c-text-secondary);
}

/* 高级面板 */
.advanced {
  padding: var(--sp-4);
  background: var(--c-surface-alt);
  border: 1px solid var(--c-border-soft);
  border-radius: var(--r-lg);
}

.adv-grid {
  display: flex;
  gap: var(--sp-3);
  flex-wrap: wrap;
}

.adv-grid .field {
  min-width: 200px;
  margin-bottom: var(--sp-3);
}

.field--grow {
  flex: 1;
}

.mono {
  font-family: var(--font-mono);
  font-size: 13px;
  line-height: 1.7;
}

.checks {
  display: flex;
  align-items: center;
  gap: var(--sp-4);
  margin-top: 6px;
  flex-wrap: wrap;
}

.check {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--c-text-secondary);
  cursor: pointer;
}
</style>