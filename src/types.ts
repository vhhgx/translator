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
}

export interface GlossaryView {
  active: string
  names: string[]
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
}

export interface BatchProgress {
  total: number
  done: number
  failed: number
  output: string
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