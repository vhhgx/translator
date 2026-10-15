<script setup lang="ts">
/**
 * 网页翻译：贴一个 URL，抓正文、翻成目标语言。
 *
 * 分两步是刻意的——先「抓取」再「翻译」。抽取算法不可能对所有站点都完美，
 * 让用户先确认抽到的是正文（而不是导航栏）再花时间翻，比直接翻错了再重来强。
 *
 * 译文按 `tag` 还原成标题/段落/引用/代码块的样式，读起来还是文章的样子。
 */
import { computed, ref } from 'vue'
import { AlertTriangle, ArrowRightLeft, Globe, LoaderCircle, Sparkles } from '@lucide/vue'
import LangSelect from '../components/LangSelect.vue'
import { api, err, toast } from '../api'
import { meta } from '../composables/useAppState'
import { useJobProgress } from '../composables/useJobProgress'
import type { WebPage } from '../types'

const LANG_KEY = 'hymt2:web-target'

const url = ref('')
const urlInput = ref<HTMLInputElement | null>(null)
const targetLang = ref(localStorage.getItem(LANG_KEY) ?? '中文')
const concurrency = ref(3)

const fetching = ref(false)
const translating = ref(false)
const page = ref<WebPage | null>(null)
/** 与 page.blocks 一一对应的译文；null 表示还没翻。 */
const translated = ref<string[] | null>(null)
/** 只看译文 / 对照看。 */
const view = ref<'both' | 'translated' | 'original'>('both')

const langs = computed<[string, string][]>(() => meta.value.languages)
const { done, total, finished, begin, percent } = useJobProgress()
const currentJob = ref('')

function changeTarget(name: string) {
  targetLang.value = name
  localStorage.setItem(LANG_KEY, name)
}

async function fetchPage() {
  if (!url.value.trim()) return
  fetching.value = true
  page.value = null
  translated.value = null
  try {
    page.value = await api.webFetch(url.value.trim())
    toast(`抽取到 ${page.value.blocks.length} 个正文块`, 'success')
  } catch (e) {
    err(e)
  } finally {
    fetching.value = false
  }
}

async function translate() {
  if (!page.value) return
  translating.value = true
  try {
    const jid = await begin(page.value.blocks.filter((b) => b.translatable).length)
    currentJob.value = jid
    translated.value = await api.webTranslate(
      targetLang.value,
      page.value.blocks,
      concurrency.value,
      jid
    )
    view.value = 'both'
    toast('翻译完成', 'success')
  } catch (e) {
    err(e)
  } finally {
    translating.value = false
  }
}

async function cancel() {
  if (!currentJob.value) return
  try {
    await api.cancelJob(currentJob.value)
    toast('正在停止…', 'info')
  } catch (e) {
    err(e)
  }
}

function reset() {
  url.value = ''
  page.value = null
  translated.value = null
}

/** 每个块的译文（可能还没翻）。 */
function tr(i: number): string | null {
  return translated.value?.[i] ?? null
}

const BLOCK_CLASS: Record<string, string> = {
  h1: 'blk--h1',
  h2: 'blk--h2',
  h3: 'blk--h3',
  h4: 'blk--h4',
  h5: 'blk--h5',
  h6: 'blk--h6',
  li: 'blk--li',
  blockquote: 'blk--quote',
  pre: 'blk--pre'
}
</script>

