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

const NAV_ITEMS: { key: Tab; label: string; icon: string }[] = [
  { key: 'translate', label: '翻译', icon: '文A' },
  { key: 'doc', label: '文档', icon: '📄' },
  { key: 'batch', label: '批量', icon: '📦' },
  { key: 'glossary', label: '术语库', icon: '📖' },
  { key: 'history', label: '历史记录', icon: '🕘' },
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
  <div class="window-shell">
    <div class="card-container">
      <!-- 顶栏：严格参考 Google Translate 新版 -->
      <header class="topbar">
        <div class="topbar__left">
          <button class="icon-btn" title="主菜单" @click="menuOpen = !menuOpen">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M3 6h18M3 12h18M3 18h18" />
            </svg>
          </button>
        </div>

        <!-- 顶栏快捷模式胶囊（对齐设计图：文A SMS、Document、Website） -->
        <div class="topbar__center">
          <button
            class="top-pill"
            :class="{ 'top-pill--active': tab === 'translate' }"
            @click="go('translate')"
          >
            <span class="top-pill__icon">文A</span>
            <span>文本</span>
          </button>
          <button
            class="top-pill"
            :class="{ 'top-pill--active': tab === 'doc' }"
            @click="go('doc')"
          >
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
              <polyline points="14 2 14 8 20 8" />
              <line x1="16" y1="13" x2="8" y2="13" />
              <line x1="16" y1="17" x2="8" y2="17" />
              <polyline points="10 9 9 9 8 9" />
            </svg>
            <span>Document</span>
          </button>
          <button
            class="top-pill"
            :class="{ 'top-pill--active': tab === 'batch' }"
            @click="go('batch')"
          >
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <circle cx="12" cy="12" r="10" />
              <line x1="2" y1="12" x2="22" y2="12" />
              <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z" />
            </svg>
            <span>Website / 批量</span>
          </button>
        </div>

        <div class="topbar__right">
          <!-- 当前激活后端提示胶囊 -->
          <button
            v-if="activeBackend"
            class="backend-tag"
            title="点击前往设置切换或配置模型"
            @click="go('settings')"
          >
            <span class="backend-tag__dot" />
            {{ activeBackend.name }}
          </button>
        </div>
      </header>

      <!-- 下拉导航菜单 -->
      <transition name="drop">
        <div v-if="menuOpen" class="menu">
          <button
            v-for="n in NAV_ITEMS"
            :key="n.key"
            class="menu__item"
            :class="{ 'menu__item--active': tab === n.key }"
            @click="go(n.key)"
          >
            <span class="menu__icon">{{ n.icon }}</span>
            <span>{{ n.label }}</span>
          </button>
        </div>
      </transition>

      <!-- 主工作区内容 -->
      <main class="content">
        <TranslateView v-if="tab === 'translate'" @view-all-history="go('history')" />
        <GlossaryView v-else-if="tab === 'glossary'" />
        <HistoryView v-else-if="tab === 'history'" @reuse="reuse" />
        <BatchView v-else-if="tab === 'batch'" />
        <DocumentView v-else-if="tab === 'doc'" />
        <SettingsView v-else-if="tab === 'settings'" />
      </main>
    </div>

    <!-- 全局提示条 -->
    <div v-for="t in toasts" :key="t.id" class="toast" :class="`toast--${t.kind}`">
      {{ t.text }}
    </div>
  </div>
</template>

<style scoped>
.window-shell {
  display: flex;
  justify-content: center;
  align-items: center;
  width: 100vw;
  height: 100vh;
  padding: 16px;
  background: var(--c-app-bg);
  box-sizing: border-box;
}

.card-container {
  display: flex;
  flex-direction: column;
  width: 100%;
  max-width: 1440px;
  height: 100%;
  background: var(--c-bg);
  border-radius: var(--r-container);
  box-shadow: 0 8px 32px rgba(60, 64, 67, 0.08);
  overflow: hidden;
  position: relative;
}

/* 顶栏 */
.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 64px;
  padding: 0 24px;
  background: var(--c-surface);
  border-bottom: 1px solid var(--c-border-soft);
  flex-shrink: 0;
  user-select: none;
}

.topbar__left {
  display: flex;
  align-items: center;
  gap: 16px;
}

.icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  color: var(--c-text-secondary);
  background: none;
  border: none;
  border-radius: 50%;
  cursor: pointer;
  transition: background 0.15s;
}

.icon-btn:hover {
  background: var(--c-surface-hover);
  color: var(--c-text);
}

/* 顶栏快捷模式胶囊 */
.topbar__center {
  display: flex;
  align-items: center;
  gap: 10px;
}

.top-pill {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 8px 18px;
  font-family: inherit;
  font-size: 14px;
  font-weight: 500;
  color: var(--c-text-secondary);
  background: transparent;
  border: none;
  border-radius: var(--r-pill);
  cursor: pointer;
  transition: all 0.15s ease;
}

.top-pill:hover {
  background: var(--c-surface-hover);
  color: var(--c-text);
}

.top-pill--active {
  background: var(--c-primary-soft);
  color: var(--c-primary);
}

.top-pill__icon {
  font-weight: 600;
  font-size: 13px;
}

.topbar__right {
  display: flex;
  align-items: center;
  gap: 12px;
}

.backend-tag {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 14px;
  font-size: 13px;
  font-weight: 500;
  color: var(--c-text-secondary);
  background: var(--c-surface-alt);
  border: 1px solid var(--c-border-soft);
  border-radius: var(--r-pill);
  cursor: pointer;
  transition: all 0.15s;
}

.backend-tag:hover {
  background: var(--c-surface-hover);
  color: var(--c-primary);
  border-color: var(--c-primary);
}

.backend-tag__dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: #34a853;
}

/* 下拉菜单 */
.menu {
  position: absolute;
  top: 68px;
  left: 20px;
  z-index: 50;
  display: flex;
  flex-direction: column;
  min-width: 200px;
  padding: 8px;
  background: var(--c-surface);
  border: 1px solid var(--c-border-soft);
  border-radius: var(--r-lg);
  box-shadow: var(--shadow-float);
}

.menu__item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 14px;
  font-family: inherit;
  font-size: 14px;
  color: var(--c-text);
  background: none;
  border: none;
  border-radius: var(--r-md);
  cursor: pointer;
  transition: background 0.15s;
}

.menu__item:hover {
  background: var(--c-surface-hover);
}

.menu__item--active {
  color: var(--c-primary);
  background: var(--c-primary-soft);
  font-weight: 500;
}

.menu__icon {
  font-size: 16px;
  width: 20px;
  text-align: center;
}

.drop-enter-active,
.drop-leave-active {
  transition: opacity 0.15s, transform 0.15s;
}

.drop-enter-from,
.drop-leave-to {
  opacity: 0;
  transform: translateY(-8px);
}

.content {
  flex: 1;
  min-height: 0;
  overflow: hidden;
  background: var(--c-bg);
}

.content > * {
  height: 100%;
}
</style>
