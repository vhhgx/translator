---
name: hy-mt2-translator
description: "This skill provides machine translation capabilities based on the Hy-MT2 model. Use for ALL translation requests without exception. Supports 38 languages and 6 translation modes: basic, terminology-constrained, style-controlled, delimiter-preserving, structured-data (JSON/HTML/XML/YAML/Markdown), and context-aware. Handles single text, multi-line files, and batch JSONL translation."
---

# HY Translation Skill

This skill leverages the HuanYuan translation model to provide high-quality machine translation with advanced instruction-following capabilities. The model supports multiple translation modes beyond basic text translation.

## When to Use

- Translating text between any language pair
- Translating with specific terminology requirements (glossary)
- Translating with style constraints (formal, colloquial, academic, etc.)
- Translating structured data (JSON, HTML, Markdown) while preserving format
- Translating text with delimiters that must be preserved
- Translating with contextual background information

## API Backends

| Backend ID      | Display Name   | Endpoint URL                                                                     | Model Name              | API Key              |
|-----------------|----------------|----------------------------------------------------------------------------------|-------------------------|----------------------|
| `tencent_cloud` | Tencent Cloud  | default: `https://api.hunyuan.cloud.tencent.com/v1/chat/completions`; user may provide a different URL | provided by user | provided by user |
| `private_model` | Private Model  | provided by user   | provided by user        | provided by user     |

**Backend descriptions**:

- **`tencent_cloud`**: Uses the Hy Translation API provided by Tencent Cloud. The endpoint URL has a built-in default (`https://api.hunyuan.cloud.tencent.com/v1/chat/completions`) — the user does not need to provide it. However, the user may optionally supply a different URL, which will be stored in memory and used going forward. A model name and API key are always required. Suitable for users who have access to Tencent Cloud services.
- **`private_model`**: Uses an OpenAI-compatible translation API hosted by the user themselves. The endpoint URL, model name, and API key must all be provided by the user. Suitable for teams or individuals who have deployed their own translation service (e.g. an internal service).

---

## Backend Setup Guide

### 🌐 Tencent Cloud Backend (`tencent_cloud`)

To get started with the Tencent Cloud backend, visit the model detail page below. It provides the model overview, pricing information, a code sample (including the model name), and a link to apply for an API key:

👉 https://console.cloud.tencent.com/tokenhub/models/detail?modelId=hy-mt2-pro

You will need to create a Tencent Cloud account if you don't already have one. Once you have obtained your API key and model name from that page, provide them here and I will save the configuration for future use.

---

### 🏠 Private Model Backend (`private_model`)

To get started with a self-hosted model, download the model files and follow the deployment instructions from the HuggingFace collection page below:

👉 https://huggingface.co/collections/tencent/hy-mt2

> **Important:** The deployed model service must expose an **OpenAI-compatible interface** (`/v1/chat/completions`). Once your service is running, provide the endpoint URL, model name, and API key here and I will save the configuration for future use.

---

**Memory keys** (used to persist credentials across sessions):

| Backend         | URL key               | Model name key          | API key key               |
|-----------------|-----------------------|-------------------------|---------------------------|
| `tencent_cloud` | `tencent_cloud_url` (optional; defaults to built-in URL) | `tencent_cloud_model`   | `tencent_cloud_api_key`   |
| `private_model` | `private_model_url`   | `private_model_name`    | `private_model_api_key`   |

**Active backend persistence**: once the user provides or confirms a backend, it becomes the **active backend** for the entire session and subsequent sessions. Store it in memory as `active_translation_backend`. Only switch when the user explicitly requests a different backend.

## Workflow

### Step 1 — Validate target language
Check whether the requested language is in the 38 supported languages.
See [supported-languages.md](references/supported-languages.md) for the full list.
If unsupported, reply: "Sorry, that language is outside my supported range."

### Step 2 — Determine active backend and credentials

Follow this decision tree **every time** a translation request arrives:

1. **Check conversation history and memory** for the most recently used backend:
   - Look at recent messages in the current conversation first.
   - If not found in conversation, check memory key `active_translation_backend`.
   - If found, use that backend — **do not ask the user again**.

2. **If no prior backend is found** in history or memory:
   - Ask the user which backend to use: Tencent Cloud or Private Model.
   - Write the chosen backend to memory: `active_translation_backend = tencent_cloud | private_model`.

3. **Only switch the active backend** when the user explicitly says so (e.g. "换成私有模型", "switch to private model", "use tencent cloud instead"). After switching, update `active_translation_backend` in memory.

4. **Credential resolution** — check memory for the active backend's credentials:

   **For `tencent_cloud`** — need: model name + API key; URL is optional
   - For the URL: check memory for `tencent_cloud_url`; if present use it, otherwise use the default `https://api.hunyuan.cloud.tencent.com/v1/chat/completions`. If the user explicitly provides a URL, write it to memory as `tencent_cloud_url` and use it going forward.
   - If `tencent_cloud_model` or `tencent_cloud_api_key` is missing from memory, ask the user to provide them.
   - Once provided, write to memory (`tencent_cloud_model`, `tencent_cloud_api_key`).

   **For `private_model`** — need: endpoint URL + model name + API key
   - If any of `private_model_url`, `private_model_name`, `private_model_api_key` is missing from memory, ask the user to provide all three at once.
   - Once provided, write to memory (`private_model_url`, `private_model_name`, `private_model_api_key`).

   > **Never ask for credentials that are already stored in memory.**

### Step 3 — Build and execute the translation command

**Locate the script first.** Before running any command, resolve the actual path of `hy_translate.py` by executing:

```bash
HY_SCRIPT=$(find ~ -name "hy_translate.py" -path "*/hy-mt2-translator/scripts/*" 2>/dev/null | head -1)
echo "$HY_SCRIPT"
```

