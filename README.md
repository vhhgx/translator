# Zephyr Translator

**可接入任意 AI 的跨端桌面翻译工具。**

填一个 OpenAI 兼容的地址，就能翻译。内置腾讯混元 Hy-MT2、DeepSeek、OpenAI、Ollama 等预置后端，
也可以随时添加自建 vLLM、llama.cpp 或任意兼容网关。工具本身**不含任何模型**，推理全部交给你选的服务。

Vue 3 + Rust (Tauri v2) · Windows / Linux · UI 全部手写 CSS，不依赖任何 UI 框架。

Rust 只负责界面宿主与 IO —— 配置、凭据、HTTP 调用、文档解析；翻译推理由本地 `llama-server`
或任意 OpenAI 兼容云端接口完成。

---

## 目录

- [30 秒上手](#30-秒上手)
- [功能一览](#功能一览)
- [接入任意 AI](#接入任意-ai)
- [划词翻译](#划词翻译选中后自动弹窗)
- [小窗模式](#小窗模式热键唤出快速翻译)
- [翻译模式与提示词](#翻译模式与提示词)
- [开发与构建](#开发与构建)
- [打包](#打包)
- [配置与数据存放](#配置与数据存放)
- [架构说明](#架构说明)
- [测试](#测试)
- [已知限制](#已知限制)

---

## 30 秒上手

想直接用，去 [**Releases**](https://github.com/vhhgx/translator/releases) 拿打好的包。

| 平台 | 下载 | 说明 |
| --- | --- | --- |
| Windows x64 | `Zephyr Translator_*_x64-setup.exe` | NSIS 安装包，双击安装 |
| Linux x64 (Debian / Ubuntu) | `*_amd64.deb` | `sudo apt install ./xxx.deb` |
| Linux x64 (通用) | `*_amd64.AppImage` | 给上 `./xxx.AppImage` 加执行权限后直接跑 |

装完做三件事就能用：

1. 在**底部状态栏**左边的「选择引擎」里挑一个后端
2. 本地模型需要先自己起 `llama-server`；云端后端去**设置 → 翻译引擎与模型**填 Base URL 和 API Key，点「测试连接」确认通了
3. 全局热键、划词翻译在**设置 → 窗口唤起与热键**里配

> **不想装模型？** 填一个云端 API 的地址和 key 就能直接用。工具不含模型，也不需要你装。

**关于 SmartScreen**：Windows 安装包没做代码签名，首次安装会提示「Windows 已保护你的电脑」，
点「更多信息 → 仍要运行」就行。这不是文件损坏，是没签名的正常表现。

**暂不提供 macOS 版**。划词翻译在 macOS 上需要走 Accessibility 取词通道，目前还没实现，
发一个划词失灵、又不公证（用户每次都得手动绕过 Gatekeeper）的包没有意义。Windows 和 Linux
是完整的。

### 从源码构建

```bash
pnpm install
pnpm app:dev          # 开发模式
pnpm app:build        # 打包 release
```

首次 `cargo build` 冷编译约 3–8 分钟。

---

## 功能一览

| 模块 | 说明 |
|---|---|
| **文本翻译** | 6 种模式（见下文），**流式输出**逐 token 渲染。模式选择收在左下角胶囊的悬浮面板里 |
| **接入任意 AI** | 底部状态栏实时切换后端；预置 Hy-MT2 / DeepSeek / OpenAI / Ollama，也可自由增删任意 OpenAI 兼容端点 |
| **术语库** | 多词库增删改查（页签双击改名）、JSON 导入导出、按需只取原文命中的词、翻译页词库胶囊直达 |
| **历史** | SQLite 本地存储，搜索即搜、★ 收藏与「仅看收藏」筛选、点击卡片回填复用 |
| **批量** | JSONL 读入，并发翻译，输出追加 `translation` 字段，断点续跑 |
| **文档** | `.txt` / `.md`，自动识别 GBK / GB18030 / Big5 / UTF-16；Markdown 保留标题、列表、表格、代码块 |
| **划词翻译** | Windows / Linux(X11) 下选中文本即自动弹窗翻译；支持**钉住**与拖拽 |
| **小窗模式** | 热键唤出「输入 + 译文」小窗，不动主窗口 |
| **系统集成** | 托盘常驻、开机自启、两个全局热键、关闭到托盘 |
| **深浅主题** | 跟随系统深浅色 + 手动切换（顶栏一键 / 设置页三选一），三个窗口同步 |

支持 **38 种语言**：中文、英语、法语、葡萄牙语、西班牙语、日语、土耳其语、俄语、阿拉伯语、韩语、
泰语、意大利语、德语、越南语、马来语、印尼语、菲律宾语、印地语、繁体中文、波兰语、捷克语、荷兰语、
高棉语、缅甸语、波斯语、古吉拉特语、乌尔都语、泰卢固语、马拉地语、希伯来语、孟加拉语、泰米尔语、
乌克兰语、藏语、哈萨克语、蒙古语、维吾尔语、粤语。完整列表见「设置」页或
[`docs/upstream/supported-languages.md`](docs/upstream/supported-languages.md)。

界面按新版设计系统重构：三级表面 + 三级文字 + 细边框的令牌体系，全部手写 CSS，
设计 token 用 CSS 变量统一，`src/styles/base.css` 里 `:root[data-theme]` 各定义一套。

---

## 接入任意 AI

只要服务提供 OpenAI 兼容的 `/v1/chat/completions`，就能接进来。

**在「设置 → 翻译引擎与模型」里点「+ 添加后端」，填三项：**

| 字段 | 说明 |
|---|---|
| **Base URL** | 服务根地址。填 `http://x`、`http://x/v1` 或完整的 `http://x/v1/chat/completions` 都行，会自动归一，不会拼出 `/v1/v1/...` |
| **模型名** | 服务实际暴露的模型 id，如 `gpt-4o-mini`、`deepseek-chat`、`qwen2.5:7b` |
| **API Key** | 本地服务通常留空。填了会存进**系统凭据管理器**，不落明文配置文件 |

点「测试连接」会探测 `/health` 并列出该服务可用的模型。

### 常见接法

| 场景 | Base URL | 模型名示例 | Key |
|---|---|---|---|
| 本地 llama.cpp | `http://127.0.0.1:8080` | 你加载的模型名 | 通常留空 |
| Ollama | `http://127.0.0.1:11434` | `qwen2.5:7b` | 留空 |
| vLLM / 自建网关 | 你的服务地址 | 你启动时指定的名字 | 视情况 |
| 腾讯混元 Hy-MT2 | `https://api.hunyuan.cloud.tencent.com` | `hy-mt2-pro` | 需要 |
| DeepSeek | `https://api.deepseek.com` | `deepseek-chat` | 需要 |
| OpenAI | `https://api.openai.com` | `gpt-4o-mini` | 需要 |

其余 OpenAI 兼容服务同理，添加一个后端即可。

### 两点说明

- **内置的 6 套提示词是针对 Hy-MT2 调过的**（逐字移植自官方 skill，见下文）。换其他模型依然可用，
  但不是针对它们优化的。想要更贴合某个模型，直接改 `src-tauri/src/prompt.rs` 里的模板即可。
- **客户端只发 `temperature`**（默认 0.1）。`top_p` / `top_k` / `repetition_penalty` 等参数走服务端默认，
  因为 OpenAI 兼容协议没给它们统一位置。不同模型若有推荐值，需要在服务端配置。

---

## 划词翻译（选中后自动弹窗）

选中一段文本，不用按任何键，光标旁直接浮出译文。

**开关**：设置页 →「窗口唤起与热键」→「划词翻译」。**默认关闭**——它要装一个全局
鼠标钩子，且每次划词都会动一下剪贴板，属于需要用户明确同意的能力。

**触发修饰键**：可以要求「按住 Ctrl / Alt / Shift 才触发」。默认「不要求」（任何拖选都弹）；
嫌吵就设成某个修饰键。

**钉住**：弹窗头部有个图钉按钮。钉住后点击别处不再自动收起，新划词也不打扰当前内容
（钉住期间想换内容，再点一下图钉取消）。弹窗头部空白处可以拖拽移动。

### 链路

```
全局鼠标事件 (rdev / XRecord) ──左键释放──► capture_selected_text()
                                              │ 清空剪贴板当哨兵 → 模拟 Ctrl+C → 读回 → 还原
                                              ▼
                        hymt2:selection ──► popup 窗口 ──► translate_stream
```

- **抓选中文本**：`src-tauri/src/selection.rs`。Windows 走 UI Automation 直接取选区，
  其它平台用划词工具的通用手法——先把剪贴板清空当哨兵，模拟一次 Ctrl+C，再读回剪贴板；
  读回来还是空的，就说明这次点击根本没选中东西，用户原来的剪贴板内容会还原。
- **弹窗**：`label: "popup"` 的独立窗口（460×300，无边框 + 透明 + 置顶 + 不抢焦点 + 不占
  任务栏），前端在 `src/views/SelectionPopup.vue`。它和主窗口共用同一个 `index.html`，
  靠窗口 label 决定挂载哪个根组件。
- **不串台**：弹窗和主窗口都走 `translate_stream`，两边各自按 `job_id` 过滤事件。
- **钉住**：前端调 `set_popup_pinned`，Rust 侧用 `PINNED: AtomicBool` 让全局鼠标监听跳过
  「点别处收起」的判断。

### 平台支持

| 平台 | 状态 |
|---|---|
| Windows | 支持（模拟 Ctrl+C + 读剪贴板） |
| Linux / X11 | 支持 |
| Linux / **Wayland** | **不可用**。普通客户端既不能模拟全局按键，也读不到别的应用的选区。设置页会显示监听启动失败的原因 |
| macOS | 未做（代码层面已具备，缺「辅助功能」权限引导） |

### 已知取舍

- **会短暂动到剪贴板**：抓取期间剪贴板被写入并还原。若恰好和用户自己的复制撞上，可能丢一次。
- **剪贴板里不是文本时直接跳过**：里面是图片之类的内容时，本次划词不触发，而不是冒险把它弄丢。
- **X11 上还原后的剪贴板归本应用所有**：本应用退出后那份内容会跟着消失（X11 剪贴板本就随属主进程存亡）。
- 目前只有「选中即弹」这一种触发方式，没有悬浮图标、也没有划词历史。

---

## 小窗模式（热键唤出快速翻译）

按热键唤出一个「输入 + 译文」的小窗，**不动主窗口**——只想快速译一句的时候，再弹一个
960×640 的主界面反而碍事。要进主界面，点小窗右上角那个按钮。

- **默认热键** `Ctrl+Shift+S`（设置页可改、也可清除）
- **Enter** 翻译，**Shift+Enter** 换行
- **Esc** 或**点到别处**（失焦）即收起
- 右上角按钮：对调方向 / 从剪贴板填入 / 在主窗口打开 / 关闭；头部空白处可拖拽移动
- 目标语言与划词弹窗**共用同一份选择**（`localStorage` 的 `hymt2:popup-target`）

主窗口显隐另有热键，默认 `Ctrl+Shift+D`。两个热键都能在设置页改或清除。

### 实现要点

- 窗口 `label: "mini"`，560×320，无边框 + 置顶 + 不占任务栏 + 居中，可调整大小。
  与划词弹窗不同，它**没有用 `transparent`**：小窗是用来打字的，透明窗口一旦渲染异常
  就会变成「存在但看不见」的窗口——那个失败模式太难排查，方角换确定性更划算。
- 它也**必须可聚焦**（要收键盘输入），而划词弹窗是 `focusable: false` 的。这一点两者
  互斥，所以不能共用一个窗口。
- 窗口常驻（关闭只隐藏），所以复位逻辑走 `hymt2:mini-shown` 事件，不能依赖 `onMounted`。
- 两个全局热键共用后端的一次 `unregister_all`，因此 `apply_hotkeys` 会**一起**重注册；
  改其中一个不会把另一个弄丢。注册结果按 `which` 逐个通过 `hymt2:hotkey` 事件回报。

### 平台支持

Windows / Linux(X11) 已按预期工作。小窗本身**不依赖模拟按键和全局鼠标钩子**（那是划词
翻译才需要的），所以理论上比划词翻译的平台适应面更宽；但热键本身能不能注册仍取决于
合成器，Wayland 下未实测。

---

## 翻译模式与提示词

6 种模式，收在翻译页左下角胶囊的悬浮面板里：

| 模式 | 作用 |
|---|---|
| **基础翻译** | 直接译，默认模式 |
| **术语约束** | 带术语表，术语表随请求注入 |
| **风格控制** | 12 种预设风格（学术、口语、字幕、法律合同、文言文……） |
| **背景语境** | 带一段背景信息，帮助消歧 |
| **分隔符保留** | 要求译文保留等量分隔符且位置不变 |
| **结构化数据** | JSON / HTML / XML / YAML / Markdown，保结构只译可见文本 |

`src-tauri/src/prompt.rs` 中的 6 套提示词**逐字移植**自官方 `hy-mt2-translator` skill
（见 `docs/upstream/`），并有单元测试锁定：

```bash
cd src-tauri && cargo test
```

`docs/upstream/` 是从 skillhub 下载的官方原始文件，作为比对基准留存。若提示词与官方不一致，
测试会失败——这是刻意的。**换用其他模型时如果想改写模板，改完记得同步更新那组黄金值测试。**

---

## 开发与构建

需要 Node ≥ 20、pnpm ≥ 8.6、Rust ≥ 1.77。

```bash
pnpm install

pnpm dev            # 只跑前端（浏览器调试，无 Tauri IPC）
pnpm app:dev        # 完整桌面应用开发模式
pnpm build          # 前端构建 + 类型检查
pnpm typecheck      # 仅类型检查
pnpm lint:format    # Prettier 格式化
```

前端只有三个依赖（`vue` / `@tauri-apps/plugin-dialog` / `@lucide/vue`），UI 样式全部手写。

### 三窗口同源

`main` / `popup` / `mini` 三个窗口共用同一个 `index.html`，靠窗口 label 决定挂载哪个根组件
（见 `src/main.ts`）。浏览器里调试时没有 Tauri IPC，取不到 label，按主窗口处理，
语言下拉等元数据走 `src/constants.ts` 的静态兜底。

### 流式实现

翻译走 SSE（`stream: true`），Rust 解析后通过 Tauri 事件推给前端逐 token 渲染：

```
llama-server ──SSE──► client::translate_stream ──emit──► hymt2:delta ──► Vue 增量追加
                                              └──emit──► hymt2:done  ──► 收尾 + 计时 + 存历史
                                              └──emit──► hymt2:error ──► 面板内显示
```

> **注意**：Tauri v2 的事件需要显式授权，否则前端 `listen()` 会被 ACL 拒绝且不报错，表现为「一直转圈」。
> 相关权限声明在 `src-tauri/capabilities/default.json`。

`max_tokens` 按输入长度动态分配，不填满上下文窗口——服务端按「输入 + 输出」一起算预算，
填满 8192 时哪怕输入很短也会溢出返回 400。

---

## 打包

### Windows

`pnpm app:build` 会先编译出 `src-tauri/target/release/zephyr-translator.exe`（约 6 MB），
再尝试用 NSIS 生成安装包。**NSIS 工具需要联网下载**，在国内网络下可能失败。

即使失败也不影响使用——**exe 本身是自包含的绿色版**，双击即可运行，无需安装。

若确实需要安装包，先装 NSIS 再打包：

```bash
choco install nsis
pnpm app:build
```

### Linux（Ubuntu / Debian）

Linux 这套走的是 `src-tauri/tauri.linux.conf.json`（平台配置，Tauri 会自动合并，
**不影响上面的 Windows 配置**）。

先装编译 WebKit 宿主的开发包：

```bash
sudo apt install -y \
  libwebkit2gtk-4.1-dev \
  libsoup-3.0-dev \
  librsvg2-dev \
  libgtk-3-dev \
  libayatana-appindicator3-dev \
  libdbus-1-dev
```

`libgtk-3-dev` 和 `libayatana-appindicator3-dev` 是托盘图标需要的，`libdbus-1-dev` 是 keyring 的
`dbus-secret-service` 后端需要的——三者在桌面环境里通常已经装好，纯净的构建机上别漏。不需要
`libssl-dev`（HTTP 走 rustls + aws-lc-sys）也不需要 `libxdo-dev`（依赖树里没有 xdo-sys）。

然后直接打包，产出 deb + AppImage：

```bash
pnpm app:build
# src-tauri/target/release/bundle/deb/zephyr-translator_1.0.0_amd64.deb
# src-tauri/target/release/bundle/appimage/zephyr-translator_1.0.0_amd64.AppImage
```

Linux 的三个特殊处理，都在配置里、不是代码改动：

- **`productName` 覆盖成 `zephyr-translator`**（ASCII、无空格）。deb 的 `Package:` 字段只接受
  `[a-z0-9][a-z0-9+.-]+`，含中文和空格会打不出来。窗口标题不受影响。
- **`bundle.linux.deb.desktopTemplate`** 指向 `linux/desktop.template`。tauri-bundler 的内置模板
  会把 `Categories` 留空（结果是菜单里没有分类）、`Name` 直接取 ASCII 的 productName，
  这个模板补上 `Categories=Utility;` 和可读 `Name`，`Exec`/`Icon`/`StartupWMClass` 仍由 tauri 填。
- **不带 `depends`**，用 tauri 的自动探测（它扫二进制链接的库）。手写一份会和自动探测叠加，
  在 `Depends:` 里出现重复项。

---

## 配置与数据存放

后端 API key 通过 **keyring** 写入系统凭据管理器（Windows Credential Manager / Linux Secret Service），
**不落明文配置文件**。配置文件里只存 `has_api_key: true/false`。

| 内容 | Windows | Linux |
|---|---|---|
| 后端设置 | `%APPDATA%/zephyr-translator/settings.json` | `~/.config/zephyr-translator/settings.json` |
| 术语库 | `%APPDATA%/zephyr-translator/glossary.json` | `~/.local/share/zephyr-translator/glossary.json` |
| 翻译历史 | `%APPDATA%/zephyr-translator/history.db` | `~/.local/share/zephyr-translator/history.db` |

> ⚠️ 本项目早期版本的数据目录是 `hy-mt2-translator`。改名后新旧数据**不会自动迁移**，
> 旧目录里的配置、术语库和历史需要手动拷过去；旧目录里的 API key 存在 keyring 的
> `hy-mt2-translator` service 下，需要在设置页重新填一次。

---

## 架构说明

```
src/                      Vue 前端
  views/                  六个功能页 + SelectionPopup.vue（划词弹窗）+ MiniWindow.vue（小窗模式）
  components/             BackendPicker（状态栏引擎胶囊）、LangSelect（语言下拉）
  composables/useAppState.ts  全局共享状态（设置 / 术语库 / 元数据 / 导航徽标计数）
  composables/useTheme.ts     深浅主题：跟随系统 + localStorage 记忆
  constants.ts            元数据的静态兜底（与 prompt.rs 保持一致）
  styles/base.css         设计 token（:root[data-theme] 双主题）与基础样式（纯手写）
  api.ts / types.ts       Tauri 命令封装与类型
  main.ts                 按窗口 label 决定挂载主界面 / 划词弹窗 / 小窗
src-tauri/
  src/prompt.rs           6 种模式的提示词模板（含保真度测试）+ 语言/风格/格式元数据
  src/client.rs           OpenAI 兼容 HTTP 客户端（流式 / 非流式 / 健康检查 / 语种检测）
  src/config.rs           后端配置 + keyring 凭据
  src/glossary.rs         术语库存储
  src/history.rs          历史记录（SQLite，含 starred 收藏字段）
  src/document.rs         txt/md 解析、编码识别、分段与重组
  src/selection.rs        划词翻译：全局鼠标监听 + 抓取选区 + 弹窗定位 + 钉住
  src/selection_win.rs    Windows 专用取词（UI Automation）
  src/tray.rs             托盘与全局热键
  src/error.rs            统一错误类型
  src/lib.rs              Tauri 命令层 + 窗口生命周期
docs/upstream/            官方 skill 原始文件（提示词比对基准）
scripts/gen_icons.py      应用图标生成
```

**分层原则**：Rust 是唯一的事实来源——配置、凭据、HTTP、文件 IO 全在 Rust 侧，
Vue 层只做展示。好处是划词弹窗、小窗、主窗口三个前端共用同一套后端能力，
不会出现某个窗口的行为和另一个不一致。

---

## 测试

```bash
cd src-tauri && cargo test                                                # 单元测试（含提示词保真度）
cd src-tauri && HYMT_URL=http://127.0.0.1:8080 cargo test --test stream -- --nocapture
```

`stream` / `live` 是端到端测试，模型服务未启动时会自动跳过（先做 TCP 探测）。
`HYMT_URL` 环境变量可指定要测的服务地址，默认 `http://127.0.0.1:8080`。

---

## 已知限制

- **只支持 `.txt` / `.md`**。docx / xlsx / pdf / pptx 本期未实现（pptx 缺可用 Rust 库）。
- Markdown 的代码块不翻译，仅保留原文。
- 批量/文档翻译对超长段落按段落切分，不会跨段落推断上下文。
- 划词翻译在 Linux/Wayland 下不可用，macOS 未做（详见上文「平台支持」）。
- Tauri v2 移动端（iOS/Android）尚未进入生产可用状态，故本期只做桌面端。

---

## 相关项目

本地模型服务与 CLI（MCP server、启动脚本、部署说明）在本项目的配套仓库 `hy-mt2-1.8b` 中，
包含 Hy-MT2 权重的 llama.cpp 启动方式。用别的模型时不需要它——任何 OpenAI 兼容服务都能直接接。
