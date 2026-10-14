<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import {
  BookOpen,
  Clock3,
  FileText,
  Globe,
  Languages,
  Minus,
  Moon,
  Settings,
  Square,
  Sun,
  X
} from '@lucide/vue'
import { toasts } from './api'
import { glossary, historyCount, reloadAll, settings, tab } from './composables/useAppState'
import { setThemeMode, useTheme } from './composables/useTheme'
import BackendPicker from './components/BackendPicker.vue'
import BatchView from './views/BatchView.vue'
import DocumentView from './views/DocumentView.vue'
import GlossaryView from './views/GlossaryView.vue'
import HistoryView from './views/HistoryView.vue'
import SettingsView from './views/SettingsView.vue'
import TranslateView from './views/TranslateView.vue'
import type { HistoryItem, Tab } from './composables/useAppState'

const { themeMode } = useTheme()

onMounted(reloadAll)

// ---------- 主题快捷切换（顶栏日/月按钮；完整三选一在设置页） ----------
function toggleTheme() {
  setThemeMode(themeMode.value === 'dark' ? 'light' : 'dark')
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

/**
 * 关闭按钮。开启「关闭到托盘」时只隐藏窗口——进程必须留着，
 * 否则全局热键会跟着一起消失，托盘图标也一并没了。
 */
async function closeWindow() {
  const w = win()
  if (!w) return
  if (settings.value.close_to_tray) {
    await w.hide()
  } else {
    await w.close()
  }
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
}

function reuse(item: HistoryItem) {
  tab.value = 'translate'
  localStorage.setItem('hymt2:reuse', JSON.stringify(item))
  setTimeout(() => window.dispatchEvent(new CustomEvent('hymt2:reuse')), 60)
}
</script>

<template>
  <div class="window-shell">
    <div class="app-frame">
      <!-- 顶栏兼作标题栏：空白处可拖拽移动窗口，双击切换最大化 -->
      <header class="titlebar" @mousedown="startDrag" @dblclick="onTitleDblClick">
        <div class="titlebar__left">
          <div class="app-brand" title="Zephyr Translator">
            <div class="brand-logo">译</div>
          </div>

          <!-- 左侧：工作台导航（分段控件） -->
          <nav class="nav-tabs">
            <button
              class="nav-tab"
              :class="{ 'nav-tab--active': tab === 'translate' }"
              @click="go('translate')"
            >
              <Languages :size="14" />
              <span>文本翻译</span>
            </button>
            <button class="nav-tab" :class="{ 'nav-tab--active': tab === 'doc' }" @click="go('doc')">
              <FileText :size="14" />
              <span>文档翻译</span>
            </button>
            <button
              class="nav-tab"
              :class="{ 'nav-tab--active': tab === 'batch' }"
              @click="go('batch')"
            >
              <Globe :size="14" />
              <span>网页 / 批量</span>
            </button>
          </nav>
        </div>

        <div class="titlebar__right">
          <!-- 右侧：个人资产区（历史 / 术语库 / 设置 / 主题） + 窗口控制 -->
          <div class="utility-nav">
            <button
              class="util-btn"
              :class="{ 'util-btn--active': tab === 'history' }"
              title="查看翻译历史记录"
              @click="go('history')"
            >
              <Clock3 :size="14" />
              <span>历史记录</span>
              <span class="util-badge">{{ historyCount }}</span>
            </button>
            <button
              class="util-btn"
              :class="{ 'util-btn--active': tab === 'glossary' }"
              title="管理自定义术语库"
              @click="go('glossary')"
            >
              <BookOpen :size="14" />
              <span>术语库</span>
              <span class="util-badge">{{ glossary.names.length }}</span>
            </button>
            <button
              class="util-btn"
              :class="{ 'util-btn--active': tab === 'settings' }"
              title="系统设置"
              @click="go('settings')"
            >
              <Settings :size="14" />
              <span>设置</span>
            </button>
            <button
              class="icon-btn"
              :title="themeMode === 'dark' ? '切换到浅色模式' : '切换到深色模式'"
              @click="toggleTheme"
            >
              <Sun v-if="themeMode === 'dark'" :size="15" />
              <Moon v-else :size="15" />
            </button>
          </div>

          <div class="titlebar__divider"></div>

          <div class="wincontrols">
            <button class="winbtn" title="最小化" @click="minimize">
              <Minus :size="14" />
            </button>
            <button class="winbtn" :title="maximized ? '还原' : '最大化'" @click="toggleMaximize">
              <Square :size="11" />
            </button>
            <button class="winbtn winbtn--close" title="关闭" @click="closeWindow">
              <X :size="14" />
            </button>
          </div>
        </div>
      </header>

      <!-- 主工作区内容 -->
      <main class="content">
        <TranslateView v-if="tab === 'translate'" />
        <GlossaryView v-else-if="tab === 'glossary'" />
        <HistoryView v-else-if="tab === 'history'" @reuse="reuse" />
        <BatchView v-else-if="tab === 'batch'" />
        <DocumentView v-else-if="tab === 'doc'" />
        <SettingsView v-else-if="tab === 'settings'" />
      </main>

      <!-- 底部全局状态栏：引擎 + 术语库状态 + 热键提示 -->
      <footer class="statusbar">
        <div class="statusbar__left">
          <BackendPicker />
          <span class="statusbar__text">术语库已启用（{{ glossary.active || '—' }}）</span>
        </div>
        <div class="statusbar__right">
          <span v-if="settings.hotkey_toggle" class="statusbar__text">
            主窗:
            <kbd class="mini-kbd">{{ settings.hotkey_toggle }}</kbd>
          </span>
          <span v-if="settings.hotkey_mini" class="statusbar__text">
            小窗:
            <kbd class="mini-kbd">{{ settings.hotkey_mini }}</kbd>
          </span>
        </div>
      </footer>
    </div>

    <!-- 全局提示条 -->
    <div v-for="t in toasts" :key="t.id" class="toast" :class="`toast--${t.kind}`">
      {{ t.text }}
    </div>
  </div>
</template>

<style scoped>
/* 自绘窗口：无系统边框，铺满整个窗口 */
.window-shell {
  width: 100vw;
  height: 100vh;
  background: var(--surface-primary);
  overflow: hidden;
}

.app-frame {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  background: var(--surface-primary);
  overflow: hidden;
  position: relative;
}

/* ---------- 顶栏 ---------- */
.titlebar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  height: var(--titlebar-h);
  padding: 0 12px 0 16px;
  background: var(--surface-primary);
  border-bottom: 1px solid var(--border-subtle);
  flex-shrink: 0;
  user-select: none;
}

