<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { listen } from '@tauri-apps/api/event'
import LangSelect from '../components/LangSelect.vue'
import { api, err, toast } from '../api'
import { glossary, meta, pickBackend, settings } from '../composables/useAppState'
import type { Mode, TermPair } from '../types'

// ---------- state ----------
const source = ref('')
const result = ref('')
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

// ---------- derived ----------
const modeOptions = computed(() => meta.value.modes.map(([k, label]) => ({ value: k as Mode, label })))
const styleOptions = computed(() => meta.value.styles)
const formatOptions = computed(() => meta.value.format_types)

const allLangs = computed<[string, string][]>(() => meta.value.languages)

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

// ---------- streaming ----------
const unlisten: Array<() => void> = []

// `crypto.randomUUID` is unavailable in non-secure contexts; fall back so a
// job id always exists rather than throwing before the request is sent.
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
        const out = document.querySelector('.pane__output')
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

onUnmounted(() => unlisten.forEach((f) => f()))

// ---------- misc ----------
// Hy-MT2 自动识别源语言，交换只需对调文本与目标语言
function swap() {
  const t = source.value
  source.value = result.value
  result.value = t
  targetLang.value = targetLang.value === '中文' ? '英语' : '中文'
}

async function copy(text: string) {
  await navigator.clipboard.writeText(text)
  toast('已复制', 'success')
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
  <div class="gt">
    <!-- 语言选择：Hy-MT2 自动识别源语言，因此只有目标语言可选 -->
    <div class="langrow">
      <div class="langrow__hint">源语言自动识别</div>
      <div class="langrow__target">
        <span class="langrow__label">译为</span>
        <LangSelect v-model="targetLang" :options="allLangs" />
      </div>
    </div>

    <!-- 双栏 -->
    <div class="panes">
      <section class="pane">
        <textarea
          v-model="source"
          class="pane__input"
          placeholder="输入或粘贴文本…"
          spellcheck="false"
          @keydown.ctrl.enter="run"
        />
        <div class="pane__bar">
          <span class="count">{{ source.length }} 字</span>
          <span class="grow" />
          <button class="tbtn" @click="showAdvanced = !showAdvanced">
            高级
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
              <path d="M6 9l6 6 6-6" />
            </svg>
          </button>
          <button class="go" :disabled="busy || !source.trim()" title="翻译 (Ctrl+Enter)" @click="run">
            <span v-if="busy" class="spin" />
            <svg v-else width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M21 12a9 9 0 11-3-6.7" />
              <path d="M21 3v6h-6" />
            </svg>
          </button>
        </div>
      </section>

      <!-- 悬浮交换按钮 -->
      <button class="swap" title="交换语言" @click="swap">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M16 3l4 4-4 4" />
          <path d="M20 7H4" />
          <path d="M8 21l-4-4 4-4" />
          <path d="M4 17h16" />
        </svg>
      </button>

      <section class="pane pane--out">
        <div class="pane__output">
          <p v-if="failed" class="result result--error">⚠️ {{ failed }}</p>
          <p v-else-if="result" class="result">
            {{ result }}<span v-if="busy" class="caret" />
          </p>
          <p v-else-if="busy" class="ph">翻译中…</p>
          <p v-else class="ph">译文</p>
        </div>
        <div class="pane__bar">
          <span v-if="result && !busy" class="count">{{ (elapsed / 1000).toFixed(1) }}s</span>
          <span class="grow" />
          <button v-if="result" class="tbtn tbtn--icon" title="复制" @click="copy(result)">
            <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <rect x="9" y="9" width="13" height="13" rx="2" />
              <path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1" />
            </svg>
          </button>
          <button v-if="result" class="tbtn tbtn--icon" title="反向翻译" @click="swap">
            <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M7 17L3 13l4-4" />
              <path d="M3 13h13a5 5 0 015 5v1" />
            </svg>
          </button>
        </div>
      </section>
    </div>

    <!-- 命中术语 -->
    <div v-if="usedTerms.length" class="hits">
      命中术语：<span v-for="t in usedTerms" :key="t" class="pill pill--sm pill--on">{{ t }}</span>
    </div>

    <!-- 高级面板 -->
    <div v-if="showAdvanced" class="adv">
      <div class="adv__row">
        <label class="adv__field">
          <span class="adv__label">翻译模式</span>
          <select v-model="mode" class="select">
            <option v-for="m in modeOptions" :key="m.value" :value="m.value">{{ m.label }}</option>
          </select>
        </label>
        <label v-if="mode === 'style'" class="adv__field">
          <span class="adv__label">目标风格</span>
          <select v-model="style" class="select">
            <option v-for="s in styleOptions" :key="s" :value="s">{{ s }}</option>
          </select>
        </label>
        <label v-if="mode === 'structured'" class="adv__field">
          <span class="adv__label">数据格式</span>
          <select v-model="formatType" class="select">
            <option v-for="f in formatOptions" :key="f" :value="f">{{ f }}</option>
          </select>
        </label>
        <label v-if="mode === 'context'" class="adv__field adv__field--grow">
          <span class="adv__label">背景信息</span>
          <input v-model="context" class="input" placeholder="例如：医学检测报告场景" />
        </label>
      </div>

      <div v-if="mode === 'terminology'" class="adv__row adv__row--col">
        <span class="adv__label">术语（每行一条：源词 =&gt; 译词）· 词库「{{ glossary.active }}」</span>
        <textarea v-model="termsEditor" class="input mono" rows="3" spellcheck="false" />
        <div class="adv__checks">
          <label class="check"><input v-model="autoFilterTerms" type="checkbox" /> 仅使用原文命中的词</label>
          <label class="check"><input v-model="ignoreGlossary" type="checkbox" /> 未命中时退回基础翻译</label>
        </div>
      </div>
    </div>

    <!-- 底部状态条 -->
    <div class="statusbar">
      <select
        class="chip"
        :value="settings.active_id"
        @change="pickBackend(($event.target as HTMLSelectElement).value)"
      >
        <option v-for="b in settings.backends" :key="b.id" :value="b.id">
          {{ b.name }}{{ b.has_api_key ? ' 🔑' : '' }}
        </option>
      </select>
      <span class="pill pill--sm">{{ modeOptions.find((m) => m.value === mode)?.label }}</span>
      <span class="grow" />
      <button class="tbtn" @click="showPrompt">查看提示词</button>
    </div>
  </div>
</template>

<style scoped>
.gt {
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
  height: 100%;
  padding: 0 var(--sp-5) var(--sp-4);
  overflow-y: auto;
}

/* 语言选择行：左侧提示自动识别，右侧为目标语言下拉 */
.langrow {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-4);
  padding-bottom: var(--sp-2);
}

