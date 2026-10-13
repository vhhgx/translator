<script setup lang="ts">
import { ref } from 'vue'
import { api, err, toast } from '../api'
import { activeBackend, saveSettings, settings } from '../composables/useAppState'
import type { Backend } from '../types'

const testing = ref('')
const testResult = ref<Record<string, string>>({})
const keyInputs = ref<Record<string, string>>({})

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
  <div class="settings">
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