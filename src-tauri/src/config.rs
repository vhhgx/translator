//! Backend (model endpoint) configuration, persisted to app config dir.
//!
//! API keys never touch the JSON file — they live in the OS credential store.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::error::{AppError, AppResult};

const SERVICE: &str = "hy-mt2-translator";

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

impl Default for Settings {
    fn default() -> Self {
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
                    name: "腾讯云 Hy-MT2".into(),
                    base_url: "https://api.hunyuan.cloud.tencent.com".into(),
                    model: "hy-mt2-pro".into(),
                    has_api_key: false,
                },
            ],
            active_id: "local".into(),
            temperature: default_temperature(),
            timeout_secs: default_timeout(),
            close_to_tray: default_true(),
            hotkey_toggle: default_hotkey(),
        }
    }
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("hy-mt2-translator")
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