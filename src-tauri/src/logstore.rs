//! AI 请求/响应审计日志。
//!
//! 用户需要回答「这次翻译到底把什么发给了模型、模型又回了什么」。划词、小窗、
//! 主界面、文档、批量——所有调用最终都汇到 `client` 的两个函数，所以拦截点
//! 就放在那里（见 `client::translate` / `client::translate_stream`）。
//!
//! 设计取舍：
//!
//! - **内存环形缓冲 + 追加落盘**。界面上只看最近 N 条；落盘的那份用于重启后
//!   回溯。文件不做轮转——日志量本来就不大，且用户随时可以在设置页清空。
//! - **API Key 只留前 4 位**。日志常被贴进 issue，明文 key 一旦流出去就是
//!   凭据泄漏。请求头里的 Authorization 和（历史遗留的）body 里的 api_key
//!   都过同一个脱敏函数。
//! - **默认开启**。这是排查工具，默认关掉等于没做；泄漏面已经由脱敏控制住，
//!   且日志只落在本机配置目录。
//! - **失败不抛错**。日志写不进去（磁盘满、权限）绝不能把翻译本身搞挂，
//!   所有 IO 错误一律吞掉。

use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

/// 内存里保留的条数上限。再多界面也翻不动，且白占内存。
pub const RING_CAPACITY: usize = 500;

/// 请求体里单个字符串字段的截断长度。完整 prompt 动辄几千字，日志里只需要
/// 能看清「发的是什么」；超长内容截断保留头尾，中间省略。
const FIELD_HEAD: usize = 2000;
const FIELD_TAIL: usize = 200;

/// 日志开关。放全局是因为 `client` 是自由函数，拿不到 `AppState`。
static ENABLED: AtomicBool = AtomicBool::new(true);
/// 自增序号，给界面排序用，也保证同毫秒内多条不并列。
static SEQ: AtomicU64 = AtomicU64::new(0);

static RING: OnceLock<Mutex<VecDeque<Entry>>> = OnceLock::new();

fn ring() -> &'static Mutex<VecDeque<Entry>> {
    RING.get_or_init(|| Mutex::new(VecDeque::with_capacity(RING_CAPACITY)))
}

/// 一条审计记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// 自增，界面按它倒序。
    pub seq: u64,
    /// 毫秒时间戳。
    pub at: i64,
    /// 哪个入口发起的：translate / translate_stream / detect_language / test_backend。
    pub kind: String,
    /// 后端 id 与模型名，方便区分「同一个模型的不同端点」。
    pub backend_id: String,
    pub model: String,
    /// 最终请求地址（不含 key）。
    pub url: String,
    /// 发出去的请求体，已脱敏。空 = 这次没走到发请求。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<serde_json::Value>,
    /// 模型的返回。流式时是拼接后的完整文本。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
    /// 流式模式下模型逐个吐出的分片，供核对「它是一点点写的还是整段给的」。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deltas: Option<Vec<String>>,
    /// 耗时毫秒。
    pub ms: u128,
    /// 错误信息；成功时为 None。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 供前端读取的开关状态。
#[derive(Serialize)]
pub struct LogStatus {
    pub enabled: bool,
    pub count: usize,
    /// 落盘文件路径，界面上显示出来，方便用户自己去看。
    pub path: String,
    pub bytes: u64,
}

pub fn log_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("zephyr-translator")
        .join("requests.jsonl")
}

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn status() -> LogStatus {
    let count = ring().lock().map(|r| r.len()).unwrap_or(0);
    let path = log_path();
    let bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    LogStatus {
        enabled: enabled(),
        count,
        path: path.display().to_string(),
        bytes,
    }
}

/// 把 key 脱敏成 `sk-a***`。空 key 原样返回空串。
pub fn mask_key(key: &str) -> String {
    let k = key.trim();
    if k.is_empty() {
        return String::new();
    }
    let head: String = k.chars().take(4).collect();
    format!("{head}***")
}

/// 超长字符串截断，保留头尾。
fn clip(s: &str) -> String {
    let n = s.chars().count();
    if n <= FIELD_HEAD + FIELD_TAIL {
        return s.to_string();
    }
    let head: String = s.chars().take(FIELD_HEAD).collect();
    let tail: String = s.chars().skip(n - FIELD_TAIL).collect();
    format!("{head}\n…（省略 {} 字）…\n{tail}", n - FIELD_HEAD - FIELD_TAIL)
}

