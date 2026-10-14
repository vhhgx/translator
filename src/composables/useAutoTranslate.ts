/**
 * 输入即自动翻译（含增量翻译）——主翻译页和小窗共用同一套行为。
 *
 * 抽出来的原因：小窗原本是「按 Enter 才译」，现在要求与主窗口一致——
 * 停止输入 1 秒后自动翻译；长文只改了末尾时，只重译增量部分再拼接。
 * 两处各写一份必然走样，所以收在这里。
 */

import { onUnmounted, ref, watch } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { api, err, toast } from '../api'
import type { TranslateRequest } from '../types'

/** 流式跑太久没有任何收尾事件时，别让界面一直卡在「翻译中」。 */
const WATCHDOG_MS = 180000

export interface AutoTranslateOptions {
  /** 输入文本。传 ref 本身，函数内部按 `.value` 读写。 */
  source: { value: string }
  /** 构造请求；增量翻译时只替换其中的 text 字段 */
  buildRequest: () => TranslateRequest
  /** 触发自动翻译的静默毫秒数，默认 1000 */
  debounceMs?: number
  /** jobId 前缀，便于在日志里区分来源（小窗用 mini-、主窗口用 j-） */
  jobPrefix?: string
  /** 译文容器的 ref，用于流式滚动到底 */
  outEl?: { value: HTMLElement | null }
  /** 一轮翻译成功结束后回调（主窗口用它刷新导航徽标） */
  onDone?: () => void
}