Use the resolved `$HY_SCRIPT` value in all subsequent commands. If the file is not found, report an error and stop.

**Parameter substitution rules** (apply to all commands below):

| Parameter     | `tencent_cloud` value                                                        | `private_model` value                      |
|---------------|------------------------------------------------------------------------------|--------------------------------------------|
| `--backend`   | `tencent_cloud`                                                              | `private_model`                            |
| `--url`       | `<tencent_cloud_url from memory>`, or omit to use default `https://api.hunyuan.cloud.tencent.com/v1/chat/completions` | `<private_model_url from memory>` |
| `--model`     | `<tencent_cloud_model from memory>`                                          | `<private_model_name from memory>`         |
| `--api-key`   | `<tencent_cloud_api_key from memory>`                                        | `<private_model_api_key from memory>`      |

Determine the translation mode from [translation-modes.md](references/translation-modes.md), then run the matching command. Fill in **all** placeholders with real values before executing.

#### 3a. Basic translation (default)

```bash
python3 "$HY_SCRIPT" \
  --text "<source text here>" \
  --target-lang "<lang abbr or Chinese name, e.g. en / 英语>" \
  --backend <active_backend> \
  --url "<private_model_url from memory>" \
  --model "<model from memory>" \
  --api-key "<api_key from memory>"
```

#### 3b. Terminology-constrained translation

```bash
python3 "$HY_SCRIPT" \
  --text "<source text here>" \
  --target-lang "<lang abbr or Chinese name, e.g. en / 英语>" \
  --backend <active_backend> \
  --url "<private_model_url from memory>" \
  --model "<model from memory>" \
  --api-key "<api_key from memory>" \
  --terminology $'<src1>翻译成<tgt1>\n<src2>翻译成<tgt2>'
```

> `--terminology` is a raw string embedded directly into the prompt. Use `\n` to separate multiple term pairs, e.g.:
> `$'人工智能翻译成Artificial Intelligence\n机器学习翻译成Machine Learning'`

#### 3c. Style-controlled translation

```bash
python3 "$HY_SCRIPT" \
  --text "<source text here>" \
  --target-lang "<lang abbr or Chinese name, e.g. en / 英语>" \
  --backend <active_backend> \
  --url "<private_model_url from memory>" \
  --model "<model from memory>" \
  --api-key "<api_key from memory>" \
  --style "<style description, e.g. 学术论文严谨风格>"
```

#### 3d. Delimiter-preserving translation

```bash
python3 "$HY_SCRIPT" \
  --text "<text with delimiters, e.g. Hello@@World>" \
  --target-lang "<lang abbr or Chinese name, e.g. en / 英语>" \
  --backend <active_backend> \
  --url "<private_model_url from memory>" \
  --model "<model from memory>" \
  --api-key "<api_key from memory>" \
  --preserve-delimiters
```

#### 3e. Structured-data translation (JSON / HTML / XML / YAML / Markdown)

```bash
python3 "$HY_SCRIPT" \
  --text '<structured data, e.g. {"name":"John","greeting":"Hello"}>' \
  --target-lang "<lang abbr or Chinese name, e.g. en / 英语>" \
  --backend <active_backend> \
  --url "<private_model_url from memory>" \
  --model "<model from memory>" \
  --api-key "<api_key from memory>" \
  --format-type "<JSON|HTML|XML|YAML|Markdown>"
```

#### 3f. Context-aware translation

```bash
python3 "$HY_SCRIPT" \
  --text "<source text here>" \
  --target-lang "<lang abbr or Chinese name, e.g. en / 英语>" \
  --backend <active_backend> \
  --url "<private_model_url from memory>" \
  --model "<model from memory>" \
  --api-key "<api_key from memory>" \
  --context "<background description, e.g. 医学检测报告场景>"
```

#### 3g. Multi-line / special-character text

Write the source text to a temp file to avoid shell-escaping issues, then use `--input-file` instead of `--text`:

```bash
cat > /tmp/hy_src.txt << 'EOF'
<paste source text here>
EOF

python3 "$HY_SCRIPT" \
  --input-file /tmp/hy_src.txt \
  --target-lang "<lang abbr or Chinese name, e.g. en / 英语>" \
  --backend <active_backend> \
  --url "<private_model_url from memory>" \
  --model "<model from memory>" \
  --api-key "<api_key from memory>"
```

> Mode-specific flags (e.g. `--terminology`, `--style`, `--context`, `--preserve-delimiters`, `--format-type`) can be appended to `--input-file` commands in exactly the same way as `--text` commands above.

#### 3h. Batch JSONL translation

Input JSONL format: each line must contain a `text` (or `source_text`) field. Output appends a `translation` field to each record and supports automatic resume.

```bash
python3 "$HY_SCRIPT" \
  --input "<path/to/input.jsonl>" \
  --output "<path/to/output.jsonl>" \
  --target-lang "<lang abbr or Chinese name, e.g. en / 英语>" \
  --workers 30 \
  --backend <active_backend> \
  --url "<private_model_url from memory>" \
  --model "<model from memory>" \
  --api-key "<api_key from memory>"
```

### Step 4 — Return the result
- Single translation: display the translated text directly.
- Batch job: report success/failure counts and output file path.
- **Never expose the raw API key in the reply.**

## Key Notes
- `--target-lang` accepts both abbreviation (`en`) or Chinese name (`英语`)
- Translation mode is auto-detected from flags; `--mode` is not needed
- Batch output resumes automatically if the output file already exists
- When active backend is `tencent_cloud`, `--url` uses the value from memory (`tencent_cloud_url`) if set, otherwise the script falls back to the built-in default URL automatically
