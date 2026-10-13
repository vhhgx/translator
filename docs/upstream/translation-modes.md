# Translation Modes Reference

## Mode Selection Guide

| User Request | Mode |
|---|---|
| Plain text to translate | `basic` |
| "按照术语表" / provides specific term mappings | `terminology` |
| "用学术风格" / "口语化" / any style adjective | `style` |
| Text contains `@@`, `[SPLIT]`, `~#~` or other delimiters | `delimiter` |
| JSON / HTML / XML / YAML / Markdown data | `structured` |
| "结合背景" / domain context provided | `context` |

---

## Mode Details & CLI Flags

### 1. Basic (`basic`) — default
```
将以下文本翻译为{target_language}，注意只需要输出翻译后的结果，不要额外解释：

{source_text}
```
No extra flags needed.

---

### 2. Terminology (`terminology`)
```
参考下面的翻译：
{terminology_string}
将以下文本翻译为{target_language}，注意只需要输出翻译后的结果，不要额外解释：

{source_text}
```
Flag: `--terminology $'人工智能翻译成Artificial Intelligence\n机器学习翻译成Machine Learning'`

> `--terminology` accepts a **raw string** that is embedded directly into the prompt.
> Use `\n` (or a real newline with `$'...'`) to separate multiple term pairs.
> Format each pair as: `源词翻译成目标词`

---

### 3. Style (`style`)
```
请将以下文本翻译为{target_language}。
注意翻译的风格要严格符合【{target_style}】

{source_text}
```
Flag: `--style "学术论文严谨风格"`

Supported styles (non-exhaustive):
- 学术论文严谨风格
- 日常口语化风格
- 科普语气
- 商业正式语气
- 新闻报道风格
- 宣传文案风格
- 小说风格
- 法律合同风格
- 中国古代文言文

---

### 4. Delimiter-Preserving (`delimiter`)
```
请将以下文本准确翻译为{target_language}。
你必须在译文中保留等量的分隔符，绝对不可遗漏、转义或翻译该符号，并注意分隔符的位置。

{source_text}
```
Flag: `--preserve-delimiters`

Common delimiters: `@@`, `[SPLIT]`, `---`, `~#~`, `\n`

---

### 5. Structured Data (`structured`)
```
请将以下{format_type} 结构化数据翻译为{target_language}。
绝对保持原有的 {format_type} 数据结构、缩进和层级完全不变。
仅翻译面向用户展示的可见文本内容。禁止翻译代码标签、键名（key）、变量占位符或任何代码属性。

{source_text}
```
Flag: `--format-type JSON` (or HTML, Markdown, XML, YAML)

---

### 6. Context-Aware (`context`)
```
【背景信息】
{background_text}

请结合背景信息将以下文本翻译为{target_language}。

【待翻译文本】
{source_text}
```
Flag: `--context "医学检测报告场景"`
