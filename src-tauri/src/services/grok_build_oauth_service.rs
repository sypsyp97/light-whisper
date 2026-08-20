use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use rand::{distributions::Alphanumeric, rngs::OsRng, Rng};
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri_plugin_keyring::KeyringExt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use crate::services::llm_provider::KEYRING_SERVICE;
use crate::state::user_profile::{LlmProviderConfig, XaiAuthMode};
use crate::state::AppState;
use crate::utils::paths;

const XAI_PROVIDER: &str = "xai";
const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const AUTHORIZE_URL: &str = "https://auth.x.ai/oauth2/authorize";
const TOKEN_URL: &str = "https://auth.x.ai/oauth2/token";
const DEVICE_CODE_URL: &str = "https://auth.x.ai/oauth2/device/code";
const SCOPE: &str = "openid profile email offline_access grok-cli:access api:access";
const CALLBACK_HOST: &str = "127.0.0.1";
const CALLBACK_PORT: u16 = 56121;
const CALLBACK_PATH: &str = "/callback";
const REDIRECT_URI: &str = "http://127.0.0.1:56121/callback";
const SESSION_KEYRING_USER: &str = "grok-build-oauth";
const SESSION_REFRESH_TOKEN_KEYRING_USER: &str = "grok-build-oauth-refresh-token";
const OAUTH_TIMEOUT_SECS: u64 = 5 * 60;
const REFRESH_SKEW_SECS: u64 = 60;
pub const GROK_BUILD_OAUTH_PREFIX: &str = "grok-build-oauth:";
#[allow(dead_code)]
pub const GROK_BUILD_INFERENCE_BASE_URL: &str = "https://cli-chat-proxy.grok.com";
#[allow(dead_code)]
pub const XAI_API_KEY_INFERENCE_BASE_URL: &str = "https://api.x.ai";
pub const GROK_BUILD_RESPONSES_URL: &str = "https://cli-chat-proxy.grok.com/v1/responses";
pub const GROK_BUILD_MODELS_URL: &str = "https://cli-chat-proxy.grok.com/v1/models";
pub const XAI_API_MODELS_URL: &str = "https://api.x.ai/v1/models";
pub const GROK_CLI_USER_AGENT: &str = "xai-grok-cli";
pub const GROK_CLI_CLIENT_IDENTIFIER: &str = "grok-shell";
pub const GROK_CLI_TOKEN_AUTH: &str = "xai-grok-cli";
pub const GROK_CLI_CLIENT_VERSION: &str = "0.2.114";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GrokBuildOauthSession {
    pub id_token: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at_ms: Option<u64>,
    pub account_id: Option<String>,
    pub email: Option<String>,
    pub plan_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GrokBuildOauthStatus {
    pub logged_in: bool,
    pub email: Option<String>,
    pub plan_type: Option<String>,
    pub account_id: Option<String>,
    pub expires_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokBuildOauthDeviceCodeChallenge {
    pub verification_url: String,
    pub user_code: String,
    pub device_code: String,
    pub interval_secs: u64,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    #[serde(default)]
    id_token: Option<String>,
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    #[serde(alias = "usercode")]
    user_code: String,
    #[serde(default, alias = "verification_url")]
    verification_uri: Option<String>,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    #[serde(default, deserialize_with = "deserialize_interval_secs")]
    interval: u64,
}

#[derive(Debug, Deserialize, Default)]
struct OAuthErrorBody {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct JwtClaims {
    #[serde(default)]
    exp: Option<u64>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    sub: Option<String>,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    plan_type: Option<String>,
    #[serde(default)]
    plan: Option<String>,
}

#[derive(Debug)]
struct OAuthCallback {
    code: String,
    stream: tokio::net::TcpStream,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedGrokBuildOauthSession {
    pub expires_at_ms: Option<u64>,
    pub account_id: Option<String>,
    pub email: Option<String>,
    pub plan_type: Option<String>,
}

fn deserialize_interval_secs<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Number(number) => number
            .as_u64()
            .ok_or_else(|| de::Error::custom("interval must be a positive integer")),
        serde_json::Value::String(value) => value
            .trim()
            .parse::<u64>()
            .map_err(|err| de::Error::custom(format!("invalid interval: {err}"))),
        serde_json::Value::Null => Ok(0),
        _ => Err(de::Error::custom("interval must be a string or number")),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn make_status(session: Option<&GrokBuildOauthSession>) -> GrokBuildOauthStatus {
    if let Some(session) = session {
        return GrokBuildOauthStatus {
            logged_in: true,
            email: session.email.clone(),
            plan_type: session.plan_type.clone(),
            account_id: session.account_id.clone(),
            expires_at_ms: session.expires_at_ms,
        };
    }

    GrokBuildOauthStatus::default()
}

fn session_meta_path() -> std::path::PathBuf {
    paths::get_data_dir().join("grok_build_oauth_session.json")
}

fn read_session_meta() -> PersistedGrokBuildOauthSession {
    std::fs::read_to_string(session_meta_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<PersistedGrokBuildOauthSession>(&raw).ok())
        .unwrap_or_default()
}

fn write_session_meta(session: &GrokBuildOauthSession) -> Result<(), String> {
    let persisted = PersistedGrokBuildOauthSession {
        expires_at_ms: session.expires_at_ms,
        account_id: session.account_id.clone(),
        email: session.email.clone(),
        plan_type: session.plan_type.clone(),
    };
    let raw = serde_json::to_string(&persisted)
        .map_err(|err| format!("序列化 Grok Build OAuth 元数据失败: {err}"))?;
    std::fs::write(session_meta_path(), raw)
        .map_err(|err| format!("保存 Grok Build OAuth 元数据失败: {err}"))
}

fn load_session_from_storage(app_handle: &tauri::AppHandle) -> Option<GrokBuildOauthSession> {
    let refresh_token = app_handle
        .keyring()
        .get_password(KEYRING_SERVICE, SESSION_REFRESH_TOKEN_KEYRING_USER)
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty());

    if let Some(refresh_token) = refresh_token {
        let meta = read_session_meta();
        return Some(GrokBuildOauthSession {
            id_token: String::new(),
            access_token: String::new(),
            refresh_token,
            expires_at_ms: meta.expires_at_ms,
            account_id: meta.account_id,
            email: meta.email,
            plan_type: meta.plan_type,
        });
    }

    app_handle
        .keyring()
        .get_password(KEYRING_SERVICE, SESSION_KEYRING_USER)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str::<GrokBuildOauthSession>(&raw).ok())
}

fn save_session_to_storage(
    app_handle: &tauri::AppHandle,
    session: &GrokBuildOauthSession,
) -> Result<(), String> {
    app_handle
        .keyring()
        .set_password(
            KEYRING_SERVICE,
            SESSION_REFRESH_TOKEN_KEYRING_USER,
            &session.refresh_token,
        )
        .map_err(|err| format!("保存 Grok Build OAuth refresh token 失败: {err}"))?;
    write_session_meta(session)?;
    let _ = app_handle
        .keyring()
        .delete_password(KEYRING_SERVICE, SESSION_KEYRING_USER);
    Ok(())
}

fn clear_session_from_storage(app_handle: &tauri::AppHandle) {
    let _ = app_handle
        .keyring()
        .delete_password(KEYRING_SERVICE, SESSION_KEYRING_USER);
    let _ = app_handle
        .keyring()
        .delete_password(KEYRING_SERVICE, SESSION_REFRESH_TOKEN_KEYRING_USER);
    let _ = std::fs::remove_file(session_meta_path());
}

fn decode_jwt_claims(jwt: &str) -> Option<JwtClaims> {
    let mut parts = jwt.split('.');
    let (_header, payload, _signature) = match (parts.next(), parts.next(), parts.next()) {
        (Some(h), Some(p), Some(s)) if !h.is_empty() && !p.is_empty() && !s.is_empty() => (h, p, s),
        _ => return None,
    };
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn enrich_session_from_tokens(session: &mut GrokBuildOauthSession) {
    let claims =
        decode_jwt_claims(&session.id_token).or_else(|| decode_jwt_claims(&session.access_token));
    if let Some(claims) = claims {
        session.email = claims
            .email
            .or(claims.preferred_username)
            .or_else(|| session.email.take());
        session.account_id = claims.sub.or_else(|| session.account_id.take());
        session.plan_type = claims
            .plan_type
            .or(claims.plan)
            .or_else(|| session.plan_type.take());
        if session.expires_at_ms.is_none() {
            session.expires_at_ms = claims.exp.map(|exp| exp.saturating_mul(1000));
        }
    }
}

fn generate_code_verifier() -> String {
    OsRng
        .sample_iter(&Alphanumeric)
        .take(64)
        .map(char::from)
        .collect()
}

fn base64_url_encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn encode_grok_build_oauth_access_token(access_token: &str) -> Option<String> {
    let access_token = access_token.trim();
    if access_token.is_empty() {
        return None;
    }
    Some(format!("{GROK_BUILD_OAUTH_PREFIX}{access_token}"))
}

pub fn decode_grok_build_oauth_access_token(input: &str) -> Option<String> {
    let payload = input.trim().strip_prefix(GROK_BUILD_OAUTH_PREFIX)?;
    let payload = payload.trim();
    if payload.is_empty() {
        return None;
    }
    Some(payload.to_string())
}

pub fn is_grok_build_oauth_origin_auth(input: &str) -> bool {
    decode_grok_build_oauth_access_token(input).is_some()
}

/// Grok Build CLI proxy has not advertised image input metadata.
/// Treat unknown support as text-only so polish/assistant do not attach
/// full-screen screenshots that stall the request for tens of seconds.
pub fn resolve_image_support_for_request(api_key: &str, observed: Option<bool>) -> Option<bool> {
    if observed.is_none() && is_grok_build_oauth_origin_auth(api_key) {
        return Some(false);
    }
    observed
}

pub fn effective_xai_auth_mode(stored_mode: Option<XaiAuthMode>, logged_in: bool) -> XaiAuthMode {
    stored_mode.unwrap_or(if logged_in {
        XaiAuthMode::Oauth
    } else {
        XaiAuthMode::ApiKey
    })
}

fn generate_pkce_pair() -> (String, String) {
    let verifier = generate_code_verifier();
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge = base64_url_encode(&hasher.finalize());
    (verifier, challenge)
}

fn generate_state() -> String {
    let bytes = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(48)
        .collect::<Vec<u8>>();
    base64_url_encode(&bytes)
}

fn form_encode(pairs: &[(&str, &str)]) -> Result<String, String> {
    Ok(reqwest::Url::parse_with_params("http://localhost", pairs)
        .map_err(|err| format!("构造 OAuth 表单失败: {err}"))?
        .query()
        .unwrap_or_default()
        .to_string())
}

fn html_escape(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn callback_html(title: &str, message: &str, auto_close: bool) -> String {
    let script = if auto_close {
        "<script>setTimeout(() => window.close(), 1200)</script>"
    } else {
        ""
    };
    let title = html_escape(title);
    let message = html_escape(message);
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{title}</title><style>body{{margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;background:#111827;color:#f9fafb;font-family:-apple-system,BlinkMacSystemFont,'SF Pro Text','PingFang SC','Helvetica Neue','Segoe UI','Microsoft YaHei UI',system-ui,sans-serif;font-weight:400;letter-spacing:-.008em;text-align:center}}h1{{margin:0;font-size:34px;line-height:1.2;font-weight:700;letter-spacing:-.02em}}p{{margin:0;font-size:21px;line-height:1.65;white-space:pre-wrap}}</style></head><body><div style=\"max-width:680px;padding:48px 32px;display:flex;flex-direction:column;align-items:center;gap:18px;\"><h1>{title}</h1><p>{message}</p></div>{script}</body></html>"
    )
}

async fn respond_with_html(
    stream: &mut tokio::net::TcpStream,
    status_line: &str,
    html: &str,
) -> Result<(), String> {
    let body = html.as_bytes();
    let response = format!(
        "HTTP/1.1 {status_line}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(|err| format!("写入 OAuth 回调响应失败: {err}"))?;
    stream
        .write_all(body)
        .await
        .map_err(|err| format!("写入 OAuth 回调响应体失败: {err}"))?;
    stream
        .flush()
        .await
        .map_err(|err| format!("刷新 OAuth 回调响应失败: {err}"))
}

async fn bind_callback_listener() -> Result<TcpListener, String> {
    TcpListener::bind((CALLBACK_HOST, CALLBACK_PORT))
        .await
        .map_err(|err| {
            format!(
                "无法占用 Grok Build OAuth 回调端口 {CALLBACK_PORT}，请关闭占用该端口的程序或改用设备码登录: {err}"
            )
        })
}

async fn wait_for_callback(
    listener: TcpListener,
    expected_state: String,
) -> Result<OAuthCallback, String> {
    let (mut stream, _) =
        tokio::time::timeout(Duration::from_secs(OAUTH_TIMEOUT_SECS), listener.accept())
            .await
            .map_err(|_| "等待 Grok Build OAuth 回调超时，请重试。".to_string())?
            .map_err(|err| format!("接受 Grok Build OAuth 回调失败: {err}"))?;

    let mut reader = BufReader::new(&mut stream);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .await
        .map_err(|err| format!("读取 OAuth 回调失败: {err}"))?;

    let path = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| "OAuth 回调请求格式不正确".to_string())?;
    let url = reqwest::Url::parse(&format!("http://localhost{path}"))
        .map_err(|err| format!("解析 OAuth 回调 URL 失败: {err}"))?;

    if url.path() != CALLBACK_PATH {
        let html = callback_html("Invalid Callback", "Unexpected callback path.", false);
        let _ = respond_with_html(&mut stream, "404 Not Found", &html).await;
        return Err("OAuth 回调路径不正确".to_string());
    }

    let query = url
        .query_pairs()
        .into_owned()
        .collect::<std::collections::HashMap<_, _>>();
    let state = query.get("state").map(String::as_str).unwrap_or_default();
    if state != expected_state {
        let html = callback_html(
            "State Mismatch",
            "Login state does not match the original request.",
            false,
        );
        let _ = respond_with_html(&mut stream, "400 Bad Request", &html).await;
        return Err("Grok Build OAuth state 校验失败，请重试。".to_string());
    }

    if let Some(error_code) = query.get("error") {
        let description = query
            .get("error_description")
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty());
        let message = match description {
            Some(description) => format!("Grok Build OAuth 登录失败: {description}"),
            None => format!("Grok Build OAuth 登录失败: {error_code}"),
        };
        let html = callback_html("Authorization Failed", &message, false);
        let _ = respond_with_html(&mut stream, "200 OK", &html).await;
        return Err(message);
    }

    let Some(code) = query
        .get("code")
        .cloned()
        .filter(|value| !value.trim().is_empty())
    else {
        let html = callback_html(
            "Missing Code",
            "Authorization code was not returned.",
            false,
        );
        let _ = respond_with_html(&mut stream, "400 Bad Request", &html).await;
        return Err("OAuth 回调缺少 authorization code".to_string());
    };

    Ok(OAuthCallback { code, stream })
}

fn build_authorize_url(code_challenge: &str, state: &str) -> Result<String, String> {
    let url = reqwest::Url::parse_with_params(
        AUTHORIZE_URL,
        &[
            ("response_type", "code"),
            ("client_id", CLIENT_ID),
            ("redirect_uri", REDIRECT_URI),
            ("scope", SCOPE),
            ("code_challenge", code_challenge),
            ("code_challenge_method", "S256"),
            ("state", state),
        ],
    )
    .map_err(|err| format!("构造 Grok Build OAuth 地址失败: {err}"))?;
    Ok(url.to_string())
}

fn oauth_http_error(prefix: &str, status: reqwest::StatusCode, body: &str) -> String {
    if let Ok(payload) = serde_json::from_str::<OAuthErrorBody>(body) {
        if let Some(description) = payload
            .error_description
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            return format!("{prefix}: {description}");
        }
        if let Some(error) = payload
            .error
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            return format!("{prefix}: {error}");
        }
    }
    format!("{prefix} {status}: {body}")
}

fn should_invalidate_refresh(status: reqwest::StatusCode, body: &str) -> bool {
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return false;
    }
    if status.is_client_error() {
        return true;
    }
    body.to_ascii_lowercase().contains("invalid_grant")
}

async fn exchange_code_for_tokens(
    client: &reqwest::Client,
    code: &str,
    code_verifier: &str,
) -> Result<TokenResponse, String> {
    let response = client
        .post(TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form_encode(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", REDIRECT_URI),
            ("client_id", CLIENT_ID),
            ("code_verifier", code_verifier),
        ])?)
        .send()
        .await
        .map_err(|err| format!("OAuth 授权码换 token 失败: {err}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(oauth_http_error("OAuth 授权码换 token 失败", status, &body));
    }

    response
        .json::<TokenResponse>()
        .await
        .map_err(|err| format!("解析 OAuth token 响应失败: {err}"))
}

async fn request_device_code(
    client: &reqwest::Client,
) -> Result<GrokBuildOauthDeviceCodeChallenge, String> {
    let response = client
        .post(DEVICE_CODE_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form_encode(&[("client_id", CLIENT_ID), ("scope", SCOPE)])?)
        .send()
        .await
        .map_err(|err| format!("请求 Grok Build 设备码失败: {err}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(oauth_http_error(
            "请求 Grok Build 设备码失败",
            status,
            &body,
        ));
    }

    let payload = response
        .json::<DeviceCodeResponse>()
        .await
        .map_err(|err| format!("解析 Grok Build 设备码响应失败: {err}"))?;
    let verification_url = payload
        .verification_uri
        .filter(|value| !value.trim().is_empty())
        .or(payload.verification_uri_complete.clone())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Grok Build 设备码响应缺少验证地址".to_string())?;

    Ok(GrokBuildOauthDeviceCodeChallenge {
        verification_url,
        user_code: payload.user_code,
        device_code: payload.device_code,
        interval_secs: payload.interval.max(1),
    })
}

