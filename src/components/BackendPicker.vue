<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { activeBackend, pickBackend, settings } from '../composables/useAppState'
import { toast } from '../api'

const open = ref(false)
const root = ref<HTMLElement | null>(null)

onMounted(() => document.addEventListener('click', onDocClick))
onUnmounted(() => {
  document.removeEventListener('click', onDocClick)
  document.removeEventListener('keydown', onKey)
})
function onDocClick(e: MouseEvent) {
  if (open.value && root.value && !root.value.contains(e.target as Node)) open.value = false
}
function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') open.value = false
}

async function choose(id: string) {
  open.value = false
  if (id === settings.value.active_id) return
  await pickBackend(id)
  const b = settings.value.backends.find((x) => x.id === id)
  if (b) toast(`已切换到 ${b.name}`, 'success')
}
</script>

<template>
  <div ref="root" class="backend-pick">
    <button class="backend-tag" title="切换翻译模型" @click.stop="open = !open">
      <span class="backend-tag__dot" />
      {{ activeBackend?.name ?? '选择模型' }}
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
        <path d="M6 9l6 6 6-6" />
      </svg>
    </button>

    <div v-if="open" class="backend-menu">
      <button
        v-for="b in settings.backends"
        :key="b.id"
        class="backend-menu__item"
        :class="{ 'backend-menu__item--on': b.id === settings.active_id }"
        @click="choose(b.id)"
      >
        <span class="backend-tag__dot" />
        <span class="backend-menu__name">{{ b.name }}</span>
        <span v-if="b.model" class="backend-menu__model">{{ b.model }}</span>
        <svg
          v-if="b.id === settings.active_id"
          width="14"
          height="14"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2.5"
        >
          <polyline points="20 6 9 17 4 12" />
        </svg>
      </button>
      <p v-if="!settings.backends.length" class="backend-menu__empty">还没有配置模型</p>
    </div>
  </div>
</template>

<style scoped>
.backend-pick {
  position: relative;
}

.backend-tag {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  font-family: inherit;
  font-size: 13px;
  font-weight: 500;
  color: var(--c-text-secondary);
  background: var(--c-surface-alt);
  border: 1px solid var(--c-border-soft);
  border-radius: var(--r-pill);
  cursor: pointer;
  transition: all 0.15s;
}

.backend-tag:hover {
  background: var(--c-surface-hover);
  color: var(--c-primary);
  border-color: var(--c-primary);
}

.backend-tag__dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: #34a853;
  flex-shrink: 0;
}

.backend-menu {
  position: absolute;
  bottom: calc(100% + 8px);
  left: 0;
  z-index: 60;
  min-width: 260px;
  padding: 8px;
  background: var(--c-surface);
  border: 1px solid var(--c-border-soft);
  border-radius: var(--r-lg);
  box-shadow: var(--shadow-float);
}

.backend-menu__item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 9px 10px;
  font-family: inherit;
  font-size: 13px;
  color: var(--c-text);
  background: none;
  border: none;
  border-radius: var(--r-md);
  cursor: pointer;
  text-align: left;
}

.backend-menu__item:hover {
  background: var(--c-surface-hover);
}

.backend-menu__item--on {
  color: var(--c-primary);
  font-weight: 600;
  background: var(--c-primary-soft);
}

.backend-menu__name {
  flex: 1;
  white-space: nowrap;
}

.backend-menu__model {
  font-size: 11px;
  color: var(--c-text-tertiary);
  max-width: 130px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.backend-menu__empty {
  margin: 8px;
  font-size: 12px;
  color: var(--c-text-tertiary);
  text-align: center;
}
</style>