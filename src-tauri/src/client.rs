//! OpenAI-compatible HTTP client for any configured backend.

use serde::{Deserialize, Serialize};

use crate::config::Backend;
use crate::error::{AppError, AppResult};
use crate::logstore;
use crate::prompt::{self, Mode};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateRequest {
    pub text: String,
    /// Abbreviation or Chinese name; resolved server-side.
    pub target_lang: String,
    #[serde(default)]
    pub mode: Mode,
    /// [[src, dst], ...] glossary pairs. Only used in `terminology` mode.
    #[serde(default)]
    pub terms: Vec<[String; 2]>,
    /// When true, only terms actually present in `text` are sent.
    #[serde(default)]
    pub auto_filter_terms: bool,
    #[serde(default)]
    pub style: String,
    #[serde(default)]
    pub format_type: String,
    #[serde(default)]
    pub context: String,
    /// Skip glossary entirely and use the basic prompt.
    #[serde(default)]
    pub ignore_glossary: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TranslateResponse {
    pub text: String,
    pub prompt: String,
    pub target_lang: String,
    pub ms: u128,
    /// Terms that were actually injected (useful for the UI's "命中术语" hint).
    pub used_terms: Vec<String>,
    /// 入库后的历史记录 id（仅在 save_history 时由 lib.rs 回填）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_id: Option<String>,
}

/// 剥到 API 根地址：容忍用户把 baseurl 填成 `/v1`、完整的
/// `/v1/chat/completions`，甚至从厂商文档里复制的 Anthropic `/v1/messages`
/// 或 Responses `/v1/responses`，几种写法都归一到同一个根。
///
/// 厂商文档常常把三种协议的完整 URL 并排列出，用户随手抄一栏就填进来了。
/// 本客户端只讲 OpenAI Chat Completions，所以一律剥回根再自己拼。
fn api_root(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    const PROTOCOL_PATHS: [&str; 3] = ["/chat/completions", "/messages", "/responses"];
    let without_path = PROTOCOL_PATHS
        .iter()
        .find_map(|p| trimmed.strip_suffix(p))
        .unwrap_or(trimmed)
        .trim_end_matches('/');
    without_path
        .strip_suffix("/v1")
        .unwrap_or(without_path)
        .to_string()
}

fn endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    // baseurl 常常直接抄成带 /v1 的形式，若再补一段就会变成
    // /v1/v1/chat/completions —— 服务端只会回 404。
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else if trimmed.ends_with("/messages") || trimmed.ends_with("/responses") {
        // 抄成了别的协议路径。剥回根再拼 chat/completions，
        // 不然会拼出 /messages/v1/chat/completions 这种没人认得的地址。
        format!("{}/v1/chat/completions", api_root(base_url))
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/chat/completions")
    } else {
        format!("{trimmed}/v1/chat/completions")
    }
}

fn health_url(base_url: &str) -> String {
    format!("{}/health", api_root(base_url))
}

/// llama-server 的上下文窗口。
const CONTEXT_TOKENS: usize = 8192;
/// 留给服务端自身开销的余量。
const CONTEXT_SAFETY: usize = 128;

/// 按输入长度分配输出预算。
///
/// `max_tokens` 不能直接填满上下文窗口：服务端是按「输入 + 输出」一起算
/// 预算的，填满 8192 时哪怕输入只有 33 tokens 也会溢出并返回 400。
/// 中文按 1 字≈1 token 估算（宁可低估输出也别高估预算）。
fn completion_budget(prompt: &str) -> u32 {
    let estimated_input = prompt.chars().count() + 32 + CONTEXT_SAFETY;
    let remaining = CONTEXT_TOKENS.saturating_sub(estimated_input);
    remaining.clamp(256, CONTEXT_TOKENS / 2) as u32
}

/// 抓取网页用的客户端：超时给得宽一些（有些站点响应慢），并跟随跳转。
pub fn web_client(timeout_secs: u64) -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs.clamp(5, 120)))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| AppError::Io(e.to_string()))
}

