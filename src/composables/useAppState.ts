import { ref, computed } from 'vue'
import { api, err } from '../api'
import {
  FALLBACK_FORMAT_TYPES,
  FALLBACK_LANGUAGES,
  FALLBACK_MODES,
  FALLBACK_STYLES
} from '../constants'
import type { Backend, GlossaryView, HistoryItem, Meta, Settings, TermPair, TranslateRequest } from '../types'

export type { HistoryItem, TermPair }

/** 全局共享的应用状态：后端配置、术语库、元数据、导航徽标计数 */

/** 主窗口当前视图。放全局是为了让翻译页的「词库胶囊」等入口也能切换页签。 */
export type Tab = 'translate' | 'glossary' | 'history' | 'batch' | 'doc' | 'settings'
export const tab = ref<Tab>('translate')

// 先用静态兜底填充，避免元数据到达前界面空白
export const meta = ref<Meta>({
  languages: FALLBACK_LANGUAGES,
  styles: FALLBACK_STYLES,
  format_types: FALLBACK_FORMAT_TYPES,
  modes: FALLBACK_MODES
})
export const settings = ref<Settings>({
  backends: [],
  active_id: '',
  temperature: 0.1,
  timeout_secs: 300,
  close_to_tray: true,
  hotkey_toggle: 'Ctrl+Shift+D',
  hotkey_mini: 'Ctrl+Shift+S',
  selection_enabled: false,
  selection_modifier: ''
})
export const glossary = ref<GlossaryView>({ active: '', names: [], counts: [], terms: [] })
export const ready = ref(false)

/** 顶栏导航徽标：历史条数 / 当前词库词条数 */
export const historyCount = ref(0)
export const glossaryTermCount = computed(() => {
  const g = glossary.value
  const i = g.names.indexOf(g.active)
  return i >= 0 ? (g.counts[i] ?? 0) : 0
})

export const activeBackend = computed<Backend | null>(
  () => settings.value.backends.find((b) => b.id === settings.value.active_id) ?? null
)

export const langName = (code: string): string =>
  meta.value.languages.find(([c]) => c === code)?.[1] ?? code

export async function loadMeta() {
  const m = await api.getMeta()
  // 后端返回为空时保留兜底数据
  meta.value = {
    languages: m.languages?.length ? m.languages : FALLBACK_LANGUAGES,
    styles: m.styles?.length ? m.styles : FALLBACK_STYLES,
    format_types: m.format_types?.length ? m.format_types : FALLBACK_FORMAT_TYPES,
    modes: m.modes?.length ? m.modes : FALLBACK_MODES
  }
}

export async function loadSettings() {
  settings.value = await api.getSettings()
}

export async function loadGlossary() {
  glossary.value = await api.glossaryGet()
}

export async function saveSettings() {
  try {
    await api.saveSettings(settings.value)
  } catch (e) {
    err(e)
  }
}

export async function pickBackend(id: string) {
  settings.value.active_id = id
  try {
    await api.setActiveBackend(id)
  } catch (e) {
    err(e)
  }
}

/** 刷新导航徽标。历史条数与词库词条数都会随使用变化，各视图在增删后调用。 */
export async function refreshCounts() {
  try {
    const [n, g] = await Promise.all([api.historyCount(), api.glossaryGet()])
    historyCount.value = n
    glossary.value = g
  } catch {
    // 浏览器调试时没有 IPC，静默跳过
  }
}

export async function reloadAll() {
  try {
    await Promise.all([loadMeta(), loadSettings(), loadGlossary(), refreshCounts()])
    ready.value = true
  } catch (e) {
    err(e)
    ready.value = true
  }
}

/** 当前目标语言名（供术语反向判断使用） */
export function isTargetChinese(targetLang: string): boolean {
  return langName(targetLang) === '中文'
}

export function buildRequest(
  p: Omit<TranslateRequest, 'target_lang'>,
  targetLang: string
): TranslateRequest {
  return { ...p, target_lang: targetLang }
}
