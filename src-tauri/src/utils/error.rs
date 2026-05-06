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
    /// macOS TCC permission denied. Carries the kind (microphone/accessibility/
    /// screen/automation) and a `settings_url` deep-link so the UI can offer a
    /// one-click "Open Settings" affordance instead of a multi-line prose error.
    #[error("{message}")]
    PermissionDenied {
        kind: String,
        settings_url: String,
        message: String,
    },
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
            AppError::PermissionDenied { .. } => "PERMISSION_DENIED",
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
            AppError::PermissionDenied { .. } => "permission",
            AppError::Other(_) => "other",
        }
    }

    /// Structured payload for the IPC `details` field. Only PermissionDenied
    /// produces a non-null body today; other variants stay `None` so the front
    /// end's existing `details === null` shape is preserved for them.
    fn details_payload(&self) -> Option<serde_json::Value> {
        match self {
            AppError::PermissionDenied {
                kind,
                settings_url,
                ..
            } => Some(serde_json::json!({
                "kind": kind,
                "settingsUrl": settings_url,
            })),
            _ => None,
        }
    }
}

#[derive(Serialize)]
struct StructuredAppError {
    code: &'static str,
    category: &'static str,
    message: String,
    /// 携带结构化诊断信息。PermissionDenied 用 `{ kind, settingsUrl }` 让前端
    /// 直接渲染「打开系统设置」按钮；其他 variant 暂时为 null，前端可继续
    /// 用 `error.details === null` 判空。
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
            details: self.details_payload(),
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
        let err: AppError = std::io::Error::other("x").into();
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

    // ---------------------------------------------------------------------
    // PermissionDenied — structured permission-denied IPC contract.
    //
    // The UI consumes `details.kind` to choose copy and `details.settingsUrl`
    // to render the "Open Settings" deeplink. Pinning these field names
    // prevents accidental rename from breaking the front-end render path.
    // ---------------------------------------------------------------------

    #[test]
    fn permission_denied_code_is_stable() {
        let err = AppError::PermissionDenied {
            kind: "microphone".into(),
            settings_url: "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone".into(),
            message: "麦克风权限尚未授予".into(),
        };
        assert_eq!(err.code(), "PERMISSION_DENIED");
        assert_eq!(err.category(), "permission");
    }

    #[test]
    fn permission_denied_serializes_details_with_kind_and_url() {
        let err = AppError::PermissionDenied {
            kind: "accessibility".into(),
            settings_url:
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
                    .into(),
            message: "辅助功能权限尚未授予".into(),
        };
        let value = serde_json::to_value(&err).expect("AppError must serialize");
        let details = value
            .get("details")
            .expect("details key must be present")
            .as_object()
            .expect("PermissionDenied serializes details as an object");
        assert_eq!(
            details.get("kind").and_then(|v| v.as_str()),
            Some("accessibility"),
            "details.kind must echo the PermissionKind tag",
        );
        assert_eq!(
            details.get("settingsUrl").and_then(|v| v.as_str()),
            Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"),
            "details.settingsUrl must be camelCase and equal the deeplink",
        );
    }

    #[test]
    fn permission_denied_message_is_the_user_facing_string() {
        // Display impl points at the `message` field — the prose the UI shows.
        // It must NOT inline the deeplink (that's structured) or the kind tag.
        let err = AppError::PermissionDenied {
            kind: "screen".into(),
            settings_url: "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture".into(),
            message: "屏幕录制权限尚未授予".into(),
        };
        assert_eq!(err.to_string(), "屏幕录制权限尚未授予");
    }

    #[test]
    fn non_permission_errors_keep_details_null() {
        // Existing front-end code uses `error.details === null` as the "no
        // structured payload" branch; we must preserve that for every variant
        // that hasn't opted in.
        for err in [
            AppError::Asr("a".into()),
            AppError::Audio("b".into()),
            AppError::Download("c".into()),
            AppError::Tauri("d".into()),
            AppError::Other("e".into()),
        ] {
            let value = serde_json::to_value(&err).expect("AppError must serialize");
            assert_eq!(
                value["details"],
                serde_json::Value::Null,
                "details must remain null for {:?}",
                err
            );
        }
    }
}
