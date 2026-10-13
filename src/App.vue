<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { toasts } from './api'
import { activeBackend, pickBackend, reloadAll, settings } from './composables/useAppState'
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
const backendMenuOpen = ref(false)

const NAV_ITEMS: { key: Tab; label: string; icon: string }[] = [
  { key: 'translate', label: '翻译', icon: '文A' },
  { key: 'doc', label: '文档', icon: '📄' },
  { key: 'batch', label: '批量', icon: '📦' },
  { key: 'glossary', label: '术语库', icon: '📖' },
  { key: 'history', label: '历史记录', icon: '🕘' },
  { key: 'settings', label: '设置', icon: '⚙' }
]

onMounted(reloadAll)

onMounted(() => {
  document.addEventListener('click', onDocClick)
  document.addEventListener('keydown', onKey)
})

onUnmounted(() => {
  document.removeEventListener('click', onDocClick)
  document.removeEventListener('keydown', onKey)
})

function onDocClick(e: MouseEvent) {
  const el = e.target as HTMLElement
  if (backendMenuOpen.value && !el.closest('.backend-pick')) {
    backendMenuOpen.value = false
  }
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') backendMenuOpen.value = false
}

async function chooseBackend(id: string) {
  backendMenuOpen.value = false
  if (id === settings.value.active_id) return
  await pickBackend(id)
}

// ---------- 自绘窗口控制（decorations: false 后系统标题栏已移除） ----------
const maximized = ref(false)

/** 浏览器预览没有 Tauri IPC，此时静默跳过而不是抛错。 */
function win() {
  try {
    return getCurrentWindow()
  } catch {
    return null
  }
}

async function syncMaximized() {
  const w = win()
  if (!w) return
  try {
    maximized.value = await w.isMaximized()
  } catch {}
}

async function minimize() {
  await win()?.minimize()
}

async function toggleMaximize() {
  const w = win()
  if (!w) return
  await w.toggleMaximize()
  await syncMaximized()
}

async function closeWindow() {
  await win()?.close()
}

/** 标题栏空白处按下即拖拽窗口；落在按钮上则不触发，否则点不到按钮。 */
function startDrag(e: MouseEvent) {
  if ((e.target as HTMLElement).closest('button')) return
  win()?.startDragging()
}

/** 双击标题栏空白处切换最大化。按钮区不触发，避免和点击冲突。 */
function onTitleDblClick(e: MouseEvent) {
  if ((e.target as HTMLElement).closest('button')) return
  toggleMaximize()
}

onMounted(() => {
  syncMaximized()
  window.addEventListener('resize', syncMaximized)
})

