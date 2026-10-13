<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { toasts } from './api'
import { activeBackend, reloadAll } from './composables/useAppState'
import BatchView from './views/BatchView.vue'
import DocumentView from './views/DocumentView.vue'
import GlossaryView from './views/GlossaryView.vue'
import HistoryView from './views/HistoryView.vue'
import SettingsView from './views/SettingsView.vue'
import TranslateView from './views/TranslateView.vue'
import type { HistoryItem } from './types'

type Tab = 'translate' | 'glossary' | 'history' | 'batch' | 'doc' | 'settings'

const tab = ref<Tab>('translate')

const NAV: { key: Tab; label: string; icon: string }[] = [
  { key: 'translate', label: '翻译', icon: '🔤' },
  { key: 'glossary', label: '术语库', icon: '📖' },
  { key: 'history', label: '历史', icon: '🕘' },
  { key: 'batch', label: '批量', icon: '📦' },
  { key: 'doc', label: '文档', icon: '📄' },
  { key: 'settings', label: '设置', icon: '⚙' }
]

onMounted(reloadAll)

function reuse(item: HistoryItem) {
  tab.value = 'translate'
  // 让 TranslateView 用这条记录填充：借 localStorage 传一次
  localStorage.setItem('hymt2:reuse', JSON.stringify(item))
  setTimeout(() => window.dispatchEvent(new CustomEvent('hymt2:reuse')), 60)
}
</script>

<template>
  <div class="app">
    <aside class="sidebar">
      <div class="sidebar__logo" title="Hy-MT2">译</div>
      <nav class="sidebar__nav">
        <button
          v-for="n in NAV"
          :key="n.key"
          class="nav-btn"
          :class="{ 'nav-btn--active': tab === n.key }"
          :title="n.label"
          @click="tab = n.key"
        >
          <span class="nav-btn__icon">{{ n.icon }}</span>
          <span class="nav-btn__label">{{ n.label }}</span>
        </button>
      </nav>
    </aside>

    <main class="main">
      <header class="header">
        <h1 class="header__title">
          {{ NAV.find((n) => n.key === tab)?.label }}
        </h1>
        <span v-if="activeBackend" class="tag">{{ activeBackend.name }}</span>
      </header>

      <section class="content">
        <TranslateView v-if="tab === 'translate'" />
        <GlossaryView v-else-if="tab === 'glossary'" />
        <HistoryView v-else-if="tab === 'history'" @reuse="reuse" />
        <BatchView v-else-if="tab === 'batch'" />
        <DocumentView v-else-if="tab === 'doc'" />
        <SettingsView v-else-if="tab === 'settings'" />
      </section>
    </main>

    <div v-for="t in toasts" :key="t.id" class="toast" :class="`toast--${t.kind}`">
      {{ t.text }}
    </div>
  </div>
</template>

<style scoped>
.app {
  display: flex;
  height: 100vh;
  overflow: hidden;
}

/* 侧栏 */
.sidebar {
  display: flex;
  flex-direction: column;
  align-items: center;
  width: var(--sidebar-w);
  padding: var(--sp-3) 0;
  background: var(--c-surface-alt);
  border-right: 1px solid var(--c-border-soft);
  flex-shrink: 0;
}

.sidebar__logo {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  margin-bottom: var(--sp-4);
  font-size: 17px;
  font-weight: 700;
  color: #fff;
  background: var(--c-primary);
  border-radius: var(--r-md);
}

.sidebar__nav {
  display: flex;
  flex-direction: column;
  gap: 4px;
  width: 100%;
  padding: 0 8px;
}

.nav-btn {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  padding: 7px 0;
  font-family: inherit;
  color: var(--c-text-secondary);
  background: transparent;
  border: none;
  border-radius: var(--r-md);
  cursor: pointer;
  transition:
    background 0.15s,
    color 0.15s;
}

.nav-btn:hover {
  background: var(--c-surface-hover);
}

.nav-btn--active {
  color: var(--c-primary);
  background: var(--c-primary-soft);
}

.nav-btn__icon {
  font-size: 17px;
  line-height: 1;
}

.nav-btn__label {
  font-size: 11px;
}

/* 主区 */
.main {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
}

.header {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  height: var(--header-h);
  padding: 0 var(--sp-5);
  border-bottom: 1px solid var(--c-border-soft);
  flex-shrink: 0;
}

.header__title {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
}

.content {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.content > * {
  height: 100%;
}
</style>