<template>
  <div class="web">
    <div class="page-toolbar">
      <div class="toolbar-left">
        <Globe :size="15" class="web__globe" />
        <input
          ref="urlInput"
          v-model="url"
          class="input url-input"
          placeholder="粘贴网页地址，例如 https://example.com/article"
          spellcheck="false"
          @keyup.enter="fetchPage"
        />
        <button
          class="btn btn--primary btn--sm"
          :disabled="fetching || !url.trim()"
          @click="fetchPage"
        >
          <LoaderCircle v-if="fetching" :size="13" class="spin" />
          <Sparkles v-else :size="13" />
          {{ fetching ? '抓取中…' : '抓取正文' }}
        </button>
      </div>

      <div class="toolbar-right">
        <label class="inline">
          目标语言
          <LangSelect
            :model-value="targetLang"
            :options="langs"
            show-label
            @update:model-value="changeTarget"
          />
        </label>
        <label class="inline">
          并发
          <input
            v-model.number="concurrency"
            class="input input--num"
            type="number"
            min="1"
            max="8"
            :disabled="translating"
          />
        </label>
        <button
          v-if="!translating"
          class="btn btn--primary btn--sm"
          :disabled="!page"
          @click="translate"
        >
          开始翻译
        </button>
        <template v-else>
          <button class="btn btn--danger btn--sm" @click="cancel">停止</button>
        </template>
      </div>
    </div>

    <!-- 进度 -->
    <div v-if="translating || finished" class="progress-wrap">
      <div class="progress-bar">
        <div class="progress-fill" :style="{ width: `${percent()}%` }" />
      </div>
      <span class="progress-text">{{ done }} / {{ total || '…' }} 块</span>
    </div>

    <!-- 抓取结果头 -->
    <div v-if="page" class="page-head">
      <div class="page-head__text">
        <h3 class="page-head__title">{{ page.title }}</h3>
        <span class="muted"
          >{{ page.url }} · {{ page.blocks.length }} 块 ·
          {{ (page.bytes / 1024).toFixed(1) }} KB</span
        >
      </div>
      <div class="page-head__actions">
        <div v-if="translated" class="seg">
          <button
            class="seg__btn"
            :class="{ 'seg__btn--on': view === 'both' }"
            @click="view = 'both'"
          >
            对照
          </button>
          <button
            class="seg__btn"
            :class="{ 'seg__btn--on': view === 'translated' }"
            @click="view = 'translated'"
          >
            仅译文
          </button>
          <button
            class="seg__btn"
            :class="{ 'seg__btn--on': view === 'original' }"
            @click="view = 'original'"
          >
            仅原文
          </button>
        </div>
        <button class="btn btn--ghost btn--sm" @click="reset">换一个</button>
      </div>
    </div>

    <!-- 正文 -->
    <div v-if="page" class="article">
      <div
        v-for="(b, i) in page.blocks"
        :key="i"
        class="blk"
        :class="[BLOCK_CLASS[b.tag] ?? 'blk--p', { 'blk--code': !b.translatable }]"
      >
        <div v-if="view !== 'translated' || !tr(i)" class="blk__src">
          <pre class="blk__text">{{ b.text }}</pre>
          <span v-if="!b.translatable" class="blk__badge">代码，不翻译</span>
        </div>
        <template v-if="translated && b.translatable">
          <div v-if="view === 'both'" class="blk__divider"><ArrowRightLeft :size="11" /></div>
          <div v-if="view !== 'original'" class="blk__dst">
            <pre class="blk__text">{{ tr(i) ?? b.text }}</pre>
          </div>
        </template>
      </div>
    </div>

    <!-- 空状态 -->
    <div v-else-if="!fetching" class="dropzone" @click="urlInput?.focus()">
      <Globe :size="34" class="dropzone__icon" />
      <span class="dropzone__title">粘贴网页地址，抽取正文后翻译</span>
      <span class="dropzone__hint">
        只支持服务端直出的 HTML。<b>单页应用</b>（React / Vue 站点）正文靠 JS 渲染，抓不到内容；
        需要登录的页面也一样。文章、文档、博客类页面效果最好。
      </span>
      <span class="dropzone__hint dropzone__hint--warn">
        <AlertTriangle :size="11" />
        正文抽取不可能对所有站点都完美。建议先「抓取正文」确认抽对了，再翻译
      </span>
    </div>
  </div>
</template>

<style scoped>
.web {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  background: var(--surface-secondary);
  overflow: hidden;
}

.web__globe {
  flex-shrink: 0;
  color: var(--text-muted);
}

.page-toolbar {
  padding: 12px 24px;
  background: var(--surface-primary);
  border-bottom: 1px solid var(--border-subtle);
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  flex-wrap: wrap;
  flex-shrink: 0;
}

.toolbar-left {
  display: flex;
  align-items: center;
  gap: 10px;
  flex: 1;
  min-width: 280px;
}

.toolbar-right {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}

.url-input {
  flex: 1;
  min-width: 200px;
  font-family: var(--font-mono);
  font-size: 12px;
}

.inline {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12.5px;
  color: var(--text-secondary);
  white-space: nowrap;
}

.input--num {
  width: 62px;
  padding: 5px 8px;
  font-size: 12.5px;
  text-align: center;
}

