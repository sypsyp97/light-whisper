use super::{discovery, discovery_at, Discovery, ISSUER};
use crate::services::jev_error_visibility_tests::{capture_warning, only_warning, response_server};
use reqwest::{dns, Client};
use std::{sync::Arc, time::Duration};

use tokio::net::TcpListener;

fn transport_warning(
    result: Result<Discovery, String>,
    records: &[(log::Level, String)],
    reason: &str,
) {
    assert_eq!(result.err().as_deref(), Some("获取 ChatGPT 授权元数据失败"));
    let warning = only_warning(records, "chatgpt_discovery");
    assert!(
        warning
            .split_whitespace()
            .any(|part| part == format!("reason={reason}")),
        "{warning}"
    );
}

struct FailingDns;
impl dns::Resolve for FailingDns {
    fn resolve(&self, _: dns::Name) -> dns::Resolving {
        Box::pin(async {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "private-query-sentinel").into())
        })
    }
}

struct PendingDns;
impl dns::Resolve for PendingDns {
    fn resolve(&self, _: dns::Name) -> dns::Resolving {
        Box::pin(std::future::pending())
    }
}

#[tokio::test]
async fn discovery_diagnostic_transport_errors_reach_the_fixed_url_owner() {
    let client = Client::builder()
        .no_proxy()
        .dns_resolver(Arc::new(FailingDns))
        .build()
        .unwrap();
    let (result, records) = capture_warning(discovery(&client)).await;
    transport_warning(result, &records, "dns");

    let client = Client::builder()
        .no_proxy()
        .dns_resolver(Arc::new(PendingDns))
        .connect_timeout(Duration::from_millis(30))
        .build()
        .unwrap();
    let (result, records) = capture_warning(discovery(&client)).await;
    transport_warning(result, &records, "timeout");

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let client = Client::builder().no_proxy().build().unwrap();
    let (result, records) = capture_warning(discovery_at(
        &client,
        &format!("http://{address}/private-path-sentinel?q=private-query-sentinel"),
    ))
    .await;
    transport_warning(result, &records, "connection_refused");
}

#[tokio::test]
async fn discovery_diagnostic_protocol_and_tls_failures() {
    let client = Client::builder().no_proxy().build().unwrap();
    for (secure, reason) in [(false, "transport"), (true, "tls")] {
        let (url, server) = response_server(b"PRIVATE-BAD-RESPONSE\r\n\r\n".to_vec()).await;
        let url = if secure {
            url.replacen("http:", "https:", 1)
        } else {
            url
        };
        let (result, records) = capture_warning(discovery_at(&client, &url)).await;
        server.await.unwrap();
        transport_warning(result, &records, reason);
    }
}