/// 探测后端是否可用，顺带把模型列表带回来给界面显示。
///
/// 先打 OpenAI 标准的 `/v1/models`：它一举两得——能验 key（401 就说明 key
/// 不对），也能把模型名捞回来。只有它走不通时才退回 `/health`。
///
/// 顺序不能反。`/health` 是 llama-server 这类本地服务才有的扩展，中转站
/// （new-api / one-api / 各家云服务）压根没这个路径，先打它只会拿到一个
/// 毫无信息量的 404，把「后端是好的」误报成「后端坏了」。
pub async fn check_health(backend: &Backend, api_key: Option<&str>) -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;

let key = api_key.filter(|k| !k.is_empty());
    let models_url = format!("{}/v1/models", api_root(&backend.base_url));
    let mut probe = client.get(&models_url);
    if let Some(k) = key {
        probe = probe.bearer_auth(k);
    }

    // 记下第一次探测的结果：失败时要把两个地址各自的真实状态一起报给用户，
    // 只报最后一个会把问题指向错误的地址。
    let models_status = match probe.send().await {
        Ok(resp) if resp.status().is_success() => {
            // 拿到模型列表就收工；解析不出来也不该算失败——能连通就够了。
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(arr) = json.get("data").and_then(|d| d.as_array()) {
                    let ids: Vec<String> = arr
                        .iter()
                        .filter_map(|v| v.get("id").and_then(|i| i.as_str()))
                        .map(|s| s.to_string())
                        .collect();
                    if !ids.is_empty() {
                        return Ok(ids);
                    }
                }
            }
            return Ok(vec![backend.model.clone()]);
        }
        Ok(resp) if resp.status() == reqwest::StatusCode::UNAUTHORIZED => {
            return Err(if key.is_some() {
                "密钥无效或已过期（服务端返回 401）".to_string()
            } else {
                "该后端需要 API key —— 填在下方「API Key」里并保存".to_string()
            });
        }
        // 其余状态（404/405/5xx）说明这个后端不提供 /v1/models，交给下面的
        // /health 兜底，不要在这里就下结论。
        Ok(resp) => format!("HTTP {}", resp.status()),
        Err(e) => format!("{e}"),
    };

    // 兜底：本地服务自带的健康检查。
    let health = health_url(&backend.base_url);
    match client.get(&health).send().await {
        Ok(resp) if resp.status().is_success() => Ok(vec![backend.model.clone()]),
        Ok(resp) => Err(format!(
            "{} 返回 {models_status}，{} 返回 HTTP {}。\
             Base URL 填到域名就行，别带 /v1/messages 这类接口路径。",
            models_url, health, resp.status()
        )),
        Err(e) => Err(format!("{} 返回 {models_status}；{health} 连不上：{e}", models_url)),
    }
}

/// Build the final prompt for a request. Split out so tests can call it directly.
pub fn compose(req: &TranslateRequest) -> AppResult<(String, String, Vec<String>)> {
    let resolved = prompt::resolve_language(&req.target_lang)?;
    let mut mode = req.mode;
    let mut terms = req.terms.clone();

    if mode == Mode::Terminology && req.auto_filter_terms {
        terms.retain(|p| !p[0].trim().is_empty() && req.text.contains(p[0].trim()));
    }

    if mode == Mode::Terminology {
        // Nothing left to constrain: fall back rather than send an empty
        // "参考下面的翻译：" block, which would confuse the model.
        if terms.is_empty() && (req.auto_filter_terms || req.ignore_glossary) {
            mode = Mode::Basic;
        } else if terms.is_empty() {
            return Err(AppError::Translate(
                "术语约束模式需要至少一条术语（或勾选「仅使用原文命中的词」）".into(),
            ));
        }
    }

    // Glossary pairs are stored one direction (commonly zh->en). Decide
    // orientation PER TERM by checking which side actually occurs in the
    // source text, so en->zh tasks flip correctly without re-authoring.
    let mut lines: Vec<String> = Vec::new();
    let mut used: Vec<String> = Vec::new();
    for pair in &terms {
        let (a, b) = (pair[0].trim(), pair[1].trim());
        if a.is_empty() || b.is_empty() {
            continue;
        }
        if req.text.contains(b) && !req.text.contains(a) {
            lines.push(format!("{b}翻译成{a}"));
            used.push(b.to_string());
        } else {
            lines.push(format!("{a}翻译成{b}"));
            used.push(a.to_string());
        }
    }
    let terminology = lines.join("\n");

    let style = if req.style.trim().is_empty() {
        "日常口语化风格"
    } else {
        req.style.trim()
    };
    let fmt = if req.format_type.trim().is_empty() {
        "JSON"
    } else {
        req.format_type.trim()
    };

    let built = prompt::build_prompt(
        &req.text,
        &resolved,
        mode,
        &terminology,
        style,
        fmt,
        &req.context,
    );
    Ok((built, resolved, used))
}

