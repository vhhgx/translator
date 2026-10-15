<script setup lang="ts">
import { onMounted, ref } from 'vue'
import type { Component } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { BookOpen, FileSearch, Info, Palette, RefreshCw, Settings as SettingsIcon, Trash2, X, Zap } from '@lucide/vue'
import { api, err, toast } from '../api'
import { activeBackend, saveSettings, settings, tab } from '../composables/useAppState'
import { setThemeMode, useTheme } from '../composables/useTheme'
import type { Backend, LogEntry, LogStatus } from '../types'

const { themeMode } = useTheme()

const testing = ref('')
const testResult = ref<Record<string, string>>({})
const keyInputs = ref<Record<string, string>>({})

// ---------- 侧栏分区 ----------
type Section = 'window' | 'engine' | 'logs' | 'appearance' | 'about'
const section = ref<Section>('window')

const SECTIONS: { key: Section; label: string; icon: Component }[] = [
  { key: 'window', label: '窗口唤起与热键', icon: Zap },
  { key: 'engine', label: '翻译引擎与模型', icon: SettingsIcon },
  { key: 'logs', label: '请求日志', icon: FileSearch },
  { key: 'appearance', label: '外观', icon: Palette },
  { key: 'about', label: '关于与更新', icon: Info }
]

// ---------- 托盘 / 热键 / 自启 ----------
const autostart = ref(false)

/** 两个全局热键：toggle = 主窗口显隐，mini = 小窗模式。 */
type Which = 'toggle' | 'mini'
const HOTKEY_ROWS: { key: Which; label: string; hint: string }[] = [
  { key: 'toggle', label: '主窗口显隐热键', hint: '按一次唤出主窗口，再按一次收起' },
  { key: 'mini', label: '小窗模式热键', hint: '唤出「输入 + 译文」小窗，不动主窗口' }
]

const hotkeyInputs = ref<Record<Which, string>>({ toggle: '', mini: '' })
const hotkeyStates = ref<Record<Which, { ok: boolean; message: string } | null>>({
  toggle: null,
  mini: null
})
/** 正在录制哪个热键；null = 没在录 */
const recording = ref<Which | null>(null)

/** 录制热键时用到的修饰键集合。 */
const MODS = ['Control', 'Alt', 'Shift', 'Meta']

/**
 * 会和全系统快捷键打架的组合，拒收。
 *
 * 全局热键是进程级的：注册了 `Ctrl+C`，那么**任何**软件里的复制都会
 * 被这个程序截走；而且按下时正好会触发热键自己的回调。这里的组合在
 * 编辑器和资源管理器里都有既定含义，误注册的代价太大。
 * 想用字母键就再加一层修饰，比如 `Ctrl+Shift+C` 是允许的。
 */
const RESERVED = new Set([
  'Ctrl+C',
  'Ctrl+V',
  'Ctrl+X',
  'Ctrl+A',
  'Ctrl+Z',
  'Ctrl+Y',
  'Ctrl+S',
  'Ctrl+O',
  'Ctrl+P',
  'Ctrl+F',
  'Ctrl+N',
  'Ctrl+W',
  'Ctrl+T',
  'Ctrl+Q',
  'Ctrl+R',
  'Alt+Tab'
])

function prettyHotkey(e: KeyboardEvent): string | null {
  const key = e.key
  if (['Control', 'Shift', 'Alt', 'Meta'].includes(key)) return null
  const mods = MODS.filter((m) => e.getModifierState(m))
  if (!mods.length) return null
  const names: Record<string, string> = {
    Control: 'Ctrl',
    Alt: 'Alt',
    Shift: 'Shift',
    Meta: 'Super'
  }
  return [...mods.map((m) => names[m]), key.length === 1 ? key.toUpperCase() : key].join('+')
}

function onRecordKey(e: KeyboardEvent) {
  const which = recording.value
  if (!which) return
  e.preventDefault()
  if (e.key === 'Escape') {
    recording.value = null
    return
  }
  const accel = prettyHotkey(e)
  if (!accel) return
  recording.value = null
  applyHotkey(which, accel)
}

/** 录制胶囊直接展示的键帽序列：Ctrl+Shift+S → [Ctrl, Shift, S] */
function hotkeyParts(accel: string): string[] {
  return accel ? accel.split('+') : []
}

/**
 * 改热键。成败不在这里判断——两个热键共用后端的一次 unregister_all，改一个
 * 会连另一个一起重注册，所以后端把两者的结果都用 `hymt2:hotkey` 事件回报。
 */
async function applyHotkey(which: Which, accel: string) {
  hotkeyInputs.value[which] = accel
  if (accel && RESERVED.has(accel)) {
    // 不往下发：一旦注册上去，这个程序会把所有软件的复制粘贴都截走。
    hotkeyStates.value[which] = { ok: false, message: `${accel} 是系统常用快捷键，换一个吧` }
    toast(`${accel} 和系统快捷键冲突，换一个`, 'error')
    return
  }
  try {
    await api.setHotkey(which, accel)
    if (which === 'toggle') settings.value.hotkey_toggle = accel
    else settings.value.hotkey_mini = accel
    toast(accel ? `热键已设为 ${accel}` : '已清除该热键', 'success')
  } catch (e) {
    hotkeyStates.value[which] = { ok: false, message: String(e) }
    err(e)
  }
}