.titlebar__left {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
}

.app-brand {
  display: flex;
  align-items: center;
  padding-right: 4px;
}

.brand-logo {
  width: 24px;
  height: 24px;
  border-radius: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
  font-weight: 700;
  color: #fff;
  background: linear-gradient(135deg, #38bdf8, #6366f1);
  box-shadow: 0 2px 8px rgba(56, 189, 248, 0.3);
}

.nav-tabs {
  display: flex;
  gap: 3px;
  padding: 3px;
  background: var(--surface-tertiary);
  border: 1px solid var(--border-subtle);
  border-radius: 9px;
}

.nav-tab {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 12px;
  font-family: inherit;
  font-size: 12.5px;
  font-weight: 500;
  color: var(--text-secondary);
  background: transparent;
  border: none;
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.15s;
  white-space: nowrap;
}

.nav-tab:hover {
  color: var(--text-primary);
}

.nav-tab--active {
  background: var(--surface-primary);
  color: var(--accent-primary);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.12);
}

.titlebar__right {
  display: flex;
  align-items: center;
  gap: 4px;
}

.utility-nav {
  display: flex;
  align-items: center;
  gap: 2px;
}

.util-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 10px;
  font-family: inherit;
  font-size: 12.5px;
  font-weight: 500;
  color: var(--text-secondary);
  background: transparent;
  border: 1px solid transparent;
  border-radius: 7px;
  cursor: pointer;
  transition: all 0.15s;
  white-space: nowrap;
}

.util-btn:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.util-btn--active {
  background: var(--surface-active);
  color: var(--accent-primary);
  border-color: var(--border-focus);
}

.util-badge {
  font-size: 10.5px;
  padding: 0 5px;
  border-radius: var(--r-pill);
  background: var(--surface-tertiary);
  color: var(--text-secondary);
  border: 1px solid var(--border-subtle);
  font-variant-numeric: tabular-nums;
}

.util-btn--active .util-badge {
  background: color-mix(in srgb, var(--accent-primary) 15%, transparent);
  color: var(--accent-primary);
  border-color: transparent;
}

.icon-btn {
  width: 30px;
  height: 30px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 8px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.15s;
}

.icon-btn:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.titlebar__divider {
  width: 1px;
  height: 16px;
  margin: 0 6px;
  background: var(--border-subtle);
}

.wincontrols {
  display: flex;
  align-items: center;
  gap: 2px;
}

.winbtn {
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.15s;
}

.winbtn:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.winbtn--close:hover {
  background: #ef4444;
  color: #fff;
}

/* ---------- 主内容区 ---------- */
.content {
  flex: 1;
  min-height: 0;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

/* ---------- 底部状态栏 ---------- */
.statusbar {
  height: var(--statusbar-h);
  padding: 0 14px;
  background: var(--surface-primary);
  border-top: 1px solid var(--border-subtle);
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-shrink: 0;
}

.statusbar__left,
.statusbar__right {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
}

.statusbar__text {
  font-size: 12px;
  color: var(--text-secondary);
  display: inline-flex;
  align-items: center;
  gap: 4px;
  white-space: nowrap;
}
</style>
