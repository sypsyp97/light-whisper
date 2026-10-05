//! Official local OSS Sign in with ChatGPT flow.
//! https://developers.openai.com/siwc/token-sharing-open-source/sign-in
use super::*;
use ring::signature::{RsaPublicKeyComponents, RSA_PKCS1_2048_8192_SHA256};

pub const RESOURCE: &str = "https://api.openai.com/v1";
const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const DISCOVERY_URL: &str = "https://auth.openai.com/.well-known/openid-configuration";
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatgptRegistration {
    pub client_id: String,
    pub subject: String,
    pub host_id: String,
    pub scopes: Vec<String>,
    pub email: Option<String>,
}

impl ChatgptRegistration {
    pub fn plan_usage_enabled(&self) -> bool {
        self.scopes.iter().any(|s| s == "chatgpt.tokens.use.direct")
            && self.scopes.iter().any(|s| s == "resource.invoke")
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatgptAccount {
    pub client_id: String,
    pub email: Option<String>,
    pub pending: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct Registry {
    host_id: String,
    accounts: Vec<ChatgptRegistration>,
    #[serde(default)]
    pending_client_ids: Vec<String>,
    #[serde(default)]
    last_client_id: Option<String>,
}

struct LoginTarget<'a> {
    client_id: &'a str,
    registration: Option<&'a ChatgptRegistration>,
}

static REGISTRY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn registry_path() -> std::path::PathBuf {
    paths::get_data_dir().join("chatgpt_accounts.json")
}

fn read_registry() -> Result<Registry, String> {
    let _guard = REGISTRY_LOCK.lock().map_err(|_| "ChatGPT 账户记录被锁定")?;
    read_registry_unlocked()
}

fn read_registry_unlocked() -> Result<Registry, String> {
    read_registry_at(&registry_path())
}

fn read_registry_at(path: &std::path::Path) -> Result<Registry, String> {
    match std::fs::read(path) {
        Ok(raw) => serde_json::from_slice(&raw).map_err(|_| "读取 ChatGPT 账户记录失败".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut bytes = [0u8; 16];
            OsRng.fill(&mut bytes);
            bytes[6] = (bytes[6] & 0x0f) | 0x40;
            bytes[8] = (bytes[8] & 0x3f) | 0x80;
            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            let registry = Registry {
                host_id: format!(
                    "urn:uuid:{}-{}-{}-{}-{}",
                    &hex[..8],
                    &hex[8..12],
                    &hex[12..16],
                    &hex[16..20],
                    &hex[20..]
                ),
                accounts: Vec::new(),
                ..Default::default()
            };
            write_registry_at(path, &registry)?;
            Ok(registry)
        }
        Err(_) => Err("读取 ChatGPT 账户记录失败".into()),
    }
}

fn write_registry_at(path: &std::path::Path, registry: &Registry) -> Result<(), String> {
    let raw = serde_json::to_vec(registry).map_err(|_| "序列化 ChatGPT 账户记录失败")?;
    paths::atomic_write(path, &raw).map_err(|_| "保存 ChatGPT 账户记录失败".into())
}

pub(super) fn forget_account(client_id: &str) -> Result<(), String> {
    forget_account_at(&registry_path(), client_id)
}

fn forget_account_at(path: &std::path::Path, client_id: &str) -> Result<(), String> {
    let _guard = REGISTRY_LOCK.lock().map_err(|_| "ChatGPT 账户记录被锁定")?;
    let mut registry = read_registry_at(path)?;
    registry.accounts.retain(|r| r.client_id != client_id);
    registry.pending_client_ids.retain(|id| id != client_id);
    if registry.last_client_id.as_deref() == Some(client_id) {
        registry.last_client_id = None;
    }
    // Keep the host identity even when the last registration is removed.
    write_registry_at(path, &registry)
}

fn select_account<'a>(
    registry: &'a Registry,
    client_id: Option<&str>,
    new_account: bool,
) -> Result<Option<LoginTarget<'a>>, String> {
    if new_account {
        return if client_id.is_none() {
            Ok(None)
        } else {
            Err("新建 ChatGPT 注册时不能指定已有账户。".into())
        };
    }
    let requested = client_id.or_else(|| preferred_client_id(registry));
    match requested {
        Some(id) => registry
            .accounts
            .iter()
            .find(|r| r.client_id == id)
            .map(|registration| LoginTarget {
                client_id: &registration.client_id,
                registration: Some(registration),
            })
            .or_else(|| {
                registry
                    .pending_client_ids
                    .iter()
                    .find(|pending| pending.as_str() == id)
                    .map(|id| LoginTarget {
                        client_id: id,
                        registration: None,
                    })
            })
            .map(Some)
            .ok_or_else(|| "ChatGPT 账户记录不存在，请添加账户。".into()),
        None => Ok(None),
    }
}