export function useAutoTranslate(opts: AutoTranslateOptions) {
  const result = ref('')
  const busy = ref(false)
  const failed = ref('')
  const elapsed = ref(0)
  const usedTerms = ref<string[]>([])
  const historyId = ref('')
  const starred = ref(false)

  /** 增量翻译基线：上次译完时的原文与译文 */
  let lastTranslated = ''
  let lastResult = ''
  let debounceTimer: number | undefined
  let watchdogTimer: number | undefined
  let autoPending = false
  /** 本次翻译保留的前段译文。增量翻译时非空，全文翻译时为空。 */
  let keep = ''

  const unlisten: Array<() => void> = []
  let currentJob = ''

  function clearListeners() {
    unlisten.forEach((f) => f())
    unlisten.length = 0
    window.clearTimeout(watchdogTimer)
  }

  function newJobId(): string {
    const c = globalThis.crypto
    const uid =
      c && typeof c.randomUUID === 'function' ? c.randomUUID() : Math.random().toString(36).slice(2, 10)
    return `${opts.jobPrefix ?? 'j-'}${Date.now()}-${uid}`
  }

  /** 中止当前翻译（换语言、清空时用）。 */
  function abort() {
    clearListeners()
    currentJob = ''
    busy.value = false
    autoPending = false
    window.clearTimeout(debounceTimer)
  }

  /**
   * 把「这段文本刚译过」记为增量基线。
   *
   * 用于翻译结果不是本组件译出来的场合：历史复用、语言方向交换。不传
   * `translated` 表示译文对不上（只认原文，不认译文），下次一改动就老老实实
   * 全文重译——宁可多译一次，也不要拿不对应的旧译文去拼接。
   */
  function setBaseline(text: string, translated = '') {
    lastTranslated = text
    lastResult = translated
  }

  /**
   * 清空输入与译文（右上角叉号 / 原文删空 / 退格 共用这一条路径）。
   * 必须连译文一起清——否则原文删光后屏幕上还留着上一次的结果。
   */
  function clearAll() {
    abort()
    opts.source.value = ''
    result.value = ''
    failed.value = ''
    elapsed.value = 0
    usedTerms.value = []
    historyId.value = ''
    starred.value = false
    lastTranslated = ''
    lastResult = ''
    keep = ''
  }

  /**
   * 退格键：输入框已经空了还按退格时，把残留译文也清掉。
   * （有内容时退格删字符，走 watch → 自动翻译 → 删空时由 schedule 清译文）
   */
  function onBackspace() {
    if (!opts.source.value) clearAll()
  }

  /**
   * 发一次翻译请求并挂上流式监听。
   * @param text 实际送出的文本（增量时是新增的 delta）
   * @param keepPrefix 已有的前段译文，流式 delta 追加在它后面
   */
  async function start(text: string, keepPrefix = '') {
    busy.value = true
    failed.value = ''
    elapsed.value = 0
    usedTerms.value = []
    historyId.value = ''
    // 新的一轮对应新的历史记录，上一轮的收藏状态不能带过来
    starred.value = false
    keep = keepPrefix
    if (!keepPrefix) result.value = ''

    const jobId = newJobId()
    currentJob = jobId
    clearListeners()

    try {
      unlisten.push(
        await listen<{ job_id: string; text: string }>('hymt2:delta', (e) => {
          if (e.payload.job_id !== currentJob) return
          result.value += e.payload.text
          const el = opts.outEl?.value
          if (el) el.scrollTop = el.scrollHeight
        })
      )
      unlisten.push(
        await listen<{
          job_id: string
          text: string
          ms: number
          used_terms: string[]
          history_id?: string
        }>('hymt2:done', (e) => {
          if (e.payload.job_id !== currentJob) return
          result.value = keep + e.payload.text
          elapsed.value = e.payload.ms
          usedTerms.value = e.payload.used_terms ?? []
          historyId.value = e.payload.history_id ?? ''
          lastTranslated = opts.source.value
          lastResult = result.value
          busy.value = false
          opts.onDone?.()
        })
      )
      unlisten.push(
        await listen<{ job_id: string; message: string }>('hymt2:error', (e) => {
          if (e.payload.job_id !== currentJob) return
          busy.value = false
          failed.value = e.payload.message
          err(e.payload.message)
        })
      )

      await api.translateStream({ ...opts.buildRequest(), text }, jobId)

      watchdogTimer = window.setTimeout(() => {
        if (busy.value && currentJob === jobId) {
          busy.value = false
          failed.value = '翻译超时或未收到结果，请重试'
        }
      }, WATCHDOG_MS)
    } catch (e) {
      busy.value = false
      failed.value = String(e)
      err(e)
    }
  }

  /** 全文翻译。 */
  function translateNow() {
    if (!opts.source.value.trim()) return
    start(opts.source.value)
  }

  /**
   * 长文本只改了末尾一小段时，不重译全文：找「上次原文」与「当前原文」的
   * 最长公共前缀，前缀对应的译文直接保留，只把新增的尾部送出去翻译再拼接。
   *
   * 全部满足才走增量，否则老老实实全文重译：
   *   - 有过一次完整翻译
   *   - 公共前缀 ≥ 200 字符（改动太靠前时上下文变化大，拼接不可靠）
   *   - 新增部分 ≤ 全文 1/3（大改动直接全文）
   */
  function computeIncrement(
    oldText: string,
    newText: string,
    oldResult: string
  ): { delta: string; keptResult: string } | null {
    if (!oldText || !oldResult) return null
    // 先快速排除「开头就变了」的大改动
    if (!newText.startsWith(oldText.slice(0, Math.min(oldText.length, 60)))) return null

    let i = 0
    const min = Math.min(oldText.length, newText.length)
    while (i < min && oldText[i] === newText[i]) i++
    if (i < 200) return null

    const delta = newText.slice(i)
    if (!delta.trim()) return null // 纯删除：让结果跟着缩短，走全文
    if (delta.length > newText.length / 3) return null

    // 译文按「原文前缀占比」近似保留前段。原文与译文长度没有稳定比例，
    // 按比例截断可能断词，所以对齐到最近的句子边界。
    const ratio = i / oldText.length
    let cut = Math.round(oldResult.length * ratio)
    const boundaries = ['。', '！', '？', '.', '!', '?', '\n']
    for (let j = cut; j < Math.min(cut + 30, oldResult.length); j++) {
      if (boundaries.includes(oldResult[j])) {
        cut = j + 1
        break
      }
    }
    return { delta, keptResult: oldResult.slice(0, cut) }
  }

  /** 停止输入 debounce 之后：能增量就增量，否则全文。 */
  function schedule() {
    window.clearTimeout(debounceTimer)
    // 原文删空时译文必须一起清掉——否则屏幕上留着上一次的结果，
    // 而输入框已经空了，看起来像是「删不掉」。
    if (!opts.source.value.trim()) {
      if (result.value || busy.value) clearAll()
      return
    }
    debounceTimer = window.setTimeout(() => {
      const text = opts.source.value
      if (text.trim() === lastTranslated.trim()) return
      if (busy.value) {
        autoPending = true
        return
      }
      const inc = computeIncrement(lastTranslated, text, lastResult)
      if (inc) start(inc.delta, inc.keptResult)
      else start(text)
    }, opts.debounceMs ?? 1000)
  }

  /** 监听输入变化，自动翻译。返回的 cancel 用于需要时解除。 */
  function watchAuto() {
    const stopWatch = watch(
      () => opts.source.value,
      () => schedule()
    )
    // 这轮结束后如果还有挂起的输入，补跑一次
    const stopBusy = watch(busy, (isBusy) => {
      if (isBusy || !autoPending) return
      autoPending = false
      schedule()
    })
    onUnmounted(() => {
      stopWatch()
      stopBusy()
    })
  }

  async function toggleStar() {
    if (!historyId.value) return
    starred.value = !starred.value
    try {
      await api.historySetStarred(historyId.value, starred.value)
      toast(starred.value ? '已收藏这条译文' : '已取消收藏', 'success')
    } catch (e) {
      starred.value = !starred.value
      err(e)
    }
  }

  onUnmounted(() => {
    clearListeners()
    window.clearTimeout(debounceTimer)
  })

  return {
    result,
    busy,
    failed,
    elapsed,
    usedTerms,
    historyId,
    starred,
    abort,
    setBaseline,
    clearAll,
    onBackspace,
    translateNow,
    schedule,
    watchAuto,
    toggleStar
  }
}