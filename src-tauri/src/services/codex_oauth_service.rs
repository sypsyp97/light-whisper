use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use rand::{distributions::Alphanumeric, rngs::OsRng, Rng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri_plugin_keyring::KeyringExt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use crate::services::llm_provider::KEYRING_SERVICE;
use crate::state::user_profile::OpenaiAuthMode;
use crate::state::AppState;
use crate::utils::paths;

const OPENAI_PROVIDER: &str = "openai";
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const ISSUER: &str = "https://auth.openai.com";
pub const ORIGINATOR: &str = "codex_cli_rs";
pub const CHATGPT_BEARER_USER_AGENT: &str = "codex-cli";
pub const CHATGPT_CODEX_RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
const DEFAULT_CALLBACK_PORT: u16 = 1455;
const CALLBACK_PATH: &str = "/auth/callback";
const SESSION_KEYRING_USER: &str = "openai-codex-oauth";
const SESSION_REFRESH_TOKEN_KEYRING_USER: &str = "openai-codex-oauth-refresh-token";
const OAUTH_TIMEOUT_SECS: u64 = 5 * 60;
const REFRESH_SKEW_SECS: u64 = 60;
const CHATGPT_BEARER_PREFIX: &str = "openai-codex-chatgpt:";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenaiCodexOauthSession {
    pub id_token: String,
    pub access_token: String,
    pub refresh_token: String,
    pub api_key: String,
    pub expires_at_ms: Option<u64>,
    pub account_id: Option<String>,
    pub email: Option<String>,
    pub plan_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OpenaiCodexOauthStatus {
    pub logged_in: bool,
    pub email: Option<String>,
    pub plan_type: Option<String>,
    pub account_id: Option<String>,
    pub expires_at_ms: Option<u64>,
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
struct TokenExchangeResponse {
    access_token: String,
}

#[derive(Debug, Deserialize, Default)]
struct JwtClaims {
    #[serde(default)]
    exp: Option<u64>,
    #[serde(default)]
    email: Option<String>,
    #[serde(rename = "https://api.openai.com/profile", default)]
    profile: Option<ProfileClaims>,
    #[serde(rename = "https://api.openai.com/auth", default)]
    auth: Option<AuthClaims>,
}

#[derive(Debug, Deserialize, Default)]
struct ProfileClaims {
    #[serde(default)]
    email: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AuthClaims {
    #[serde(default)]
    chatgpt_account_id: Option<String>,
    #[serde(default)]
    chatgpt_plan_type: Option<String>,
}

#[derive(Debug)]
struct OAuthCallback {
    code: String,
    stream: tokio::net::TcpStream,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatgptBearerToken {
    pub access_token: String,
    pub account_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedOpenaiCodexOauthSession {
    pub expires_at_ms: Option<u64>,
    pub account_id: Option<String>,
    pub email: Option<String>,
    pub plan_type: Option<String>,
}

struct CallbackListeners {
    ipv4: TcpListener,
    ipv6: Option<TcpListener>,
    port: u16,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn make_status(session: Option<&OpenaiCodexOauthSession>) -> OpenaiCodexOauthStatus {
    if let Some(session) = session {
        return OpenaiCodexOauthStatus {
            logged_in: true,
            email: session.email.clone(),
            plan_type: session.plan_type.clone(),
            account_id: session.account_id.clone(),
            expires_at_ms: session.expires_at_ms,
        };
    }

    OpenaiCodexOauthStatus::default()
}

fn session_meta_path() -> std::path::PathBuf {
    paths::get_data_dir().join("openai_codex_oauth_session.json")
}

fn read_session_meta() -> PersistedOpenaiCodexOauthSession {
    std::fs::read_to_string(session_meta_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<PersistedOpenaiCodexOauthSession>(&raw).ok())
        .unwrap_or_default()
}

fn write_session_meta(session: &OpenaiCodexOauthSession) -> Result<(), String> {
    let persisted = PersistedOpenaiCodexOauthSession {
        expires_at_ms: session.expires_at_ms,
        account_id: session.account_id.clone(),
        email: session.email.clone(),
        plan_type: session.plan_type.clone(),
    };
    let raw = serde_json::to_string(&persisted)
        .map_err(|err| format!("序列化 Codex OAuth 元数据失败: {err}"))?;
    std::fs::write(session_meta_path(), raw)
        .map_err(|err| format!("保存 Codex OAuth 元数据失败: {err}"))
}

fn load_session_from_storage(app_handle: &tauri::AppHandle) -> Option<OpenaiCodexOauthSession> {
    let refresh_token = app_handle
        .keyring()
        .get_password(KEYRING_SERVICE, SESSION_REFRESH_TOKEN_KEYRING_USER)
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty());

    if let Some(refresh_token) = refresh_token {
        let meta = read_session_meta();
        return Some(OpenaiCodexOauthSession {
            id_token: String::new(),
            access_token: String::new(),
            refresh_token,
            api_key: String::new(),
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
        .and_then(|raw| serde_json::from_str::<OpenaiCodexOauthSession>(&raw).ok())
}

fn save_session_to_storage(
    app_handle: &tauri::AppHandle,
    session: &OpenaiCodexOauthSession,
) -> Result<(), String> {
    app_handle
        .keyring()
        .set_password(
            KEYRING_SERVICE,
            SESSION_REFRESH_TOKEN_KEYRING_USER,
            &session.refresh_token,
        )
        .map_err(|err| format!("保存 Codex OAuth refresh token 失败: {err}"))?;
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

fn enrich_session_from_tokens(
    session: &mut OpenaiCodexOauthSession,
    id_token_fallback: Option<&str>,
) {
    let claims = decode_jwt_claims(&session.id_token)
        .or_else(|| id_token_fallback.and_then(decode_jwt_claims))
        .or_else(|| decode_jwt_claims(&session.access_token));

    if let Some(claims) = claims {
        session.email = claims
            .email
            .or_else(|| claims.profile.and_then(|profile| profile.email));
        if let Some(auth) = claims.auth {
            session.account_id = auth
                .chatgpt_account_id
                .or_else(|| session.account_id.take());
            session.plan_type = auth.chatgpt_plan_type.or_else(|| session.plan_type.take());
        }
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

fn encode_chatgpt_bearer_token(token: &ChatgptBearerToken) -> Option<String> {
    let raw = serde_json::to_vec(token).ok()?;
    Some(format!(
        "{CHATGPT_BEARER_PREFIX}{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
    ))
}

pub fn decode_chatgpt_bearer_token(input: &str) -> Option<ChatgptBearerToken> {
    let payload = input.trim().strip_prefix(CHATGPT_BEARER_PREFIX)?;
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    serde_json::from_slice::<ChatgptBearerToken>(&raw).ok()
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
        .map(|ch| ch as u8)
        .collect::<Vec<_>>();
    base64_url_encode(&bytes)
}

fn oauth_error_message(error_code: &str, error_description: Option<&str>) -> String {
    if error_code == "access_denied"
        && error_description
            .unwrap_or_default()
            .to_ascii_lowercase()
            .contains("missing_codex_entitlement")
    {
        return "当前 ChatGPT 工作区没有 Codex 权限，请联系管理员开通。".to_string();
    }

    if let Some(description) = error_description.filter(|value| !value.trim().is_empty()) {
        return format!("OpenAI Codex OAuth 登录失败: {description}");
    }

    format!("OpenAI Codex OAuth 登录失败: {error_code}")
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
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{title}</title></head><body style=\"margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;background:#111827;color:#f9fafb;font-family:system-ui,-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;text-align:center;\"><div style=\"max-width:680px;padding:48px 32px;display:flex;flex-direction:column;align-items:center;gap:18px;\"><h1 style=\"margin:0;font-size:34px;line-height:1.2;font-weight:700;\">{title}</h1><p style=\"margin:0;font-size:21px;line-height:1.65;white-space:pre-wrap;\">{message}</p></div>{script}</body></html>"
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

fn is_ipv6_loopback_unavailable(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::AddrNotAvailable | std::io::ErrorKind::Unsupported
    ) || matches!(err.raw_os_error(), Some(10047) | Some(10049))
}

async fn bind_callback_listeners() -> Result<CallbackListeners, String> {
    for preferred_port in [Some(DEFAULT_CALLBACK_PORT), None] {
        for _attempt in 0..8 {
            let ipv4 = TcpListener::bind(("127.0.0.1", preferred_port.unwrap_or(0)))
                .await
                .map_err(|err| format!("启动 OAuth IPv4 回调服务失败: {err}"))?;
            let actual_port = ipv4
                .local_addr()
                .map_err(|err| format!("读取 OAuth 回调端口失败: {err}"))?
                .port();

            match TcpListener::bind(("::1", actual_port)).await {
                Ok(ipv6) => {
                    return Ok(CallbackListeners {
                        ipv4,
                        ipv6: Some(ipv6),
                        port: actual_port,
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {
                    drop(ipv4);
                    continue;
                }
                Err(err) if is_ipv6_loopback_unavailable(&err) => {
                    return Ok(CallbackListeners {
                        ipv4,
                        ipv6: None,
                        port: actual_port,
                    });
                }
                Err(err) => {
                    return Err(format!("启动 OAuth IPv6 回调服务失败: {err}"));
                }
            }
        }
    }

    Err("启动 OAuth 回调服务失败：无法同时占用本地回调端口。".to_string())
}

async fn accept_callback_connection(
    listeners: CallbackListeners,
) -> Result<(tokio::net::TcpStream, std::net::SocketAddr), String> {
    let CallbackListeners { ipv4, ipv6, .. } = listeners;
    if let Some(ipv6) = ipv6 {
        tokio::select! {
            result = ipv4.accept() => result.map_err(|err| format!("接受 OpenAI OAuth IPv4 回调失败: {err}")),
            result = ipv6.accept() => result.map_err(|err| format!("接受 OpenAI OAuth IPv6 回调失败: {err}")),
        }
    } else {
        ipv4.accept()
            .await
            .map_err(|err| format!("接受 OpenAI OAuth 回调失败: {err}"))
    }
}

async fn wait_for_callback(
    listeners: CallbackListeners,
    expected_state: String,
) -> Result<OAuthCallback, String> {
    let (mut stream, _) = tokio::time::timeout(
        Duration::from_secs(OAUTH_TIMEOUT_SECS),
        accept_callback_connection(listeners),
    )
    .await
    .map_err(|_| "等待 OpenAI OAuth 回调超时，请重试。".to_string())??;

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
        return Err("OpenAI OAuth state 校验失败，请重试。".to_string());
    }

    if let Some(error_code) = query.get("error") {
        let message = oauth_error_message(
            error_code,
            query.get("error_description").map(String::as_str),
        );
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

async fn exchange_code_for_tokens(
    client: &reqwest::Client,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<TokenResponse, String> {
    let response = client
        .post(format!("{ISSUER}/oauth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(
            reqwest::Url::parse_with_params(
                "http://localhost",
                &[
                    ("grant_type", "authorization_code"),
                    ("code", code),
                    ("redirect_uri", redirect_uri),
                    ("client_id", CLIENT_ID),
                    ("code_verifier", code_verifier),
                ],
            )
            .map_err(|err| format!("构造 OAuth 授权码交换参数失败: {err}"))?
            .query()
            .unwrap_or_default()
            .to_string(),
        )
        .send()
        .await
        .map_err(|err| format!("OAuth 授权码换 token 失败: {err}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("OAuth 授权码换 token 失败 {status}: {body}"));
    }

    response
        .json::<TokenResponse>()
        .await
        .map_err(|err| format!("解析 OAuth token 响应失败: {err}"))
}

async fn refresh_tokens(
    client: &reqwest::Client,
    refresh_token: &str,
) -> Result<TokenResponse, String> {
    let response = client
        .post(format!("{ISSUER}/oauth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(
            reqwest::Url::parse_with_params(
                "http://localhost",
                &[
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh_token),
                    ("client_id", CLIENT_ID),
                ],
            )
            .map_err(|err| format!("构造 OAuth 刷新参数失败: {err}"))?
            .query()
            .unwrap_or_default()
            .to_string(),
        )
        .send()
        .await
        .map_err(|err| format!("刷新 Codex OAuth token 失败: {err}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("刷新 Codex OAuth token 失败 {status}: {body}"));
    }

    response
        .json::<TokenResponse>()
        .await
        .map_err(|err| format!("解析刷新 token 响应失败: {err}"))
}

async fn exchange_id_token_for_api_key(
    client: &reqwest::Client,
    id_token: &str,
) -> Result<String, String> {
    let response = client
        .post(format!("{ISSUER}/oauth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(
            reqwest::Url::parse_with_params(
                "http://localhost",
                &[
                    (
                        "grant_type",
                        "urn:ietf:params:oauth:grant-type:token-exchange",
                    ),
                    ("client_id", CLIENT_ID),
                    ("requested_token", "openai-api-key"),
                    ("subject_token", id_token),
                    (
                        "subject_token_type",
                        "urn:ietf:params:oauth:token-type:id_token",
                    ),
                ],
            )
            .map_err(|err| format!("构造 OpenAI API Key 交换参数失败: {err}"))?
            .query()
            .unwrap_or_default()
            .to_string(),
        )
        .send()
        .await
        .map_err(|err| format!("交换 OpenAI API Key 失败: {err}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("交换 OpenAI API Key 失败 {status}: {body}"));
    }

    let payload = response
        .json::<TokenExchangeResponse>()
        .await
        .map_err(|err| format!("解析 OpenAI API Key 交换响应失败: {err}"))?;

    Ok(payload.access_token)
}

fn build_authorize_url(
    redirect_uri: &str,
    code_challenge: &str,
    state: &str,
) -> Result<String, String> {
    let url = reqwest::Url::parse_with_params(
        &format!("{ISSUER}/oauth/authorize"),
        &[
            ("response_type", "code"),
            ("client_id", CLIENT_ID),
            ("redirect_uri", redirect_uri),
            (
                "scope",
                "openid profile email offline_access api.connectors.read api.connectors.invoke",
            ),
            ("code_challenge", code_challenge),
            ("code_challenge_method", "S256"),
            ("id_token_add_organizations", "true"),
            ("codex_cli_simplified_flow", "true"),
            ("state", state),
            ("originator", ORIGINATOR),
        ],
    )
    .map_err(|err| format!("构造 OpenAI OAuth 地址失败: {err}"))?;

    Ok(url.to_string())
}

fn session_needs_refresh(session: &OpenaiCodexOauthSession) -> bool {
    match session.expires_at_ms {
        Some(expires_at_ms) => expires_at_ms <= now_ms().saturating_add(REFRESH_SKEW_SECS * 1000),
        None => false,
    }
}

async fn refresh_session_if_needed(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    mut session: OpenaiCodexOauthSession,
) -> Result<OpenaiCodexOauthSession, String> {
    if !session_needs_refresh(&session)
        && (!session.api_key.trim().is_empty() || !session.access_token.trim().is_empty())
    {
        return Ok(session);
    }

    let needs_rehydration =
        session.id_token.trim().is_empty() || session.access_token.trim().is_empty();

    let refreshed = if session_needs_refresh(&session) || needs_rehydration {
        if session.refresh_token.trim().is_empty() {
            return Err("OpenAI Codex OAuth 会话缺少 refresh token，请重新登录。".to_string());
        }
        let token_response = refresh_tokens(&state.http_client, &session.refresh_token).await?;
        let id_token = token_response
            .id_token
            .unwrap_or_else(|| session.id_token.clone());
        let access_token = token_response.access_token;
        let refresh_token = token_response
            .refresh_token
            .unwrap_or_else(|| session.refresh_token.clone());
        let expires_at_ms = token_response
            .expires_in
            .map(|expires_in| now_ms().saturating_add(expires_in * 1000));

        OpenaiCodexOauthSession {
            id_token,
            access_token,
            refresh_token,
            api_key: String::new(),
            expires_at_ms,
            account_id: session.account_id.take(),
            email: session.email.take(),
            plan_type: session.plan_type.take(),
        }
    } else {
        session
    };

    let mut refreshed = refreshed;
    refreshed.api_key = match exchange_id_token_for_api_key(&state.http_client, &refreshed.id_token)
        .await
    {
        Ok(api_key) => api_key,
        Err(err) => {
            log::warn!(
                "OpenAI Codex OAuth 无法交换 OpenAI API Key，将继续使用 ChatGPT bearer 模式: {}",
                err
            );
            String::new()
        }
    };
    enrich_session_from_tokens(&mut refreshed, None);
    save_session_to_storage(app_handle, &refreshed)?;
    state.set_openai_codex_oauth_session(Some(refreshed.clone()));
    Ok(refreshed)
}

pub fn sync_runtime_session(
    app_handle: &tauri::AppHandle,
    state: &AppState,
) -> Option<OpenaiCodexOauthSession> {
    let session = load_session_from_storage(app_handle);
    state.set_openai_codex_oauth_session(session.clone());
    session
}

pub fn status(state: &AppState) -> OpenaiCodexOauthStatus {
    make_status(state.read_openai_codex_oauth_session().as_ref())
}

pub async fn login(
    app_handle: &tauri::AppHandle,
    state: &AppState,
) -> Result<OpenaiCodexOauthStatus, String> {
    let listeners = bind_callback_listeners().await?;
    let redirect_uri = format!("http://localhost:{}{CALLBACK_PATH}", listeners.port);
    let (code_verifier, code_challenge) = generate_pkce_pair();
    let state_token = generate_state();
    let auth_url = build_authorize_url(&redirect_uri, &code_challenge, &state_token)?;

    webbrowser::open(&auth_url).map_err(|err| format!("打开浏览器失败: {err}"))?;

    let OAuthCallback { code, mut stream } = wait_for_callback(listeners, state_token).await?;
    let token_response =
        match exchange_code_for_tokens(&state.http_client, &code, &redirect_uri, &code_verifier)
            .await
        {
            Ok(tokens) => tokens,
            Err(err) => {
                let html = callback_html("Authorization Failed", &err, false);
                let _ = respond_with_html(&mut stream, "200 OK", &html).await;
                return Err(err);
            }
        };

    let id_token = match token_response.id_token.clone() {
        Some(id_token) if !id_token.trim().is_empty() => id_token,
        _ => {
            let err = "OAuth 响应缺少 id_token，无法继续。".to_string();
            let html = callback_html("Authorization Failed", &err, false);
            let _ = respond_with_html(&mut stream, "200 OK", &html).await;
            return Err(err);
        }
    };
    let refresh_token = match token_response
        .refresh_token
        .clone()
        .filter(|value| !value.trim().is_empty())
    {
        Some(refresh_token) => refresh_token,
        None => {
            let err = "OAuth 响应缺少 refresh_token，无法继续。".to_string();
            let html = callback_html("Authorization Failed", &err, false);
            let _ = respond_with_html(&mut stream, "200 OK", &html).await;
            return Err(err);
        }
    };
    let api_key = match exchange_id_token_for_api_key(&state.http_client, &id_token).await {
        Ok(api_key) => api_key,
        Err(err) => {
            log::warn!(
                "OpenAI Codex OAuth 无法交换 OpenAI API Key，将继续使用 ChatGPT bearer 模式: {}",
                err
            );
            String::new()
        }
    };

    let mut session = OpenaiCodexOauthSession {
        id_token,
        access_token: token_response.access_token,
        refresh_token,
        api_key,
        expires_at_ms: token_response
            .expires_in
            .map(|expires_in| now_ms().saturating_add(expires_in * 1000)),
        account_id: None,
        email: None,
        plan_type: None,
    };
    enrich_session_from_tokens(&mut session, None);

    if let Err(err) = save_session_to_storage(app_handle, &session) {
        let html = callback_html("Authorization Failed", &err, false);
        let _ = respond_with_html(&mut stream, "200 OK", &html).await;
        return Err(err);
    }
    state.set_openai_codex_oauth_session(Some(session.clone()));
    let html = callback_html(
        "Authorization Successful",
        "可以关闭这个页面并返回轻语。",
        true,
    );
    let _ = respond_with_html(&mut stream, "200 OK", &html).await;

    Ok(make_status(Some(&session)))
}

pub fn logout(app_handle: &tauri::AppHandle, state: &AppState) {
    clear_session_from_storage(app_handle);
    state.set_openai_codex_oauth_session(None);
}

pub async fn resolve_api_key_for_provider(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    provider: &str,
    manual_api_key: &str,
) -> Result<String, String> {
    let manual_api_key = manual_api_key.trim();

    // 非 OpenAI provider：直接返回用户填的 key（可能为空，由调用方决定报错）
    if provider != OPENAI_PROVIDER {
        return Ok(manual_api_key.to_string());
    }

    // OpenAI：按用户选的认证方式分支。存储的偏好可能是 None（用户还没点过开关），
    // 那就按 OAuth 登录状态智能推断：登录过 → Oauth；没登录 → ApiKey。
    // 这跟前端的默认计算保持一致，避免前后端对同一条 profile 给出不同决策。
    let stored_mode = state.llm_provider_config().openai_auth_mode;
    let effective_mode = stored_mode.unwrap_or_else(|| {
        if state.read_openai_codex_oauth_session().is_some() {
            OpenaiAuthMode::Oauth
        } else {
            OpenaiAuthMode::ApiKey
        }
    });

    match effective_mode {
        OpenaiAuthMode::ApiKey => {
            // 明确走 API Key：只看手填的 key，即使 OAuth 已登录也不用
            Ok(manual_api_key.to_string())
        }
        OpenaiAuthMode::Oauth => {
            // 明确走 OAuth：读 session，忽略手填 key
            let Some(session) = state.read_openai_codex_oauth_session() else {
                return Ok(String::new());
            };

            let session = refresh_session_if_needed(app_handle, state, session).await?;
            if !session.api_key.trim().is_empty() {
                return Ok(session.api_key);
            }

            if session.access_token.trim().is_empty() {
                return Ok(String::new());
            }

            encode_chatgpt_bearer_token(&ChatgptBearerToken {
                access_token: session.access_token,
                account_id: session.account_id,
            })
            .ok_or_else(|| "编码 OpenAI Codex bearer 会话失败".to_string())
        }
    }
}