fn preferred_client_id(registry: &Registry) -> Option<&str> {
    registry
        .last_client_id
        .as_deref()
        .filter(|id| {
            registry.accounts.iter().any(|r| r.client_id == *id)
                || registry
                    .pending_client_ids
                    .iter()
                    .any(|pending| pending == id)
        })
        .or_else(|| registry.accounts.last().map(|r| r.client_id.as_str()))
        .or_else(|| registry.pending_client_ids.last().map(String::as_str))
}

fn remember_pending(client_id: &str) -> Result<(), String> {
    remember_pending_at(&registry_path(), client_id)
}

fn remember_pending_at(path: &std::path::Path, client_id: &str) -> Result<(), String> {
    let _guard = REGISTRY_LOCK.lock().map_err(|_| "ChatGPT 账户记录被锁定")?;
    let mut registry = read_registry_at(path)?;
    if !registry.accounts.iter().any(|r| r.client_id == client_id) {
        registry.pending_client_ids.retain(|id| id != client_id);
        registry.pending_client_ids.push(client_id.to_owned());
    }
    registry.last_client_id = Some(client_id.to_owned());
    write_registry_at(path, &registry)
}

pub(super) fn remember_account(account: &ChatgptRegistration) -> Result<(), String> {
    remember_account_at(&registry_path(), account)
}

fn remember_account_at(
    path: &std::path::Path,
    account: &ChatgptRegistration,
) -> Result<(), String> {
    let _guard = REGISTRY_LOCK.lock().map_err(|_| "ChatGPT 账户记录被锁定")?;
    let mut registry = read_registry_at(path)?;
    if registry
        .accounts
        .iter()
        .any(|r| r.client_id == account.client_id && r.subject != account.subject)
    {
        return Err("ChatGPT 账户身份与原注册不一致，请重新添加账户。".into());
    }
    registry
        .accounts
        .retain(|r| r.client_id != account.client_id);
    registry.accounts.push(account.clone());
    registry
        .pending_client_ids
        .retain(|id| id != &account.client_id);
    registry.last_client_id = Some(account.client_id.clone());
    write_registry_at(path, &registry)
}

pub(super) fn saved_accounts() -> Vec<ChatgptAccount> {
    // Status reads must not register a new host or touch protected credentials.
    let Ok(raw) = std::fs::read(registry_path()) else {
        return Vec::new();
    };
    let Ok(registry) = serde_json::from_slice::<Registry>(&raw) else {
        return Vec::new();
    };
    account_summaries(registry)
}

fn account_summaries(registry: Registry) -> Vec<ChatgptAccount> {
    let preferred = preferred_client_id(&registry).map(str::to_owned);
    let mut accounts: Vec<_> = registry
        .accounts
        .into_iter()
        .map(|r| ChatgptAccount {
            client_id: r.client_id,
            email: r.email,
            pending: false,
        })
        .collect();
    accounts.extend(
        registry
            .pending_client_ids
            .into_iter()
            .map(|client_id| ChatgptAccount {
                client_id,
                email: None,
                pending: true,
            }),
    );
    if let Some(index) = accounts
        .iter()
        .position(|r| Some(&r.client_id) == preferred.as_ref())
    {
        let recent = accounts.remove(index);
        accounts.push(recent);
    }
    accounts
}

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    jwks_uri: String,
    revocation_endpoint: String,
}

fn trusted_auth_url(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "https"
            && u.host_str() == Some("auth.openai.com")
            && u.port_or_known_default() == Some(443)
            && u.username().is_empty()
            && u.password().is_none()
    })
}

