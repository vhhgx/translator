<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { api, err, toast } from '../api'
import { glossary, loadGlossary } from '../composables/useAppState'
import { open, save } from '@tauri-apps/plugin-dialog'

const rows = ref<[string, string][]>([])
const newName = ref('')
const importing = ref('')

async function reload() {
  await loadGlossary()
  rows.value = glossary.value.terms.map((t) => [t[0], t[1]] as [string, string])
}

onMounted(reload)

function addRow() {
  rows.value.push(['', ''])
}

function removeRow(i: number) {
  rows.value.splice(i, 1)
}

async function save_() {
  try {
    await api.glossarySetTerms(glossary.value.active, rows.value.filter((r) => r[0].trim() && r[1].trim()))
    await reload()
    toast(`已保存 ${rows.value.length} 条术语`, 'success')
  } catch (e) {
    err(e)
  }
}

async function activate(name: string) {
  try {
    await api.glossaryActivate(name)
    await reload()
    toast(`已切换到「${name}」`, 'success')
  } catch (e) {
    err(e)
  }
}

async function createSet() {
  const n = newName.value.trim()
  if (!n) return
  try {
    await api.glossaryCreate(n)
    newName.value = ''
    await reload()
    toast(`已创建词库「${n}」`, 'success')
  } catch (e) {
    err(e)
  }
}

async function removeSet(name: string) {
  if (!confirm(`确定删除词库「${name}」？`)) return
  try {
    await api.glossaryDelete(name)
    await reload()
    toast('已删除')
  } catch (e) {
    err(e)
  }
}

async function doExport() {
  try {
    const path = await save({
      title: '导出词库',
      defaultPath: `${glossary.value.active}.json`,
      filters: [{ name: 'JSON', extensions: ['json'] }]
    })
    if (!path) return
    const payload = await api.glossaryExport(glossary.value.active)
    await writeText(path, JSON.stringify(payload, null, 2))
    toast(`已导出 ${payload.terms.length} 条`, 'success')
  } catch (e) {
    err(e)
  }
}

async function doImport() {
  try {
    const path = await open({
      title: '导入词库',
      multiple: false,
      filters: [{ name: 'JSON', extensions: ['json'] }]
    })
    if (!path) return
    const text = await api.readTextFile(path)
    const payload = JSON.parse(text)
    const terms: [string, string][] = Array.isArray(payload) ? payload : payload.terms
    const name = importing.value.trim() || glossary.value.active
    const n = await api.glossaryImport(name, terms, false)
    await reload()
    toast(`已导入到「${name}」，共 ${n} 条`, 'success')
  } catch (e) {
    err(e)
  }
}

/** @tauri-apps/plugin-fs 未引入，这里用 Rust 命令写文件更简单。 */
async function writeText(path: string, content: string) {
  const { invoke } = await import('@tauri-apps/api/core')
  await invoke('doc_save', { path, content })
}
</script>

<template>
  <div class="glossary">
    <div class="glossary__head">
      <div class="glossary__sets">
        <button
          v-for="n in glossary.names"
          :key="n"
          class="chip"
          :class="{ 'chip--active': n === glossary.active }"
          @click="activate(n)"
        >
          {{ n }}
          <span class="chip__del" @click.stop="removeSet(n)">×</span>
        </button>
      </div>
      <div class="glossary__actions">
        <input v-model="newName" class="input input--sm" placeholder="新词库名" @keyup.enter="createSet" />
        <button class="btn btn--sm" @click="createSet">新建</button>
        <button class="btn btn--sm" @click="doExport">导出</button>
      </div>
    </div>

    <div class="glossary__import">
      <input v-model="importing" class="input input--sm" placeholder="导入到（留空=当前词库）" />
      <button class="btn btn--sm" @click="doImport">导入 JSON</button>
    </div>

    <table class="table">
      <thead>
        <tr>
          <th style="width: 40%">源词</th>
          <th style="width: 40%">译词</th>
          <th style="width: 60px"></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="(row, i) in rows" :key="i">
          <td><input v-model="row[0]" class="input" /></td>
          <td><input v-model="row[1]" class="input" /></td>
          <td><button class="btn btn--ghost btn--sm" @click="removeRow(i)">删</button></td>
        </tr>
      </tbody>
    </table>

    <div v-if="!rows.length" class="empty">
      <span class="empty__icon">📖</span>
      <span>这个词库还没有术语，点「添加一行」开始</span>
    </div>

    <div class="glossary__foot">
      <button class="btn btn--sm" @click="addRow">+ 添加一行</button>
      <button class="btn btn--primary" @click="save_">保存词库</button>
    </div>
  </div>
</template>

<style scoped>
.glossary {
  height: 100%;
  padding: var(--sp-4) var(--sp-5);
  overflow-y: auto;
}

.glossary__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-3);
  margin-bottom: var(--sp-3);
  flex-wrap: wrap;
}

.glossary__sets {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}

.chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 5px 12px;
  font-size: 13px;
  background: var(--c-surface-alt);
  border: 1px solid var(--c-border-soft);
  border-radius: var(--r-full);
  cursor: pointer;
  color: var(--c-text-secondary);
}

.chip--active {
  background: var(--c-primary-soft);
  border-color: var(--c-primary);
  color: var(--c-primary);
}

.chip__del {
  font-size: 15px;
  opacity: 0.5;
}

.chip__del:hover {
  opacity: 1;
  color: var(--c-danger);
}

.glossary__actions,
.glossary__import {
  display: flex;
  gap: 6px;
  align-items: center;
}

.glossary__import {
  margin-bottom: var(--sp-3);
}

.input--sm {
  width: 160px;
  padding: 6px 10px;
  font-size: 13px;
}

.glossary__foot {
  display: flex;
  justify-content: space-between;
  gap: var(--sp-2);
  margin-top: var(--sp-4);
  padding-top: var(--sp-3);
  border-top: 1px solid var(--c-border-soft);
}
</style>