function describeHotkey(accel: string, registered: boolean, label: string) {
  if (!accel) return { ok: true, message: `${label}未启用` }
  return registered
    ? { ok: true, message: `${label}已注册：${accel}` }
    : { ok: false, message: `${label}未生效：${accel}（可能被其他软件占用）` }
}

async function toggleAutostart(v: boolean) {
  try {
    await api.setAutostart(v)
    autostart.value = v
    toast(v ? '已设为开机自启' : '已取消开机自启', 'success')
  } catch (e) {
    // 失败时把开关弹回去，别让界面显示的状态和系统实际不符
    autostart.value = !v
    err(e)
  }
}

// ---------- 划词翻译 ----------
type SelStatus = {
  enabled: boolean
  started: boolean
  modifier: string
  error: string | null
}
const selStatus = ref<SelStatus | null>(null)

async function refreshSelStatus() {
  try {
    selStatus.value = await api.selectionStatus()
  } catch {
    // 浏览器预览没有 IPC
    selStatus.value = null
  }
}

async function toggleSelection(v: boolean) {
  settings.value.selection_enabled = v
  await saveSettings()
  // 监听线程是异步拉起的，稍等一下再查真实状态
  setTimeout(refreshSelStatus, 400)
}

onMounted(async () => {
  try {
    autostart.value = await api.getAutostart()
  } catch {
    autostart.value = false
  }
  // 启动时后端推的事件早于本组件挂载，必然收不到，所以主动查一次真实状态
  try {
    const st = await api.hotkeyStatus()
    hotkeyInputs.value = { toggle: st.toggle.accel, mini: st.mini.accel }
    hotkeyStates.value = {
      toggle: describeHotkey(st.toggle.accel, st.toggle.registered, '全局热键'),
      mini: describeHotkey(st.mini.accel, st.mini.registered, '小窗热键')
    }
  } catch {
    /* 浏览器预览没有 IPC，忽略即可 */
  }
  // 之后的换绑结果走事件实时更新，`which` 用来区分是哪一个热键
  listen<{ which: string; ok: boolean; message: string }>('hymt2:hotkey', (e) => {
    const which: Which = e.payload.which === 'mini' ? 'mini' : 'toggle'
    hotkeyStates.value[which] = { ok: e.payload.ok, message: e.payload.message }
  })

  refreshSelStatus()
})

/** 切到日志分区时才拉取，避免每次打开设置都读一遍日志。 */
function selectSection(key: Section) {
  section.value = key
  if (key === 'logs') loadLogs()
}

async function test(b: Backend) {
  testing.value = b.id
  testResult.value = { ...testResult.value, [b.id]: '' }
  try {
    const models = await api.testBackend(b)
    testResult.value = { ...testResult.value, [b.id]: `在线 · 模型 ${models.join(', ')}` }
  } catch (e) {
    testResult.value = { ...testResult.value, [b.id]: String(e).slice(0, 90) }
  } finally {
    testing.value = ''
  }
}

/**
 * 换 Key：填了才换。
 *
 * 留空**不等于**删除——输入框在已有 key 时也是空着的（不回显密钥），
 * 空这个状态本身有歧义。用户回到这张卡片看到空框，随手点一下「保存」就
 * 把正在用的凭据清掉了，下一次翻译才报 401，隔了半天才反应过来。
 * 所以「没打算改」就什么都不做，删除交给单独的按钮。
 */
async function saveKey(b: Backend) {
  const key = (keyInputs.value[b.id] ?? '').trim()
  if (!key) {
    toast(b.has_api_key ? '未输入新 Key，未做修改' : '还没有 Key 可保存')
    return
  }
  try {
    await api.setApiKey(b.id, key)
    keyInputs.value[b.id] = ''
    b.has_api_key = true
    await saveSettings()
    toast('API key 已保存到系统凭据库', 'success')
  } catch (e) {
    err(e)
  }
}

/** 删除 Key 是破坏性动作，必须显式触发，不跟「保存」共用一个空输入。 */
async function clearKey(b: Backend) {
  try {
    await api.setApiKey(b.id, '')
    keyInputs.value[b.id] = ''
    b.has_api_key = false
    await saveSettings()
    toast('API key 已删除', 'success')
  } catch (e) {
    err(e)
  }
}

function addBackend() {
  const n = settings.value.backends.length + 1
  settings.value.backends.push({
    id: `custom-${Date.now()}`,
    name: `自定义后端 ${n}`,
    base_url: 'http://',
    model: '',
    has_api_key: false
  })
  saveSettings()
}

async function removeBackend(b: Backend) {
  settings.value.backends = settings.value.backends.filter((x) => x.id !== b.id)
  await saveSettings()
  toast('已删除')
}

const APP_VERSION = '1.0.0'

