/**
 * 文档 / 批量翻译的实时进度。
 *
 * 后端在 `hymt2:job` 事件里报 `{ job_id, done, total, failed, finished }`，
 * 按 `job_id` 过滤 —— 两个页签各自持有自己的 jobId，互不串台。
 *
 * 事件按时间节流（后端每 120ms 一条），但收尾那条一定会发，所以
 * `finished` 之后就能安全地把进度条收起来。
 */
import { onUnmounted, ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import type { JobProgress } from '../types'

export function useJobProgress() {
  const jobId = ref('')
  const done = ref(0)
  const total = ref(0)
  const failed = ref(0)

  /** 后端收尾事件到达。取消时也走这里，前端据此收掉进度条。 */
  const finished = ref(false)

  const unlisten: Array<() => void> = []
  let ready: Promise<void> | null = null

  /** 开跑前调用：换一个新 jobId 并清零进度。 */
  async function begin(totalHint = 0): Promise<string> {
    done.value = 0
    failed.value = 0
    finished.value = false
    total.value = totalHint
    jobId.value = `job-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
    await ensureListening()
    return jobId.value
  }

  async function ensureListening() {
    if (ready) return ready
    ready = (async () => {
      try {
        unlisten.push(
          await listen<JobProgress>('hymt2:job', (e) => {
            if (e.payload.job_id !== jobId.value) return
            done.value = e.payload.done
            total.value = e.payload.total
            failed.value = e.payload.failed
            if (e.payload.finished) finished.value = true
          })
        )
      } catch {
        /* 浏览器预览没有 IPC */
      }
    })()
    return ready
  }

  onUnmounted(() => unlisten.forEach((f) => f()))

  const percent = () => (total.value > 0 ? Math.min(100, (done.value / total.value) * 100) : 0)

  return { jobId, done, total, failed, finished, begin, ensureListening, percent }
}
