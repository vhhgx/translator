<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { Clock3, Search, Star, Trash2 } from '@lucide/vue'
import { api, err, toast } from '../api'
import { historyCount, langName, refreshCounts, settings } from '../composables/useAppState'
import type { HistoryItem } from '../types'

const emit = defineEmits<{ (e: 'reuse', item: HistoryItem): void }>()

const PAGE = 60
const items = ref<HistoryItem[]>([])
const search = ref('')
const starredOnly = ref(false)
const loading = ref(false)
const limit = ref(PAGE)
const total = ref(0)

onMounted(load)

/** 输入即搜：300ms 防抖，不需要回车确认。 */
let searchTimer: number | undefined
watch(search, () => {
  window.clearTimeout(searchTimer)
  searchTimer = window.setTimeout(() => {
    limit.value = PAGE
    load()
  }, 300)
})

watch(starredOnly, () => {
  limit.value = PAGE
  load()
})

onUnmounted(() => window.clearTimeout(searchTimer))

async function load() {
  loading.value = true
  try {
    items.value = await api.historyList(
      search.value.trim() || undefined,
      starredOnly.value ? true : undefined,
      limit.value
    )
    total.value = await api.historyCount()
  } catch (e) {
    err(e)
  } finally {
    loading.value = false
  }
}

function loadMore() {
  limit.value += PAGE
  load()
}

const hasMore = computed(() => items.value.length >= limit.value)

async function toggleStar(item: HistoryItem) {
  const next = !item.starred
  item.starred = next // 乐观更新
  try {
    await api.historySetStarred(item.id, next)
    // 「仅看收藏」下取消收藏，这条要离开列表
    if (starredOnly.value && !next) {
      items.value = items.value.filter((i) => i.id !== item.id)
    }
  } catch (e) {
    item.starred = !next
    err(e)
  }
}

async function remove(id: string) {
  try {
    await api.historyDelete(id)
    items.value = items.value.filter((i) => i.id !== id)
    total.value = Math.max(0, total.value - 1)
    historyCount.value = total.value
  } catch (e) {
    err(e)
  }
}

async function clearAll() {
  if (!confirm('确定清空全部翻译历史？')) return
  try {
    await api.historyClear()
    items.value = []
    total.value = 0
    toast('已清空', 'success')
    void refreshCounts()
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

/** backend_id → 设置里配置的引擎名，没匹配上就显示 id 截断。 */
function backendName(id: string): string {
  const b = settings.value.backends.find((x) => x.id === id)
  if (b) return b.name
  return id.length > 10 ? `${id.slice(0, 8)}…` : id
}
</script>

<template>
  <div class="hist">
    <!-- 工具栏：搜索 + 收藏过滤 + 清空 -->
    <div class="page-toolbar">
      <div class="toolbar-left">
        <div class="search-box">
          <Search :size="14" class="search-box__icon" />
          <input
            v-model="search"
            class="search-input"
            placeholder="搜索原文或译文关键字…"
            type="text"
          />
        </div>
        <div class="filter-pills">
          <button
            class="filter-pill"
            :class="{ 'filter-pill--active': !starredOnly }"
            @click="starredOnly = false"
          >
            全部记录 ({{ total }})
          </button>
          <button
            class="filter-pill"
            :class="{ 'filter-pill--active': starredOnly }"
            @click="starredOnly = true"
          >
            <Star :size="11" /> 仅看收藏
          </button>
        </div>
      </div>

      <div class="toolbar-right">
        <span class="toolbar-hint">点击任意卡片可回填至翻译台</span>
        <button class="btn btn--danger btn--sm" @click="clearAll">
          <Trash2 :size="13" />
          清空记录
        </button>
      </div>
    </div>

    <!-- 卡片网格 -->
    <div class="hist-scroll">
      <div v-if="!items.length && !loading" class="empty">
        <Clock3 :size="32" class="empty__icon" />
        <span>{{ search ? '没有匹配的记录' : starredOnly ? '还没有收藏的翻译' : '还没有翻译历史' }}</span>
      </div>

      <div v-else class="history-grid">
        <article
          v-for="item in items"
          :key="item.id"
          class="history-card"
          @click="emit('reuse', item)"
        >
          <div class="h-card-header">
            <div class="h-meta-tags">
              <span class="h-lang-tag">自动 → {{ langName(item.target_lang) }}</span>
              <span class="h-engine-tag">{{ MODE_LABEL[item.mode] ?? item.mode }} · {{ backendName(item.backend_id) }}</span>
            </div>
            <div class="h-card-actions" @click.stop>
              <span class="h-time">{{ fmt(item.created_at) }}</span>
              <button
                class="icon-btn star-btn"
                :class="{ 'star-btn--on': item.starred }"
                :title="item.starred ? '取消收藏' : '加入收藏'"
                @click="toggleStar(item)"
              >
                <Star :size="13" :fill="item.starred ? 'currentColor' : 'none'" />
              </button>
              <button class="icon-btn icon-btn--danger" title="删除此条" @click="remove(item.id)">
                <Trash2 :size="13" />
              </button>
            </div>
          </div>

          <div class="h-card-body">
            <p class="h-source">{{ item.source }}</p>
            <div class="h-divider" />
            <p class="h-target" @click.stop="copy(item.translation)" title="点击复制译文">
              {{ item.translation }}
            </p>
          </div>
        </article>
      </div>

      <!-- 加载更多 -->
      <div v-if="items.length && hasMore" class="load-more">
        <button class="btn btn--sm" :disabled="loading" @click="loadMore">
          {{ loading ? '加载中…' : `加载更多（已显示 ${items.length} 条）` }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.hist {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  background: var(--surface-secondary);
}

/* ---------- 工具栏 ---------- */
.page-toolbar {
  padding: 12px 24px;
  background: var(--surface-primary);
  border-bottom: 1px solid var(--border-subtle);
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  flex-shrink: 0;
}

.toolbar-left {
  display: flex;
  align-items: center;
  gap: 12px;
  flex: 1;
  min-width: 0;
}

.toolbar-right {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-shrink: 0;
}

.toolbar-hint {
  font-size: 12px;
  color: var(--text-muted);
  white-space: nowrap;
}

.search-box {
  position: relative;
  width: 280px;
  flex-shrink: 1;
}

.search-box__icon {
  position: absolute;
  left: 11px;
  top: 50%;
  transform: translateY(-50%);
  color: var(--text-muted);
  pointer-events: none;
}

.search-input {
  width: 100%;
  padding: 7px 12px 7px 32px;
  border-radius: 8px;
  border: 1px solid var(--border-subtle);
  background: var(--surface-secondary);
  color: var(--text-primary);
  font-family: inherit;
  font-size: 12.5px;
  outline: none;
  transition: all 0.15s;
}

.search-input:focus {
  border-color: var(--border-focus);
  background: var(--surface-primary);
  box-shadow: 0 0 0 3px var(--accent-glow);
}

.filter-pills {
  display: flex;
  background: var(--surface-tertiary);
  padding: 2px;
  border-radius: 7px;
  border: 1px solid var(--border-subtle);
  flex-shrink: 0;
}

.filter-pill {
  padding: 4px 10px;
  border-radius: 5px;
  font-family: inherit;
  font-size: 12px;
  color: var(--text-secondary);
  background: transparent;
  border: none;
  cursor: pointer;
  transition: all 0.15s;
  white-space: nowrap;
}

.filter-pill--active {
  background: var(--surface-primary);
  color: var(--text-primary);
  font-weight: 500;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.08);
}

/* ---------- 卡片网格 ---------- */
.hist-scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 20px 24px;
}

.history-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 14px;
  align-content: start;
}