// ---------- 请求日志 ----------
const logs = ref<LogEntry[]>([])
const logStatus = ref<LogStatus | null>(null)
const logLoading = ref(false)
/** 当前展开查看详情的记录序号；null = 全收起 */
const openSeq = ref<number | null>(null)

async function loadLogs() {
  logLoading.value = true
  try {
    const [entries, st] = await Promise.all([api.logList(200), api.logStatus()])
    logs.value = entries
    logStatus.value = st
  } catch (e) {
    err(e)
  } finally {
    logLoading.value = false
  }
}

async function toggleLogEnabled(v: boolean) {
  try {
    await api.logSetEnabled(v)
    if (!logStatus.value) await loadLogs()
    else logStatus.value = { ...logStatus.value, enabled: v }
    toast(v ? '已开启请求记录' : '已暂停请求记录', 'success')
  } catch (e) {
    err(e)
  }
}

async function clearLogs() {
  try {
    await api.logClear()
    logs.value = []
    openSeq.value = null
    await loadLogs()
    toast('日志已清空', 'success')
  } catch (e) {
    err(e)
  }
}

/** 把 JSON 拍成带缩进的字符串——用户明确要求可读，不要一长串。 */
function prettyJson(v: unknown): string {
  try {
    return JSON.stringify(v, null, 2)
  } catch {
    return String(v)
  }
}

/** 响应体是字符串形式的 JSON 时解析出来美化；不是就当纯文本。 */
function safeJson(s: string): unknown {
  try {
    return JSON.parse(s)
  } catch {
    return s
  }
}

