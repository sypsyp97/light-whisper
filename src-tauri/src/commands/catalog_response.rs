use reqwest::{RequestBuilder, Response};
use serde_json::Value;

#[derive(Debug)]
pub(super) enum CatalogResponse {
    Payload(Value),
    Http(Response),
}

#[derive(Debug, thiserror::Error)]
pub(super) enum CatalogFetchError {
    #[error("拉取模型列表失败: {0}")]
    Send(reqwest::Error),
    #[error("读取模型列表响应超时")]
    Timeout,
    #[error("模型列表响应传输中断")]
    Interrupted,
    #[error("模型列表 JSON 响应不完整")]
    Truncated,
    #[error("模型列表响应不是有效的 JSON（第 {line} 行，第 {column} 列）")]
    Invalid { line: usize, column: usize },
}

async fn read_catalog(request: RequestBuilder) -> Result<CatalogResponse, CatalogFetchError> {
    let response = request.send().await.map_err(CatalogFetchError::Send)?;
    if !response.status().is_success() {
        return Ok(CatalogResponse::Http(response));
    }
    let bytes = response.bytes().await.map_err(|error| {
        if error.is_timeout() {
            CatalogFetchError::Timeout
        } else {
            CatalogFetchError::Interrupted
        }
    })?;
    serde_json::from_slice(&bytes)
        .map(CatalogResponse::Payload)
        .map_err(|error| {
            if error.is_eof() {
                CatalogFetchError::Truncated
            } else {
                CatalogFetchError::Invalid {
                    line: error.line(),
                    column: error.column(),
                }
            }
        })
}

pub(super) async fn fetch_catalog_response(
    request: RequestBuilder,
    retry_incomplete: bool,
) -> Result<CatalogResponse, CatalogFetchError> {
    let retry = if retry_incomplete {
        request.try_clone()
    } else {
        None
    };
    let result = read_catalog(request).await;
    if matches!(
        result,
        Err(CatalogFetchError::Timeout
            | CatalogFetchError::Interrupted
            | CatalogFetchError::Truncated)
    ) {
        if let Some(retry) = retry {
            return read_catalog(retry).await;
        }
    }
    result
}

#[cfg(test)]
#[path = "catalog_response_tests.rs"]
mod tests;
