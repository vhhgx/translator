<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'

const props = withDefaults(
  defineProps<{
    modelValue: string
    options: [string, string][]
    align?: 'left' | 'right'
  }>(),
  { align: 'left' }
)

const emit = defineEmits<{ (e: 'update:modelValue', v: string): void }>()

const open = ref(false)
const query = ref('')
const root = ref<HTMLElement | null>(null)
const searchBox = ref<HTMLInputElement | null>(null)
const listEl = ref<HTMLElement | null>(null)

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase()
  if (!q) return props.options
  return props.options.filter(
    ([code, name]) =>
      name.toLowerCase().includes(q) || code.toLowerCase().includes(q)
  )
})

watch(open, async (v) => {
  if (v) {
    query.value = ''
    await nextTick()
    searchBox.value?.focus()
  }
})

function choose(name: string) {
  emit('update:modelValue', name)
  open.value = false
}

function onDocClick(e: MouseEvent) {
  if (open.value && root.value && !root.value.contains(e.target as Node)) {
    open.value = false
  }
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
})
</script>

<template>
  <div ref="root" class="langpick">
    <button class="pill pill--icon-only" title="展开更多语言" @click="open = !open">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
        <path d="M6 9l6 6 6-6" />
      </svg>
    </button>

    <div v-if="open" class="langpick__menu" :class="{ 'langpick__menu--right': align === 'right' }">
      <input ref="searchBox" v-model="query" class="langpick__search" placeholder="搜索 38 种语言…" />
      <div ref="listEl" class="langpick__list">
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
  </div>
</template>

<style scoped>
.langpick {
  position: relative;
  display: inline-flex;
}

.pill--icon-only {
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

.pill--icon-only:hover {
  background: var(--c-surface-hover);
  color: var(--c-text);
}

.langpick__menu {
  position: absolute;
  top: calc(100% + 8px);
  left: 0;
  z-index: 50;
  width: 260px;
  padding: 10px;
  background: var(--c-surface);
  border: 1px solid var(--c-border-soft);
  border-radius: var(--r-lg);
  box-shadow: var(--shadow-float);
}

.langpick__menu--right {
  left: auto;
  right: 0;
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
  background: #fff;
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
