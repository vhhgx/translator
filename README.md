# Hy-MT2 翻译工具

基于腾讯混元 **Hy-MT2** 翻译模型的跨端桌面翻译工具。Vue 3 + Rust (Tauri v2)，UI 全部手写 CSS，不依赖任何 UI 框架。

Rust 只负责界面宿主与 IO —— 配置、凭据、HTTP 调用、文档解析；翻译推理由本地 `llama-server` 或任意 OpenAI 兼容云端接口完成。

## 功能

| 模块 | 说明 |
|---|---|
| **翻译** | 6 种模式：基础 / 术语约束 / 风格控制 / 背景语境 / 分隔符保留 / 结构化数据。**流式输出**，逐 token 渲染 |
| **后端切换** | 顶部下拉实时切换；可配置多个 OpenAI 兼容端点（baseurl + 模型名 + API key） |
| **术语库** | 多词库增删改查、JSON 导入导出、按语言对组织、可只取原文命中的词 |
| **历史** | SQLite 本地存储，支持搜索、复用 |
| **批量** | JSONL 读入，并发翻译，输出追加 `translation` 字段，断点续跑 |
| **文档** | `.txt` / `.md`，自动识别 GBK / GB18030 / Big5 / UTF-16；Markdown 保留标题、列表、表格、代码块 |
| **划词翻译** | Windows / Linux(X11) 下选中文本即自动弹窗翻译；见下文 |
| **小窗模式** | 热键唤出「输入 + 译文」小窗，不动主窗口；见下文 |

界面参考 Google 翻译：语言 Tab 常驻在两个卡片上方，中间是交换按钮，底部是字数统计与操作按钮；六种翻译模式收在「高级」面板里。全部手写 CSS，设计 token 用 CSS 变量统一。

## 快速开始

```bash
pnpm install
pnpm app:dev          # 开发模式
pnpm app:build        # 打包 release
```

首次 `cargo build` 冷编译约 3–8 分钟。

### 关于安装包

`pnpm app:build` 会先编译出 `src-tauri/target/release/hy-mt2-translator.exe`（约 6 MB），再尝试用 NSIS 生成安装包。**NSIS 工具需要联网下载**，在国内网络下可能失败。

即使失败也不影响使用——**exe 本身是自包含的绿色版**，双击即可运行，无需安装。已验证的副本在项目根目录 `Hy-MT2-翻译工具.exe`。

若确实需要安装包，先装 NSIS 再打包：

```bash
choco install nsis
pnpm app:build
```

### Linux 打包（Ubuntu / Debian）

Linux 这套走的是 `src-tauri/tauri.linux.conf.json`（平台配置，Tauri 会自动合并，**不影响上面的 Windows 配置**）。

先装编译 WebKit 宿主的开发包：

```bash
sudo apt install -y libwebkit2gtk-4.1-dev libsoup-3.0-dev librsvg2-dev
```

`libgtk-3-dev` 和 `libayatana-appindicator3-dev` 也要有（托盘图标需要）。`libdbus-1-dev` 是
keyring 的 `dbus-secret-service` 后端要用的，通常随桌面环境已在。不需要 `libssl-dev`
（HTTP 走 rustls + aws-lc-sys）也不需要 `libxdo-dev`（依赖树里没有 xdo-sys）。

然后直接打包，产出 deb + AppImage：

```bash
pnpm app:build
# src-tauri/target/release/bundle/deb/hy-mt2-translator_0.1.0_amd64.deb
# src-tauri/target/release/bundle/appimage/hy-mt2-translator_0.1.0_amd64.AppImage
```

三个 Linux 特有的改动，都不是代码改动：

- **`productName` 覆盖成 `hy-mt2-translator`**。原来的「Hy-MT2 翻译工具」含中文和空格，
  deb 的 `Package:` 字段只接受 `[a-z0-9][a-z0-9+.-]+`，会打不出来。窗口标题不受影响，仍是中文。
- **`bundle.linux.deb.desktopTemplate`** 指向 `linux/desktop.template`。tauri-bundler 的内置模板
  会把 `Categories` 留空（结果是菜单里没有分类）、`Name` 直接取 ASCII 的 productName，
  这个模板补上 `Categories=Utility;` 和中文 `Name`，`Exec`/`Icon`/`StartupWMClass` 仍由 tauri 填。
- **不带 `depends`**，用 tauri 的自动探测（它扫二进制链接的库）。手写一份会和自动探测叠加，
  在 `Depends:` 里出现重复项。

### 关于安装包

### 接入本地模型（可选）

工具本身不含模型。若用本地 `llama-server`，先在 `F:\hy-mt2-1.8b` 启动服务：

```bash
cd F:/hy-mt2-1.8b && npm run start
```

然后在「设置」中确认后端地址为 `http://127.0.0.1:8080`、模型名 `hy-mt2`，点「测试连接」。

Linux 上的对应目录是 `~/workspace/www/hy-mt2-1.8b`（脚本、权重与部署说明都在那边的 README）：

```bash
cd ~/workspace/www/hy-mt2-1.8b && ./start-server.sh
```

它默认监听 `127.0.0.1:8081`（本机 8080 已被别的服务占用），所以要在「设置」里把该后端的
baseurl 一并改成 `http://127.0.0.1:8081`。另外注意工具默认 `temperature: 0.1` 会覆盖服务端的
官方推荐值 0.7——客户端只发 temperature，`top_p`/`top_k`/`repetition_penalty` 才走服务端默认。

## 划词翻译（选中后自动弹窗）

