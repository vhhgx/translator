/**
 * 深浅主题：跟随系统 + 用户偏好记忆。
 *
 * 偏好存 localStorage（`auto | light | dark`，默认 auto），不进 Rust 配置——
 * 它纯粹是表现层状态，三个窗口各自读同一份即可保持同步（同源 localStorage 共享）。
 * `auto` 模式监听系统深浅色变化实时切换。
 */

import { ref } from 'vue'

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
}

export function useTheme() {
  return { themeMode, resolvedTheme, setThemeMode }
}
