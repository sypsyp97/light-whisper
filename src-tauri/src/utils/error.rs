use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("ASR错误: {0}")]
    Asr(String),
    #[error("音频错误: {0}")]
    Audio(String),
    #[error("下载错误: {0}")]
    Download(String),
    #[error("IO错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("序列化错误: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Tauri错误: {0}")]
    Tauri(String),
    #[error("{0}")]
    Other(String),
}

impl AppError {
    /// 稳定的机器可读错误码（前端用 switch / 路由）。
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Asr(_) => "ASR_ERROR",
            AppError::Audio(_) => "AUDIO_ERROR",
            AppError::Download(_) => "DOWNLOAD_ERROR",
            AppError::Io(_) => "IO_ERROR",
            AppError::Serde(_) => "SERDE_ERROR",
            AppError::Tauri(_) => "TAURI_ERROR",
            AppError::Other(_) => "OTHER_ERROR",
        }
    }

    /// 高层归类，方便前端按类别决定提示样式。
    pub fn category(&self) -> &'static str {
        match self {
            AppError::Asr(_) => "asr",
            AppError::Audio(_) => "audio",
            AppError::Download(_) => "network",
            AppError::Io(_) | AppError::Serde(_) => "system",
            AppError::Tauri(_) => "tauri",
            AppError::Other(_) => "other",
        }
    }
}

#[derive(Serialize)]
struct StructuredAppError {
    code: &'static str,
    category: &'static str,
    message: String,
    /// 预留给未来携带结构化诊断信息（schema、status、provider 详情等）。
    /// 当前所有 variant 都没有 payload，统一序列化为 JSON null，让前端
    /// 永远能 `error.details === null` 判空而不是 `'details' in error`。
    details: Option<serde_json::Value>,
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        StructuredAppError {
            code: self.code(),
            category: self.category(),
            message: self.to_string(),
            details: None,
        }
        .serialize(serializer)
    }
}

impl From<tauri::Error> for AppError {
    fn from(err: tauri::Error) -> Self {
        AppError::Tauri(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    //! Tests for the structured AppError IPC contract.
    //!
    //! Contract:
    //!   - `AppError::code()` returns a stable string per variant (e.g.
    //!     "ASR_ERROR", "AUDIO_ERROR", "DOWNLOAD_ERROR", "IO_ERROR",
    //!     "SERDE_ERROR", "TAURI_ERROR", "OTHER_ERROR").
    //!   - `AppError::category()` maps each code to a coarse category
    //!     used by the UI to decide how to render the error.
    //!   - The `Serialize` impl emits a structured object with keys
    //!     `code`, `category`, `message`, `details` (instead of a single
    //!     string).
    use super::AppError;

    #[test]
    fn app_error_asr_code_and_category() {
        let err = AppError::Asr("foo".into());
        assert_eq!(err.code(), "ASR_ERROR");
        assert_eq!(err.category(), "asr");
    }

    #[test]
    fn app_error_audio_code_and_category() {
        let err = AppError::Audio("bar".into());
        assert_eq!(err.code(), "AUDIO_ERROR");
        assert_eq!(err.category(), "audio");
    }

    #[test]
    fn app_error_download_uses_network_category() {
        let err = AppError::Download("connection refused".into());
        assert_eq!(err.code(), "DOWNLOAD_ERROR");
        assert_eq!(err.category(), "network");
    }

    #[test]
    fn app_error_io_uses_system_category() {
        let err: AppError =
            std::io::Error::new(std::io::ErrorKind::Other, "x").into();
        assert_eq!(err.code(), "IO_ERROR");
        assert_eq!(err.category(), "system");
    }

    #[test]
    fn app_error_other_code_and_category() {
        let err = AppError::Other("unexpected".into());
        assert_eq!(err.code(), "OTHER_ERROR");
        assert_eq!(err.category(), "other");
    }

    #[test]
    fn app_error_serializes_to_object_with_required_keys() {
        let err = AppError::Asr("err".into());
        let value = serde_json::to_value(&err).expect("AppError must serialize");

        assert!(
            value.is_object(),
            "AppError must serialize to a JSON object; got {}",
            value
        );
        let obj = value.as_object().expect("object");
        assert!(obj.contains_key("code"), "missing key `code`: {:?}", obj);
        assert!(
            obj.contains_key("category"),
            "missing key `category`: {:?}",
            obj
        );
        assert!(
            obj.contains_key("message"),
            "missing key `message`: {:?}",
            obj
        );
        assert!(
            obj.contains_key("details"),
            "missing key `details`: {:?}",
            obj
        );
    }

    #[test]
    fn app_error_serialized_message_matches_to_string() {
        let err = AppError::Asr("err".into());
        let value = serde_json::to_value(&err).expect("AppError must serialize");
        assert_eq!(
            value["message"],
            serde_json::Value::String("ASR错误: err".to_string()),
            "message field must equal AppError::to_string() output \
             (Display impl from #[error(...)] above)"
        );
    }

    #[test]
    fn app_error_serialized_details_is_null() {
        let err = AppError::Other("x".into());
        let value = serde_json::to_value(&err).expect("AppError must serialize");
        assert_eq!(
            value["details"],
            serde_json::Value::Null,
            "details must default to JSON null when no structured payload is attached"
        );
    }

    #[test]
    fn app_error_serialized_code_and_category_match_methods() {
        let err = AppError::Tauri("y".into());
        let expected_code = err.code();
        let expected_category = err.category();
        let value = serde_json::to_value(&err).expect("AppError must serialize");
        assert_eq!(
            value["code"],
            serde_json::Value::String(expected_code.to_string()),
            "serialized `code` must equal AppError::code()"
        );
        assert_eq!(
            value["category"],
            serde_json::Value::String(expected_category.to_string()),
            "serialized `category` must equal AppError::category()"
        );
    }
}
