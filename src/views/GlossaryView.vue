<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { BookOpen, Download, Plus, Trash2, Upload, X } from '@lucide/vue'
import { api, err, toast } from '../api'
import { glossary, loadGlossary } from '../composables/useAppState'
import { open, save } from '@tauri-apps/plugin-dialog'
import { invoke } from '@tauri-apps/api/core'

const rows = ref<[string, string][]>([])
const newName = ref('')
const importing = ref('')
/** 当前词库是否有未保存的行编辑（切页签/离开前提示的依据） */
const dirty = ref(false)
/** 双击页签改名 */
const renaming = ref('')
const renameEl = ref<HTMLInputElement | null>(null)

function countOf(name: string): number {
  const g = glossary.value
  const i = g.names.indexOf(name)
  return i >= 0 ? (g.counts[i] ?? 0) : 0
}

async function reload() {
  await loadGlossary()
  rows.value = glossary.value.terms.map((t) => [t[0], t[1]] as [string, string])
  dirty.value = false
}

onMounted(reload)

function markDirty() {
  dirty.value = true
}

function addRow() {
  rows.value.push(['', ''])
  dirty.value = true
}

function removeRow(i: number) {
  rows.value.splice(i, 1)
  dirty.value = true
}

async function save_() {
  try {
    const terms = rows.value.filter((r) => r[0].trim() && r[1].trim())
    await api.glossarySetTerms(glossary.value.active, terms)
    await reload()
    toast(`已保存 ${terms.length} 条术语`, 'success')
  } catch (e) {
    err(e)
  }
}