#[tokio::test]
async fn discovery_diagnostic_status_metadata_and_success() {
    let client = Client::builder().no_proxy().build().unwrap();
    let (url, server) = response_server(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\nprivate-body-sentinel".to_vec()).await;
    let (result, records) = capture_warning(discovery_at(&client, &url)).await;
    server.await.unwrap();
    assert_eq!(result.err().as_deref(), Some("ChatGPT 授权元数据格式无效"));
    assert!(only_warning(&records, "chatgpt_discovery").contains("reason=response_read"));
    let valid = r#"{"issuer":"https://auth.openai.com","jwks_uri":"https://auth.openai.com/jwks","revocation_endpoint":"https://auth.openai.com/revoke"}"#;
    let untrusted_jwks = valid.replace("/jwks", ".private-body-sentinel.invalid/jwks");
    let untrusted_issuer = valid.replace(
        &format!("{ISSUER}\""),
        "https://private-body-sentinel.invalid\"",
    );
    let untrusted_revocation = valid.replace("/revoke", ".private-body-sentinel.invalid/revoke");
    for (status, body, error, reason) in [
        (
            "503 Service Unavailable",
            "private-body-sentinel",
            Some("ChatGPT 授权元数据不可用"),
            "http_status",
        ),
        (
            "200 OK",
            "private-body-sentinel",
            Some("ChatGPT 授权元数据格式无效"),
            "invalid_metadata",
        ),
        (
            "200 OK",
            &untrusted_jwks,
            Some("ChatGPT 授权元数据来源无效"),
            "untrusted_metadata",
        ),
        (
            "200 OK",
            &untrusted_issuer,
            Some("ChatGPT 授权元数据来源无效"),
            "untrusted_metadata",
        ),
        (
            "200 OK",
            &untrusted_revocation,
            Some("ChatGPT 授权元数据来源无效"),
            "untrusted_metadata",
        ),
        ("200 OK", valid, None, ""),
    ] {
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let (url, server) = response_server(response.into_bytes()).await;
        let (result, records) = capture_warning(discovery_at(&client, &url)).await;
        server.await.unwrap();
        if let Some(error) = error {
            assert_eq!(result.err().as_deref(), Some(error));
            let warning = only_warning(&records, "chatgpt_discovery");
            assert!(
                warning.contains(&format!("reason={reason}"))
                    || (reason == "invalid_metadata" && warning.contains("reason=invalid_json")),
                "{warning}"
            );
            if reason == "http_status" {
                assert!(warning.contains("503"), "{warning}");
            }
        } else {
            assert_eq!(result.unwrap().issuer, ISSUER);
            assert!(records.is_empty(), "{records:?}");
        }
    }
}

#[tokio::test]
async fn discovery_diagnostic_rejected_proxy_connection() {
    let (proxy_url, server) =
        response_server(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n".to_vec()).await;
    let mut proxy_url = reqwest::Url::parse(&proxy_url).unwrap();
    proxy_url.set_path("/");
    proxy_url.set_query(None);
    let client = Client::builder()
        .proxy(reqwest::Proxy::all(proxy_url).unwrap())
        .build()
        .unwrap();
    let (result, records) = capture_warning(discovery(&client)).await;
    server.await.unwrap();
    transport_warning(result, &records, "connect");
}
#[tokio::test]
async fn discovery_diagnostic_tls_handshake_eof() {
    use tokio::io::AsyncReadExt;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "https://{}/private-path-sentinel",
        listener.local_addr().unwrap()
    );
    let peer = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut header = [0u8; 5];
        socket.read_exact(&mut header).await.unwrap();
        assert_eq!(header[0], 22, "expected a TLS handshake record");
        let mut handshake = vec![0; u16::from_be_bytes([header[3], header[4]]) as usize];
        socket.read_exact(&mut handshake).await.unwrap();
        assert_eq!(
            handshake.first(),
            Some(&1),
            "expected ClientHello before EOF"
        );
    });
    let client = Client::builder().no_proxy().build().unwrap();
    let (result, records) = capture_warning(discovery_at(&client, &endpoint)).await;
    peer.await.unwrap();
    transport_warning(result, &records, "tls");
}

#[tokio::test]
async fn discovery_diagnostic_stalled_response_body() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "http://{}/private-path-sentinel",
        listener.local_addr().unwrap()
    );
    let (release, wait_for_timeout) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 2048];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n{")
            .await
            .unwrap();
        let _ = wait_for_timeout.await;
    });
    let client = Client::builder()
        .no_proxy()
        .read_timeout(Duration::from_millis(500))
        .build()
        .unwrap();
    let (result, records) = capture_warning(discovery_at(&client, &endpoint)).await;
    release.send(()).unwrap();
    server.await.unwrap();
    assert_eq!(result.err().as_deref(), Some("ChatGPT 授权元数据格式无效"));
    let message = only_warning(&records, "chatgpt_discovery");
    assert!(
        message
            .split_whitespace()
            .any(|field| field == "stage=response_body"),
        "{message}"
    );
    assert!(
        message
            .split_whitespace()
            .any(|field| field == "reason=timeout"),
        "{message}"
    );
}