.langrow__hint {
  font-size: 13px;
  color: var(--c-text-tertiary);
}

.langrow__target {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
}

.langrow__label {
  font-size: 13px;
  color: var(--c-text-secondary);
}

/* 药丸 */
.pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 8px 14px;
  font-family: inherit;
  font-size: 14px;
  color: var(--c-text-secondary);
  background: none;
  border: none;
  border-radius: var(--r-pill);
  cursor: pointer;
  white-space: nowrap;
  transition: background 0.15s;
}

.pill:hover {
  background: var(--c-surface-hover);
}

.pill--on {
  color: var(--c-pill-text);
  background: var(--c-pill-bg);
}

.pill--on:hover {
  background: var(--c-pill-bg);
}

/* 双栏 */
.panes {
  position: relative;
  display: grid;
  grid-template-columns: 1fr 1fr;
  flex: 1;
  min-height: 300px;
}

.pane {
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  background: var(--c-surface-alt);
}

.pane:first-child {
  border-top-left-radius: var(--r-xl);
  border-bottom-left-radius: var(--r-xl);
}

.pane--out {
  border-top-right-radius: var(--r-xl);
  border-bottom-right-radius: var(--r-xl);
}

.pane__input,
.pane__output {
  flex: 1;
  min-height: 0;
  padding: var(--sp-5) var(--sp-5) var(--sp-2);
  font-family: var(--font-ui);
  font-size: 17px;
  line-height: 1.75;
  color: var(--c-text);
  background: transparent;
  border: none;
  outline: none;
  resize: none;
  overflow-y: auto;
}

