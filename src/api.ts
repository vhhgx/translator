/** Tauri command 调用的薄封装 + 全局提示条 */
import { invoke } from '@tauri-apps/api/core'
import { ref } from 'vue'
import type {
  Backend,
  BatchProgress,
  DocProgress,
  ExportPayload,
  GlossaryView,
  HistoryItem,
  LoadedDocument,
  Meta,
  Settings,
  TranslateRequest,
  TranslateResponse
} from './types'

// ---------- toast ----------
export interface Toast {
  id: number
  text: string
  kind: 'info' | 'error' | 'success'
}
export const toasts = ref<Toast[]>([])
let toastSeq = 0

export function toast(text: string, kind: Toast['kind'] = 'info', ms = 3200) {
  const id = ++toastSeq
  toasts.value.push({ id, text, kind })
  setTimeout(() => {
    toasts.value = toasts.value.filter((t) => t.id !== id)
  }, ms)
}

export const err = (e: unknown) => toast(String(e), 'error', 6000)

// ---------- commands ----------
export const api = {
  getMeta: () => invoke<Meta>('get_meta'),
  getSettings: () => invoke<Settings>('get_settings'),
  saveSettings: (s: Settings) => invoke<void>('save_settings', { s }),
  setActiveBackend: (id: string) => invoke<void>('set_active_backend', { id }),
  setApiKey: (backendId: string, apiKey: string) =>
    invoke<{ backend_id: string; has_key: boolean }>('set_api_key', { backendId, apiKey }),
  testBackend: (backend: Backend) => invoke<string[]>('test_backend', { backend }),

  translate: (req: TranslateRequest, saveHistory = true) =>
    invoke<TranslateResponse>('translate', { req, saveHistory }),
  translateStream: (req: TranslateRequest, jobId: string) =>
    invoke<void>('translate_stream', { req, jobId }),
  previewPrompt: (req: TranslateRequest) => invoke<string>('preview_prompt', { req }),

  glossaryGet: () => invoke<GlossaryView>('glossary_get'),
  glossarySetTerms: (name: string, terms: [string, string][]) =>
    invoke<void>('glossary_set_terms', { name, terms }),
  glossaryActivate: (name: string) => invoke<void>('glossary_activate', { name }),
  glossaryCreate: (name: string) => invoke<void>('glossary_create', { name }),
  glossaryRename: (old: string, nw: string) => invoke<void>('glossary_rename', { old, new: nw }),
  glossaryDelete: (name: string) => invoke<void>('glossary_delete', { name }),
  glossaryExport: (name: string) => invoke<ExportPayload>('glossary_export', { name }),
  glossaryImport: (name: string, terms: [string, string][], replace = false) =>
    invoke<number>('glossary_import', { name, terms, replace }),

  historyList: (search?: string, limit = 100) =>
    invoke<HistoryItem[]>('history_list', { search: search ?? null, limit }),
  historyDelete: (id: string) => invoke<void>('history_delete', { id }),
  historyClear: () => invoke<void>('history_clear'),

  docLoad: (path: string) => invoke<LoadedDocument>('doc_load', { path }),
  docOutputPath: (path: string, targetLang: string) =>
    invoke<string>('doc_output_path', { path, targetLang }),
  docSave: (path: string, content: string) => invoke<void>('doc_save', { path, content }),
  docTranslate: (
    path: string,
    targetLang: string,
    terms: [string, string][],
    concurrency?: number
  ) => invoke<DocProgress>('doc_translate', { path, targetLang, terms, concurrency: concurrency ?? null }),

  batchTranslate: (
    input: string,
    output: string,
    targetLang: string,
    textField: string,
    concurrency: number,
    resume: boolean
  ) =>
    invoke<BatchProgress>('batch_translate', {
      input,
      output,
      targetLang,
      textField,
      concurrency,
      resume
    }),

  readTextFile: (path: string) => invoke<string>('read_text_file', { path })
}