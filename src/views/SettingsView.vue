<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { api, err, toast } from '../api'
import { activeBackend, saveSettings, settings } from '../composables/useAppState'
import type { Backend } from '../types'

const testing = ref('')
const testResult = ref<Record<string, string>>({})
const keyInputs = ref<Record<string, string>>({})

// ---------- 托盘 / 热键 / 自启 ----------
const autostart = ref(false)
const hotkeyInput = ref('')
const hotkeyState = ref<{ ok: boolean; message: string } | null>(null)
const recording = ref(false)

/** 录制热键时用到的修饰键集合。 */
const MODS = ['Control', 'Alt', 'Shift', 'Meta']

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
  if (!recording.value) return
  e.preventDefault()
  if (e.key === 'Escape') {
    recording.value = false
    return
  }
  const accel = prettyHotkey(e)
  if (!accel) return
  recording.value = false
  applyHotkey(accel)
}

async function applyHotkey(accel: string) {
  hotkeyInput.value = accel
  try {
    await api.setHotkey(accel)
    settings.value.hotkey_toggle = accel
    await saveSettings()
    hotkeyState.value = { ok: true, message: `全局热键已注册：${accel}` }
    toast(`全局热键已设为 ${accel}`, 'success')
  } catch (e) {
    // 注册失败（多半被别的软件占用）必须明确告知，否则用户无从排查
    hotkeyState.value = { ok: false, message: String(e) }
    err(e)
  }
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

onMounted(async () => {
  hotkeyInput.value = settings.value.hotkey_toggle
  try {
    autostart.value = await api.getAutostart()
  } catch {
    autostart.value = false
  }
  // 启动时后端推的事件早于本组件挂载，必然收不到，所以主动查一次真实状态
  try {
    const st = await api.hotkeyStatus()
    hotkeyInput.value = st.accel
    hotkeyState.value = st.accel
      ? {
          ok: st.registered,
          message: st.registered
            ? `全局热键已注册：${st.accel}`
            : `全局热键未生效：${st.accel}（可能被其他软件占用）`
        }
      : { ok: true, message: '未启用全局热键' }
  } catch {
    /* 浏览器预览没有 IPC，忽略即可 */
  }
  // 之后的换绑结果走事件实时更新
  listen<{ ok: boolean; message: string }>('hymt2:hotkey', (e) => {
    hotkeyState.value = e.payload
  })
})

async function test(b: Backend) {
  testing.value = b.id
  testResult.value = { ...testResult.value, [b.id]: '' }
  try {
    const models = await api.testBackend(b)
    testResult.value = { ...testResult.value, [b.id]: `✓ 在线 · 模型 ${models.join(', ')}` }
  } catch (e) {
    testResult.value = { ...testResult.value, [b.id]: `✗ ${String(e).slice(0, 90)}` }
  } finally {
    testing.value = ''
  }
}

async function saveKey(b: Backend) {
  try {
    const key = keyInputs.value[b.id] ?? ''
    await api.setApiKey(b.id, key)
    keyInputs.value[b.id] = ''
    b.has_api_key = !b.has_api_key || !!key
    await saveSettings()
    toast(key ? 'API key 已保存到系统凭据库' : 'API key 已清除', 'success')
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
</script>

<template>
  <div class="settings" :class="{ 'settings--recording': recording }" @keydown="onRecordKey">
    <!-- 托盘与全局热键 -->
    <section class="card tray-card">
      <h3 class="card__title">窗口唤起与系统集成</h3>

      <label class="row">
        <span class="row__label">
          关闭时最小化到托盘
          <em class="row__hint">关窗后进程仍在，全局热键才会继续有效；从托盘菜单「退出」才真正结束</em>
        </span>
        <input
          v-model="settings.close_to_tray"
          class="switch"
          type="checkbox"
          @change="saveSettings()"
        />
      </label>

      <label class="row">
        <span class="row__label">
          开机自动启动
          <em class="row__hint">注册到系统启动项，后台常驻以便随时唤起</em>
        </span>
        <input
          class="switch"
          type="checkbox"
          :checked="autostart"
          @change="toggleAutostart(($event.target as HTMLInputElement).checked)"
        />
      </label>

      <div class="row">
        <span class="row__label">
          全局热键（显示/隐藏窗口）
          <em class="row__hint">
            {{ recording ? '按下想要的组合键，Esc 取消' : '点击「录制」后按下组合键' }}
          </em>
        </span>
        <div class="hotkey">
          <input v-model="hotkeyInput" class="input input--sm hotkey__input" readonly />
          <button class="btn btn--sm" @click="recording = !recording">
            {{ recording ? '取消' : '录制' }}
          </button>
          <button class="btn btn--sm" @click="applyHotkey(hotkeyInput.trim())">应用</button>
          <button
            class="btn btn--sm"
            title="取消全局热键"
            @click="applyHotkey('')"
          >
            清除
          </button>
        </div>
      </div>

      <p v-if="hotkeyState" class="hotkey__state" :class="hotkeyState.ok ? 'ok' : 'bad'">
        {{ hotkeyState.ok ? '✓' : '✗' }} {{ hotkeyState.message }}
      </p>
      <p class="tray-card__note">
        托盘图标：左键单击直接唤出窗口，右键菜单提供「显示窗口 / 退出」。
      </p>
    </section>

    <div class="settings__head">
      <h2>后端配置</h2>
      <div class="settings__global">
        <label class="inline">
          温度
          <input v-model.number="settings.temperature" class="input input--sm" type="number" min="0" max="1" step="0.1" @change="saveSettings()" />
        </label>
        <label class="inline">
          超时(秒)
          <input v-model.number="settings.timeout_secs" class="input input--sm" type="number" min="30" step="30" @change="saveSettings()" />
        </label>
        <button class="btn btn--sm" @click="saveSettings()">保存全部</button>
      </div>
    </div>

    <div class="backends">
      <div v-for="b in settings.backends" :key="b.id" class="card backend" :class="{ 'backend--active': b.id === settings.active_id }">
        <div class="backend__head">
          <input v-model="b.name" class="input backend__name" @change="saveSettings()" />
          <span v-if="b.id === settings.active_id" class="tag tag--success">当前使用</span>
          <button class="btn btn--sm" :disabled="testing === b.id" @click="test(b)">
            {{ testing === b.id ? '测试中…' : '测试连接' }}
          </button>
          <button class="btn btn--sm btn--danger" @click="removeBackend(b)">删除</button>
        </div>

        <div class="backend__row">
          <div class="field backend__url">
            <label class="label">Base URL</label>
            <input v-model="b.base_url" class="input" placeholder="http://127.0.0.1:8080" @change="saveSettings()" />
          </div>
          <div class="field backend__model">
            <label class="label">模型名</label>
            <input v-model="b.model" class="input" placeholder="hy-mt2" @change="saveSettings()" />
          </div>
        </div>

        <div class="field">
          <label class="label">
            API Key
            <span v-if="b.has_api_key" class="tag tag--success">已保存到系统凭据库</span>
            <span v-else class="tag">未设置（本地服务通常不需要）</span>
          </label>
          <div class="backend__key">
            <input v-model="keyInputs[b.id]" class="input" type="password" placeholder="留空并保存 = 清除 key" />
            <button class="btn btn--sm" @click="saveKey(b)">保存 Key</button>
          </div>
        </div>

        <p v-if="testResult[b.id]" class="backend__result">{{ testResult[b.id] }}</p>
      </div>
    </div>

    <button class="btn" @click="addBackend">+ 添加后端</button>

    <div class="card note">
      <h3 class="card__title">说明</h3>
      <ul>
        <li>后端需提供 OpenAI 兼容接口（<code>/v1/chat/completions</code>）。</li>
        <li>本地 llama.cpp 服务地址：<code>http://127.0.0.1:8080</code>，无需 API key。</li>
        <li>API key 通过系统凭据管理器存储，不写入配置文件。</li>
        <li>温度默认 0.1：翻译任务需要稳定输出，官方脚本即用此值。</li>
      </ul>
      <p v-if="activeBackend" class="note__current">
        当前后端：<strong>{{ activeBackend.name }}</strong> — {{ activeBackend.base_url }}
      </p>
    </div>
  </div>
</template>

<style scoped>

/* ---------- 托盘 / 热键 / 自启 ---------- */

.tray-card {
  margin-bottom: var(--sp-5);
}

.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-4);
  padding: var(--sp-3) 0;
  border-bottom: 1px solid var(--c-border-soft);
}

.row:last-of-type {
  border-bottom: none;
}

.row__label {
  display: flex;
  flex-direction: column;
  gap: 2px;
  font-size: 14px;
  color: var(--c-text);
}

.row__hint {
  font-size: 12px;
  font-style: normal;
  color: var(--c-text-tertiary);
}

.switch {
  width: 38px;
  height: 20px;
  flex-shrink: 0;
  cursor: pointer;
  accent-color: var(--c-primary);
}

.hotkey {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  flex-shrink: 0;
}

.hotkey__input {
  width: 150px;
  font-family: var(--font-mono);
  font-size: 12px;
  text-align: center;
}

.hotkey__state {
  margin: var(--sp-3) 0 0;
  font-size: 12px;
}

.hotkey__state.ok {
  color: var(--c-success);
}

.hotkey__state.bad {
  color: var(--c-danger);
}

.tray-card__note {
  margin: var(--sp-3) 0 0;
  font-size: 12px;
  color: var(--c-text-tertiary);
}

/* 录制热键时吃掉输入框焦点，避免按键被输入框消费 */
.settings--recording {
  user-select: none;
}
.settings {
  max-width: 860px;
  padding: var(--sp-4) var(--sp-5);
  overflow-y: auto;
  height: 100%;
}

.settings__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: var(--sp-4);
  flex-wrap: wrap;
  gap: var(--sp-2);
}

