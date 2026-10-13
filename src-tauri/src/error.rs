use serde::{Serialize, Serializer};

#[derive(Debug)]
pub enum AppError {
    Io(String),
    Credential(String),
    Http(String),
    Translate(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Io(m) => write!(f, "文件错误: {m}"),
            AppError::Credential(m) => write!(f, "凭据存储错误: {m}"),
            AppError::Http(m) => write!(f, "网络错误: {m}"),
            AppError::Translate(m) => write!(f, "翻译失败: {m}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

impl From<String> for AppError {
    fn from(e: String) -> Self {
        AppError::Translate(e)
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;