async fn poll_device_code_token(
    client: &reqwest::Client,
    challenge: &GrokBuildOauthDeviceCodeChallenge,
) -> Result<TokenResponse, String> {
    let max_wait = Duration::from_secs(15 * 60);
    let mut interval = Duration::from_secs(challenge.interval_secs.clamp(1, 30));
    let start = Instant::now();

    loop {
        let response = client
            .post(TOKEN_URL)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form_encode(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", challenge.device_code.as_str()),
                ("client_id", CLIENT_ID),
            ])?)
            .send()
            .await
            .map_err(|err| format!("轮询 Grok Build 设备码授权失败: {err}"))?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if status.is_success() {
            return serde_json::from_str::<TokenResponse>(&body)
                .map_err(|err| format!("解析 Grok Build 设备码 token 响应失败: {err}"));
        }

        let error = serde_json::from_str::<OAuthErrorBody>(&body)
            .ok()
            .and_then(|payload| payload.error)
            .unwrap_or_default();
        match error.as_str() {
            "authorization_pending" => {}
            "slow_down" => {
                interval = interval.saturating_add(Duration::from_secs(5));
            }
            _ => {
                return Err(oauth_http_error("Grok Build 设备码授权失败", status, &body));
            }
        }

        if start.elapsed() >= max_wait {
            return Err("Grok Build 设备码登录超时，请重新开始登录。".to_string());
        }
        let remaining = max_wait.saturating_sub(start.elapsed());
        tokio::time::sleep(interval.min(remaining)).await;
    }
}

