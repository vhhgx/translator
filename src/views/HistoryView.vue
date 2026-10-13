<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { api, err, toast } from '../api'
import type { HistoryItem } from '../types'

const emit = defineEmits<{ (e: 'reuse', item: HistoryItem): void }>()

const items = ref<HistoryItem[]>([])
const search = ref('')
const loading = ref(false)

onMounted(load)

async function load() {
  loading.value = true
  try {
    items.value = await api.historyList(search.value, 200)
  } catch (e) {
    err(e)
  } finally {
    loading.value = false
  }
}

async function remove(id: string) {
  try {
    await api.historyDelete(id)
    items.value = items.value.filter((i) => i.id !== id)
  } catch (e) {
    err(e)
  }
}

async function clearAll() {
  if (!confirm('确定清空全部翻译历史？')) return
  try {
    await api.historyClear()
    items.value = []
    toast('已清空', 'success')
  } catch (e) {
    err(e)
  }
}

async function copy(text: string) {
  await navigator.clipboard.writeText(text)
  toast('已复制', 'success')
}

function fmt(ts: number) {
  const d = new Date(ts)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}

const MODE_LABEL: Record<string, string> = {
  basic: '基础',
  terminology: '术语',
  style: '风格',
  context: '语境',
  delimiter: '分隔符',
  structured: '结构化'
}
</script>

<template>
  <div class="history">
    <div class="history__bar">
      <input v-model="search" class="input history__search" placeholder="搜索原文或译文…" @keyup.enter="load" />
      <button class="btn btn--sm" @click="load">搜索</button>
      <button class="btn btn--sm btn--danger" @click="clearAll">清空</button>
    </div>

    <div v-if="!items.length && !loading" class="empty">
      <span class="empty__icon">🕘</span>
      <span>{{ search ? '没有匹配的记录' : '还没有翻译历史' }}</span>
    </div>

    <div v-for="item in items" :key="item.id" class="card item">
      <div class="item__meta">
        <span class="tag tag--primary">{{ item.target_lang }}</span>
        <span class="tag">{{ MODE_LABEL[item.mode] ?? item.mode }}</span>
        <span class="item__time">{{ fmt(item.created_at) }}</span>
        <span class="item__spacer" />
        <button class="btn btn--ghost btn--sm" @click="copy(item.translation)">复制译文</button>
        <button class="btn btn--ghost btn--sm" @click="emit('reuse', item)">复用</button>
        <button class="btn btn--ghost btn--sm btn--danger" @click="remove(item.id)">删除</button>
      </div>
      <div class="item__src">{{ item.source }}</div>
      <div class="item__out">{{ item.translation }}</div>
    </div>
  </div>
</template>

<style scoped>
.history {
  height: 100%;
  padding: var(--sp-4) var(--sp-5);
  overflow-y: auto;
}

.history__bar {
  display: flex;
  gap: var(--sp-2);
  margin-bottom: var(--sp-3);
}

.history__search {
  flex: 1;
  max-width: 400px;
}

.item {
  margin-bottom: var(--sp-2);
  padding: var(--sp-3);
}

.item__meta {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: var(--sp-2);
}

.item__spacer {
  flex: 1;
}

.item__time {
  font-size: 12px;
  color: var(--c-text-tertiary);
}

.item__src {
  margin-bottom: 6px;
  padding-left: var(--sp-2);
  font-size: 13px;
  color: var(--c-text-secondary);
  border-left: 2px solid var(--c-border);
  white-space: pre-wrap;
}

.item__out {
  padding-left: var(--sp-2);
  font-size: 14px;
  border-left: 2px solid var(--c-primary);
  white-space: pre-wrap;
}
</style>