选中一段文本，不用按任何键，光标旁直接浮出译文。

**开关**：设置页 →「窗口唤起与系统集成」→「划词翻译」。**默认关闭**——它要装一个全局
鼠标钩子，且每次划词都会动一下剪贴板，属于需要用户明确同意的能力。

**触发修饰键**：可以要求「按住 Ctrl / Alt / Shift 才触发」。默认「不要求」（任何拖选都弹）；
嫌吵就设成某个修饰键。

### 链路

```
全局鼠标事件 (rdev / XRecord) ──左键释放──► capture_selected_text()
                                              │ 清空剪贴板当哨兵 → 模拟 Ctrl+C → 读回 → 还原
                                              ▼
                        hymt2:selection ──► popup 窗口 ──► translate_stream
```

- **抓选中文本**：`src-tauri/src/selection.rs`。用的是划词工具的通用手法——先把剪贴板清空
  当哨兵，模拟一次 Ctrl+C，再读回剪贴板；读回来还是空的，就说明这次点击根本没选中东西。
  用户原来的剪贴板内容会还原。
- **弹窗**：`label: "popup"` 的独立窗口（460×300，无边框 + 透明 + 置顶 + 不抢焦点 + 不占
  任务栏），前端在 `src/views/SelectionPopup.vue`。它和主窗口共用同一个 `index.html`，
  靠窗口 label 决定挂载哪个根组件。
- **不串台**：弹窗和主窗口都走 `translate_stream`，两边各自按 `job_id` 过滤事件。

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

## 小窗模式（热键唤出快速翻译）

按热键唤出一个「输入 + 译文」的小窗，**不动主窗口**——只想快速译一句的时候，再弹一个
960×640 的主界面反而碍事。要进主界面，点小窗右上角那个按钮。

- **默认热键** `Ctrl+Shift+S`（设置页可改、也可清除）
- **Enter** 翻译，**Shift+Enter** 换行
- **Esc** 或**点到别处**（失焦）即收起
- 右上角三个按钮：从剪贴板填入 / 在主窗口打开 / 关闭
- 目标语言与划词弹窗**共用同一份选择**（`localStorage` 的 `hymt2:popup-target`）

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

## 提示词保真度

`src-tauri/src/prompt.rs` 中的 6 套提示词**逐字移植**自官方 `hy-mt2-translator` skill（见 `docs/upstream/`），并有单元测试锁定：

```bash
cd src-tauri && cargo test
```

`docs/upstream/` 是从 skillhub 下载的官方原始文件，作为比对基准留存。若提示词与官方不一致，测试会失败——这是刻意的。

## 流式实现

翻译走 SSE（`stream: true`），Rust 解析后通过 Tauri 事件推给前端逐 token 渲染：

```
llama-server ──SSE──► client::translate_stream ──emit──► hymt2:delta ──► Vue 增量追加
                                              └──emit──► hymt2:done  ──► 收尾 + 计时 + 存历史
                                              └──emit──► hymt2:error ──► 面板内显示
```

> **注意**：Tauri v2 的事件需要显式授权，否则前端 `listen()` 会被 ACL 拒绝且不报错，表现为「一直转圈」。相关权限声明在 `src-tauri/capabilities/default.json`。

测试：

```bash
cd src-tauri && cargo test            # 含流式一致性测试
HYMT_URL=http://127.0.0.1:8080 cargo test --test stream -- --nocapture
```

模型服务未启动时端到端测试会自动跳过。

## API Key 存储

后端 API key 通过 **keyring** 写入系统凭据管理器（Windows Credential Manager），**不落明文配置文件**。配置文件里只存 `has_api_key: true/false`。

配置位置：
- 后端设置：`%APPDATA%/hy-mt2-translator/settings.json`
- 术语库：`%APPDATA%/hy-mt2-translator/glossary.json`
- 翻译历史：`%APPDATA%/hy-mt2-translator/history.db`

## 项目结构

```
src/                      Vue 前端
  views/                  六个功能页 + SelectionPopup.vue（划词弹窗）+ MiniWindow.vue（小窗模式）
  styles/base.css         设计 token 与基础样式（纯手写）
  api.ts / types.ts       Tauri 命令封装与类型
  main.ts                 按窗口 label 决定挂载主界面 / 划词弹窗 / 小窗
src-tauri/
  src/prompt.rs           6 种模式的提示词模板（含保真度测试）
  src/client.rs           OpenAI 兼容 HTTP 客户端
  src/config.rs           后端配置 + keyring 凭据
  src/glossary.rs         术语库存储
  src/history.rs          历史记录（SQLite）
  src/document.rs         txt/md 解析、编码识别、分段与重组
  src/selection.rs        划词翻译：全局鼠标监听 + 抓选中文本 + 弹窗定位
  src/tray.rs             托盘与全局热键
  src/lib.rs              Tauri 命令层
docs/upstream/            官方 skill 原始文件（比对基准）
scripts/gen_icons.py      应用图标生成
```

## 已知限制

- **只支持 `.txt` / `.md`**。docx / xlsx / pdf / pptx 本期未实现（pptx 缺可用 Rust 库）。
- Markdown 的代码块不翻译，仅保留原文。
- Tauri v2 移动端（iOS/Android）尚未进入生产可用状态，故本期只做桌面端。
- 批量/文档翻译对超长段落按段落切分，不会跨段落推断上下文。

## 相关项目

`F:\hy-mt2-1.8b` —— 本地模型服务与 CLI（MCP server、npm scripts 都在那边）。