struct RefreshFailure {
    message: String,
    invalidate_session: bool,
}

async fn refresh_tokens(
    client: &reqwest::Client,
    refresh_token: &str,
) -> Result<TokenResponse, RefreshFailure> {
    let form = form_encode(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", CLIENT_ID),
    ])
    .map_err(|message| RefreshFailure {
        message,
        invalidate_session: false,
    })?;
    let response = client
        .post(TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await
        .map_err(|err| RefreshFailure {
            message: format!("刷新 Grok Build OAuth token 失败: {err}"),
            invalidate_session: false,
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(RefreshFailure {
            message: oauth_http_error("刷新 Grok Build OAuth token 失败", status, &body),
            invalidate_session: should_invalidate_refresh(status, &body),
        });
    }

    response
        .json::<TokenResponse>()
        .await
        .map_err(|err| RefreshFailure {
            message: format!("解析刷新 token 响应失败: {err}"),
            invalidate_session: false,
        })
}

fn session_needs_refresh(session: &GrokBuildOauthSession) -> bool {
    match session.expires_at_ms {
        Some(expires_at_ms) => expires_at_ms <= now_ms().saturating_add(REFRESH_SKEW_SECS * 1000),
        None => false,
    }
}

fn session_has_runtime_auth_material(session: &GrokBuildOauthSession) -> bool {
    !session.access_token.trim().is_empty()
}

fn session_from_token_response(
    token_response: TokenResponse,
    previous: Option<&GrokBuildOauthSession>,
) -> Result<GrokBuildOauthSession, String> {
    let access_token = token_response.access_token.trim().to_string();
    if access_token.is_empty() {
        return Err("OAuth 响应缺少 access_token，无法继续。".to_string());
    }
    let refresh_token = token_response
        .refresh_token
        .filter(|value| !value.trim().is_empty())
        .or_else(|| previous.map(|session| session.refresh_token.clone()))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "OAuth 响应缺少 refresh_token，无法继续。".to_string())?;

    let mut session = GrokBuildOauthSession {
        id_token: token_response
            .id_token
            .filter(|value| !value.trim().is_empty())
            .or_else(|| previous.map(|session| session.id_token.clone()))
            .unwrap_or_default(),
        access_token,
        refresh_token,
        expires_at_ms: token_response
            .expires_in
            .map(|expires_in| now_ms().saturating_add(expires_in * 1000))
            .or_else(|| previous.and_then(|session| session.expires_at_ms)),
        account_id: previous.and_then(|session| session.account_id.clone()),
        email: previous.and_then(|session| session.email.clone()),
        plan_type: previous.and_then(|session| session.plan_type.clone()),
    };
    enrich_session_from_tokens(&mut session);
    Ok(session)
}

