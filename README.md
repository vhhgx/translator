# Hy-MT2 翻译工具

基于腾讯混元 **Hy-MT2** 翻译模型的跨端桌面翻译工具。Vue 3 + Rust (Tauri v2)，UI 全部手写 CSS，不依赖任何 UI 框架。

Rust 只负责界面宿主与 IO —— 配置、凭据、HTTP 调用、文档解析；翻译推理由本地 `llama-server` 或任意 OpenAI 兼容云端接口完成。

## 功能

| 模块 | 说明 |
|---|---|
| **翻译** | 6 种模式：基础 / 术语约束 / 风格控制 / 背景语境 / 分隔符保留 / 结构化数据 |
| **后端切换** | 顶部下拉实时切换；可配置多个 OpenAI 兼容端点（baseurl + 模型名 + API key） |
| **术语库** | 多词库增删改查、JSON 导入导出、按语言对组织、可只取原文命中的词 |
| **历史** | SQLite 本地存储，支持搜索、复用 |
| **批量** | JSONL 读入，并发翻译，输出追加 `translation` 字段，断点续跑 |
| **文档** | `.txt` / `.md`，自动识别 GBK / GB18030 / Big5 / UTF-16；Markdown 保留标题、列表、表格、代码块 |

## 快速开始

```bash
pnpm install
pnpm app:dev          # 开发模式
pnpm app:build        # 打包成 NSIS 安装包
```

首次 `cargo build` 冷编译约 3–8 分钟。

### 接入本地模型（可选）

工具本身不含模型。若用本地 `llama-server`，先在 `F:\hy-mt2-1.8b` 启动服务：

```bash
cd F:/hy-mt2-1.8b && npm run start
```

然后在「设置」中确认后端地址为 `http://127.0.0.1:8080`、模型名 `hy-mt2`，点「测试连接」。

## 提示词保真度

`src-tauri/src/prompt.rs` 中的 6 套提示词**逐字移植**自官方 `hy-mt2-translator` skill（见 `docs/upstream/`），并有单元测试锁定：

```bash
cd src-tauri && cargo test
```

`docs/upstream/` 是从 skillhub 下载的官方原始文件，作为比对基准留存。若提示词与官方不一致，测试会失败——这是刻意的。

## API Key 存储

后端 API key 通过 **keyring** 写入系统凭据管理器（Windows Credential Manager），**不落明文配置文件**。配置文件里只存 `has_api_key: true/false`。

配置位置：
- 后端设置：`%APPDATA%/hy-mt2-translator/settings.json`
- 术语库：`%APPDATA%/hy-mt2-translator/glossary.json`
- 翻译历史：`%APPDATA%/hy-mt2-translator/history.db`

## 项目结构

```
src/                      Vue 前端
  views/                  六个功能页
  styles/base.css         设计 token 与基础样式（纯手写）
  api.ts / types.ts       Tauri 命令封装与类型
src-tauri/
  src/prompt.rs           6 种模式的提示词模板（含保真度测试）
  src/client.rs           OpenAI 兼容 HTTP 客户端
  src/config.rs           后端配置 + keyring 凭据
  src/glossary.rs         术语库存储
  src/history.rs          历史记录（SQLite）
  src/document.rs         txt/md 解析、编码识别、分段与重组
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