/// Streaming counterpart of [`translate`].
///
/// Calls `stream: true` and parses the SSE feed, returning an async iterator of
/// text deltas. Kept separate from `translate` so the non-streaming path (batch,
/// document) stays a single simple request/response.
pub async fn translate_stream(
    backend: &Backend,
    api_key: Option<&str>,
    temperature: f32,
    timeout_secs: u64,
    req: &TranslateRequest,
    mut on_delta: impl FnMut(&str),
) -> AppResult<String> {
    let (prompt_text, _resolved, _used) = compose(req)?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| AppError::Http(e.to_string()))?;

    let body = serde_json::json!({
        "model": backend.model,
        "messages": [{ "role": "user", "content": prompt_text }],
        "temperature": temperature,
        "max_tokens": completion_budget(&prompt_text),
        "stream": true,
    });

    let url = endpoint(&backend.base_url);
    let mut builder = client.post(&url);
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        builder = builder.bearer_auth(key);
    }

    let started = std::time::Instant::now();
    let mut resp = match builder.json(&body).send().await {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("{e}\n\n请确认该后端地址可达（本地服务是否已启动？）");
            record_stream_error(backend, &url, &body, started, &msg);
            return Err(AppError::Http(msg));
        }
    };

    let status = resp.status();
    if !status.is_success() {
        let raw = resp.text().await.unwrap_or_default();
        let msg = format!("HTTP {status}: {}", &raw[..raw.len().min(400)]);
        record_stream_error(backend, &url, &body, started, &msg);
        return Err(AppError::Http(msg));
    }

    let mut full = String::new();
    let mut buf = String::new();
    // 诊断用：把每个分片留下，供日志还原「模型是怎么一点点吐出来的」。
    let mut deltas: Vec<String> = Vec::new();

    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| AppError::Http(format!("读取流失败: {e}")))?
    {
        buf.push_str(&String::from_utf8_lossy(&chunk));

        // SSE frames are separated by blank lines; keep the remainder buffered.
        while let Some(pos) = buf.find('\n') {
            let line: String = buf.drain(..=pos).collect();
            let line = line.trim();
            if line.is_empty() || line.starts_with(':') || line.starts_with("event:") {
                continue;
            }
            let Some(payload) = line.strip_prefix("data:") else {
                continue;
            };
            let payload = payload.trim();
            if payload == "[DONE]" {
                continue;
            }
            let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) else {
                continue;
            };
            if let Some(delta) = json
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("delta"))
                .and_then(|d| d.get("content"))
                .and_then(|v| v.as_str())
            {
                if !delta.is_empty() {
                    full.push_str(delta);
                    deltas.push(delta.to_string());
                    on_delta(delta);
                }
            }
        }
    }

    let ms = started.elapsed().as_millis();
    if full.trim().is_empty() {
        logstore::record(
            "translate_stream",
            &backend.id,
            &backend.model,
            &url,
            Some(&body),
            None,
            None,
            ms,
            Some("模型返回了空内容"),
        );
        return Err(AppError::Translate("模型返回了空内容".into()));
    }
    logstore::record(
        "translate_stream",
        &backend.id,
        &backend.model,
        &url,
        Some(&body),
        Some(&full),
        Some(&deltas),
        ms,
        None,
    );
    Ok(full)
}