async fn discovery(client: &reqwest::Client) -> Result<Discovery, String> {
    let data: Discovery = client
        .get(DISCOVERY_URL)
        .timeout(HTTP_TIMEOUT)
        .send()
        .await
        .map_err(|_| "获取 ChatGPT 授权元数据失败")?
        .error_for_status()
        .map_err(|_| "ChatGPT 授权元数据不可用")?
        .json()
        .await
        .map_err(|_| "ChatGPT 授权元数据格式无效")?;
    if data.issuer != ISSUER
        || !trusted_auth_url(&data.jwks_uri)
        || !trusted_auth_url(&data.revocation_endpoint)
    {
        return Err("ChatGPT 授权元数据来源无效".into());
    }
    Ok(data)
}

#[derive(Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}
#[derive(Deserialize)]
struct Jwk {
    kid: String,
    kty: String,
    n: String,
    e: String,
    alg: Option<String>,
}
#[derive(Deserialize)]
struct IdClaims {
    iss: String,
    aud: serde_json::Value,
    sub: String,
    exp: u64,
    #[serde(default)]
    nbf: Option<u64>,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

fn validate_id_token(
    token: &str,
    jwks: &Jwks,
    client_id: &str,
    nonce: Option<&str>,
    subject: Option<&str>,
    now_secs: u64,
) -> Result<IdClaims, String> {
    let parts = token.split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err("ChatGPT 身份 token 格式无效".into());
    }
    let decode = |s: &str| {
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(s)
            .map_err(|_| "ChatGPT 身份 token 编码无效".to_string())
    };
    let header: serde_json::Value =
        serde_json::from_slice(&decode(parts[0])?).map_err(|_| "ChatGPT 身份 token 头无效")?;
    if header["alg"].as_str() != Some("RS256") {
        return Err("ChatGPT 身份 token 签名算法无效".into());
    }
    let key = jwks
        .keys
        .iter()
        .find(|k| {
            Some(k.kid.as_str()) == header["kid"].as_str()
                && k.kty == "RSA"
                && k.alg.as_deref().is_none_or(|a| a == "RS256")
        })
        .ok_or("ChatGPT 身份 token 签名密钥不存在")?;
    RsaPublicKeyComponents {
        n: &decode(&key.n)?,
        e: &decode(&key.e)?,
    }
    .verify(
        &RSA_PKCS1_2048_8192_SHA256,
        format!("{}.{}", parts[0], parts[1]).as_bytes(),
        &decode(parts[2])?,
    )
    .map_err(|_| "ChatGPT 身份 token 签名验证失败")?;
    let claims: IdClaims =
        serde_json::from_slice(&decode(parts[1])?).map_err(|_| "ChatGPT 身份 token 内容无效")?;
    let audience_matches = claims.aud.as_str() == Some(client_id)
        || claims
            .aud
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(client_id)));
    if claims.iss != ISSUER
        || !audience_matches
        || claims.exp <= now_secs
        || claims.nbf.is_some_and(|n| n > now_secs.saturating_add(60))
        || claims.sub.trim().is_empty()
        || nonce.is_some_and(|n| claims.nonce.as_deref() != Some(n))
        || subject.is_some_and(|s| claims.sub != s)
    {
        return Err("ChatGPT 身份、有效期或登录 nonce 验证失败".into());
    }
    Ok(claims)
}

async fn verified_claims(
    client: &reqwest::Client,
    token: &str,
    client_id: &str,
    nonce: Option<&str>,
    subject: Option<&str>,
) -> Result<IdClaims, String> {
    let jwks = signing_keys(client).await?;
    validate_id_token(token, &jwks, client_id, nonce, subject, now_ms() / 1000)
}

async fn signing_keys(client: &reqwest::Client) -> Result<Jwks, String> {
    let metadata = discovery(client).await?;
    let jwks: Jwks = client
        .get(metadata.jwks_uri)
        .timeout(HTTP_TIMEOUT)
        .send()
        .await
        .map_err(|_| "获取 ChatGPT 签名密钥失败")?
        .error_for_status()
        .map_err(|_| "ChatGPT 签名密钥不可用")?
        .json()
        .await
        .map_err(|_| "ChatGPT 签名密钥格式无效")?;
    Ok(jwks)
}