fn persist_login_session(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    session: GrokBuildOauthSession,
) -> Result<GrokBuildOauthStatus, String> {
    save_session_to_storage(app_handle, &session)?;
    state.set_grok_build_oauth_session(Some(session.clone()));
    Ok(make_status(Some(&session)))
}

pub fn should_prewarm_runtime_session(
    provider: &str,
    config: &LlmProviderConfig,
    session: Option<&GrokBuildOauthSession>,
) -> bool {
    if provider != XAI_PROVIDER {
        return false;
    }
    let Some(session) = session else {
        return false;
    };
    if effective_xai_auth_mode(config.xai_auth_mode, true) != XaiAuthMode::Oauth {
        return false;
    }
    session_needs_refresh(session) || !session_has_runtime_auth_material(session)
}

async fn refresh_session_if_needed(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    session: GrokBuildOauthSession,
) -> Result<GrokBuildOauthSession, String> {
    if !session_needs_refresh(&session) && session_has_runtime_auth_material(&session) {
        return Ok(session);
    }
    if session.refresh_token.trim().is_empty() {
        return Err("Grok Build OAuth 会话缺少 refresh token，请重新登录。".to_string());
    }

    let token_response = match refresh_tokens(&state.http_client, &session.refresh_token).await {
        Ok(token_response) => token_response,
        Err(err) => {
            if err.invalidate_session {
                logout(app_handle, state);
                return Err("Grok Build 登录已失效，请重新登录。".to_string());
            }
            return Err(err.message);
        }
    };

    let refreshed = session_from_token_response(token_response, Some(&session))?;
    save_session_to_storage(app_handle, &refreshed)?;
    state.set_grok_build_oauth_session(Some(refreshed.clone()));
    Ok(refreshed)
}