.spin {
  animation: web-spin 0.9s linear infinite;
}

@keyframes web-spin {
  to {
    transform: rotate(360deg);
  }
}

/* 进度 */
.progress-wrap {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 24px;
  background: var(--surface-primary);
  border-bottom: 1px solid var(--border-subtle);
  flex-shrink: 0;
}

.progress-bar {
  flex: 1;
  height: 5px;
  border-radius: var(--r-pill);
  background: var(--surface-tertiary);
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  background: var(--accent-primary);
  border-radius: var(--r-pill);
  transition: width 0.25s ease;
}

.progress-text {
  font-size: 12px;
  color: var(--text-secondary);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

/* 页头 */
.page-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  padding: 14px 24px;
  background: var(--surface-primary);
  border-bottom: 1px solid var(--border-subtle);
  flex-shrink: 0;
}

.page-head__text {
  min-width: 0;
}

.page-head__title {
  margin: 0 0 3px;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.page-head__actions {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-shrink: 0;
}

.seg {
  display: flex;
  padding: 2px;
  background: var(--surface-tertiary);
  border-radius: var(--r-pill);
}

.seg__btn {
  padding: 4px 11px;
  font-size: 12px;
  color: var(--text-secondary);
  background: transparent;
  border: none;
  border-radius: var(--r-pill);
  cursor: pointer;
  transition: all 0.15s;
}

.seg__btn--on {
  color: var(--text-primary);
  background: var(--surface-primary);
  box-shadow: var(--shadow-sm);
}

/* 正文 */
.article {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 20px 24px 40px;
}

.blk {
  margin-bottom: 10px;
  padding: 10px 14px;
  background: var(--surface-primary);
  border: 1px solid var(--border-subtle);
  border-radius: var(--r-md);
}

.blk__text {
  margin: 0;
  font-family: inherit;
  font-size: 13.5px;
  line-height: 1.75;
  color: var(--text-primary);
  white-space: pre-wrap;
  word-break: break-word;
}

.blk__badge {
  display: inline-block;
  margin-top: 6px;
  padding: 1px 7px;
  font-size: 10.5px;
  color: var(--text-muted);
  background: var(--surface-tertiary);
  border-radius: var(--r-pill);
}

.blk__divider {
  display: flex;
  justify-content: center;
  margin: 8px 0;
  color: var(--text-muted);
}

.blk__dst .blk__text {
  color: var(--accent-primary);
}

/* 按标签排版 */
.blk--h1,
.blk--h2 {
  border-left: 3px solid var(--accent-primary);
}

.blk--h1 .blk__text {
  font-size: 19px;
  font-weight: 700;
}

.blk--h2 .blk__text {
  font-size: 16.5px;
  font-weight: 650;
}

.blk--h3 .blk__text,
.blk--h4 .blk__text,
.blk--h5 .blk__text,
.blk--h6 .blk__text {
  font-size: 14.5px;
  font-weight: 600;
}

.blk--li {
  margin-left: 18px;
  border-left: 2px solid var(--border-subtle);
}

.blk--li .blk__text::before {
  content: '• ';
  color: var(--text-muted);
}

.blk--quote {
  border-left: 3px solid var(--border-focus);
  background: var(--surface-tertiary);
}

.blk--quote .blk__text {
  font-style: italic;
  color: var(--text-secondary);
}

.blk--code,
.blk--pre {
  background: var(--surface-tertiary);
}

.blk--code .blk__text,
.blk--pre .blk__text {
  font-family: var(--font-mono);
  font-size: 12px;
  overflow-x: auto;
}

/* 空状态 */
.dropzone {
  flex: 1;
  margin: 24px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 32px;
  text-align: center;
  color: var(--text-muted);
  background: var(--surface-primary);
  border: 2px dashed var(--border-subtle);
  border-radius: var(--r-lg);
  cursor: text;
}

.dropzone__icon {
  opacity: 0.45;
}

.dropzone__title {
  font-size: 13.5px;
  font-weight: 500;
  color: var(--text-secondary);
}

.dropzone__hint {
  max-width: 480px;
  font-size: 12px;
  line-height: 1.65;
}

.dropzone__hint--warn {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}

.muted {
  font-size: 12px;
  color: var(--text-muted);
}
</style>