onUnmounted(() => window.removeEventListener('resize', syncMaximized))

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
      <!-- 顶栏兼作标题栏：空白处可拖拽移动窗口，双击切换最大化 -->
      <header class="topbar" @mousedown="startDrag" @dblclick="onTitleDblClick">
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
          <!-- 当前模型下拉选择：直接切换后端，不再跳转到设置页 -->
          <div class="backend-pick">
            <button
              class="backend-tag"
              title="切换翻译模型"
              @click.stop="backendMenuOpen = !backendMenuOpen"
            >
              <span class="backend-tag__dot" />
              {{ activeBackend?.name ?? '选择模型' }}
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
                <path d="M6 9l6 6 6-6" />
              </svg>
            </button>

            <div v-if="backendMenuOpen" class="backend-menu">
              <button
                v-for="b in settings.backends"
                :key="b.id"
                class="backend-menu__item"
                :class="{ 'backend-menu__item--on': b.id === settings.active_id }"
                @click="chooseBackend(b.id)"
              >
                <span class="backend-tag__dot" />
                <span class="backend-menu__name">{{ b.name }}</span>
                <span v-if="b.model" class="backend-menu__model">{{ b.model }}</span>
                <svg
                  v-if="b.id === settings.active_id"
                  width="14"
                  height="14"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="2.5"
                >
                  <polyline points="20 6 9 17 4 12" />
                </svg>
              </button>
              <button class="backend-menu__manage" @click="go('settings')">
                管理模型与 API Key
              </button>
            </div>
          </div>
        </div>

        <!-- 自绘窗口控制：系统装饰已关闭 -->
        <div class="wincontrols">
          <button class="winbtn" title="最小化" @click="minimize">
            <svg width="10" height="10" viewBox="0 0 10 10">
              <rect x="0" y="4.5" width="10" height="1" fill="currentColor" />
            </svg>
          </button>
          <button class="winbtn" :title="maximized ? '还原' : '最大化'" @click="toggleMaximize">
            <svg v-if="maximized" width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1">
              <rect x="0.5" y="2.5" width="7" height="7" />
              <path d="M2.5 2.5V0.5h7v7h-2" />
            </svg>
            <svg v-else width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1">
              <rect x="0.5" y="0.5" width="9" height="9" />
            </svg>
          </button>
          <button class="winbtn winbtn--close" title="关闭" @click="closeWindow">
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.2">
              <path d="M0 0l10 10M10 0L0 10" />
            </svg>
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
        <TranslateView v-if="tab === 'translate'" />
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
/* 自绘窗口：无系统边框，铺满整个窗口，不要内边距与圆角 */
.window-shell {
  width: 100vw;
  height: 100vh;
  background: var(--c-bg);
  overflow: hidden;
}

.card-container {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  background: var(--c-bg);
  overflow: hidden;
  position: relative;
}

/* 顶栏兼标题栏，右侧留出窗口按钮的位置 */
.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 40px;
  padding: 0 0 0 12px;
  background: var(--c-surface);
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

.backend-pick {
  position: relative;
}

.backend-tag {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 14px;
  font-family: inherit;
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
  flex-shrink: 0;
}

.backend-menu {
  position: absolute;
  top: calc(100% + 8px);
  right: 0;
  z-index: 60;
  min-width: 260px;
  padding: 8px;
  background: var(--c-surface);
  border: 1px solid var(--c-border-soft);
  border-radius: var(--r-lg);
  box-shadow: var(--shadow-float);
}

.backend-menu__item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 9px 10px;
  font-family: inherit;
  font-size: 13px;
  color: var(--c-text);
  background: none;
  border: none;
  border-radius: var(--r-md);
  cursor: pointer;
  text-align: left;
}

.backend-menu__item:hover {
  background: var(--c-surface-hover);
}

.backend-menu__item--on {
  color: var(--c-primary);
  font-weight: 600;
  background: var(--c-primary-soft);
}

.backend-menu__name {
  flex: 1;
  white-space: nowrap;
}

.backend-menu__model {
  font-size: 11px;
  color: var(--c-text-tertiary);
  max-width: 130px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.backend-menu__manage {
  width: 100%;
  margin-top: 4px;
  padding: 9px 10px;
  font-family: inherit;
  font-size: 12px;
  color: var(--c-text-secondary);
  background: none;
  border: none;
  border-top: 1px solid var(--c-border-soft);
  border-radius: 0 0 var(--r-md) var(--r-md);
  cursor: pointer;
  text-align: left;
}

.backend-menu__manage:hover {
  color: var(--c-primary);
}

/* 自绘窗口按钮：对齐 Windows 原生尺寸，关闭悬停转红 */
.wincontrols {
  display: flex;
  align-items: stretch;
  align-self: stretch;
  margin-left: 8px;
}

.winbtn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 46px;
  border: none;
  background: none;
  color: var(--c-text-secondary);
  cursor: pointer;
  transition: background 0.12s;
}

.winbtn:hover {
  background: var(--c-surface-hover);
  color: var(--c-text);
}

.winbtn--close:hover {
  background: #e81123;
  color: #fff;
}

/* 下拉菜单 */
.menu {
  position: absolute;
  top: 44px;
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
