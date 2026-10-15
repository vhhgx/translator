/** 与 Rust 后端对应的类型定义 */

export type Mode = 'basic' | 'terminology' | 'style' | 'context' | 'delimiter' | 'structured'

export interface Backend {
  id: string
  name: string
  base_url: string
  model: string
  has_api_key: boolean
}

export interface Settings {
  backends: Backend[]
  active_id: string
  temperature: number
  timeout_secs: number
  /** 点关闭时隐藏到托盘而不是退出，否则全局热键会跟着进程一起消失 */
  close_to_tray: boolean
  /** 显示/隐藏窗口的全局热键，如 "Ctrl+Shift+D"；空串表示不注册 */
  hotkey_toggle: string
  /** 小窗模式的全局热键，如 "Ctrl+Shift+S"；空串表示不注册 */
  hotkey_mini: string
  /** 划词翻译：选中文本后在光标处自动弹窗（需常驻进程 + 全局鼠标监听） */
  selection_enabled: boolean
  /** 触发划词所需的修饰键："" = 不要求，"ctrl" / "alt" / "shift" */
  selection_modifier: string
}

/** 一个全局热键的注册状态 */
export interface HotkeyEntry {
  accel: string
  registered: boolean
}

/** 两个全局热键各自的注册状态 */
export interface HotkeyStatus {
  toggle: HotkeyEntry
  mini: HotkeyEntry
}

/** 一条术语：[源词, 译词] */
export type TermPair = [string, string]

export interface TranslateRequest {
  text: string
  target_lang: string
  mode: Mode
  terms: TermPair[]
  auto_filter_terms: boolean
  style: string
  format_type: string
  context: string
  ignore_glossary: boolean
}

export interface TranslateResponse {
  text: string
  prompt: string
  target_lang: string
  ms: number
  used_terms: string[]
  /** 入库后的历史记录 id（save_history 时才有） */
  history_id?: string
}

export interface HistoryItem {
  id: string
  created_at: number
  source: string
  translation: string
  source_lang: string
  target_lang: string
  backend_id: string
  mode: string
  starred: boolean
}

export interface GlossaryView {
  active: string
  names: string[]
  /** 与 names 对齐的各词库词条数 */
  counts: number[]
  terms: TermPair[]
}

export interface ExportPayload {
  name: string
  terms: TermPair[]
}

export interface DocSegment {
  index: number
  translatable: boolean
  kind: string
  text: string
}

export interface LoadedDocument {
  path: string
  encoding: string
  kind: string
  segments: DocSegment[]
  raw: string
  lines: number
}

export interface DocProgress {
  out_path: string
  done: number
  total: number
  failed: number
  errors: [number, string][]
  /** 因用户取消而提前结束；部分结果已保存到 out_path。 */
  cancelled: boolean
}

export interface BatchProgress {
  total: number
  done: number
  failed: number
  output: string
  cancelled: boolean
}

/** 文档/批量翻译的实时进度事件（`hymt2:job`）。 */
export interface JobProgress {
  job_id: string
  done: number
  total: number
  failed: number
  /** 任务收尾时的最后一条事件。 */
  finished?: boolean
  cancelled?: boolean
}

/** 网页抽取出的正文块。tag 用于前端排版（标题、段落、代码块等）。 */
export interface WebBlock {
  tag: string
  text: string
  /** 代码块等不翻译。 */
  translatable: boolean
}

export interface WebPage {
  url: string
  title: string
  blocks: WebBlock[]
  bytes: number
}

export interface Meta {
  languages: [string, string][]
  styles: string[]
  format_types: string[]
  modes: [string, string][]
}

export interface KeyStatus {
  backend_id: string
  has_key: boolean
}

/** 一条 AI 请求审计记录（对应 Rust 的 logstore::Entry） */
export interface LogEntry {
  seq: number
  at: number
  /** 发起入口：translate / translate_stream / detect_language */
  kind: string
  backend_id: string
  model: string
  url: string
  /** 发出去的请求体（已脱敏、长文本已截断） */
  request?: unknown
  /** 模型返回。非流式是原始响应体，流式是拼接后的完整文本 */
  response?: string
  /** 流式模式下的逐个分片 */
  deltas?: string[]
  ms: number
  error?: string
}

export interface LogStatus {
  enabled: boolean
  count: number
  /** 落盘文件路径 */
  path: string
  bytes: number
}