/// 流式请求在「拿到响应之前」就失败时补一条日志。
fn record_stream_error(
    backend: &Backend,
    url: &str,
    body: &serde_json::Value,
    started: std::time::Instant,
    msg: &str,
) {
    logstore::record(
        "translate_stream",
        &backend.id,
        &backend.model,
        url,
        Some(body),
        None,
        None,
        started.elapsed().as_millis(),
        Some(msg),
    );
}

/// Non-streaming translation: one request, one response.
pub async fn translate(
    backend: &Backend,
    api_key: Option<&str>,
    temperature: f32,
    timeout_secs: u64,
    req: &TranslateRequest,
) -> AppResult<TranslateResponse> {
    let (prompt_text, resolved, used_terms) = compose(req)?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| AppError::Http(e.to_string()))?;

    let mut body = serde_json::json!({
        "model": backend.model,
        "messages": [{ "role": "user", "content": prompt_text }],
        "temperature": temperature,
        "max_tokens": completion_budget(&prompt_text),
        "stream": false,
    });

    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        body["api_key"] = serde_json::Value::Null;
        let _ = key; // key travels in the header below, not the body
    }

    let url = endpoint(&backend.base_url);
    let mut builder = client.post(&url);
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        builder = builder.bearer_auth(key);
    }

    let started = std::time::Instant::now();
    let resp = match builder.json(&body).send().await {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("{e}\n\n请确认该后端地址可达（本地服务是否已启动？）");
            logstore::record(
                "translate",
                &backend.id,
                &backend.model,
                &url,
                Some(&body),
                None,
                None,
                started.elapsed().as_millis(),
                Some(&msg),
            );
            return Err(AppError::Http(msg));
        }
    };

    let status = resp.status();
    let raw = resp.text().await.map_err(|e| AppError::Http(e.to_string()))?;
    let ms = started.elapsed().as_millis();
    if !status.is_success() {
        let msg = format!("HTTP {status}: {}", &raw[..raw.len().min(400)]);
        logstore::record(
            "translate",
            &backend.id,
            &backend.model,
            &url,
            Some(&body),
            Some(&raw[..raw.len().min(400)]),
            None,
            ms,
            Some(&msg),
        );
        return Err(AppError::Http(msg));
    }

    let json: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| AppError::Http(format!("响应解析失败: {e}")))?;

    let msg = json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .ok_or_else(|| AppError::Translate(format!("响应中没有 choices 字段: {}", &raw[..raw.len().min(200)])))?;

    let out = msg
        .get("content")
        .and_then(|v| v.as_str())
        .or_else(|| msg.get("reasoning_content").and_then(|v| v.as_str()))
        .unwrap_or("")
        .trim()
        .to_string();

    if out.is_empty() {
        logstore::record(
            "translate",
            &backend.id,
            &backend.model,
            &url,
            Some(&body),
            Some(&raw),
            None,
            ms,
            Some("模型返回了空内容"),
        );
        return Err(AppError::Translate("模型返回了空内容".into()));
    }

    // 记原始响应体：用户要看的是「模型到底回了什么」，包括 choices 之外的字段。
    logstore::record(
        "translate",
        &backend.id,
        &backend.model,
        &url,
        Some(&body),
        Some(&raw),
        None,
        ms,
        None,
    );

    Ok(TranslateResponse {
        text: out,
        prompt: prompt_text,
        target_lang: resolved,
        ms,
        used_terms,
        history_id: None,
    })
}