pub fn sync_runtime_session(
    app_handle: &tauri::AppHandle,
    state: &AppState,
) -> Option<GrokBuildOauthSession> {
    let session = load_session_from_storage(app_handle);
    state.set_grok_build_oauth_session(session.clone());
    session
}

pub fn status(state: &AppState) -> GrokBuildOauthStatus {
    make_status(state.read_grok_build_oauth_session().as_ref())
}

pub async fn prewarm_runtime_session(
    app_handle: &tauri::AppHandle,
    state: &AppState,
) -> Result<(), String> {
    let provider = state.active_llm_provider();
    let config = state.llm_provider_config();
    let session = state.read_grok_build_oauth_session();
    if !should_prewarm_runtime_session(&provider, &config, session.as_ref()) {
        return Ok(());
    }
    let session = session.expect("prewarm decision requires a session");
    refresh_session_if_needed(app_handle, state, session).await?;
    Ok(())
}

pub async fn login(
    app_handle: &tauri::AppHandle,
    state: &AppState,
) -> Result<GrokBuildOauthStatus, String> {
    let listener = bind_callback_listener().await?;
    let (code_verifier, code_challenge) = generate_pkce_pair();
    let state_token = generate_state();
    let auth_url = build_authorize_url(&code_challenge, &state_token)?;

    webbrowser::open(&auth_url).map_err(|err| format!("打开浏览器失败: {err}"))?;

    let OAuthCallback { code, mut stream } = wait_for_callback(listener, state_token).await?;
    let token_response =
        match exchange_code_for_tokens(&state.http_client, &code, &code_verifier).await {
            Ok(tokens) => tokens,
            Err(err) => {
                let html = callback_html("Authorization Failed", &err, false);
                let _ = respond_with_html(&mut stream, "200 OK", &html).await;
                return Err(err);
            }
        };

    let session = match session_from_token_response(token_response, None) {
        Ok(session) => session,
        Err(err) => {
            let html = callback_html("Authorization Failed", &err, false);
            let _ = respond_with_html(&mut stream, "200 OK", &html).await;
            return Err(err);
        }
    };

    let status = match persist_login_session(app_handle, state, session) {
        Ok(status) => status,
        Err(err) => {
            let html = callback_html("Authorization Failed", &err, false);
            let _ = respond_with_html(&mut stream, "200 OK", &html).await;
            return Err(err);
        }
    };
    let html = callback_html(
        "Authorization Successful",
        "可以关闭这个页面并返回轻语。",
        true,
    );
    let _ = respond_with_html(&mut stream, "200 OK", &html).await;
    Ok(status)
}

