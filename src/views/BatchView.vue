<script setup lang="ts">
import { ref } from 'vue'
import { open, save } from '@tauri-apps/plugin-dialog'
import { api, err, toast } from '../api'
import type { BatchProgress } from '../types'

const inputPath = ref('')
const outputPath = ref('')
const targetLang = ref('英语')
const textField = ref('text')
const concurrency = ref(3)
const resume = ref(true)
const running = ref(false)
const result = ref<BatchProgress | null>(null)

async function pickInput() {
  try {
    const p = await open({
      title: '选择 JSONL 文件',
      multiple: false,
      filters: [{ name: 'JSONL', extensions: ['jsonl', 'ndjson', 'json'] }]
    })
    if (!p) return
    inputPath.value = p
    if (!outputPath.value) {
      outputPath.value = p.replace(/(\.[^.]+)?$/, '.translated.jsonl')
    }
  } catch (e) {
    err(e)
  }
}

async function pickOutput() {
  const p = await save({
    title: '选择输出位置',
    defaultPath: outputPath.value || 'output.jsonl',
    filters: [{ name: 'JSONL', extensions: ['jsonl'] }]
  })
  if (p) outputPath.value = p
}

async function run() {
  if (!inputPath.value || !outputPath.value) {
    toast('请先选择输入和输出路径')
    return
  }
  running.value = true
  result.value = null
  try {
    const r = await api.batchTranslate(
      inputPath.value,
      outputPath.value,
      targetLang.value,
      textField.value,
      concurrency.value,
      resume.value
    )
    result.value = r
    if (r.failed === 0) toast(`完成 ${r.done}/${r.total}`, 'success')
    else toast(`完成 ${r.done}/${r.total}，失败 ${r.failed}`, 'error', 6000)
  } catch (e) {
    err(e)
  } finally {
    running.value = false
  }
}
</script>

<template>
  <div class="batch">
    <div class="card">
      <h3 class="card__title">批量翻译 JSONL</h3>

      <div class="field">
        <label class="label">输入文件（每行一个 JSON，含 <code>{{ textField }}</code> 字段）</label>
        <div class="row">
          <input v-model="inputPath" class="input" placeholder="选择 .jsonl 文件…" />
          <button class="btn btn--sm" @click="pickInput">选择</button>
        </div>
      </div>

      <div class="field">
        <label class="label">输出文件（自动追加 translation 字段）</label>
        <div class="row">
          <input v-model="outputPath" class="input" placeholder="输出路径…" />
          <button class="btn btn--sm" @click="pickOutput">另存为</button>
        </div>
      </div>

      <div class="row row--tight">
        <label class="inline">目标语言
          <input v-model="targetLang" class="input input--sm" style="width: 90px" />
        </label>
        <label class="inline">文本字段
          <input v-model="textField" class="input input--sm" style="width: 100px" />
        </label>
        <label class="inline">并发
          <input v-model.number="concurrency" class="input input--sm" type="number" min="1" max="16" style="width: 60px" />
        </label>
        <label class="inline">
          <input v-model="resume" type="checkbox" /> 断点续跑
        </label>
      </div>

      <button class="btn btn--primary" :disabled="running || !inputPath" @click="run">
        {{ running ? '翻译中…' : '开始批量翻译' }}
      </button>
    </div>

    <div v-if="result" class="card result">
      <h3 class="card__title">结果</h3>
      <p>
        共 <strong>{{ result.total }}</strong> 条，成功 <strong>{{ result.done }}</strong> 条<template v-if="result.failed">，失败 <strong>{{ result.failed }}</strong> 条</template>
      </p>
      <p class="muted">输出：<code>{{ result.output }}</code></p>
    </div>

    <div class="card">
      <h3 class="card__title">说明</h3>
      <ul class="muted">
        <li>输入示例：<code>{'{"id":1,"text":"今天天气真好"}'}</code></li>
        <li>输出会在每行追加 <code>translation</code> 字段，其余字段原样保留。</li>
        <li>「断点续跑」会读取已存在的输出文件并跳过重复内容。</li>
        <li>本地 CPU 推理较慢，建议并发 2–4；云端 API 可调高。</li>
      </ul>
    </div>
  </div>
</template>

<style scoped>
.batch {
  height: 100%;
  padding: var(--sp-4) var(--sp-5);
  overflow-y: auto;
}

.card {
  margin-bottom: var(--sp-3);
}

.row {
  display: flex;
  gap: var(--sp-2);
}

.row--tight {
  flex-wrap: wrap;
  margin-bottom: var(--sp-3);
}

.inline {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  color: var(--c-text-secondary);
}

.input--sm {
  width: auto;
  padding: 5px 8px;
  font-size: 13px;
}

code {
  padding: 1px 5px;
  font-family: var(--font-mono);
  font-size: 12px;
  background: var(--c-surface-hover);
  border-radius: var(--r-sm);
}

.muted {
  font-size: 13px;
  color: var(--c-text-secondary);
  line-height: 1.8;
}

.muted ul {
  margin: 0;
  padding-left: 20px;
}
</style>