/// 语种检测：极简 prompt，只让模型回语言代码（en / zh / ja …）。
/// 用的是同一个 OpenAI 兼容端点，temperature 取 0 保证稳定输出。
pub async fn detect_language(
    backend: &Backend,
    key: Option<&str>,
    timeout_secs: u64,
    text: &str,
) -> AppResult<String> {
    // 太长没必要：前 200 个字符对语种识别已经足够
    let sample: String = text.chars().take(200).collect();
    let body = serde_json::json!({
        "model": backend.model,
        "temperature": 0.0,
        "max_tokens": 16,
        "stream": false,
        "messages": [{
            "role": "user",
            "content": format!(
                "Detect the language of the following text and reply with ONLY its \
                 language code (e.g. en, zh, ja, fr). No explanation.\n\n{sample}"
            )
        }]
    });
    let url = endpoint(&backend.base_url);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs.min(30)))
        .build()
        .map_err(|e| AppError::Http(e.to_string()))?;
    let mut req = client.post(&url).json(&body);
    if let Some(k) = key {
        req = req.bearer_auth(k);
    }
    let started = std::time::Instant::now();
    let raw = match req.send().await {
        Ok(r) => r.text().await.map_err(|e| AppError::Http(e.to_string()))?,
        Err(e) => {
            let msg = e.to_string();
            logstore::record(
                "detect_language",
                &backend.id,
                &backend.model,
                &url,
                Some(&body),
                None,
                None,
                started.elapsed().as_millis(),
                Some(&msg),
            );
            return Err(AppError::Http(msg));
        }
    };
    let json: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| AppError::Http(format!("响应解析失败: {e}")))?;
    let out = json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase();
    logstore::record(
        "detect_language",
        &backend.id,
        &backend.model,
        &url,
        Some(&body),
        Some(&raw),
        None,
        started.elapsed().as_millis(),
        None,
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basic(text: &str, lang: &str) -> TranslateRequest {
        TranslateRequest {
            text: text.into(),
            target_lang: lang.into(),
            mode: Mode::Basic,
            terms: vec![],
            auto_filter_terms: false,
            style: String::new(),
            format_type: String::new(),
            context: String::new(),
            ignore_glossary: false,
        }
    }

    #[test]
    fn endpoint_building() {
        assert_eq!(
            endpoint("http://127.0.0.1:8080"),
            "http://127.0.0.1:8080/v1/chat/completions"
        );
        assert_eq!(
            endpoint("http://127.0.0.1:8080/"),
            "http://127.0.0.1:8080/v1/chat/completions"
        );
        assert_eq!(
            endpoint("http://x/v1/chat/completions"),
            "http://x/v1/chat/completions"
        );
        assert_eq!(health_url("http://127.0.0.1:8080"), "http://127.0.0.1:8080/health");
    }

    /// 厂商文档把三种协议的完整 URL 并排列出，很容易抄错一栏；
    /// 三种填法都应归一到同一个 OpenAI Chat Completions 端点。
    #[test]
    fn endpoint_accepts_any_documented_protocol_url() {
        const EXPECTED: &str = "https://rong-ai.com/v1/chat/completions";
        assert_eq!(endpoint("https://rong-ai.com/v1/chat/completions"), EXPECTED);
        assert_eq!(endpoint("https://rong-ai.com/v1/messages"), EXPECTED);
        assert_eq!(endpoint("https://rong-ai.com/v1/responses"), EXPECTED);
        assert_eq!(endpoint("https://rong-ai.com/v1"), EXPECTED);
        assert_eq!(endpoint("https://rong-ai.com/"), EXPECTED);
        assert_eq!(endpoint("https://rong-ai.com"), EXPECTED);
        for form in [
            "https://rong-ai.com/v1/chat/completions",
            "https://rong-ai.com/v1/messages",
            "https://rong-ai.com/v1/responses",
        ] {
            assert_eq!(api_root(form), "https://rong-ai.com", "api_root({form})");
            assert_eq!(health_url(form), "https://rong-ai.com/health", "health_url({form})");
        }
    }

    /// baseurl 带 /v1 是最常见的写法，拼错就会得到 /v1/v1/chat/completions。
    #[test]
    fn endpoint_accepts_baseurl_already_ending_in_v1() {
        assert_eq!(
            endpoint("http://127.0.0.1:8080/v1"),
            "http://127.0.0.1:8080/v1/chat/completions"
        );
        assert_eq!(
            endpoint("http://127.0.0.1:8080/v1/"),
            "http://127.0.0.1:8080/v1/chat/completions"
        );
        // 三种写法必须收敛到同一个地址
        assert_eq!(
            endpoint("http://127.0.0.1:8080/v1"),
            endpoint("http://127.0.0.1:8080")
        );
        assert_eq!(
            endpoint("http://127.0.0.1:8080/v1"),
            endpoint("http://127.0.0.1:8080/v1/chat/completions")
        );
        // 健康检查与模型列表都从同一个根推导，不能出现 /v1/v1
        assert_eq!(
            health_url("http://127.0.0.1:8080/v1"),
            "http://127.0.0.1:8080/health"
        );
        assert_eq!(
            health_url("http://127.0.0.1:8080/v1/chat/completions"),
            "http://127.0.0.1:8080/health"
        );
    }

    /// 输入 + 输出必须一起塞进上下文窗口，max_tokens 填满窗口必然 400。
    #[test]
    fn completion_budget_fits_within_context() {
        // 短句：输出最多只能占窗口的一半
        let short = completion_budget("你好");
        assert!(short <= (CONTEXT_TOKENS / 2) as u32, "{short}");

        // 长文：预算随输入收缩，且总账仍留在窗口内
        let long_prompt = "翻".repeat(5000);
        let long = completion_budget(&long_prompt);
        let total = long_prompt.chars().count() + long as usize;
        assert!(total < CONTEXT_TOKENS, "输入 {} + 输出 {long} 超出窗口", long_prompt.chars().count());

        // 即便输入几乎占满窗口，也要返回一个非零值而不是下溢
        assert!(completion_budget(&"x".repeat(CONTEXT_TOKENS * 2)) > 0);
    }

    #[test]
    fn glossary_prompts() {
        let mut r = basic("请提交订单", "en");
        r.mode = Mode::Terminology;
        r.terms = vec![["订单".into(), "ticket".into()]];
        let (p, lang, used) = compose(&r).unwrap();
        assert_eq!(lang, "英语");
        assert_eq!(used, vec!["订单".to_string()]);
        assert!(p.contains("参考下面的翻译：\n订单翻译成ticket\n"), "{p}");
    }

    #[test]
    fn glossary_reversed_for_english_source() {
        let mut r = basic("Submit the ticket", "中文");
        r.mode = Mode::Terminology;
        r.terms = vec![["订单".into(), "ticket".into()]];
        let (p, _, _) = compose(&r).unwrap();
        assert!(p.contains("ticket翻译成订单"), "{p}");
    }

    #[test]
    fn auto_filter_keeps_only_present_terms() {
        let mut r = basic("这里提到订单", "en");
        r.mode = Mode::Terminology;
        r.auto_filter_terms = true;
        r.terms = vec![
            ["订单".into(), "ticket".into()],
            ["发票".into(), "invoice".into()],
        ];
        let (p, _, used) = compose(&r).unwrap();
        assert_eq!(used, vec!["订单".to_string()]);
        assert!(!p.contains("发票"), "{p}");
    }

    #[test]
    fn terminology_without_terms_errors() {
        let mut r = basic("x", "en");
        r.mode = Mode::Terminology;
        assert!(compose(&r).is_err());
    }

    #[test]
    fn auto_filter_falls_back_to_basic_when_no_hits() {
        let mut r = basic("完全无关的文本", "en");
        r.mode = Mode::Terminology;
        r.auto_filter_terms = true;
        r.terms = vec![["订单".into(), "ticket".into()]];
        let (p, _, used) = compose(&r).unwrap();
        assert!(used.is_empty());
        assert!(p.starts_with("将以下文本翻译为英语"), "{p}");
    }
}