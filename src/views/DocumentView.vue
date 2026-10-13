<script setup lang="ts">
import { ref } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { api, err, toast } from '../api'
import type { DocProgress, LoadedDocument } from '../types'

const doc = ref<LoadedDocument | null>(null)
const targetLang = ref('英语')
const concurrency = ref(3)
const running = ref(false)
const progress = ref<DocProgress | null>(null)
const preview = ref('')

async function pick() {
  try {
    const path = await open({
      title: '选择文档',
      multiple: false,
      filters: [{ name: '文本文档 / Markdown', extensions: ['txt', 'md', 'markdown'] }]
    })
    if (!path) return
    doc.value = await api.docLoad(path)
    preview.value = doc.value.raw
    progress.value = null
    const code = doc.value.encoding
    toast(`已载入 ${doc.value.segments.length} 个段落 · 编码 ${code}`, 'success')
  } catch (e) {
    err(e)
  }
}

async function run() {
  if (!doc.value) return
  running.value = true
  progress.value = null
  try {
    const r = await api.docTranslate(doc.value.path, targetLang.value, [], concurrency.value)
    progress.value = r
    if (r.failed === 0) toast(`翻译完成 → ${r.out_path}`, 'success')
    else toast(`完成 ${r.done}/${r.total}，失败 ${r.failed} 段`, 'error', 6000)
  } catch (e) {
    err(e)
  } finally {
    running.value = false
  }
}

const KIND_LABEL: Record<string, string> = {
  prose: '正文',
  heading: '标题',
  list: '列表',
  table: '表格',
  quote: '引用',
  code: '代码(不译)',
  frontmatter: '前言(不译)'
}

function kindCount(d: LoadedDocument): Record<string, number> {
  const out: Record<string, number> = {}
  for (const s of d.segments) out[s.kind] = (out[s.kind] ?? 0) + 1
  return out
}
</script>

<template>
  <div class="doc">
    <div class="doc__bar">
      <button class="btn btn--primary btn--sm" @click="pick">选择文档…</button>
      <span v-if="doc" class="doc__info">
        {{ doc.path.split(/[\\/]/).pop() }} · {{ doc.kind === 'markdown' ? 'Markdown' : '纯文本' }} ·
        {{ doc.lines }} 行 · {{ doc.encoding }}
      </span>
      <span class="spacer" />
      <label class="inline">目标语言
        <input v-model="targetLang" class="input input--sm" style="width: 90px" />
      </label>
      <label class="inline">并发
        <input v-model.number="concurrency" class="input input--sm" type="number" min="1" max="8" style="width: 60px" />
      </label>
      <button class="btn btn--sm" :disabled="!doc || running" @click="run">
        {{ running ? '翻译中…' : '开始翻译' }}
      </button>
    </div>

    <div v-if="progress" class="doc__result" :class="{ 'doc__result--err': progress.failed > 0 }">
      <strong>{{ progress.done }} / {{ progress.total }}</strong> 段完成
      <span v-if="progress.failed"> · {{ progress.failed }} 段失败</span>
      <div class="doc__out">输出：<code>{{ progress.out_path }}</code></div>
      <details v-if="progress.errors.length" class="doc__errors">
        <summary>查看失败详情</summary>
        <p v-for="[i, msg] in progress.errors" :key="i">段落 {{ i }}：{{ msg }}</p>
      </details>
    </div>

    <div v-if="!doc" class="empty">
      <span class="empty__icon">📄</span>
      <span>选择一个 .txt 或 .md 文件开始</span>
      <span class="empty__hint">自动识别 GBK / GB18030 / Big5 等编码，Markdown 会保留标题、列表和代码块</span>
    </div>

    <template v-else>
      <div class="doc__segments">
        <h4 class="doc__h4">段落预览（{{ doc.segments.length }} 段，代码块不会翻译）</h4>
        <div class="doc__chiprow">
          <span v-for="(k, v) in kindCount(doc)" :key="k" class="tag">{{ KIND_LABEL[k] ?? k }} × {{ v }}</span>
        </div>
      </div>
      <pre class="doc__preview">{{ preview }}</pre>
    </template>
  </div>
</template>

<style scoped>
.doc {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: var(--sp-4) var(--sp-5);
  overflow-y: auto;
}

.doc__bar {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  flex-wrap: wrap;
  margin-bottom: var(--sp-3);
}

.doc__info {
  font-size: 12px;
  color: var(--c-text-secondary);
}

.spacer {
  flex: 1;
}

.inline {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  color: var(--c-text-secondary);
}

.input--sm {
  padding: 5px 8px;
  font-size: 13px;
}

.doc__result {
  padding: var(--sp-3);
  margin-bottom: var(--sp-3);
  font-size: 13px;
  background: #e6f4ea;
  border: 1px solid #a8dab5;
  border-radius: var(--r-md);
}

.doc__result--err {
  background: #fce8e6;
  border-color: #f5c6c2;
}

.doc__out code {
  font-family: var(--font-mono);
  font-size: 12px;
}

.doc__errors {
  margin-top: var(--sp-2);
  font-size: 12px;
}

.doc__segments {
  margin-bottom: var(--sp-2);
}

.doc__h4 {
  margin: 0 0 6px;
  font-size: 13px;
  color: var(--c-text-secondary);
}

.doc__chiprow {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}

.doc__preview {
  flex: 1;
  min-height: 200px;
  margin: 0;
  padding: var(--sp-3);
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.7;
  white-space: pre-wrap;
  word-break: break-word;
  background: var(--c-surface-alt);
  border: 1px solid var(--c-border-soft);
  border-radius: var(--r-md);
  overflow: auto;
}

.empty__hint {
  max-width: 420px;
  font-size: 12px;
  color: var(--c-text-tertiary);
}
</style>