.pane__input::placeholder,
.ph {
  color: var(--c-text-tertiary);
}

.ph {
  margin: 0;
}

.result {
  margin: 0;
  white-space: pre-wrap;
  word-break: break-word;
}

.result--error {
  color: var(--c-danger);
  font-size: 15px;
}

/* 底部工具条 */
.pane__bar {
  display: flex;
  align-items: center;
  gap: var(--sp-1);
  padding: var(--sp-2) var(--sp-4) var(--sp-3);
}

.count {
  font-size: 12px;
  color: var(--c-text-tertiary);
}

.grow {
  flex: 1;
}

.tbtn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 7px 12px;
  font-family: inherit;
  font-size: 13px;
  color: var(--c-text-secondary);
  background: none;
  border: none;
  border-radius: var(--r-pill);
  cursor: pointer;
}

.tbtn:hover {
  background: var(--c-surface-hover);
  color: var(--c-text);
}

.tbtn--icon {
  padding: 7px;
}

.go {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 38px;
  height: 38px;
  color: var(--c-text-secondary);
  background: none;
  border: 1px solid var(--c-border);
  border-radius: 50%;
  cursor: pointer;
  transition: background 0.15s;
}

.go:hover:not(:disabled) {
  background: var(--c-surface-hover);
}

.go:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.spin {
  width: 15px;
  height: 15px;
  border: 2px solid var(--c-border);
  border-top-color: var(--c-text-secondary);
  border-radius: 50%;
  animation: spin 0.7s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

/* 悬浮交换按钮 */
.swap {
  position: absolute;
  left: 50%;
  top: 50%;
  z-index: 10;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 52px;
  height: 52px;
  color: var(--c-pill-text);
  background: var(--c-pill-bg);
  border: none;
  border-radius: var(--r-xl);
  box-shadow: var(--shadow-float);
  cursor: pointer;
  transform: translate(-50%, -50%);
  transition:
    transform 0.18s,
    box-shadow 0.18s;
}

.swap:hover {
  transform: translate(-50%, -50%) scale(1.06);
  box-shadow: 0 4px 12px rgba(60, 64, 67, 0.24);
}

/* 流式光标 */
.caret {
  display: inline-block;
  width: 2px;
  height: 1.05em;
  margin-left: 2px;
  vertical-align: text-bottom;
  background: var(--c-primary);
  animation: blink 1s step-end infinite;
}

@keyframes blink {
  50% {
    opacity: 0;
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
.adv {
  padding: var(--sp-4);
  background: var(--c-surface-alt);
  border-radius: var(--r-lg);
  border-left: 3px solid var(--c-pill-bg);
}

.adv__row {
  display: flex;
  gap: var(--sp-3);
  flex-wrap: wrap;
}

.adv__row--col {
  flex-direction: column;
  margin-top: var(--sp-2);
}

.adv__field {
  min-width: 190px;
}

.adv__field--grow {
  flex: 1;
}

.adv__label {
  display: block;
  margin-bottom: 4px;
  font-size: 12px;
  color: var(--c-text-secondary);
}

.adv__checks {
  display: flex;
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

.mono {
  font-family: var(--font-mono);
  font-size: 13px;
  line-height: 1.7;
}

/* 底部状态条 */
.statusbar {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  margin-top: var(--sp-1);
  padding-top: var(--sp-3);
  border-top: 1px solid var(--c-divider);
}

.chip {
  padding: 5px 26px 5px 12px;
  font-family: inherit;
  font-size: 12px;
  color: var(--c-pill-text);
  background-color: var(--c-pill-bg);
  border: none;
  border-radius: var(--r-pill);
  appearance: none;
  cursor: pointer;
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='10' viewBox='0 0 24 24' fill='none' stroke='%23185abc' stroke-width='3'%3E%3Cpath d='M6 9l6 6 6-6'/%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right 10px center;
}
</style>