<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { api, err, toast } from '../api'
import { activeBackend, glossary, langName, meta, pickBackend, settings } from '../composables/useAppState'
import type { Mode, TermPair } from '../types'

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

const modeOptions = computed(() => meta.value.modes.map(([k, label]) => ({ value: k as Mode, label })))
const styleOptions = computed(() => meta.value.styles)
const formatOptions = computed(() => meta.value.format_types)
const langOptions = computed(() => meta.value.languages.map(([code, name]) => ({ value: name, code })))

onMounted(() => {
  loadGlossaryTerms()
  // 复用历史记录
  window.addEventListener('hymt2:reuse', (e) => {
    const item = (e as CustomEvent).detail
    source.value = item.source
    result.value = item.translation
    targetLang.value = item.target_lang
    mode.value = item.mode as Mode
  })
  const pending = localStorage.getItem('hymt2:reuse')
  if (pending) {
    localStorage.removeItem('hymt2:reuse')
    window.dispatchEvent(new CustomEvent('hymt2:reuse', { detail: JSON.parse(pending) }))
  }
})

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

function swap() {
  const tmp = source.value
  source.value = result.value
  result.value = tmp
  const t = targetLang.value
  targetLang.value = langName('zh') === t ? '英语' : '中文'
}

async function run() {
  if (!source.value.trim()) {
    toast('请输入要翻译的内容')
    return
  }
  busy.value = true
  result.value = ''
  usedTerms.value = []
  try {
    const req = {
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
    const r = await api.translate(req)
    result.value = r.text
    usedTerms.value = r.used_terms
    elapsed.value = r.ms
  } catch (e) {
    err(e)
  } finally {
    busy.value = false
  }
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
    toast('提示词已生成，见控制台')
    console.log(p)
  } catch (e) {
    err(e)
  }
}
</script>

<template>
  <div class="translate">
    <!-- 顶部工具条 -->
    <div class="toolbar">
      <select
        class="select toolbar__backend"
        :value="settings.active_id"
        @change="pickBackend(($event.target as HTMLSelectElement).value)"
      >
        <option v-for="b in settings.backends" :key="b.id" :value="b.id">
          {{ b.name }}{{ b.has_api_key ? ' 🔑' : '' }}
        </option>
      </select>

      <select v-model="mode" class="select toolbar__mode">
        <option v-for="m in modeOptions" :key="m.value" :value="m.value">{{ m.label }}</option>
      </select>

      <span class="toolbar__spacer" />

      <button v-if="activeBackend" class="btn btn--ghost btn--sm" @click="showPrompt">
        查看提示词
      </button>
    </div>

    <!-- 模式专属参数 -->
    <div v-if="mode !== 'basic'" class="mode-params">
      <div v-if="mode === 'style'" class="mode-params__item">
        <label class="label">目标风格</label>
        <select v-model="style" class="select">
          <option v-for="s in styleOptions" :key="s" :value="s">{{ s }}</option>
        </select>
      </div>
      <div v-if="mode === 'context'" class="mode-params__item mode-params__item--grow">
        <label class="label">背景信息</label>
        <input v-model="context" class="input" placeholder="例如：医学检测报告场景 / 游戏本地化" />
      </div>
      <div v-if="mode === 'structured'" class="mode-params__item">
        <label class="label">数据格式</label>
        <select v-model="formatType" class="select">
          <option v-for="f in formatOptions" :key="f" :value="f">{{ f }}</option>
        </select>
      </div>
      <div v-if="mode === 'terminology'" class="mode-params__terms">
        <label class="label">
          术语（每行一条：源词 => 译词）· 词库「{{ glossary.active }}」
        </label>
        <textarea v-model="termsEditor" class="input" rows="3" spellcheck="false" />
        <div class="mode-params__checks">
          <label class="check">
            <input v-model="autoFilterTerms" type="checkbox" />
            仅使用原文命中的词
          </label>
          <label class="check">
            <input v-model="ignoreGlossary" type="checkbox" />
            未命中时退回基础翻译
          </label>
        </div>
      </div>
    </div>

    <!-- 双栏 -->
    <div class="panes">
      <section class="pane">
        <div class="pane__head">
          <span class="pane__title">原文</span>
          <span class="pane__count">{{ source.length }} 字</span>
        </div>
        <textarea
          v-model="source"
          class="pane__input"
          placeholder="输入或粘贴要翻译的内容…"
          spellcheck="false"
          @keydown.ctrl.enter="run"
        />
      </section>

      <div class="pane__mid">
        <button class="swap" title="交换 (Ctrl+S)" @click="swap">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M16 3l4 4-4 4" />
            <path d="M20 7H4" />
            <path d="M8 21l-4-4 4-4" />
            <path d="M4 17h16" />
          </svg>
        </button>
        <button class="btn btn--primary btn--icon" :disabled="busy || !source.trim()" title="翻译 (Ctrl+Enter)" @click="run">
          <span v-if="busy" class="spin" />
          <svg v-else width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M5 12h14" />
            <path d="M12 5l7 7-7 7" />
          </svg>
        </button>
      </div>

      <section class="pane">
        <div class="pane__head">
          <select v-model="targetLang" class="lang-select">
            <option v-for="l in langOptions" :key="l.code" :value="l.value">{{ l.value }}</option>
          </select>
          <span v-if="result" class="tag">{{ (elapsed / 1000).toFixed(1) }}s</span>
          <button v-if="result" class="btn btn--ghost btn--icon" title="复制译文" @click="copy(result)">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <rect x="9" y="9" width="13" height="13" rx="2" />
              <path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1" />
            </svg>
          </button>
        </div>
        <div class="pane__output">
          <p v-if="busy" class="pane__placeholder">翻译中…</p>
          <p v-else-if="result" class="result">{{ result }}</p>
          <p v-else class="pane__placeholder">译文将显示在这里</p>
        </div>
        <div v-if="usedTerms.length" class="terms-hit">
          命中术语：<span v-for="t in usedTerms" :key="t" class="tag tag--primary">{{ t }}</span>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.translate {
  display: flex;
  flex-direction: column;
  height: 100%;
  gap: var(--sp-2);
}

