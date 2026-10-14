<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'

const props = withDefaults(
  defineProps<{
    modelValue: string
    options: [string, string][]
    align?: 'left' | 'right'
    /** 触发器是否显示当前选中项文本。主窗口里语言名在药丸上，触发器只要 chevron；
     *  小窗/弹窗没有药丸，触发器本身要把当前语言显示出来。 */
    showLabel?: boolean
  }>(),
  { align: 'left', showLabel: false }
)

const emit = defineEmits<{ (e: 'update:modelValue', v: string): void }>()

const open = ref(false)
const query = ref('')
const triggerEl = ref<HTMLElement | null>(null)
const menuEl = ref<HTMLElement | null>(null)
const searchBox = ref<HTMLInputElement | null>(null)
const pos = ref({ left: 0, top: 0 })

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase()
  if (!q) return props.options
  return props.options.filter(
    ([code, name]) =>
      name.toLowerCase().includes(q) || code.toLowerCase().includes(q)
  )
})

/**
 * 菜单用 fixed 定位并 Teleport 到 body。
 *
 * 之前它是卡片内的绝对定位元素：卡片带 overflow:hidden，既会被圆角裁掉，
 * 撑大的可滚动区域还会让点击时整个 header 横向滚动，药丸整排被推走。
 */
function place() {
  const t = triggerEl.value
  const m = menuEl.value
  if (!t || !m) return
  const r = t.getBoundingClientRect()
  const mw = m.offsetWidth
  const mh = m.offsetHeight
  const gap = 8
  const margin = 8

  // 小窗（560×320）里菜单可能比视口还高：按可用高度收敛列表，
  // 否则底部会被窗口裁掉一半。
  const avail = window.innerHeight - margin * 2
  const listEl = m.querySelector<HTMLElement>('.langpick__list')
  if (listEl) {
    const chromeH = mh - listEl.offsetHeight // 搜索框 + padding
    listEl.style.maxHeight = `${Math.max(90, Math.min(listEl.offsetHeight, avail - chromeH - gap))}px`
  }

  // 下方放得下就往下，否则翻到上方
  const effH = m.offsetHeight
  const top =
    window.innerHeight - r.bottom > effH + gap ? r.bottom + gap : Math.max(margin, r.top - effH - gap)

  let left = props.align === 'right' ? r.right - mw : r.left
  left = Math.min(Math.max(margin, left), window.innerWidth - mw - margin)

  pos.value = { left, top }
}

watch(open, async (v) => {
  if (v) {
    query.value = ''
    await nextTick()
    // 先渲染再量，菜单高度取决于过滤结果，两遍定位更稳
    place()
    await nextTick()
    searchBox.value?.focus()
  } else {
    window.removeEventListener('resize', place)
    window.removeEventListener('scroll', place, true)
  }
  if (v) {
    window.addEventListener('resize', place)
    window.addEventListener('scroll', place, true)
  }
})

function choose(name: string) {
  emit('update:modelValue', name)
  open.value = false
}

function onDocClick(e: MouseEvent) {
  if (!open.value) return
  const t = e.target as Node
  if (triggerEl.value?.contains(t) || menuEl.value?.contains(t)) return
  open.value = false
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') open.value = false
}

onMounted(() => {
  document.addEventListener('click', onDocClick)
  document.addEventListener('keydown', onKey)
})
onUnmounted(() => {
  document.removeEventListener('click', onDocClick)
  document.removeEventListener('keydown', onKey)
  window.removeEventListener('resize', place)
  window.removeEventListener('scroll', place, true)
})
</script>

<template>
  <span ref="triggerEl" class="langpick">
    <button
      class="langpick__trigger"
      :class="{ 'langpick__trigger--labeled': showLabel }"
      :title="showLabel ? '切换目标语言' : '展开更多语言'"
      @click="open = !open"
    >
      <span v-if="showLabel" class="langpick__label">{{ modelValue }}</span>
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
        <path d="M6 9l6 6 6-6" />
      </svg>
    </button>

    <Teleport to="body">
      <div
        v-if="open"
        ref="menuEl"
        class="langpick__menu"
        :style="{ left: `${pos.left}px`, top: `${pos.top}px` }"
      >
        <input ref="searchBox" v-model="query" class="langpick__search" placeholder="搜索 38 种语言…" />
        <div class="langpick__list">
          <button
            v-for="[code, name] in filtered"
            :key="code"
            class="langpick__opt"
            :class="{ 'langpick__opt--on': name === modelValue }"
            @click="choose(name)"
          >
            <span class="langpick__name">{{ name }}</span>
            <span class="langpick__code">{{ code }}</span>
          </button>
          <p v-if="!filtered.length" class="langpick__none">没有匹配的语言</p>
        </div>
      </div>
    </Teleport>
  </span>
</template>

<style scoped>
.langpick {
  display: inline-flex;
}

.langpick__trigger {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  padding: 0;
  color: var(--c-text-secondary);
  background: none;
  border: none;
  border-radius: 50%;
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}

.langpick__trigger:hover {
  background: var(--c-surface-hover);
  color: var(--c-text);
}

/* 带文本标签的触发器（小窗 / 划词弹窗）：胶囊形而不是圆钮 */
.langpick__trigger--labeled {
  width: auto;
  height: auto;
  gap: 4px;
  padding: 2px 6px 2px 8px;
  border-radius: 6px;
  background: var(--c-surface-alt);
  border: 1px solid var(--c-border-soft);
}

.langpick__trigger--labeled svg {
  width: 12px;
  height: 12px;
}

.langpick__label {
  font-size: 12px;
  font-weight: 600;
  color: var(--c-primary);
  max-width: 90px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.langpick__menu {
  position: fixed;
  z-index: 200;
  width: 260px;
  padding: 10px;
  background: var(--popover-bg);
  backdrop-filter: blur(20px);
  border: 1px solid var(--border-subtle);
  border-radius: var(--r-md);
  box-shadow: var(--popover-shadow);
  box-sizing: border-box;
}

.langpick__search {
  width: 100%;
  padding: 8px 14px;
  font-family: inherit;
  font-size: 13px;
  color: var(--c-text);
  background: var(--c-surface-alt);
  border: none;
  border-radius: var(--r-pill);
  outline: none;
  box-sizing: border-box;
}

.langpick__search:focus {
  background: var(--c-surface);
  box-shadow: 0 0 0 2px var(--c-primary);
}

.langpick__list {
  max-height: 280px;
  margin-top: 8px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.langpick__opt {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  font-family: inherit;
  font-size: 13px;
  color: var(--c-text);
  background: none;
  border: none;
  border-radius: var(--r-md);
  cursor: pointer;
  text-align: left;
  transition: background 0.12s;
}

.langpick__opt:hover {
  background: var(--c-surface-hover);
}

.langpick__opt--on {
  color: var(--c-primary);
  background: var(--c-primary-soft);
  font-weight: 600;
}

.langpick__code {
  font-size: 11px;
  color: var(--c-text-tertiary);
  text-transform: uppercase;
}

.langpick__none {
  margin: 16px 0;
  font-size: 12px;
  color: var(--c-text-tertiary);
  text-align: center;
}
</style>