#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
hy_translate.py - Hy Translation Skill script

Supports two API backends via OpenAI-compatible interface:
  - tencent_cloud : Tencent Cloud Hy  (https://api.hunyuan.cloud.tencent.com/v1/chat/completions)
  - private_model : Self-hosted private model service (pass --url and --model at runtime)

Usage examples:
  # Basic (Tencent Cloud, default)
  python3 hy_translate.py --text "Hello" --target-lang "中文" --api-key "sk-xxx"

  # Basic (Private Model)
  python3 hy_translate.py --text "Hello" --target-lang "中文" \
      --backend private_model --api-key "sk-xxx"

  # Terminology-constrained
  python3 hy_translate.py --text "Void Walker" --target-lang "日语" \
      --terminology "Void Walker=虚空を歩む者" --api-key "sk-xxx"

  # Style-controlled
  python3 hy_translate.py --text "Hello" --target-lang "中文" \
      --style "学术论文严谨风格" --api-key "sk-xxx"

  # Delimiter-preserving
  python3 hy_translate.py --text "Hello@@World" --target-lang "中文" \
      --preserve-delimiters --api-key "sk-xxx"

  # Structured data (JSON / HTML / XML / YAML / Markdown)
  python3 hy_translate.py --text '{"name":"John","greeting":"Hello"}' \
      --target-lang "中文" --format-type JSON --api-key "sk-xxx"

  # Context-aware
  python3 hy_translate.py --text "positive" --target-lang "中文" \
      --context "医学检测报告场景" --api-key "sk-xxx"

  # Multi-line text via file
  python3 hy_translate.py --input-file /tmp/hy_src.txt \
      --target-lang "英语" --api-key "sk-xxx"

  # Batch JSONL
  python3 hy_translate.py --input input.jsonl --output output.jsonl \
      --target-lang "中文" --workers 30 --api-key "sk-xxx"
"""

import os
import sys
import json
import argparse
import logging
import time
import uuid
import threading
import requests
from concurrent.futures import ThreadPoolExecutor, as_completed

logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s [%(levelname)s] %(message)s',
    datefmt='%Y-%m-%d %H:%M:%S'
)
logger = logging.getLogger(__name__)
WRITE_LOCK = threading.Lock()

# ===================== API Backend Configuration =====================
# tencent_cloud : Tencent Cloud Hy (default)
# private_model : Self-hosted private model service
API_BACKENDS = {
    "tencent_cloud": {
        "display_name": "Tencent Cloud",
        "url": "https://api.hunyuan.cloud.tencent.com/v1/chat/completions",
        "default_model": "",
    },
    "private_model": {
        "display_name": "Private Model",
        "url": "",
        "default_model": "",
    },
}

MAX_RETRIES = 3
REQUEST_TIMEOUT = 300

# ===================== Supported Languages =====================
SUPPORTED_LANGUAGES = {
    "zh": "中文", "en": "英语", "fr": "法语", "pt": "葡萄牙语",
    "es": "西班牙语", "ja": "日语", "tr": "土耳其语", "ru": "俄语",
    "ar": "阿拉伯语", "ko": "韩语", "th": "泰语", "it": "意大利语",
    "de": "德语", "vi": "越南语", "ms": "马来语", "id": "印尼语",
    "tl": "菲律宾语", "hi": "印地语", "zh-Hant": "繁体中文",
    "pl": "波兰语", "cs": "捷克语", "nl": "荷兰语", "km": "高棉语",
    "my": "缅甸语", "fa": "波斯语", "gu": "古吉拉特语", "ur": "乌尔都语",
    "te": "泰卢固语", "mr": "马拉地语", "he": "希伯来语", "bn": "孟加拉语",
    "ta": "泰米尔语", "uk": "乌克兰语", "bo": "藏语", "kk": "哈萨克语",
    "mn": "蒙古语", "ug": "维吾尔语", "yue": "粤语",
}
LANG_NAME_TO_ABBR = {v: k for k, v in SUPPORTED_LANGUAGES.items()}


def resolve_language(lang: str) -> str:
    """Resolve language abbreviation or Chinese name to Chinese display name."""
    if lang in SUPPORTED_LANGUAGES:
        return SUPPORTED_LANGUAGES[lang]
    if lang in LANG_NAME_TO_ABBR:
        return lang
    raise ValueError(
        f"Unsupported language: '{lang}'.\n"
        f"Supported abbreviations: {', '.join(SUPPORTED_LANGUAGES.keys())}\n"
        f"Supported Chinese names: {', '.join(LANG_NAME_TO_ABBR.keys())}"
    )


def build_prompt(source_text: str, target_lang: str, mode: str = "basic", **kwargs) -> str:
    """Build translation prompt based on mode."""
    if mode == "basic":
        return (
            f"将以下文本翻译为{target_lang}，"
            f"注意只需要输出翻译后的结果，不要额外解释：\n\n{source_text}"
        )
    elif mode == "terminology":
        # terminology is passed as a raw string and embedded directly into the prompt.
        # Expected format (as provided by the caller):
        #   "src1翻译成tgt1\nsrc2翻译成tgt2\n..."
        # or any human-readable term-mapping text.
        term_str = kwargs.get("terminology", "")
        return (
            f"参考下面的翻译：\n{term_str}\n"
            f"将以下文本翻译为{target_lang}，"
            f"注意只需要输出翻译后的结果，不要额外解释：\n\n{source_text}"
        )
    elif mode == "style":
        style = kwargs.get("style", "日常口语化风格")
        return (
            f"请将以下文本翻译为{target_lang}。\n"
            f"注意翻译的风格要严格符合【{style}】\n\n{source_text}"
        )
    elif mode == "delimiter":
        return (
            f"请将以下文本准确翻译为{target_lang}。\n"
            f"你必须在译文中保留等量的分隔符，绝对不可遗漏、转义或翻译该符号，"
            f"并注意分隔符的位置。\n\n{source_text}"
        )
    elif mode == "structured":
        fmt = kwargs.get("format_type", "JSON")
        return (
            f"请将以下{fmt} 结构化数据翻译为{target_lang}。\n"
            f"绝对保持原有的 {fmt} 数据结构、缩进和层级完全不变。\n"
            f"仅翻译面向用户展示的可见文本内容。"
            f"禁止翻译代码标签、键名（key）、变量占位符或任何代码属性。\n\n\n{source_text}"
        )
    elif mode == "context":
        context = kwargs.get("context", "")
        return (
            f"【背景信息】\n{context}\n\n"
            f"请结合背景信息将以下文本翻译为{target_lang}。\n\n"
            f"【待翻译文本】\n{source_text}"
        )
    else:
        raise ValueError(f"Unknown mode: {mode}")


def call_api(backend: str, api_key: str, prompt: str, model: str = None, url: str = None) -> str:
    """Call translation API via OpenAI-compatible interface."""
    cfg = API_BACKENDS[backend]
    url = url or cfg["url"]
    model = model or cfg["default_model"]

    if not url:
        raise ValueError(
            f"No endpoint URL provided for backend '{backend}'. "
            f"Pass --url <endpoint> on the command line."
        )

    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {api_key}",
    }
    payload = {
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.1,
        "max_tokens": 4096,
        "stream": False,
    }

    for attempt in range(MAX_RETRIES):
        try:
            resp = requests.post(url, headers=headers, json=payload, timeout=REQUEST_TIMEOUT)
            if resp.status_code == 200:
                data = resp.json()
                msg = data["choices"][0]["message"]
                content = msg.get("content") or msg.get("reasoning_content") or ""
                return content.strip()
            else:
                logger.warning(f"HTTP {resp.status_code} (attempt {attempt+1}): {resp.text[:200]}")
        except Exception as e:
            logger.warning(f"Request error (attempt {attempt+1}): {e}")

        if attempt < MAX_RETRIES - 1:
            time.sleep(1 * (attempt + 1))

    return ""


def auto_detect_mode(**kwargs) -> str:
    if kwargs.get("terminology"):
        return "terminology"
    if kwargs.get("style"):
        return "style"
    if kwargs.get("format_type"):
        return "structured"
    if kwargs.get("context"):
        return "context"
    if kwargs.get("preserve_delimiters"):
        return "delimiter"
    return "basic"


def process_jsonl_file(input_path, output_path, backend, api_key, target_lang,
                       mode="basic", max_workers=30, limit=None, model=None, url=None, **kwargs):
    """Batch-translate a JSONL file. Supports resume."""
    data = []
    with open(input_path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line:
                data.append(json.loads(line))
    if limit:
        data = data[:limit]

    existing = 0
    if os.path.exists(output_path):
        with open(output_path, "r", encoding="utf-8") as f:
            for line in f:
                if line.strip():
                    existing += 1
        if existing:
            logger.info(f"Resuming: skipping {existing} already processed records")
            data = data[existing:]

    if not data:
        logger.info("All records already processed")
        return

    total = len(data)
    success_count = fail_count = 0
    start_time = time.time()

    def process_one(idx, item):
        text = item.get("text") or item.get("source_text") or item.get("origin_text", "")
        item_target = item.get("target_lang", target_lang)
        item_mode = item.get("mode", mode)
        item_kwargs = dict(kwargs)
        for k in ("terminology", "style", "context", "format_type", "preserve_delimiters"):
            if item.get(k):
                item_kwargs[k] = item[k]
        if item_kwargs.get("preserve_delimiters"):
            item_mode = "delimiter"

        prompt = build_prompt(text, item_target, mode=item_mode, **item_kwargs)
        result = call_api(backend, api_key, prompt, model=model, url=url)

        output_item = dict(item)
        output_item["translation"] = result
        with WRITE_LOCK:
            with open(output_path, "a", encoding="utf-8") as f:
                f.write(json.dumps(output_item, ensure_ascii=False) + "\n")
        return idx, result

    with ThreadPoolExecutor(max_workers=max_workers) as executor:
        futures = {executor.submit(process_one, i, item): i for i, item in enumerate(data)}
        for future in as_completed(futures):
            try:
                _, result = future.result()
                if result:
                    success_count += 1
                else:
                    fail_count += 1
                done = success_count + fail_count
                if done % 50 == 0 or done == total:
                    elapsed = time.time() - start_time
                    logger.info(
                        f"Progress: {done}/{total} | success={success_count} fail={fail_count} | "
                        f"QPS={done/elapsed:.1f}"
                    )
            except Exception as e:
                fail_count += 1
                logger.error(f"Item error: {e}")

    elapsed = time.time() - start_time
    logger.info(
        f"\n{'='*50}\nBatch complete!\n"
        f"  Total: {total} | Success: {success_count} | Failed: {fail_count}\n"
        f"  Time: {elapsed:.1f}s | QPS: {total/elapsed:.2f}\n"
        f"  Output: {output_path}\n{'='*50}"
    )


def main():
    parser = argparse.ArgumentParser(description="HY Translation Tool")
    parser.add_argument("--text", "-t", help="Text to translate (single mode)")
    parser.add_argument("--input-file", help="Read source text from a file (alternative to --text)")
    parser.add_argument("--target-lang", "-l", required=True,
                        help="Target language abbreviation or Chinese name (e.g. en, 英语)")
    parser.add_argument("--backend", default="tencent_cloud",
                        choices=["tencent_cloud", "private_model"],
                        help="API backend: tencent_cloud (default) or private_model")
    parser.add_argument("--api-key", required=True, help="API key")
    parser.add_argument("--url", help="Override endpoint URL (required for private_model backend)")
    parser.add_argument("--model", help="Override default model name (required for private_model backend)")
    parser.add_argument("--input", "-i", help="Input JSONL file for batch mode")
    parser.add_argument("--output", "-o", help="Output JSONL file for batch mode")
    parser.add_argument("--workers", "-w", type=int, default=30, help="Concurrent workers")
    parser.add_argument("--limit", type=int, help="Process only first N records")

    # Mode-specific
    parser.add_argument("--terminology", help='Term pairs: "src1=tgt1,src2=tgt2"')
    parser.add_argument("--style", help="Translation style")
    parser.add_argument("--format-type", help="Structured format: JSON/HTML/Markdown/XML/YAML")
    parser.add_argument("--context", help="Background context for disambiguation")
    parser.add_argument("--preserve-delimiters", action="store_true",
                        help="Preserve delimiters (@@, [SPLIT], etc.)")

    args = parser.parse_args()

    # Resolve language
    try:
        target_language = resolve_language(args.target_lang)
    except ValueError as e:
        print(f"[ERROR] {e}", file=sys.stderr)
        sys.exit(1)

    # Resolve API key
    api_key = args.api_key

    # Validate private_model requires --url and --model
    if args.backend == "private_model":
        if not args.url:
            parser.error("--url is required when using --backend private_model")
        if not args.model:
            parser.error("--model is required when using --backend private_model")

    # Build kwargs
    kwargs = {}
    if args.terminology:
        # Pass terminology string directly; no parsing into dict/list.
        # The caller is responsible for formatting it as ready-to-use prompt text,
        # e.g.: "人工智能翻译成Artificial Intelligence\n机器学习翻译成Machine Learning"
        kwargs["terminology"] = args.terminology
    if args.style:
        kwargs["style"] = args.style
    if args.format_type:
        kwargs["format_type"] = args.format_type
    if args.context:
        kwargs["context"] = args.context
    if args.preserve_delimiters:
        kwargs["preserve_delimiters"] = True

    # Resolve source text (--text or --input-file)
    single_text = args.text
    if not single_text and args.input_file:
        with open(args.input_file, "r", encoding="utf-8") as f:
            single_text = f.read()

    if single_text:
        mode = auto_detect_mode(**kwargs)
        prompt = build_prompt(single_text, target_language, mode=mode, **kwargs)
        result = call_api(args.backend, api_key, prompt, model=args.model, url=args.url)
        print(result)
    elif args.input:
        if not args.output:
            base = os.path.splitext(args.input)[0]
            args.output = f"{base}_translated.jsonl"
        mode = auto_detect_mode(**kwargs)
        process_jsonl_file(
            args.input, args.output, args.backend, api_key, target_language,
            mode=mode, max_workers=args.workers, limit=args.limit, model=args.model, url=args.url, **kwargs
        )
    else:
        parser.error("Provide --text, --input-file (single mode) or --input (batch mode)")


if __name__ == "__main__":
    main()
