use serde::{Deserialize, Serialize};

use crate::utils::AppError;

const GITHUB_RELEASE_API: &str =
    "https://api.github.com/repos/sypsyp97/light-whisper/releases/latest";
const GITHUB_RELEASES_URL: &str = "https://github.com/sypsyp97/light-whisper/releases";
const UPDATER_USER_AGENT: &str = concat!("light-whisper/", env!("CARGO_PKG_VERSION"));
const UPDATER_REQUEST_TIMEOUT_SECS: u64 = 15;
/// 限制 `open_app_release_page` 可打开的 host 白名单。命令是从前端传 URL
/// 进来的，不加白名单等于把"打开任意外部 URL"的能力暴露给 IPC。
const ALLOWED_RELEASE_HOSTS: &[&str] = &["github.com", "www.github.com"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInfo {
    pub available: bool,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub notes: Option<String>,
    pub published_at: Option<String>,
    pub release_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    body: Option<String>,
    published_at: Option<String>,
    html_url: String,
}

#[tauri::command]
pub async fn check_app_update() -> Result<AppUpdateInfo, AppError> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let release = fetch_latest_release().await?;
    let latest_version = normalize_version(&release.tag_name);
    let available = is_version_newer(&latest_version, &current_version);

    Ok(AppUpdateInfo {
        available,
        current_version,
        latest_version: Some(latest_version),
        notes: if available {
            release.body.filter(|body| !body.trim().is_empty())
        } else {
            None
        },
        published_at: release.published_at,
        release_url: Some(release.html_url),
    })
}

#[tauri::command]
pub async fn open_app_release_page(url: Option<String>) -> Result<String, AppError> {
    let target = url
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|value| validate_release_url(&value).map(|_| value))
        .transpose()?
        .unwrap_or_else(|| GITHUB_RELEASES_URL.to_string());
    open_external_url(&target)?;
    Ok("已打开 GitHub Release 页面".to_string())
}

/// 仅允许 https + GitHub host 通过。前端理应只传 `check_app_update` 返回的
/// `release_url`，但既然命令暴露在 IPC，就在后端把这条假设固化下来。
fn validate_release_url(value: &str) -> Result<(), AppError> {
    let parsed = reqwest::Url::parse(value)
        .map_err(|err| AppError::Other(format!("非法 Release URL: {}", err)))?;
    if parsed.scheme() != "https" {
        return Err(AppError::Other(format!(
            "Release URL 仅支持 https，收到 scheme: {}",
            parsed.scheme()
        )));
    }
    let host = parsed.host_str().unwrap_or("");
    if !ALLOWED_RELEASE_HOSTS.contains(&host) {
        return Err(AppError::Other(format!(
            "Release URL host 不在白名单内: {}",
            host
        )));
    }
    Ok(())
}

async fn fetch_latest_release() -> Result<GitHubRelease, AppError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(UPDATER_REQUEST_TIMEOUT_SECS))
        .build()
        .map_err(|err| AppError::Download(format!("构建 HTTP 客户端失败: {}", err)))?;
    let response = client
        .get(GITHUB_RELEASE_API)
        .header(reqwest::header::USER_AGENT, UPDATER_USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|err| AppError::Download(format!("请求 GitHub Release 失败: {}", err)))?;

    if !response.status().is_success() {
        return Err(AppError::Download(format!(
            "GitHub Release 检查失败: HTTP {}",
            response.status()
        )));
    }

    response
        .json::<GitHubRelease>()
        .await
        .map_err(|err| AppError::Download(format!("解析 GitHub Release 数据失败: {}", err)))
}

fn normalize_version(version: &str) -> String {
    version.trim().trim_start_matches('v').to_string()
}

fn parse_version(version: &str) -> Vec<u64> {
    normalize_version(version)
        .split('.')
        .map(|part| {
            let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse::<u64>().unwrap_or(0)
        })
        .collect()
}

fn is_version_newer(latest: &str, current: &str) -> bool {
    let latest_parts = parse_version(latest);
    let current_parts = parse_version(current);
    let max_len = latest_parts.len().max(current_parts.len());

    for index in 0..max_len {
        let latest_part = latest_parts.get(index).copied().unwrap_or(0);
        let current_part = current_parts.get(index).copied().unwrap_or(0);
        if latest_part > current_part {
            return true;
        }
        if latest_part < current_part {
            return false;
        }
    }

    false
}

#[cfg(target_os = "windows")]
fn open_external_url(url: &str) -> Result<(), AppError> {
    std::process::Command::new("rundll32")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn()
        .map_err(|err| AppError::Other(format!("打开下载页面失败: {}", err)))?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn open_external_url(url: &str) -> Result<(), AppError> {
    std::process::Command::new("open")
        .arg(url)
        .spawn()
        .map_err(|err| AppError::Other(format!("打开下载页面失败: {}", err)))?;
    Ok(())
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn open_external_url(url: &str) -> Result<(), AppError> {
    std::process::Command::new("xdg-open")
        .arg(url)
        .spawn()
        .map_err(|err| AppError::Other(format!("打开下载页面失败: {}", err)))?;
    Ok(())
}