fn authorize_url(
    redirect: &str,
    challenge: &str,
    state: &str,
    nonce: &str,
    registry: &Registry,
    account: Option<&LoginTarget<'_>>,
) -> Result<String, String> {
    let mut parameters = vec![
        (
            "client_id",
            account.map_or("dynamic_agent_client", |r| r.client_id),
        ),
        ("ext_agent_host_id", registry.host_id.as_str()),
        ("redirect_uri", redirect),
        ("response_type", "code"),
        ("scope", SCOPES),
        ("resource", RESOURCE),
        ("state", state),
        ("nonce", nonce),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
    ];
    if account.is_none() {
        parameters.push(("agent_name_hint", "light-whisper"));
    } else if account
        .and_then(|r| r.registration)
        .is_some_and(|r| !r.plan_usage_enabled())
    {
        // User explicitly asked to enable a previously declined plan grant.
        parameters.push(("prompt", "consent"));
    }
    reqwest::Url::parse_with_params(&format!("{ISSUER}/api/accounts/authorize"), &parameters)
        .map(|u| u.to_string())
        .map_err(|_| "构造 ChatGPT 登录地址失败".into())
}

async fn token_request(
    client: &reqwest::Client,
    form: &[(&str, &str)],
) -> Result<TokenResponse, String> {
    token_request_at(client, &format!("{ISSUER}/api/accounts/oauth/token"), form).await
}

async fn token_request_at(
    client: &reqwest::Client,
    endpoint: &str,
    form: &[(&str, &str)],
) -> Result<TokenResponse, String> {
    let response = client
        .post(endpoint)
        .timeout(HTTP_TIMEOUT)
        .form(form)
        .send()
        .await
        .map_err(|_| "ChatGPT token 请求失败，请稍后重试")?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|_| "读取 ChatGPT token 响应失败")?;
    if !status.is_success() {
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
        // Never echo an upstream body: token endpoints can include credentials.
        let code = value["error"]
            .as_str()
            .or_else(|| value["error"]["code"].as_str())
            .unwrap_or("");
        if matches!(
            code,
            "invalid_grant"
                | "invalid_refresh_token"
                | "token_expired"
                | "refresh_token_expired"
                | "refresh_token_reused"
                | "refresh_token_invalidated"
        ) {
            return Err("CHATGPT_REAUTH_REQUIRED: ChatGPT 授权已失效，请重新登录。".into());
        }
        return Err(format!("ChatGPT token 请求失败 ({status})，请稍后重试。"));
    }
    serde_json::from_slice(&bytes).map_err(|_| "ChatGPT token 响应格式无效".into())
}

fn session_from_tokens(
    tokens: TokenResponse,
    mut registration: ChatgptRegistration,
) -> Result<OpenaiCodexOauthSession, String> {
    if let Some(scope) = tokens.scope {
        registration.scopes = scope.split_whitespace().map(str::to_string).collect();
    }
    if tokens.access_token.trim().is_empty()
        || tokens
            .refresh_token
            .as_deref()
            .is_none_or(|t| t.trim().is_empty())
    {
        return Err("ChatGPT 登录未返回完整凭据，请重新登录。".into());
    }
    Ok(OpenaiCodexOauthSession {
        email: registration.email.clone(),
        account_id: Some(registration.subject.clone()),
        registration: Some(registration),
        id_token: tokens.id_token.unwrap_or_default(),
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token.unwrap(),
        expires_at_ms: tokens
            .expires_in
            .map(|s| now_ms().saturating_add(s.saturating_mul(1000))),
        ..Default::default()
    })
}

