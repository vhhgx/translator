<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { activeBackend, pickBackend, settings } from '../composables/useAppState'
import { toast } from '../api'

const open = ref(false)
const root = ref<HTMLElement | null>(null)

onMounted(() => {
  document.addEventListener('click', onDocClick)
  document.addEventListener('keydown', onKey)
})
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
    <button class="engine-pill" title="切换翻译引擎" @click.stop="open = !open">
      <span class="status-dot" />
      <span class="engine-pill__name">{{ activeBackend?.name ?? '选择引擎' }}</span>
      <svg
        width="10"
        height="10"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2.5"
      >
        <polyline points="18 15 12 9 6 15" />
      </svg>
    </button>

    <transition name="menu">
      <div v-if="open" class="engine-menu">
        <button
          v-for="b in settings.backends"
          :key="b.id"
          class="engine-menu__item"
          :class="{ 'engine-menu__item--on': b.id === settings.active_id }"
          @click="choose(b.id)"
        >
          <span class="engine-menu__name">{{ b.name }}</span>
          <span v-if="b.model" class="engine-menu__model">{{ b.model }}</span>
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
        <p v-if="!settings.backends.length" class="engine-menu__empty">还没有配置引擎</p>
      </div>
    </transition>
  </div>
</template>

<style scoped>
.backend-pick {
  position: relative;
}

/* 引擎胶囊：位于底部状态栏，点击向上弹出菜单 */
.engine-pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 3px 10px;
  font-family: inherit;
  font-size: 12px;
  font-weight: 500;
  color: var(--text-primary);
  background: var(--surface-tertiary);
  border: 1px solid var(--border-subtle);
  border-radius: var(--r-pill);
  cursor: pointer;
  transition: all 0.15s;
}

.engine-pill:hover {
  border-color: var(--border-focus);
  background: var(--surface-hover);
}

.engine-pill__name {
  max-width: 140px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.engine-menu {
  position: absolute;
  bottom: calc(100% + 8px);
  left: 0;
  z-index: 60;
  min-width: 260px;
  padding: 8px;
  background: var(--popover-bg);
  backdrop-filter: blur(20px);
  border: 1px solid var(--border-subtle);
  border-radius: var(--r-md);
  box-shadow: var(--popover-shadow);
  animation: pop-up 0.16s cubic-bezier(0.16, 1, 0.3, 1);
  transform-origin: bottom left;
}

.engine-menu__item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 8px 10px;
  font-family: inherit;
  font-size: 13px;
  color: var(--text-primary);
  background: none;
  border: none;
  border-radius: var(--r-sm);
  cursor: pointer;
  text-align: left;
  transition: background 0.12s;
}

.engine-menu__item:hover {
  background: var(--surface-hover);
}

.engine-menu__item--on {
  color: var(--accent-primary);
  font-weight: 600;
  background: var(--surface-active);
}

.engine-menu__name {
  flex: 1;
  white-space: nowrap;
}

.engine-menu__model {
  font-size: 11px;
  color: var(--text-muted);
  max-width: 130px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.engine-menu__empty {
  margin: 8px;
  font-size: 12px;
  color: var(--text-muted);
  text-align: center;
}

.menu-enter-active,
.menu-leave-active {
  transition:
    opacity 0.14s,
    transform 0.14s;
}
.menu-enter-from,
.menu-leave-to {
  opacity: 0;
  transform: translateY(6px) scale(0.98);
}
</style>
