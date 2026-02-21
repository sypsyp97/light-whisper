use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("FunASR错误: {0}")]
    FunASR(String),
    #[error("音频错误: {0}")]
    Audio(String),
    #[error("IO错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("序列化错误: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Tauri错误: {0}")]
    Tauri(String),
    #[error("{0}")]
    Other(String),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl From<tauri::Error> for AppError {
    fn from(err: tauri::Error) -> Self {
        AppError::Tauri(err.to_string())
    }
}