pub(super) async fn login(
    app: &tauri::AppHandle,
    state: &AppState,
    client_id: Option<&str>,
    new_account: bool,
) -> Result<OpenaiCodexOauthStatus, String> {
    let login = state
        .openai_codex_oauth_state()
        .begin_login_after_refresh()
        .await;
    let registry = read_registry()?;
    let previous = select_account(&registry, client_id, new_account)?;
    let listeners = bind_callback_listeners().await?;
    let redirect = format!("http://127.0.0.1:{}{CALLBACK_PATH}", listeners.port);
    let (verifier, challenge) = generate_pkce_pair();
    let pending_state = generate_state();
    let nonce = generate_state();
    let url = authorize_url(
        &redirect,
        &challenge,
        &pending_state,
        &nonce,
        &registry,
        previous.as_ref(),
    )?;
    webbrowser::open(&url).map_err(|_| "打开 ChatGPT 登录浏览器失败")?;
    let OAuthCallback {
        code,
        client_id: issued_id,
        mut stream,
    } = wait_for_callback(listeners, pending_state).await?;
    let result = async {
        let issued_id = match (previous.as_ref(), issued_id) {
            (Some(account), Some(id)) if id != account.client_id => {
                return Err("ChatGPT 返回的客户端与所选账户不一致".into())
            }
            (Some(account), _) => account.client_id.to_owned(),
            (None, Some(id)) if !id.trim().is_empty() && id != "dynamic_agent_client" => id,
            _ => return Err("ChatGPT 注册没有返回客户端 ID，请重新登录。".into()),
        };
        // The client is already registered upstream. Retain its ID before token
        // exchange, without publishing an identity or credentials yet.
        if !state
            .openai_codex_oauth_state()
            .record_login_progress(login.token(), || {
                if previous.as_ref().and_then(|r| r.registration).is_none() {
                    remember_pending(&issued_id)
                } else {
                    Ok(())
                }
            })?
        {
            return Err("ChatGPT 登录已被更新的操作取代，请重试。".into());
        }
        let tokens = token_request(
            &state.http_client,
            &[
                ("grant_type", "authorization_code"),
                ("client_id", &issued_id),
                ("code", &code),
                ("code_verifier", &verifier),
                ("redirect_uri", &redirect),
                ("resource", RESOURCE),
            ],
        )
        .await?;
        let claims = verified_claims(
            &state.http_client,
            tokens
                .id_token
                .as_deref()
                .ok_or("ChatGPT 登录缺少身份 token")?,
            &issued_id,
            Some(&nonce),
            previous
                .as_ref()
                .and_then(|r| r.registration)
                .map(|r| r.subject.as_str()),
        )
        .await?;
        let registration = ChatgptRegistration {
            client_id: issued_id,
            subject: claims.sub,
            host_id: registry.host_id.clone(),
            scopes: Vec::new(),
            email: claims.email,
        };
        let session = session_from_tokens(tokens, registration)?;
        persist_login_session(app, state, login.token(), session)
    }
    .await;
    let html = match &result {
        Ok(status) if !status.plan_usage_enabled => callback_html(
            "Signed In",
            "已登录，但未允许轻语使用 ChatGPT 套餐额度。请返回轻语重新授权。",
            false,
        ),
        Ok(_) => callback_html(
            "Authorization Successful",
            "可以关闭这个页面并返回轻语。",
            true,
        ),
        Err(error) => callback_html("Authorization Failed", error, false),
    };
    let _ = respond_with_html(&mut stream, "200 OK", &html).await;
    result
}

pub(super) async fn refresh(
    client: &reqwest::Client,
    session: &OpenaiCodexOauthSession,
) -> Result<OpenaiCodexOauthSession, String> {
    let registration = session
        .registration
        .as_ref()
        .ok_or("ChatGPT 客户端记录缺失")?;
    // Fetch verification material before rotating the refresh token. A failed
    // metadata request must not discard a successfully rotated credential.
    let keys = signing_keys(client).await?;
    let mut tokens = token_request(
        client,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", &registration.client_id),
            ("refresh_token", &session.refresh_token),
            ("resource", RESOURCE),
        ],
    )
    .await?;
    if let Some(id) = &tokens.id_token {
        validate_id_token(
            id,
            &keys,
            &registration.client_id,
            None,
            Some(&registration.subject),
            now_ms() / 1000,
        )?;
    }
    if tokens.refresh_token.is_none() {
        tokens.refresh_token = Some(session.refresh_token.clone());
    }
    let mut refreshed = session_from_tokens(tokens, registration.clone())?;
    if refreshed.id_token.is_empty() {
        refreshed.id_token.clone_from(&session.id_token);
    }
    Ok(refreshed)
}