/** 切页签会丢弃未保存的行编辑，先确认。 */
async function activate(name: string) {
  if (name === glossary.value.active) return
  if (dirty.value && !confirm('当前词库有未保存的修改，切换将丢弃，确定？')) return
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

/** 双击页签改名，回车提交、Esc/失焦取消。 */
function startRename(name: string) {
  renaming.value = name
  // 等输入框渲染出来后聚焦并全选
  window.setTimeout(() => renameEl.value?.select(), 30)
}

async function commitRename(e: KeyboardEvent | FocusEvent) {
  const oldName = renaming.value
  const input = e.target as HTMLInputElement
  const n = input.value.trim()
  renaming.value = ''
  if (!n || n === oldName) return
  try {
    await api.glossaryRename(oldName, n)
    await reload()
    toast(`已改名为「${n}」`, 'success')
  } catch (e2) {
    err(e2)
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

/** 写文件走 Rust 命令，避免额外依赖 plugin-fs。 */
async function writeText(path: string, content: string) {
  await invoke('doc_save', { path, content })
}
</script>

<template>
  <div class="glossary">
    <!-- 顶部工具栏：词库页签 + 导入导出 -->
    <div class="page-toolbar">
      <div class="glossary-tabs-bar">
        <template v-for="n in glossary.names" :key="n">
          <button
            v-if="renaming !== n"
            class="glossary-tab"
            :class="{ 'glossary-tab--active': n === glossary.active }"
            :title="`点击切换 · 双击改名`"
            @click="activate(n)"
            @dblclick="startRename(n)"
          >
            <BookOpen :size="13" />
            <span>{{ n }} ({{ countOf(n) }})</span>
            <span class="glossary-tab__del" title="删除该词库" @click.stop="removeSet(n)">
              <X :size="11" />
            </span>
          </button>
          <input
            v-else
            ref="renameEl"
            class="glossary-rename-input"
            :value="n"
            @keydown.enter.prevent="commitRename"
            @keydown.esc.prevent="renaming = ''"
            @blur="commitRename"
          />
        </template>
        <input
          v-model="newName"
          class="new-set-input"
          placeholder="新词库名…"
          @keyup.enter="createSet"
        />
        <button class="btn btn--sm" @click="createSet">
          <Plus :size="13" />
          新建
        </button>
      </div>

      <div class="toolbar-right">
        <input v-model="importing" class="import-input" placeholder="导入到（留空=当前词库）" />
        <button class="btn btn--sm" title="将 JSON 词条合并至指定词库" @click="doImport">
          <Upload :size="13" />
          导入 JSON
        </button>
        <button class="btn btn--sm" @click="doExport">
          <Download :size="13" />
          导出词库
        </button>
      </div>
    </div>

    <!-- 术语表格 -->
    <div class="glossary-workspace">
      <div class="glossary-table-card">
        <table class="g-table">
          <thead>
            <tr>
              <th style="width: 44%">源词 (Source Term)</th>
              <th style="width: 44%">目标译词 (Target Term)</th>
              <th style="width: 12%; text-align: right">操作</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(row, i) in rows" :key="i">
              <td><input v-model="row[0]" class="g-cell-input" placeholder="输入源词…" @input="markDirty" /></td>
              <td><input v-model="row[1]" class="g-cell-input" placeholder="输入对应译词…" @input="markDirty" /></td>
              <td style="text-align: right">
                <button class="icon-btn icon-btn--danger" title="删除该行" @click="removeRow(i)">
                  <Trash2 :size="14" />
                </button>
              </td>
            </tr>
          </tbody>
        </table>
        <div v-if="!rows.length" class="empty">
          <BookOpen :size="32" class="empty__icon" />
          <span>这个词库还没有术语，点下方「添加一行」开始</span>
        </div>
      </div>

      <div class="glossary-footer-bar">
        <button class="btn btn--sm" @click="addRow">+ 添加一行术语</button>
        <div class="footer-right">
          <span class="autosave-hint">
            <span v-if="dirty" class="dirty-dot" title="有未保存的修改"></span>
            {{ dirty ? '有未保存的修改' : '修改已实时同步到草稿' }}
          </span>
          <button class="btn btn--primary btn--sm" @click="save_">保存词库</button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.glossary {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  background: var(--surface-secondary);
}

/* ---------- 工具栏 ---------- */
.page-toolbar {
  padding: 12px 24px;
  background: var(--surface-primary);
  border-bottom: 1px solid var(--border-subtle);
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  flex-shrink: 0;
}

.glossary-tabs-bar {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
  min-width: 0;
}

.glossary-tab {
  padding: 6px 12px;
  border-radius: 7px;
  background: var(--surface-tertiary);
  border: 1px solid var(--border-subtle);
  color: var(--text-secondary);
  font-family: inherit;
  font-size: 12.5px;
  font-weight: 500;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  transition: all 0.15s;
  white-space: nowrap;
}

.glossary-tab:hover {
  color: var(--text-primary);
}

.glossary-tab--active {
  background: var(--surface-active);
  color: var(--accent-primary);
  border-color: var(--border-focus);
}

.glossary-tab__del {
  opacity: 0.5;
  font-size: 11px;
  padding: 1px 3px;
  border-radius: 4px;
}

.glossary-tab__del:hover {
  opacity: 1;
  background: var(--status-danger-bg);
  color: var(--status-danger);
}

.glossary-rename-input {
  padding: 5px 10px;
  border-radius: 7px;
  background: var(--surface-primary);
  border: 1px solid var(--border-focus);
  color: var(--text-primary);
  font-family: inherit;
  font-size: 12.5px;
  outline: none;
  width: 150px;
  box-shadow: 0 0 0 3px var(--accent-glow);
}

.new-set-input {
  width: 130px;
  padding: 5px 10px;
  border-radius: 7px;
  background: var(--surface-secondary);
  border: 1px solid var(--border-subtle);
  color: var(--text-primary);
  font-family: inherit;
  font-size: 12.5px;
  outline: none;
}

.new-set-input:focus {
  border-color: var(--border-focus);
}

.toolbar-right {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.import-input {
  width: 170px;
  padding: 5px 10px;
  border-radius: 7px;
  background: var(--surface-secondary);
  border: 1px solid var(--border-subtle);
  color: var(--text-primary);
  font-family: inherit;
  font-size: 12.5px;
  outline: none;
}

.import-input:focus {
  border-color: var(--border-focus);
}

/* ---------- 表格区 ---------- */
.glossary-workspace {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 20px 24px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.glossary-table-card {
  background: var(--surface-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 11px;
  overflow: hidden;
}

.g-table {
  width: 100%;
  border-collapse: collapse;
  text-align: left;
}

.g-table th {
  padding: 10px 16px;
  font-size: 11.5px;
  font-weight: 600;
  color: var(--text-muted);
  background: var(--surface-tertiary);
  border-bottom: 1px solid var(--border-subtle);
  text-transform: uppercase;
  letter-spacing: 0.03em;
}

.g-table td {
  padding: 6px 12px;
  border-bottom: 1px solid var(--border-subtle);
  vertical-align: middle;
}

.g-table tr:last-child td {
  border-bottom: none;
}

.g-table tbody tr:hover {
  background: var(--surface-hover);
}

/* 内联可编辑单元格：平时无框，hover 显边、focus 高亮 */
.g-cell-input {
  width: 100%;
  padding: 6px 10px;
  border-radius: 6px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-primary);
  font-family: inherit;
  font-size: 13.5px;
  outline: none;
  transition: all 0.15s;
}

.g-cell-input:hover {
  border-color: var(--border-subtle);
  background: var(--surface-secondary);
}

.g-cell-input:focus {
  border-color: var(--border-focus);
  background: var(--surface-secondary);
  box-shadow: 0 0 0 2px var(--accent-glow);
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
}

.icon-btn:hover {
  color: var(--text-primary);
  background: var(--surface-hover);
}

.icon-btn--danger:hover {
  color: var(--status-danger);
  background: var(--status-danger-bg);
}

/* ---------- 底栏 ---------- */
.glossary-footer-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 4px 2px;
}

.footer-right {
  display: flex;
  align-items: center;
  gap: 12px;
}

.autosave-hint {
  font-size: 12px;
  color: var(--text-muted);
  display: flex;
  align-items: center;
  gap: 6px;
}

.dirty-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--star-active);
  box-shadow: 0 0 6px var(--star-active);
}
</style>
