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
  HotkeyStatus,
  LoadedDocument,
  LogEntry,
  LogStatus,
  Meta,
  Settings,
  TranslateRequest,
  TranslateResponse,
  WebBlock,
  WebPage
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
  // 把当前主题偏好告诉后端：主题只存 localStorage，Rust 读不到，
  // 而托盘菜单的主题子菜单要给对勾，得由前端喂一次。
  reportTheme: (mode: string) => invoke<void>('report_theme', { mode }),
  setActiveBackend: (id: string) => invoke<void>('set_active_backend', { id }),
  setApiKey: (backendId: string, apiKey: string) =>
    invoke<{ backend_id: string; has_key: boolean }>('set_api_key', { backendId, apiKey }),
  testBackend: (backend: Backend) => invoke<string[]>('test_backend', { backend }),

  // ---------- 托盘 / 全局热键 / 开机自启 ----------
  // 热键注册结果由后端通过 hymt2:hotkey 事件逐个回报（带 which 字段），
  // 因为两个热键共用一次 unregister_all，改一个会连另一个一起重注册。
  setHotkey: (kind: 'toggle' | 'mini', accel: string) =>
    invoke<string>('set_hotkey', { kind, accel }),
  getHotkey: () => invoke<string>('get_hotkey'),
  hotkeyStatus: () => invoke<HotkeyStatus>('hotkey_status'),
  getAutostart: () => invoke<boolean>('get_autostart'),
  setAutostart: (enable: boolean) => invoke<void>('set_autostart', { enable }),
  showWindow: () => invoke<void>('show_window'),

  // ---------- 小窗模式 ----------
  hideMini: () => invoke<void>('hide_mini'),
  setMiniPinned: (pinned: boolean) => invoke<void>('set_mini_pinned', { pinned }),

  // ---------- 划词翻译 ----------
  // 只在 Windows / Linux(X11) 可用；Wayland 下监听起不来，error 里会带原因
  selectionStatus: () =>
    invoke<{ enabled: boolean; started: boolean; modifier: string; error: string | null }>(
      'selection_status'
    ),
  hideSelectionPopup: () => invoke<void>('hide_selection_popup'),
  takePendingSelection: () => invoke<string | null>('take_pending_selection'),
  setPopupPinned: (pinned: boolean) => invoke<void>('set_popup_pinned', { pinned }),
  copyText: (text: string) => invoke<void>('copy_text', { text }),
  readClipboardText: () => invoke<string>('read_clipboard_text'),

  translate: (req: TranslateRequest, saveHistory = true) =>
    invoke<TranslateResponse>('translate', { req, saveHistory }),
  translateStream: (req: TranslateRequest, jobId: string) =>
    invoke<void>('translate_stream', { req, jobId }),
  detectLanguage: (text: string) => invoke<string>('detect_language', { text }),
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

  historyList: (search?: string, starred?: boolean, limit = 100) =>
    invoke<HistoryItem[]>('history_list', {
      search: search ?? null,
      starred: starred ?? null,
      limit
    }),
  historyCount: () => invoke<number>('history_count'),
  historySetStarred: (id: string, starred: boolean) =>
    invoke<void>('history_set_starred', { id, starred }),
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
    concurrency?: number,
    jobId?: string,
    outPath?: string
  ) =>
    invoke<DocProgress>('doc_translate', {
      path,
      targetLang,
      terms,
      concurrency: concurrency ?? null,
      jobId: jobId ?? null,
      outPath: outPath ?? null
    }),

  batchTranslate: (
    input: string,
    output: string,
    targetLang: string,
    textField: string,
    concurrency: number,
    resume: boolean,
    jobId?: string
  ) =>
    invoke<BatchProgress>('batch_translate', {
      input,
      output,
      targetLang,
      textField,
      concurrency,
      resume,
      jobId: jobId ?? null
    }),

  /** 取消正在跑的文档/批量任务。返回它是否还在跑。 */
  cancelJob: (jobId: string) => invoke<boolean>('cancel_job', { jobId }),

  /** 抓取网页并抽出正文（不翻译）。 */
  webFetch: (url: string) => invoke<WebPage>('web_fetch', { url }),

  /** 翻译已抓取的正文块，按原下标返回译文。 */
  webTranslate: (targetLang: string, blocks: WebBlock[], concurrency?: number, jobId?: string) =>
    invoke<string[]>('web_translate', {
      targetLang,
      blocks,
      concurrency: concurrency ?? null,
      jobId: jobId ?? null
    }),

  readTextFile: (path: string) => invoke<string>('read_text_file', { path }),

  // ---------- AI 请求审计日志 ----------
  logList: (limit = 200) => invoke<LogEntry[]>('log_list', { limit }),
  logStatus: () => invoke<LogStatus>('log_status'),
  logSetEnabled: (enabled: boolean) => invoke<void>('log_set_enabled', { enabled }),
  logClear: () => invoke<void>('log_clear')
}