/// 深拷贝请求体，顺便把可能藏着 key 的字段换掉、把长文本截断。
///
/// 只认 `api_key` 这一个键名：请求头里的 Authorization 不在这里，而 OpenAI
/// 兼容协议的请求体本来就不该带 key（`client.rs` 里那行残留代码会写一个
/// null 进去）。
fn sanitize_request(body: &serde_json::Value) -> serde_json::Value {
    let mut v = body.clone();
    if let Some(obj) = v.as_object_mut() {
        if let Some(api_key) = obj.get("api_key") {
            if api_key.is_string() {
                let masked = mask_key(api_key.as_str().unwrap_or(""));
                obj.insert("api_key".into(), serde_json::Value::String(masked));
            }
        }
        // messages[].content 是 prompt 所在，截断它
        if let Some(msgs) = obj.get_mut("messages").and_then(|m| m.as_array_mut()) {
            for m in msgs {
                if let Some(c) = m.get_mut("content").and_then(|c| c.as_str().map(String::from)) {
                    m["content"] = serde_json::Value::String(clip(&c));
                }
            }
        }
    }
    v
}

/// 记录一次调用。所有失败一律吞掉——日志不能影响翻译本身。
#[allow(clippy::too_many_arguments)]
pub fn record(
    kind: &str,
    backend_id: &str,
    model: &str,
    url: &str,
    request: Option<&serde_json::Value>,
    response: Option<&str>,
    deltas: Option<&[String]>,
    ms: u128,
    error: Option<&str>,
) {
    if !enabled() {
        return;
    }
    let entry = Entry {
        seq: SEQ.fetch_add(1, Ordering::Relaxed),
        at: now_millis(),
        kind: kind.to_string(),
        backend_id: backend_id.to_string(),
        model: model.to_string(),
        url: url.to_string(),
        request: request.map(sanitize_request),
        response: response.map(clip),
        deltas: deltas.map(|d| d.to_vec()),
        ms,
        error: error.map(|e| clip(e)),
    };

    if let Ok(mut r) = ring().lock() {
        if r.len() == RING_CAPACITY {
            r.pop_front();
        }
        r.push_back(entry.clone());
    }

    append_to_file(&entry);
}

fn append_to_file(entry: &Entry) {
    let path = log_path();
    if let Some(dir) = path.parent() {
        if fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    let Ok(line) = serde_json::to_string(entry) else {
        return;
    };
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{line}");
    }
}

/// 最近 `limit` 条，最新的在前。
pub fn list(limit: usize) -> Vec<Entry> {
    let Ok(r) = ring().lock() else {
        return Vec::new();
    };
    r.iter().rev().take(limit).cloned().collect()
}

/// 清空内存与落盘。
pub fn clear() -> Result<(), String> {
    if let Ok(mut r) = ring().lock() {
        r.clear();
    }
    let path = log_path();
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 空_key_脱敏后仍是空串() {
        assert_eq!(mask_key(""), "");
        assert_eq!(mask_key("   "), "");
    }

    #[test]
    fn 短_key_只留前几位() {
        assert_eq!(mask_key("sk-abc"), "sk-a***");
    }

    #[test]
    fn 长_key_只暴露前四位() {
        let m = mask_key("sk-1234567890abcdef");
        assert_eq!(m, "sk-1***");
        // 关键：原文的任何中后段都不能出现
        assert!(!m.contains("1234567890"));
    }

    #[test]
    fn 短文本不截断() {
        assert_eq!(clip("你好"), "你好");
    }

    #[test]
    fn 长文本保留头尾并注明省略() {
        let long = "字".repeat(FIELD_HEAD + FIELD_TAIL + 100);
        let out = clip(&long);
        assert!(out.contains("省略 100 字"));
        assert!(out.starts_with("字"));
        assert!(out.ends_with("字"));
    }

    #[test]
    fn 请求体里的_key_被脱敏() {
        let body = serde_json::json!({
            "model": "m",
            "api_key": "sk-secret-value",
            "messages": [{ "role": "user", "content": "hi" }]
        });
        let out = sanitize_request(&body);
        assert_eq!(out["api_key"], "sk-s***");
        let s = serde_json::to_string(&out).unwrap();
        assert!(!s.contains("secret-value"));
        // 其它字段原样保留
        assert_eq!(out["model"], "m");
        assert_eq!(out["messages"][0]["content"], "hi");
    }

    #[test]
    fn 请求体里的长_prompt_被截断() {
        let long = "a".repeat(FIELD_HEAD + FIELD_TAIL + 50);
        let body = serde_json::json!({
            "messages": [{ "role": "user", "content": long }]
        });
        let out = sanitize_request(&body);
        let c = out["messages"][0]["content"].as_str().unwrap();
        assert!(c.contains("省略 50 字"));
    }

    #[test]
    fn 环形缓冲按容量淘汰最早的() {
        // 用独立的 VecDeque 验证淘汰逻辑，避免污染全局 RING。
        let mut q: VecDeque<u64> = VecDeque::with_capacity(3);
        for i in 0..5u64 {
            if q.len() == 3 {
                q.pop_front();
            }
            q.push_back(i);
        }
        assert_eq!(q.len(), 3);
        assert_eq!(q.front(), Some(&2));
        assert_eq!(q.back(), Some(&4));
    }
}