.settings__head h2 {
  margin: 0;
  font-size: 18px;
}

.settings__global {
  display: flex;
  align-items: center;
  gap: var(--sp-3);
}

.inline {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  color: var(--c-text-secondary);
}

.input--sm {
  width: 80px;
}

.backends {
  display: flex;
  flex-direction: column;
  gap: var(--sp-3);
  margin-bottom: var(--sp-3);
}

.backend--active {
  border-color: var(--c-primary);
  box-shadow: 0 0 0 1px var(--c-primary);
}

.backend__head {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  margin-bottom: var(--sp-3);
}

.backend__name {
  flex: 1;
  font-weight: 600;
}

.backend__row {
  display: flex;
  gap: var(--sp-3);
}

.backend__url {
  flex: 2;
}

.backend__model {
  flex: 1;
}

.backend__key {
  display: flex;
  gap: var(--sp-2);
}

.backend__result {
  margin: 0;
  padding: 6px 10px;
  font-size: 12px;
  font-family: var(--font-mono);
  background: var(--c-surface-alt);
  border-radius: var(--r-sm);
  color: var(--c-text-secondary);
}

.note {
  margin-top: var(--sp-4);
}

.note ul {
  margin: 0;
  padding-left: 20px;
  font-size: 13px;
  color: var(--c-text-secondary);
  line-height: 1.9;
}

.note code {
  padding: 1px 5px;
  font-family: var(--font-mono);
  font-size: 12px;
  background: var(--c-surface-hover);
  border-radius: var(--r-sm);
}

.note__current {
  margin: var(--sp-3) 0 0;
  padding-top: var(--sp-3);
  font-size: 13px;
  border-top: 1px solid var(--c-border-soft);
}
</style>