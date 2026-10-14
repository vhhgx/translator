//! Backend (model endpoint) configuration, persisted to app config dir.
//!
//! API keys never touch the JSON file — they live in the OS credential store.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::error::{AppError, AppResult};

const SERVICE: &str = "zephyr-translator";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Backend {
    /// Stable identifier, used as the credential-store key.
    pub id: String,
    /// Shown in the UI dropdown.
    pub name: String,
    /// OpenAI-compatible endpoint, e.g. `http://127.0.0.1:8080`.
    pub base_url: String,
    /// Model name sent in the request body.
    pub model: String,
    /// Whether an API key is stored (the value itself is never serialized).
    #[serde(default)]
    pub has_api_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub backends: Vec<Backend>,
    pub active_id: String,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// 点关闭时隐藏到托盘而不是退出，否则全局热键会跟着进程一起消失。
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    /// 显示/隐藏窗口的全局热键，写成 "Ctrl+Shift+D" 这样的形式。
    #[serde(default = "default_hotkey")]
    pub hotkey_toggle: String,
    /// 小窗模式的全局热键：唤出一个只做输入+译文的小窗，不动主窗口。
    /// 空串表示不注册。
    #[serde(default = "default_hotkey_mini")]
    pub hotkey_mini: String,
    /// 划词翻译：选中文本后在光标处自动弹窗。默认关闭——它要装全局鼠标钩子，
    /// 且每次划词都会动一下剪贴板，属于需要用户明确同意的能力。
    #[serde(default)]
    pub selection_enabled: bool,
    /// 触发划词所需的修饰键："" = 不要求，"ctrl" / "alt" / "shift"。
    /// 不要求修饰键时任何拖选都会弹窗，容易吵，留给用户自己权衡。
    #[serde(default)]
    pub selection_modifier: String,
}

fn default_temperature() -> f32 {
    0.1
}
fn default_timeout() -> u64 {
    300
}
fn default_true() -> bool {
    true
}
fn default_hotkey() -> String {
    "Ctrl+Shift+D".to_string()
}
fn default_hotkey_mini() -> String {
    "Ctrl+Shift+S".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        // 出厂预置一批 OpenAI 兼容端点，只是省去用户手填 baseurl 的功夫——
        // 任何兼容的服务（含自建 vLLM / Ollama / 各类网关）都能再加后端。
        Self {
            backends: vec![
                Backend {
                    id: "local".into(),
                    name: "本地模型 (llama.cpp)".into(),
                    base_url: "http://127.0.0.1:8080".into(),
                    model: "hy-mt2".into(),
                    has_api_key: false,
                },
                Backend {
                    id: "tencent".into(),
                    name: "腾讯混元 Hy-MT2".into(),
                    base_url: "https://api.hunyuan.cloud.tencent.com".into(),
                    model: "hy-mt2-pro".into(),
                    has_api_key: false,
                },
                Backend {
                    id: "deepseek".into(),
                    name: "DeepSeek".into(),
                    base_url: "https://api.deepseek.com".into(),
                    model: "deepseek-chat".into(),
                    has_api_key: false,
                },
                Backend {
                    id: "openai".into(),
                    name: "OpenAI".into(),
                    base_url: "https://api.openai.com".into(),
                    model: "gpt-4o-mini".into(),
                    has_api_key: false,
                },
                Backend {
                    id: "ollama".into(),
                    name: "Ollama".into(),
                    base_url: "http://127.0.0.1:11434".into(),
                    model: "qwen2.5:7b".into(),
                    has_api_key: false,
                },
            ],
            active_id: "local".into(),
            temperature: default_temperature(),
            timeout_secs: default_timeout(),
            close_to_tray: default_true(),
            hotkey_toggle: default_hotkey(),
            hotkey_mini: default_hotkey_mini(),
            selection_enabled: false,
            selection_modifier: String::new(),
        }
    }
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("zephyr-translator")
        .join("settings.json")
}

impl Settings {
    pub fn load() -> Self {
        let path = config_path();
        let Ok(text) = fs::read_to_string(&path) else {
            return Self::default();
        };
        match serde_json::from_str::<Settings>(&text) {
            Ok(mut s) => {
                // Refresh the has_api_key flags from the real credential store.
                for b in &mut s.backends {
                    b.has_api_key = load_secret(&b.id).is_some();
                }
                s
            }
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> AppResult<()> {
        let path = config_path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| AppError::Io(e.to_string()))?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::Io(e.to_string()))?;
        fs::write(&path, text).map_err(|e| AppError::Io(e.to_string()))
    }

    pub fn active(&self) -> Option<&Backend> {
        self.backends
            .iter()
            .find(|b| b.id == self.active_id)
            .or_else(|| self.backends.first())
    }
}

/// Store an API key in the OS credential manager (Windows Credential Manager etc).
pub fn store_secret(backend_id: &str, secret: &str) -> AppResult<()> {
    let entry = keyring::Entry::new(SERVICE, backend_id)
        .map_err(|e| AppError::Credential(e.to_string()))?;
    entry
        .set_password(secret)
        .map_err(|e| AppError::Credential(e.to_string()))
}

pub fn load_secret(backend_id: &str) -> Option<String> {
    keyring::Entry::new(SERVICE, backend_id)
        .ok()?
        .get_password()
        .ok()
}

pub fn delete_secret(backend_id: &str) -> AppResult<()> {
    let entry = keyring::Entry::new(SERVICE, backend_id)
        .map_err(|e| AppError::Credential(e.to_string()))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Credential(e.to_string())),
    }
}