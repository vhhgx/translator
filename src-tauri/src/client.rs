//! OpenAI-compatible HTTP client for any configured backend.

use serde::{Deserialize, Serialize};

use crate::config::Backend;
use crate::error::{AppError, AppResult};
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
}

fn endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1/chat/completions")
    }
}

fn health_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    let root = trimmed
        .strip_suffix("/v1")
        .unwrap_or(trimmed);
    format!("{root}/health")
}

pub async fn check_health(backend: &Backend) -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    let url = health_url(&backend.base_url);
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    // Try the models endpoint for a friendly display, but don't require it.
    let models_url = {
        let trimmed = backend.base_url.trim_end_matches('/');
        let root = trimmed.strip_suffix("/v1").unwrap_or(trimmed);
        format!("{root}/v1/models")
    };
    if let Ok(m) = client.get(&models_url).send().await {
        if let Ok(json) = m.json::<serde_json::Value>().await {
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
    }
    Ok(vec![backend.model.clone()])
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
        "max_tokens": 8192,
        "stream": false,
    });

    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        body["api_key"] = serde_json::Value::Null;
        let _ = key; // key travels in the header below, not the body
    }

    let mut builder = client.post(endpoint(&backend.base_url));
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        builder = builder.bearer_auth(key);
    }

    let started = std::time::Instant::now();
    let resp = builder
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Http(format!("{e}\n\n请确认该后端地址可达（本地服务是否已启动？）")))?;

    let status = resp.status();
    let raw = resp.text().await.map_err(|e| AppError::Http(e.to_string()))?;
    if !status.is_success() {
        return Err(AppError::Http(format!("HTTP {status}: {}", &raw[..raw.len().min(400)])));
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
        return Err(AppError::Translate("模型返回了空内容".into()));
    }

    Ok(TranslateResponse {
        text: out,
        prompt: prompt_text,
        target_lang: resolved,
        ms: started.elapsed().as_millis(),
        used_terms,
    })
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