pub(super) async fn revoke(
    client: &reqwest::Client,
    session: &OpenaiCodexOauthSession,
) -> Result<(), String> {
    const UNCONFIRMED: &str =
        "ChatGPT 本地登录已清除，远端撤销未确认，可在 ChatGPT 设置中断开轻语。";
    let registration = session.registration.as_ref().ok_or(UNCONFIRMED)?;
    let metadata = discovery(client).await.map_err(|_| UNCONFIRMED)?;
    client
        .post(metadata.revocation_endpoint)
        .timeout(HTTP_TIMEOUT)
        .form(&[
            ("token", session.refresh_token.as_str()),
            ("token_type_hint", "refresh_token"),
            ("client_id", registration.client_id.as_str()),
        ])
        .send()
        .await
        .map_err(|_| UNCONFIRMED)?
        .error_for_status()
        .map_err(|_| UNCONFIRMED)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_identity_rejects_wrong_signature_issuer_audience_nonce_subject_and_time() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("chatgpt_sign_in_tokens.json")).unwrap();
        let keys: Jwks = serde_json::from_value(fixture["jwks"].clone()).unwrap();
        let token = fixture["valid"].as_str().unwrap();
        let now = 1_800_000_000;
        let check = |t: &str, client: &str, nonce: &str, subject: &str, time| {
            validate_id_token(t, &keys, client, Some(nonce), Some(subject), time)
        };
        assert!(check(token, "oaiapp_test", "test-nonce", "test-subject", now).is_ok());
        assert!(check(token, "other-client", "test-nonce", "test-subject", now).is_err());
        assert!(check(token, "oaiapp_test", "other-nonce", "test-subject", now).is_err());
        assert!(check(token, "oaiapp_test", "test-nonce", "other-subject", now).is_err());
        assert!(check(
            token,
            "oaiapp_test",
            "test-nonce",
            "test-subject",
            4_102_444_800
        )
        .is_err());
        for name in ["wrongIssuer", "future"] {
            assert!(check(
                fixture[name].as_str().unwrap(),
                "oaiapp_test",
                "test-nonce",
                "test-subject",
                now
            )
            .is_err());
        }
        let mut parts: Vec<String> = token.split('.').map(str::to_string).collect();
        parts[1] = base64_url_encode(br#"{"iss":"https://auth.openai.com","aud":"oaiapp_test","sub":"attacker","exp":4102444800,"nonce":"test-nonce"}"#);
        assert!(check(
            &parts.join("."),
            "oaiapp_test",
            "test-nonce",
            "attacker",
            now
        )
        .is_err());
    }

    #[tokio::test]
    async fn token_http_contract_uses_form_and_does_not_echo_credentials_on_failure() {
        use tokio::io::AsyncReadExt;
        for (status, payload, expected) in [
            (
                "200 OK",
                r#"{"access_token":"new-access","refresh_token":"rotated","scope":"resource.invoke chatgpt.tokens.use.direct","expires_in":3600}"#,
                true,
            ),
            (
                "400 Bad Request",
                r#"{"error":"invalid_grant","refresh_token":"secret-must-not-leak"}"#,
                false,
            ),
            (
                "400 Bad Request",
                r#"{"error":"invalid_refresh_token","refresh_token":"secret-must-not-leak"}"#,
                false,
            ),
            (
                "400 Bad Request",
                r#"{"error":"token_expired","refresh_token":"secret-must-not-leak"}"#,
                false,
            ),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}/token", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                loop {
                    let mut buffer = [0; 1024];
                    let n = socket.read(&mut buffer).await.unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&buffer[..n]);
                    if let Some(offset) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&request[..offset]).to_ascii_lowercase();
                        let length: usize = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length: "))
                            .unwrap()
                            .parse()
                            .unwrap();
                        if request.len() >= offset + 4 + length {
                            break;
                        }
                    }
                }
                let request = String::from_utf8(request).unwrap();
                assert!(request.contains("application/x-www-form-urlencoded"));
                assert!(request.contains("client_id=oaiapp_test"));
                assert!(request.contains("resource=https%3A%2F%2Fapi.openai.com%2Fv1"));
                socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}", payload.len()).as_bytes()).await.unwrap();
            });
            let result = token_request_at(
                &reqwest::Client::new(),
                &endpoint,
                &[("client_id", "oaiapp_test"), ("resource", RESOURCE)],
            )
            .await;
            if expected {
                assert_eq!(result.unwrap().refresh_token.as_deref(), Some("rotated"));
            } else {
                let error = result.unwrap_err();
                assert!(error.starts_with("CHATGPT_REAUTH_REQUIRED:"));
                assert!(!error.contains("secret-must-not-leak"));
            }
            server.await.unwrap();
        }
    }

    #[test]
    fn initial_and_returning_authorization_bind_identity_and_resource() {
        let registry = Registry {
            host_id: "urn:uuid:fixed-host".into(),
            accounts: vec![],
            ..Default::default()
        };
        let account = ChatgptRegistration {
            client_id: "oaiapp_saved".into(),
            subject: "subject".into(),
            host_id: registry.host_id.clone(),
            scopes: vec![],
            email: None,
        };
        let target = LoginTarget {
            client_id: &account.client_id,
            registration: Some(&account),
        };
        for previous in [None, Some(&target)] {
            let url = reqwest::Url::parse(
                &authorize_url(
                    "http://127.0.0.1:54321/auth/callback",
                    "challenge",
                    "state",
                    "nonce",
                    &registry,
                    previous,
                )
                .unwrap(),
            )
            .unwrap();
            let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
            assert_eq!(
                query["client_id"],
                previous.map_or("dynamic_agent_client", |r| r.client_id)
            );
            assert_eq!(query["resource"], RESOURCE);
            assert_eq!(query["nonce"], "nonce");
            assert_eq!(query["ext_agent_host_id"], registry.host_id);
            assert_eq!(query.contains_key("agent_name_hint"), previous.is_none());
            assert_eq!(
                query["redirect_uri"],
                "http://127.0.0.1:54321/auth/callback"
            );
        }
    }

    #[test]
    fn returning_sign_in_reuses_latest_registration_unless_add_is_explicit() {
        let account = |id: &str| ChatgptRegistration {
            client_id: id.into(),
            subject: "subject".into(),
            host_id: "host".into(),
            scopes: vec![],
            email: Some("same@example.invalid".into()),
        };
        let registry = Registry {
            host_id: "host".into(),
            accounts: vec![account("first"), account("second")],
            ..Default::default()
        };
        assert_eq!(
            select_account(&registry, None, false)
                .unwrap()
                .unwrap()
                .client_id,
            "second"
        );
        assert_eq!(
            select_account(&registry, Some("first"), false)
                .unwrap()
                .unwrap()
                .client_id,
            "first"
        );
        assert!(select_account(&registry, None, true).unwrap().is_none());
        assert!(select_account(&registry, Some("missing"), false).is_err());
        assert!(select_account(&registry, Some("first"), true).is_err());
        let empty = Registry {
            host_id: "host".into(),
            accounts: vec![],
            ..Default::default()
        };
        assert!(select_account(&empty, None, false).unwrap().is_none());
    }

    #[test]
    fn registration_removal_persists_only_the_chosen_id_and_preserves_host() {
        let path = std::env::temp_dir().join(format!("chatgpt-registry-{}.json", generate_state()));
        let account = |id: &str| ChatgptRegistration {
            client_id: id.into(),
            subject: "subject".into(),
            host_id: "host".into(),
            scopes: vec![],
            email: Some("same@example.invalid".into()),
        };
        let registry = Registry {
            host_id: "host".into(),
            accounts: vec![account("first"), account("second")],
            ..Default::default()
        };
        write_registry_at(&path, &registry).unwrap();
        forget_account_at(&path, "first").unwrap();
        let remaining = read_registry_at(&path).unwrap();
        assert_eq!(remaining.host_id, "host");
        assert_eq!(remaining.accounts.len(), 1);
        assert_eq!(remaining.accounts[0].client_id, "second");
        forget_account_at(&path, "first").unwrap(); // Retry is idempotent.
        forget_account_at(&path, "second").unwrap();
        let empty = read_registry_at(&path).unwrap();
        assert!(empty.accounts.is_empty());
        assert_eq!(empty.host_id, "host");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn issued_registration_survives_failure_restart_and_promotes_without_duplication() {
        let path = std::env::temp_dir().join(format!("chatgpt-pending-{}.json", generate_state()));
        read_registry_at(&path).unwrap();
        remember_pending_at(&path, "issued").unwrap();
        let registry = read_registry_at(&path).unwrap();
        let target = select_account(&registry, None, false).unwrap().unwrap();
        assert_eq!(target.client_id, "issued");
        assert!(target.registration.is_none());
        let retry_url = reqwest::Url::parse(
            &authorize_url(
                "http://127.0.0.1:54321/auth/callback",
                "challenge",
                "state",
                "nonce",
                &registry,
                Some(&target),
            )
            .unwrap(),
        )
        .unwrap();
        let retry_query: std::collections::HashMap<_, _> =
            retry_url.query_pairs().into_owned().collect();
        assert_eq!(retry_query["client_id"], "issued");
        assert!(!retry_query.contains_key("agent_name_hint"));
        let summaries = account_summaries(registry);
        assert_eq!(summaries.len(), 1);
        assert!(summaries[0].pending);
        assert!(summaries[0].email.is_none());
        let account = ChatgptRegistration {
            client_id: "issued".into(),
            subject: "verified-subject".into(),
            host_id: read_registry_at(&path).unwrap().host_id,
            scopes: vec![],
            email: None,
        };
        remember_account_at(&path, &account).unwrap();
        remember_account_at(&path, &account).unwrap();
        let registry = read_registry_at(&path).unwrap();
        assert!(registry.pending_client_ids.is_empty());
        assert_eq!(registry.accounts.len(), 1);
        assert!(select_account(&registry, None, false)
            .unwrap()
            .unwrap()
            .registration
            .is_some());
        // Failed additions remain separate, but signing in to an existing
        // registration again makes that registration the returning default.
        remember_pending_at(&path, "unfinished-two").unwrap();
        remember_pending_at(&path, "unfinished-three").unwrap();
        assert_eq!(
            account_summaries(read_registry_at(&path).unwrap())
                .last()
                .unwrap()
                .client_id,
            "unfinished-three"
        );
        remember_account_at(&path, &account).unwrap();
        let registry = read_registry_at(&path).unwrap();
        assert_eq!(
            select_account(&registry, None, false)
                .unwrap()
                .unwrap()
                .client_id,
            "issued"
        );
        assert_eq!(
            account_summaries(registry).last().unwrap().client_id,
            "issued"
        );
        forget_account_at(&path, "unfinished-two").unwrap();
        forget_account_at(&path, "unfinished-three").unwrap();
        forget_account_at(&path, "issued").unwrap();
        assert!(account_summaries(read_registry_at(&path).unwrap()).is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn discovery_failure_reports_local_sign_out_and_unconfirmed_revocation() {
        use tokio::io::AsyncReadExt;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0u8; 2048];
            assert!(socket.read(&mut buffer).await.unwrap() > 0);
            socket
                .write_all(
                    b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
        });
        let client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(proxy).unwrap())
            .build()
            .unwrap();
        let session = OpenaiCodexOauthSession {
            registration: Some(ChatgptRegistration {
                client_id: "fixture".into(),
                subject: "fixture".into(),
                host_id: "host".into(),
                scopes: vec![],
                email: None,
            }),
            ..Default::default()
        };
        let error = revoke(&client, &session).await.unwrap_err();
        assert!(error.contains("本地"));
        assert!(error.contains("远端撤销未确认"));
        server.await.unwrap();
    }

    #[test]
    fn granted_scopes_do_not_assume_requested_permissions() {
        let registration = ChatgptRegistration {
            client_id: "oaiapp_test".into(),
            subject: "subject".into(),
            host_id: "host".into(),
            scopes: vec!["resource.invoke".into(), "chatgpt.tokens.use.direct".into()],
            email: None,
        };
        let tokens: TokenResponse = serde_json::from_value(serde_json::json!({"access_token":"token","refresh_token":"refresh","scope":"openid email","expires_in":3600})).unwrap();
        let session = session_from_tokens(tokens, registration).unwrap();
        assert!(!session.registration.unwrap().plan_usage_enabled());
    }
}
