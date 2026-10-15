/**
 * 深浅主题：跟随系统 + 用户偏好记忆。
 *
 * 偏好存 localStorage（`auto | light | dark`，默认 auto），不进 Rust 配置——
 * 它纯粹是表现层状态，三个窗口各自读同一份即可保持同步（同源 localStorage 共享）。
 * `auto` 模式监听系统深浅色变化实时切换。
 */

import { ref } from 'vue'
import { listen } from '@tauri-apps/api/event'

export type ThemeMode = 'auto' | 'light' | 'dark'
export type ResolvedTheme = 'light' | 'dark'

const KEY = 'hymt2:theme'

const themeMode = ref<ThemeMode>('auto')
const resolvedTheme = ref<ResolvedTheme>('light')
let initialized = false

function readMode(): ThemeMode {
  const v = localStorage.getItem(KEY)
  return v === 'light' || v === 'dark' || v === 'auto' ? v : 'auto'
}

function systemPrefersDark(): boolean {
  return window.matchMedia('(prefers-color-scheme: dark)').matches
}

function apply() {
  const resolved: ResolvedTheme =
    themeMode.value === 'auto' ? (systemPrefersDark() ? 'dark' : 'light') : themeMode.value
  resolvedTheme.value = resolved
  document.documentElement.setAttribute('data-theme', resolved)
}

/** 在应用挂载前调用（main.ts），三个窗口都走，避免首帧闪错色。 */
export function initTheme() {
  if (initialized) return
  initialized = true
  themeMode.value = readMode()
  apply()
  // 冷启动就把偏好喂给后端，否则托盘菜单的主题对勾会一直停在「跟随系统」
  report()
  window
    .matchMedia('(prefers-color-scheme: dark)')
    .addEventListener('change', () => {
      if (themeMode.value === 'auto') apply()
    })
  // 三个窗口共用同一份 localStorage（同源 WebView），storage 事件只会
  // 广播给**其它**窗口——正好用来做主/弹/小窗的主题联动。
  window.addEventListener('storage', (e) => {
    if (e.key !== KEY || e.newValue === null) return
    const v = e.newValue
    if (v === 'light' || v === 'dark' || v === 'auto') {
      themeMode.value = v
      apply()
    }
  })
}

export function setThemeMode(mode: ThemeMode) {
  themeMode.value = mode
  localStorage.setItem(KEY, mode)
  apply()
  report()
}

/**
 * 把偏好告诉后端，托盘菜单的主题子菜单靠它打对勾。
 * 浏览器预览没有 IPC，失败静默——那里根本没有托盘。
 */
function report() {
  void import('../api')
    .then((m) => m.api.reportTheme(themeMode.value))
    .catch(() => {})
}

export function useTheme() {
  return { themeMode, resolvedTheme, setThemeMode }
}

/** 托盘里切主题走的就是这条事件，和设置页的按钮是同一条路。 */
export function listenThemeFromTray() {
  void listen<{ mode: ThemeMode }>('hymt2:theme', (e) => {
    if (e.payload.mode === 'light' || e.payload.mode === 'dark' || e.payload.mode === 'auto') {
      setThemeMode(e.payload.mode)
    }
  }).catch(() => {})
}
