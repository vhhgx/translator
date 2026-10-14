import { createApp } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import App from './App.vue'
import MiniWindow from './views/MiniWindow.vue'
import SelectionPopup from './views/SelectionPopup.vue'
import './styles/base.css'

/**
 * 同一个 index.html 同时服务三个窗口，用窗口 label 区分该挂载哪个根组件：
 *   main  = 完整主界面
 *   popup = 划词翻译弹窗（透明、不抢焦点）
 *   mini  = 小窗模式（热键唤出，输入 + 译文）
 * 浏览器里调试时没有 Tauri IPC，取不到 label，按主窗口处理。
 */
function windowLabel(): string {
  try {
    return getCurrentWindow().label
  } catch {
    return ''
  }
}

const label = windowLabel()

if (label === 'popup') {
  // 弹窗是 transparent 窗口，base.css 里 body 的不透明底色要放掉
  document.documentElement.classList.add('is-popup')
  createApp(SelectionPopup).mount('#app')
} else if (label === 'mini') {
  createApp(MiniWindow).mount('#app')
} else {
  createApp(App).mount('#app')
}
