import { ref, computed } from 'vue'
import { api, err } from '../api'
import type { Backend, GlossaryView, Meta, Settings, TermPair, TranslateRequest } from '../types'

/** 全局共享的应用状态：后端配置、术语库、元数据 */
export const meta = ref<Meta>({ languages: [], styles: [], format_types: [], modes: [] })
export const settings = ref<Settings>({ backends: [], active_id: '', temperature: 0.1, timeout_secs: 300 })
export const glossary = ref<GlossaryView>({ active: '', names: [], terms: [] })
export const ready = ref(false)

export const activeBackend = computed<Backend | null>(
  () => settings.value.backends.find((b) => b.id === settings.value.active_id) ?? null
)

export const langName = (code: string): string =>
  meta.value.languages.find(([c]) => c === code)?.[1] ?? code

export async function loadMeta() {
  meta.value = await api.getMeta()
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

export async function reloadAll() {
  try {
    await Promise.all([loadMeta(), loadSettings(), loadGlossary()])
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

export type { TermPair }