@media (max-width: 1000px) {
  .history-grid {
    grid-template-columns: 1fr;
  }
}

.history-card {
  background: var(--surface-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 11px;
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  height: 176px;
  transition: all 0.18s;
  cursor: pointer;
  overflow: hidden;
}

.history-card:hover {
  border-color: var(--border-focus);
  transform: translateY(-1px);
  box-shadow: 0 8px 20px -6px rgba(0, 0, 0, 0.18);
}

.h-card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  flex-shrink: 0;
}

.h-meta-tags {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.h-lang-tag {
  font-size: 11.5px;
  font-weight: 500;
  padding: 2px 8px;
  border-radius: 5px;
  background: var(--surface-active);
  color: var(--accent-primary);
  border: 1px solid var(--border-focus);
  white-space: nowrap;
}

.h-engine-tag {
  font-size: 11px;
  padding: 2px 6px;
  border-radius: 5px;
  background: var(--surface-tertiary);
  color: var(--text-secondary);
  border: 1px solid var(--border-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.h-card-actions {
  display: flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
}

.h-time {
  font-size: 11.5px;
  color: var(--text-muted);
  margin-right: 4px;
  font-variant-numeric: tabular-nums;
}

.icon-btn {
  width: 24px;
  height: 24px;
  border-radius: 5px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: all 0.15s;
}

.icon-btn:hover {
  color: var(--text-primary);
  background: var(--surface-hover);
}

.icon-btn--danger:hover {
  color: var(--status-danger);
  background: var(--status-danger-bg);
}

.star-btn--on {
  color: var(--star-active);
}

.h-card-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 6px;
  overflow: hidden;
}

.h-source,
.h-target {
  margin: 0;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
  line-height: 1.45;
  word-break: break-word;
}

.h-source {
  font-size: 13.5px;
  font-weight: 500;
  color: var(--text-primary);
}

.h-divider {
  height: 1px;
  background: var(--border-subtle);
  flex-shrink: 0;
}

.h-target {
  font-size: 13px;
  color: var(--text-secondary);
  cursor: pointer;
}

.h-target:hover {
  color: var(--text-primary);
}

.load-more {
  display: flex;
  justify-content: center;
  padding: 16px 0 4px;
}
</style>
