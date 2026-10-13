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
    items.value = await api.historyList(search.value, 60)
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
  return `${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
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
  <div class="hist">
    <div class="hist__bar">
      <input v-model="search" class="input search" placeholder="搜索原文或译文…" @keyup.enter="load" />
      <button class="pill" @click="load">搜索</button>
      <span class="grow" />
      <span class="muted">{{ items.length }} 条</span>
      <button class="pill pill--danger" @click="clearAll">清空</button>
    </div>

    <div v-if="!items.length && !loading" class="empty">
      <span class="empty__icon">🕘</span>
      <span>{{ search ? '没有匹配的记录' : '还没有翻译历史' }}</span>
    </div>

    <div class="grid">
      <article v-for="item in items" :key="item.id" class="card">
        <div class="card__head">
          <span class="pair">
            <span class="pair__lang">自动</span>
            <span class="pair__arrow">›</span>
            <span class="pair__lang">{{ item.target_lang }}</span>
            <span class="pair__mode">{{ MODE_LABEL[item.mode] ?? item.mode }}</span>
          </span>
          <span class="grow" />
          <span class="card__time">{{ fmt(item.created_at) }}</span>
          <button class="mini" title="复用" @click="emit('reuse', item)">
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z" />
            </svg>
          </button>
          <button class="mini" title="删除" @click="remove(item.id)">
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6" />
            </svg>
          </button>
        </div>

        <p class="card__src">{{ item.source }}</p>
        <p class="card__out" @click="copy(item.translation)">{{ item.translation }}</p>
      </article>
    </div>
  </div>
</template>

<style scoped>
.hist {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: var(--sp-2) var(--sp-5) var(--sp-5);
  overflow-y: auto;
}

.hist__bar {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  margin-bottom: var(--sp-3);
}

.search {
  max-width: 320px;
  border-radius: var(--r-pill);
}

.grow {
  flex: 1;
}

.muted {
  font-size: 12px;
  color: var(--c-text-tertiary);
}

/* 卡片网格 */
.grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: var(--sp-3);
  align-items: start;
}

@media (max-width: 1000px) {
  .grid {
    grid-template-columns: repeat(2, 1fr);
  }
}

.card {
  display: flex;
  flex-direction: column;
  padding: var(--sp-3);
  background: var(--c-card);
  border-radius: var(--r-lg);
  cursor: default;
  transition: box-shadow 0.15s;
}

.card:hover {
  box-shadow: var(--shadow-md);
}

.card__head {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-bottom: var(--sp-2);
}

.pair {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 5px 12px;
  font-size: 12px;
  color: var(--c-pill-text);
  background: var(--c-pill-bg);
  border-radius: var(--r-pill);
}

.pair__arrow {
  font-size: 14px;
  opacity: 0.7;
}

.pair__mode {
  padding-left: 6px;
  border-left: 1px solid rgba(24, 90, 188, 0.25);
  opacity: 0.75;
}

.card__time {
  font-size: 11px;
  color: var(--c-text-tertiary);
}

.mini {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  color: var(--c-text-secondary);
  background: none;
  border: none;
  border-radius: 50%;
  cursor: pointer;
}

.mini:hover {
  background: rgba(60, 64, 67, 0.1);
}

.card__src,
.card__out {
  margin: 0;
  font-size: 14px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-word;
}

.card__src {
  color: var(--c-text);
}

.card__out {
  margin-top: var(--sp-2);
  padding-top: var(--sp-2);
  color: var(--c-text-secondary);
  border-top: 1px solid rgba(60, 64, 67, 0.15);
  cursor: pointer;
}

.card__out:hover {
  color: var(--c-text);
}

.pill--danger {
  color: var(--c-danger);
}

.pill--danger:hover {
  background: #fce8e6;
}
</style>