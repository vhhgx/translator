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
const menuOpen = ref(false)

const NAV: { key: Tab; label: string; icon: string }[] = [
  { key: 'translate', label: '翻译', icon: '🔤' },
  { key: 'glossary', label: '术语库', icon: '📖' },
  { key: 'history', label: '历史', icon: '🕘' },
  { key: 'batch', label: '批量', icon: '📦' },
  { key: 'doc', label: '文档', icon: '📄' },
  { key: 'settings', label: '设置', icon: '⚙' }
]

onMounted(reloadAll)

function go(t: Tab) {
  tab.value = t
  menuOpen.value = false
}

function reuse(item: HistoryItem) {
  tab.value = 'translate'
  localStorage.setItem('hymt2:reuse', JSON.stringify(item))
  setTimeout(() => window.dispatchEvent(new CustomEvent('hymt2:reuse')), 60)
}
</script>

<template>
  <div class="app">
    <!-- 顶栏 -->
    <header class="topbar">
      <button class="icon-btn" title="菜单" @click="menuOpen = !menuOpen">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M3 6h18M3 12h18M3 18h18" />
        </svg>
      </button>

      <div class="brand">
        <span class="brand__dot">H</span>
        <span class="brand__text">Hy-MT2 翻译</span>
      </div>

      <nav class="topnav">
        <button
          v-for="n in NAV"
          :key="n.key"
          class="topnav__item"
          :class="{ 'topnav__item--active': tab === n.key }"
          @click="go(n.key)"
        >
          <span class="topnav__icon">{{ n.icon }}</span>
          {{ n.label }}
        </button>
      </nav>

      <span class="topbar__spacer" />
      <span v-if="activeBackend" class="tag">{{ activeBackend.name }}</span>
    </header>

    <!-- 汉堡菜单 -->
    <transition name="drop">
      <div v-if="menuOpen" class="menu">
        <button
          v-for="n in NAV"
          :key="n.key"
          class="menu__item"
          :class="{ 'menu__item--active': tab === n.key }"
          @click="go(n.key)"
        >
          <span>{{ n.icon }}</span>{{ n.label }}
        </button>
      </div>
    </transition>

    <main class="content">
      <TranslateView v-if="tab === 'translate'" />
      <GlossaryView v-else-if="tab === 'glossary'" />
      <HistoryView v-else-if="tab === 'history'" @reuse="reuse" />
      <BatchView v-else-if="tab === 'batch'" />
      <DocumentView v-else-if="tab === 'doc'" />
      <SettingsView v-else-if="tab === 'settings'" />
    </main>

    <div v-for="t in toasts" :key="t.id" class="toast" :class="`toast--${t.kind}`">
      {{ t.text }}
    </div>
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
  overflow: hidden;
  background: var(--c-bg);
}

/* 顶栏 */
.topbar {
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  height: 56px;
  padding: 0 var(--sp-4);
  flex-shrink: 0;
}

.icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 38px;
  height: 38px;
  color: var(--c-text-secondary);
  background: none;
  border: none;
  border-radius: 50%;
  cursor: pointer;
}

.icon-btn:hover {
  background: var(--c-surface-hover);
}

.brand {
  display: flex;
  align-items: center;
  gap: 8px;
}

.brand__dot {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  font-size: 14px;
  font-weight: 700;
  color: #fff;
  background: var(--c-primary);
  border-radius: 50%;
}

.brand__text {
  font-size: 17px;
  font-weight: 500;
  color: var(--c-text-secondary);
}

.topnav {
  display: flex;
  align-items: center;
  gap: 2px;
  margin-left: var(--sp-4);
}

.topnav__item {
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
  transition: background 0.15s;
}

.topnav__item:hover {
  background: var(--c-surface-hover);
}

.topnav__item--active {
  color: var(--c-pill-text);
  background: var(--c-pill-bg);
}

.topnav__icon {
  font-size: 15px;
}

.topbar__spacer {
  flex: 1;
}

/* 汉堡菜单 */
.menu {
  position: absolute;
  top: 52px;
  left: 12px;
  z-index: 50;
  display: flex;
  flex-direction: column;
  min-width: 180px;
  padding: 8px;
  background: var(--c-surface);
  border: 1px solid var(--c-border-soft);
  border-radius: var(--r-lg);
  box-shadow: var(--shadow-md);
}

.menu__item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  font-family: inherit;
  font-size: 14px;
  text-align: left;
  color: var(--c-text);
  background: none;
  border: none;
  border-radius: var(--r-md);
  cursor: pointer;
}

.menu__item:hover {
  background: var(--c-surface-hover);
}

.menu__item--active {
  color: var(--c-pill-text);
  background: var(--c-primary-soft);
}

.drop-enter-active,
.drop-leave-active {
  transition:
    opacity 0.15s,
    transform 0.15s;
}

.drop-enter-from,
.drop-leave-to {
  opacity: 0;
  transform: translateY(-6px);
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