pub async fn start_device_code_login(
    state: &AppState,
) -> Result<GrokBuildOauthDeviceCodeChallenge, String> {
    let challenge = request_device_code(&state.http_client).await?;
    if let Err(err) = webbrowser::open(&challenge.verification_url) {
        log::warn!("打开 Grok Build 设备码验证页失败，前端将展示 URL: {}", err);
    }
    Ok(challenge)
}

pub async fn complete_device_code_login(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    challenge: GrokBuildOauthDeviceCodeChallenge,
) -> Result<GrokBuildOauthStatus, String> {
    if challenge.device_code.trim().is_empty() {
        return Err("Grok Build 设备码已失效，请重新开始登录。".to_string());
    }
    let token_response = poll_device_code_token(&state.http_client, &challenge).await?;
    let session = session_from_token_response(token_response, None)?;
    persist_login_session(app_handle, state, session)
}

pub fn logout(app_handle: &tauri::AppHandle, state: &AppState) {
    clear_session_from_storage(app_handle);
    state.set_grok_build_oauth_session(None);
}

pub fn grok_cli_request_headers(access_token: &str) -> Result<reqwest::header::HeaderMap, String> {
    use reqwest::header::{
        HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT,
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {access_token}"))
            .map_err(|err| format!("Grok Build token 包含非法字符: {err}"))?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static(GROK_CLI_USER_AGENT));
    headers.insert(
        HeaderName::from_static("x-grok-client-identifier"),
        HeaderValue::from_static(GROK_CLI_CLIENT_IDENTIFIER),
    );
    headers.insert(
        "X-XAI-Token-Auth",
        HeaderValue::from_static(GROK_CLI_TOKEN_AUTH),
    );
    headers.insert(
        HeaderName::from_static("x-grok-client-version"),
        HeaderValue::from_static(GROK_CLI_CLIENT_VERSION),
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    Ok(headers)
}

pub async fn resolve_oauth_origin_api_key(
    app_handle: &tauri::AppHandle,
    state: &AppState,
) -> Result<String, String> {
    let Some(session) = state.read_grok_build_oauth_session() else {
        return Ok(String::new());
    };
    let session = refresh_session_if_needed(app_handle, state, session).await?;
    Ok(encode_grok_build_oauth_access_token(&session.access_token).unwrap_or_default())
}