function fmtTime(ms: number): string {
  const d = new Date(ms)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`
}

function fmtSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

/** 入口名友好化 */
function kindLabel(kind: string): string {
  return (
    {
      translate: '单次翻译',
      translate_stream: '流式翻译',
      detect_language: '语种检测'
    }[kind] ?? kind
  )
}

async function copyLog(e: LogEntry) {
  try {
    await api.copyText(prettyJson(e))
    toast('已复制该条记录', 'success')
  } catch (err2) {
    err(err2)
  }
}
</script>

<template>
  <div class="settings" :class="{ 'settings--recording': recording }" @keydown="onRecordKey">
    <!-- 左侧分区导航 -->
    <aside class="settings-sidebar">
      <div class="sidebar-title">偏好设置</div>
      <button
        v-for="s in SECTIONS"
        :key="s.key"
        class="sidebar-item"
        :class="{ 'sidebar-item--active': section === s.key }"
        @click="selectSection(s.key)"
      >
        <component :is="s.icon" :size="15" class="sidebar-item__icon" />
        <span>{{ s.label }}</span>
      </button>
      <button class="sidebar-item" @click="tab = 'glossary'">
        <BookOpen :size="15" class="sidebar-item__icon" />
        <span>术语库管理 ↗</span>
      </button>
    </aside>

    <!-- 右侧内容区 -->
    <div class="settings-content">
      <!-- ========== 分区 1：窗口唤起与热键 ========== -->
      <template v-if="section === 'window'">
        <div class="section-header">
          <h2>窗口唤起与系统集成</h2>
          <p>管理后台常驻行为、全局快捷键注册以及系统级划词监听策略</p>
        </div>

        <div class="settings-card">
          <div class="setting-row">
            <div class="setting-info">
              <div class="setting-title">关闭时最小化到系统托盘</div>
              <div class="setting-desc">关闭主窗口后进程保持后台常驻，确保全局热键随时可用；从托盘菜单「退出」才真正结束</div>
            </div>
            <label class="switch">
              <input
                v-model="settings.close_to_tray"
                type="checkbox"
                @change="saveSettings()"
              />
              <span class="slider" />
            </label>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <div class="setting-title">开机自动启动</div>
              <div class="setting-desc">注册到操作系统启动项，登录后静默常驻后台</div>
            </div>
            <label class="switch">
              <input
                type="checkbox"
                :checked="autostart"
                @change="toggleAutostart(($event.target as HTMLInputElement).checked)"
              />
              <span class="slider" />
            </label>
          </div>
        </div>

        <!-- 热键 -->
        <div class="settings-card">
          <div v-for="r in HOTKEY_ROWS" :key="r.key" class="setting-row">
            <div class="setting-info">
              <div class="setting-title">
                {{ r.label }}
                <span
                  v-if="hotkeyStates[r.key]?.ok && hotkeyInputs[r.key]"
                  class="badge-status"
                  >已注册</span
                >
                <span
                  v-else-if="hotkeyStates[r.key] && !hotkeyStates[r.key]?.ok"
                  class="badge-status badge-status--bad"
                  >未生效</span
                >
              </div>
              <div class="setting-desc">
                {{ recording === r.key ? '按下想要的组合键，Esc 取消' : r.hint }}
              </div>
            </div>
            <div class="hotkey-control">
              <button
                class="hotkey-recorder"
                :class="{ 'hotkey-recorder--recording': recording === r.key }"
                title="点击后直接按下新组合键录制"
                @click="recording = recording === r.key ? null : r.key"
              >
                <template v-if="recording === r.key">
                  <span class="recording-hint">请按下组合键…</span>
                </template>
                <template v-else-if="hotkeyParts(hotkeyInputs[r.key]).length">
                  <template v-for="(k, i) in hotkeyParts(hotkeyInputs[r.key])" :key="i">
                    <kbd class="kbd-key">{{ k }}</kbd>
                    <span v-if="i < hotkeyParts(hotkeyInputs[r.key]).length - 1" class="kbd-plus">+</span>
                  </template>
                </template>
                <span v-else class="recording-hint">未设置</span>
              </button>
              <button
                v-if="hotkeyInputs[r.key] && recording !== r.key"
                class="icon-btn"
                title="清除该热键"
                @click="applyHotkey(r.key, '')"
              >
                <X :size="13" />
              </button>
            </div>
          </div>
        </div>

        <!-- 划词翻译 -->
        <div class="settings-card">
          <div class="setting-row">
            <div class="setting-info">
              <div class="setting-title">
                划词翻译（选中后自动弹窗）
                <span v-if="selStatus?.enabled && !selStatus.error" class="badge-status">
                  <span class="badge-dot" /> 监听运行中
                </span>
              </div>
              <div class="setting-desc">
                监听全局鼠标，选中文本即在光标处弹出译文。仅 Windows 与 Linux(X11) 可用；Wayland 下无法模拟按键，会启动失败
              </div>
            </div>
            <label class="switch">
              <input
                type="checkbox"
                :checked="settings.selection_enabled"
                @change="toggleSelection(($event.target as HTMLInputElement).checked)"
              />
              <span class="slider" />
            </label>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <div class="setting-title">划词触发修饰键</div>
              <div class="setting-desc">
                抓取要借剪贴板中转，期间剪贴板会被短暂替换。建议改成按住 Ctrl，用拖选 + Ctrl 才触发。
              </div>
            </div>
            <select
              v-model="settings.selection_modifier"
              class="custom-select"
              @change="saveSettings()"
            >
              <option value="">不要求</option>
              <option value="ctrl">按住 Ctrl + 拖选（推荐）</option>
              <option value="alt">按住 Alt + 拖选</option>
              <option value="shift">按住 Shift + 拖选</option>
            </select>
          </div>

          <p v-if="selStatus?.error" class="state-msg state-msg--bad">
            划词监听启动失败：{{ selStatus.error }}（Wayland 下属预期）
          </p>
        </div>
      </template>

      <!-- ========== 分区 2：翻译引擎与模型 ========== -->
      <template v-else-if="section === 'engine'">
        <div class="section-header">
          <h2>翻译引擎与模型</h2>
          <p>配置 OpenAI 兼容后端、模型参数与凭据；可随时在主界面底栏切换</p>
        </div>

        <div class="settings-card">
          <div class="setting-row">
            <div class="setting-info">
              <div class="setting-title">采样温度</div>
              <div class="setting-desc">翻译任务需要稳定输出，默认 0.1（官方推荐值）</div>
            </div>
            <input
              v-model.number="settings.temperature"
              class="num-input"
              type="number"
              min="0"
              max="1"
              step="0.1"
              @change="saveSettings()"
            />
          </div>
          <div class="setting-row">
            <div class="setting-info">
              <div class="setting-title">请求超时（秒）</div>
              <div class="setting-desc">单次翻译请求的最长等待时间</div>
            </div>
            <input
              v-model.number="settings.timeout_secs"
              class="num-input"
              type="number"
              min="30"
              step="30"
              @change="saveSettings()"
            />
          </div>
        </div>

        <div v-for="b in settings.backends" :key="b.id" class="settings-card backend" :class="{ 'backend--active': b.id === settings.active_id }">
          <!-- 头部：左=名称+状态标签，右=操作（测试为强调钮，删除为轻量图标钮） -->
          <div class="backend__head">
            <input v-model="b.name" class="backend__name" @change="saveSettings()" />
            <span v-if="b.id === settings.active_id" class="backend__live-tag">
              <span class="badge-dot" /> 当前使用
            </span>
            <span class="grow" />
            <button class="btn btn--primary btn--sm" :disabled="testing === b.id" @click="test(b)">
              {{ testing === b.id ? '测试中…' : '测试连接' }}
            </button>
            <button class="icon-btn icon-btn--danger" title="删除该后端" @click="removeBackend(b)">
              <Trash2 :size="14" />
            </button>
          </div>

          <!-- 字段区：两栏 + API Key 行，间距统一走卡片内部节奏 -->
          <div class="backend__body">
            <div class="backend__row">
              <div class="backend__field backend__field--url">
                <label class="backend__label">
                  Base URL
                  <!-- 厂商文档常把三种协议的完整路径并排列出，很容易抄错一栏 -->
                  <span class="backend__key-tag">只填到域名，不带 /v1/...</span>
                </label>
                <input
                  v-model="b.base_url"
                  class="input"
                  placeholder="https://api.example.com"
                  @change="saveSettings()"
                />
              </div>
              <div class="backend__field">
                <label class="backend__label">模型名</label>
                <input v-model="b.model" class="input" placeholder="模型名，如 gpt-4o-mini" @change="saveSettings()" />
              </div>
            </div>

            <div class="backend__field">
              <label class="backend__label">
                API Key
                <span v-if="b.has_api_key" class="backend__key-tag backend__key-tag--saved">已保存到系统凭据库</span>
                <span v-else class="backend__key-tag">未设置（本地服务通常不需要）</span>
              </label>
              <div class="backend__key">
                <input
                  v-model="keyInputs[b.id]"
                  class="input"
                  type="password"
                  :placeholder="b.has_api_key ? '已保存，留空表示不修改' : '粘贴 API Key'"
                />
                <button class="btn btn--sm" @click="saveKey(b)">保存 Key</button>
                <button
                  v-if="b.has_api_key"
                  class="btn btn--sm backend__key-del"
                  title="从系统凭据库中删除这把 Key"
                  @click="clearKey(b)"
                >
                  删除
                </button>
              </div>
            </div>
          </div>

          <p v-if="testResult[b.id]" class="backend__result">{{ testResult[b.id] }}</p>
        </div>

        <button class="btn btn--ghost backend__add" @click="addBackend">+ 添加后端</button>

        <div class="settings-card note">
          <div class="setting-title">说明</div>
          <ul>
            <li>后端需提供 OpenAI 兼容接口（<code>/v1/chat/completions</code>）。</li>
            <li>任意 OpenAI 兼容服务均可：云端 API（DeepSeek、OpenAI、腾讯混元等）、本地 llama.cpp、Ollama、vLLM 或自建网关。</li>
            <li>Base URL 可填根地址、<code>/v1</code> 或完整 <code>/v1/chat/completions</code>，会自动归一。</li>
            <li>API key 通过系统凭据管理器存储，不写入配置文件。</li>
            <li v-if="activeBackend">
              当前后端：<strong>{{ activeBackend.name }}</strong> — {{ activeBackend.base_url }}
            </li>
          </ul>
        </div>
      </template>

      <!-- ========== 分区 3：外观 ========== -->
      <template v-else-if="section === 'appearance'">
        <div class="section-header">
          <h2>外观</h2>
          <p>深浅主题跟随系统或手动固定；三个窗口（主窗口 / 划词弹窗 / 小窗）同步切换</p>
        </div>

        <div class="settings-card">
          <div class="setting-row">
            <div class="setting-info">
              <div class="setting-title">主题模式</div>
              <div class="setting-desc">选择「跟随系统」时，切换系统深浅色会实时生效</div>
            </div>
            <div class="theme-picker">
              <button
                v-for="m in ['auto', 'light', 'dark']"
                :key="m"
                class="theme-pill"
                :class="{ 'theme-pill--active': themeMode === m }"
                @click="setThemeMode(m as 'auto' | 'light' | 'dark')"
              >
                {{ m === 'auto' ? '跟随系统' : m === 'light' ? '☀️ 浅色' : '🌙 深色' }}
              </button>
            </div>
          </div>
        </div>
      </template>

      <!-- ========== 分区 4：请求日志 ========== -->
      <template v-else-if="section === 'logs'">
        <div class="section-header">
          <h2>AI 请求日志</h2>
          <p>记录每次发给模型的完整请求（prompt）与模型返回的原始内容，用于核对与排障。API Key 已脱敏，仅保留前 4 位</p>
        </div>

        <div class="settings-card">
          <div class="setting-row">
            <div class="setting-info">
              <div class="setting-title">
                记录请求与返回
                <span v-if="logStatus" class="badge-status" :class="{ 'badge-status--bad': !logStatus.enabled }">
                  {{ logStatus.enabled ? '记录中' : '已暂停' }}
                </span>
              </div>
              <div class="setting-desc">
                关闭后不再写入新记录，已有日志保留。日志只落本机，路径见下方
              </div>
            </div>
            <label class="switch">
              <input
                type="checkbox"
                :checked="logStatus?.enabled ?? true"
                @change="toggleLogEnabled(($event.target as HTMLInputElement).checked)"
              />
              <span class="slider" />
            </label>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <div class="setting-title">存储</div>
              <div class="setting-desc">
                内存保留最近 {{ logStatus?.count ?? 0 }} 条（上限 500）；落盘 {{ fmtSize(logStatus?.bytes ?? 0) }}
                <div class="about-mono log-path">{{ logStatus?.path || '—' }}</div>
              </div>
            </div>
            <div class="log-actions">
              <button class="btn btn--sm" :disabled="logLoading" @click="loadLogs">
                <RefreshCw :size="13" /> 刷新
              </button>
              <button class="btn btn--sm btn--danger" @click="clearLogs">
                <Trash2 :size="13" /> 清空
              </button>
            </div>
          </div>
        </div>

        <div v-if="!logs.length" class="settings-card log-empty">
          {{ logLoading ? '读取中…' : '还没有记录。去翻译一次，这里就会出现完整的请求与返回。' }}
        </div>

        <div v-for="e in logs" :key="e.seq" class="settings-card log-entry">
          <button class="log-head" @click="openSeq = openSeq === e.seq ? null : e.seq">
            <span class="log-kind">{{ kindLabel(e.kind) }}</span>
            <span class="log-model">{{ e.model || '—' }}</span>
            <span v-if="e.error" class="badge-status badge-status--bad">失败</span>
            <span v-else class="badge-status">成功</span>
            <span class="log-spacer" />
            <span class="log-ms">{{ e.ms }} ms</span>
            <span class="log-time">{{ fmtTime(e.at) }}</span>
            <span class="log-caret" :class="{ 'log-caret--open': openSeq === e.seq }">›</span>
          </button>

          <div v-if="openSeq === e.seq" class="log-body">
            <div class="log-meta">
              <span class="about-label">端点</span>
              <span class="about-mono">{{ e.url }}</span>
            </div>
            <div class="log-meta">
              <span class="about-label">后端</span>
              <span class="about-mono">{{ e.backend_id }}</span>
            </div>

            <div v-if="e.request" class="log-block">
              <div class="log-block__title">
                <span>发送给 AI</span>
                <button class="btn btn--sm btn--ghost" @click="copyLog(e)">复制整条</button>
              </div>
              <pre class="log-pre">{{ prettyJson(e.request) }}</pre>
            </div>

            <div v-if="e.error" class="log-block">
              <div class="log-block__title"><span>错误</span></div>
              <pre class="log-pre log-pre--err">{{ e.error }}</pre>
            </div>

            <div v-if="e.response" class="log-block">
              <div class="log-block__title">
                <span>AI 返回{{ e.kind === 'translate_stream' ? '（完整文本）' : '（原始响应体）' }}</span>
              </div>
              <pre class="log-pre">{{ e.kind === 'translate_stream' ? e.response : prettyJson(safeJson(e.response)) }}</pre>
            </div>

            <div v-if="e.deltas?.length" class="log-block">
              <div class="log-block__title">
                <span>流式分片（{{ e.deltas.length }} 段）</span>
              </div>
              <div class="log-deltas">
                <span v-for="(d, i) in e.deltas" :key="i" class="log-delta">{{ d }}</span>
              </div>
            </div>
          </div>
        </div>
      </template>

      <!-- ========== 分区 5：关于 ========== -->
      <template v-else-if="section === 'about'">
        <div class="section-header">
          <h2>关于与更新</h2>
          <p>Zephyr Translator — 可接入任意 OpenAI 兼容端点的跨端桌面翻译工具</p>
        </div>

        <div class="settings-card about-grid">
          <div class="about-item">
            <span class="about-label">版本</span>
            <span>v{{ APP_VERSION }}</span>
          </div>
          <div class="about-item">
            <span class="about-label">当前引擎</span>
            <span>{{ activeBackend ? `${activeBackend.name}（${activeBackend.model || '未指定模型'}）` : '未配置' }}</span>
          </div>
          <div class="about-item">
            <span class="about-label">配置目录</span>
            <span class="about-mono">%APPDATA%/zephyr-translator</span>
          </div>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.settings {
  display: grid;
  grid-template-columns: 200px 1fr;
  height: 100%;
  min-height: 0;
  background: var(--surface-secondary);
  overflow: hidden;
}

/* ---------- 侧栏 ---------- */
.settings-sidebar {
  background: var(--surface-primary);
  border-right: 1px solid var(--border-subtle);
  padding: 16px 10px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  overflow-y: auto;
}

.sidebar-title {
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--text-muted);
  padding: 4px 10px 10px;
}

.sidebar-item {
  padding: 8px 12px;
  border-radius: 8px;
  font-family: inherit;
  font-size: 13px;
  color: var(--text-secondary);
  background: transparent;
  border: none;
  text-align: left;
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 8px;
  transition: all 0.15s;
}

.sidebar-item__icon {
  width: 16px;
  text-align: center;
  flex-shrink: 0;
}

.sidebar-item:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.sidebar-item--active {
  background: var(--surface-active);
  color: var(--accent-primary);
  font-weight: 500;
}

/* ---------- 内容区 ---------- */
.settings-content {
  padding: 24px 32px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 16px;
  /* 高度不够时滚动而不是压缩卡片——压缩会把卡片下半行裁掉 */
  align-content: start;
}

.settings-content > * {
  flex-shrink: 0;
}

.section-header h2 {
  margin: 0 0 4px;
  font-size: 16px;
  font-weight: 600;
}

.section-header p {
  margin: 0;
  font-size: 12.5px;
  color: var(--text-secondary);
}

.settings-card {
  background: var(--surface-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 12px;
  overflow: hidden;
}

.setting-row {
  padding: 14px 20px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 24px;
  border-bottom: 1px solid var(--border-subtle);
}

.setting-row:last-child {
  border-bottom: none;
}

.setting-info {
  flex: 1;
  min-width: 0;
}

.setting-title {
  font-size: 13.5px;
  font-weight: 500;
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 4px;
}

.setting-desc {
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.5;
}

.badge-status {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  font-weight: 500;
  padding: 1px 7px;
  border-radius: var(--r-pill);
  background: color-mix(in srgb, var(--status-online) 12%, transparent);
  color: var(--status-online);
  border: 1px solid color-mix(in srgb, var(--status-online) 25%, transparent);
}

.badge-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: currentColor;
  box-shadow: 0 0 5px currentColor;
}

.badge-status--bad {
  background: var(--status-danger-bg);
  color: var(--status-danger);
  border-color: color-mix(in srgb, var(--status-danger) 25%, transparent);
}

/* CSS 开关（原型同款） */
.switch {
  position: relative;
  display: inline-block;
  width: 40px;
  height: 22px;
  flex-shrink: 0;
  cursor: pointer;
}

.switch input {
  opacity: 0;
  width: 0;
  height: 0;
}

.slider {
  position: absolute;
  cursor: pointer;
  inset: 0;
  background-color: var(--surface-tertiary);
  border: 1px solid var(--border-subtle);
  transition: 0.2s;
  border-radius: var(--r-pill);
}

.slider:before {
  position: absolute;
  content: '';
  height: 16px;
  width: 16px;
  left: 2px;
  bottom: 2px;
  background-color: var(--text-secondary);
  transition: 0.2s;
  border-radius: 50%;
}

.switch input:checked + .slider {
  background-color: var(--accent-primary);
  border-color: var(--accent-primary);
}

.switch input:checked + .slider:before {
  transform: translateX(18px);
  background-color: #ffffff;
}

/* 热键录制器（键帽样式） */
.hotkey-control {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}

.hotkey-recorder {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 5px 12px;
  border-radius: 8px;
  background: var(--surface-secondary);
  border: 1px solid var(--border-subtle);
  cursor: pointer;
  transition: all 0.15s;
  min-width: 130px;
  justify-content: center;
  font-family: inherit;
}

.hotkey-recorder:hover {
  border-color: var(--border-focus);
  background: var(--surface-hover);
}

.hotkey-recorder--recording {
  border-color: var(--accent-primary);
  box-shadow: 0 0 0 3px var(--accent-glow);
  background: var(--surface-active);
}

.recording-hint {
  font-size: 12px;
  color: var(--accent-primary);
}

.kbd-plus {
  font-size: 11px;
  color: var(--text-muted);
}

.icon-btn {
  width: 26px;
  height: 26px;
  border-radius: 6px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: all 0.15s;
  font-size: 12px;
}

.icon-btn:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.custom-select {
  appearance: none;
  background: var(--surface-secondary);
  border: 1px solid var(--border-subtle);
  color: var(--text-primary);
  padding: 7px 30px 7px 12px;
  border-radius: 8px;
  font-family: inherit;
  font-size: 12.5px;
  cursor: pointer;
  outline: none;
  flex-shrink: 0;
}

.num-input {
  width: 90px;
  padding: 7px 10px;
  border-radius: 8px;
  background: var(--surface-secondary);
  border: 1px solid var(--border-subtle);
  color: var(--text-primary);
  font-family: inherit;
  font-size: 13px;
  outline: none;
  text-align: center;
  flex-shrink: 0;
}

.num-input:focus {
  border-color: var(--border-focus);
}

.state-msg {
  margin: 0;
  padding: 10px 20px;
  font-size: 12px;
}

.state-msg--bad {
  color: var(--status-danger);
  background: var(--status-danger-bg);
}

/* ---------- 引擎分区 ---------- */
.grow {
  flex: 1;
}

.backend--active {
  border-color: var(--border-focus);
  box-shadow: 0 0 0 1px var(--border-focus);
}

/* 头部：名称与状态居左，操作居右，统一基线 */
.backend__head {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-subtle);
}

.backend__name {
  flex: 0 1 260px;
  min-width: 0;
  padding: 6px 10px;
  border-radius: 6px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-primary);
  font-family: inherit;
  font-size: 14px;
  font-weight: 600;
  outline: none;
  transition: all 0.15s;
}

.backend__name:hover {
  border-color: var(--border-subtle);
}

.backend__name:focus {
  border-color: var(--border-focus);
  background: var(--surface-secondary);
}

/* 「当前使用」状态标签：绿色呼吸点 + 文字 */
.backend__live-tag {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  font-weight: 500;
  padding: 2px 8px;
  border-radius: var(--r-pill);
  background: color-mix(in srgb, var(--status-online) 10%, transparent);
  color: var(--status-online);
  white-space: nowrap;
}

/* 字段区：卡片内统一 16px/12px 节奏，不再借 base 的 .field 边距 */
.backend__body {
  padding: 16px 20px 18px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.backend__row {
  display: flex;
  gap: 12px;
}

.backend__field {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 5px;
}

.backend__field--url {
  flex: 1.8;
}

.backend__label {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  font-weight: 500;
  color: var(--text-secondary);
}

.backend__key-tag {
  font-size: 11px;
  font-weight: 400;
  padding: 1px 7px;
  border-radius: var(--r-pill);
  background: var(--surface-tertiary);
  color: var(--text-muted);
  border: 1px solid var(--border-subtle);
}

.backend__key-tag--saved {
  background: color-mix(in srgb, var(--status-online) 10%, transparent);
  border-color: color-mix(in srgb, var(--status-online) 25%, transparent);
  color: var(--status-online);
}

.backend__key {
  display: flex;
  gap: 8px;
}

/* 删除 Key：默认低调，hover 才示警——破坏性动作不该跟「保存」一样显眼 */
.backend__key-del:hover {
  color: var(--status-danger);
  border-color: color-mix(in srgb, var(--status-danger) 45%, transparent);
  background: var(--status-danger-bg);
}

.backend__result {
  margin: 0;
  padding: 10px 20px 14px;
  font-size: 12px;
  font-family: var(--font-mono);
  color: var(--text-secondary);
  word-break: break-all;
  background: var(--surface-secondary);
  border-top: 1px solid var(--border-subtle);
}

/* 危险图标钮：轻量，hover 才显红 */
.icon-btn--danger:hover {
  color: var(--status-danger);
  background: var(--status-danger-bg);
}

.backend__add {
  align-self: flex-start;
}

.note ul {
  margin: 8px 0 0;
  padding: 0 20px 14px;
  font-size: 12.5px;
  color: var(--text-secondary);
  line-height: 1.9;
}

.note {
  padding-top: 14px;
}

.note .setting-title {
  padding: 0 20px;
}

.note code {
  padding: 1px 5px;
  font-family: var(--font-mono);
  font-size: 12px;
  background: var(--surface-hover);
  border-radius: var(--r-sm);
}

/* ---------- 外观分区 ---------- */
.theme-picker {
  display: flex;
  gap: 4px;
  background: var(--surface-tertiary);
  padding: 3px;
  border-radius: 8px;
  border: 1px solid var(--border-subtle);
  flex-shrink: 0;
}

.theme-pill {
  padding: 5px 12px;
  border-radius: 6px;
  font-family: inherit;
  font-size: 12px;
  font-weight: 500;
  color: var(--text-secondary);
  background: transparent;
  border: none;
  cursor: pointer;
  transition: all 0.15s;
  white-space: nowrap;
}

.theme-pill--active {
  background: var(--surface-primary);
  color: var(--accent-primary);
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.1);
}

/* ---------- 关于分区 ---------- */
.about-grid {
  padding: 6px 20px;
}

.about-item {
  display: flex;
  align-items: baseline;
  gap: 16px;
  padding: 10px 0;
  border-bottom: 1px solid var(--border-subtle);
  font-size: 13px;
  color: var(--text-primary);
}

.about-item:last-child {
  border-bottom: none;
}

.about-label {
  width: 90px;
  flex-shrink: 0;
  font-size: 12px;
  color: var(--text-muted);
}

.about-mono {
  font-family: var(--font-mono);
  font-size: 12px;
}

/* 录制热键时吃掉输入框焦点，避免按键被输入框消费 */
.settings--recording {
  user-select: none;
}

/* ---------- 请求日志分区 ---------- */
.log-actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}

.log-path {
  margin-top: 4px;
  color: var(--text-muted);
  word-break: break-all;
}

.log-empty {
  padding: 24px;
  text-align: center;
  font-size: 13px;
  color: var(--text-muted);
}

.log-entry {
  padding: 0;
  overflow: hidden;
}

.log-head {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 12px 16px;
  background: transparent;
  border: none;
  cursor: pointer;
  font-family: inherit;
  font-size: 12.5px;
  color: var(--text-secondary);
  text-align: left;
}

.log-head:hover {
  background: var(--surface-hover);
}

.log-kind {
  font-weight: 600;
  color: var(--text-primary);
  flex-shrink: 0;
}

.log-model {
  font-family: var(--font-mono);
  color: var(--text-muted);
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.log-spacer {
  flex: 1;
}

.log-ms,
.log-time {
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text-muted);
  flex-shrink: 0;
}

.log-caret {
  flex-shrink: 0;
  font-size: 16px;
  line-height: 1;
  color: var(--text-muted);
  transition: transform 0.15s;
  transform: rotate(90deg);
}

.log-caret--open {
  transform: rotate(-90deg);
}

.log-body {
  padding: 4px 16px 16px;
  border-top: 1px solid var(--border-subtle);
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.log-meta {
  display: flex;
  align-items: baseline;
  gap: 12px;
  font-size: 12px;
  color: var(--text-secondary);
}

.log-block__title {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
  margin-bottom: 6px;
}

.log-pre {
  margin: 0;
  padding: 12px;
  background: var(--surface-secondary);
  border: 1px solid var(--border-subtle);
  border-radius: 8px;
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.55;
  color: var(--text-primary);
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 360px;
  overflow: auto;
}

.log-pre--err {
  color: var(--danger, #e5484d);
}

.log-deltas {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  padding: 10px;
  background: var(--surface-secondary);
  border: 1px solid var(--border-subtle);
  border-radius: 8px;
  max-height: 200px;
  overflow: auto;
}

.log-delta {
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text-secondary);
  background: var(--surface-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 4px;
  padding: 1px 5px;
  white-space: pre-wrap;
  word-break: break-word;
}
</style>