/* 工具条 */
.toolbar {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  flex-shrink: 0;
}

.toolbar__backend {
  width: 200px;
}

.toolbar__mode {
  width: 150px;
}

.toolbar__spacer {
  flex: 1;
}

/* 模式参数 */
.mode-params {
  display: flex;
  gap: var(--sp-3);
  align-items: flex-end;
  flex-wrap: wrap;
  padding: var(--sp-3);
  background: var(--c-surface-alt);
  border-radius: var(--r-md);
  flex-shrink: 0;
}

.mode-params__item {
  min-width: 180px;
}

.mode-params__item--grow {
  flex: 1;
  min-width: 260px;
}

.mode-params__terms {
  flex: 1;
  min-width: 320px;
}

.mode-params__checks {
  display: flex;
  gap: var(--sp-4);
  margin-top: 6px;
}

.check {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--c-text-secondary);
  cursor: pointer;
}

/* 双栏 */
.panes {
  display: grid;
  grid-template-columns: 1fr auto 1fr;
  flex: 1;
  min-height: 0;
  border: 1px solid var(--c-border-soft);
  border-radius: var(--r-lg);
  overflow: hidden;
}

.pane {
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
}

.pane__head {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  padding: var(--sp-2) var(--sp-3);
  border-bottom: 1px solid var(--c-border-soft);
  min-height: 44px;
}

.pane__title {
  font-size: 13px;
  font-weight: 600;
  color: var(--c-text-secondary);
}

.pane__count {
  margin-left: auto;
  font-size: 12px;
  color: var(--c-text-tertiary);
}

.lang-select {
  padding: 4px 26px 4px 8px;
  font-size: 14px;
  font-weight: 600;
  border: 1px solid transparent;
  background-color: transparent;
  border-radius: var(--r-sm);
}

.lang-select:hover {
  background-color: var(--c-surface-hover);
}

.pane__input,
.pane__output {
  flex: 1;
  min-height: 0;
  padding: var(--sp-3);
  font-family: var(--font-ui);
  font-size: 15px;
  line-height: 1.7;
  border: none;
  outline: none;
  resize: none;
  background: transparent;
  color: var(--c-text);
  overflow-y: auto;
}

.pane__input::placeholder,
.pane__placeholder {
  color: var(--c-text-tertiary);
}

.pane__placeholder {
  margin: 0;
}

.result {
  margin: 0;
  white-space: pre-wrap;
  word-break: break-word;
}

.terms-hit {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
  padding: 6px var(--sp-3);
  font-size: 12px;
  color: var(--c-text-secondary);
  border-top: 1px solid var(--c-border-soft);
}

/* 中间控制 */
.pane__mid {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--sp-3);
  padding: 0 var(--sp-2);
  border-left: 1px solid var(--c-border-soft);
  border-right: 1px solid var(--c-border-soft);
  background: var(--c-surface-alt);
}

.swap {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  color: var(--c-text-secondary);
  background: var(--c-surface);
  border: 1px solid var(--c-border);
  border-radius: var(--r-full);
  cursor: pointer;
  transition:
    transform 0.2s,
    color 0.15s;
}

.swap:hover {
  color: var(--c-primary);
  transform: rotate(180deg);
}

.spin {
  width: 14px;
  height: 14px;
  border: 2px solid rgba(255, 255, 255, 0.35);
  border-top-color: #fff;
  border-radius: 50%;
  animation: spin 0.7s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>