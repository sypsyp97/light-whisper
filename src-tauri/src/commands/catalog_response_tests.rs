use super::{fetch_catalog_response, CatalogResponse};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

const CATALOG: &str = r#"{"models":[{"slug":"fixture-model"}]}"#;
const PREFIX: &str = r#"{"models":["private-fixture-marker","#;

#[derive(Clone, Copy)]
enum Reply {
    Json(&'static str),
    ShortBody,
    Stall,
    Unauthorized,
}

async fn fetch(replies: &[Reply], retry: bool) -> (Result<CatalogResponse, String>, Vec<String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let url = format!(
        "http://{}/models?version=fixture",
        listener.local_addr().unwrap()
    );
    let replies = replies.to_vec();
    let (stop, mut stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        let mut stalled = Vec::new();
        loop {
            let socket = tokio::select! {
                result = listener.accept() => result.unwrap().0,
                _ = &mut stopped => return requests,
            };
            let mut client = BufReader::new(socket);
            let mut headers = String::new();
            loop {
                let mut line = String::new();
                let count = client.read_line(&mut line).await.unwrap();
                assert_ne!(count, 0);
                if line == "\r\n" {
                    break;
                }
                headers.push_str(&line);
            }
            let reply = replies
                .get(requests.len())
                .copied()
                .expect("unexpected retry");
            requests.push(headers);
            let (status, body, length) = match reply {
                Reply::Json(body) => ("200 OK", body, body.len()),
                Reply::ShortBody | Reply::Stall => ("200 OK", PREFIX, CATALOG.len() + 100),
                Reply::Unauthorized => ("401 Unauthorized", "denied", 6),
            };
            let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n{body}");
            client
                .get_mut()
                .write_all(response.as_bytes())
                .await
                .unwrap();
            if matches!(reply, Reply::Stall) {
                // Hold the connection open until the request deadline expires.
                stalled.push(client);
            }
        }
    });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let request = client
        .get(url)
        .bearer_auth("fixture")
        .timeout(Duration::from_secs(2));
    let result = fetch_catalog_response(request, retry)
        .await
        .map_err(|err| err.to_string());
    let _ = stop.send(());
    (result, server.await.unwrap())
}

fn error(result: Result<CatalogResponse, String>) -> String {
    let message = result.expect_err("expected a catalog failure");
    assert!(!message.contains("private-fixture-marker"));
    message
}

#[tokio::test]
async fn catalog_response_retries_short_transport_and_complete_json_eof_once() {
    for first in [Reply::ShortBody, Reply::Json(PREFIX), Reply::Stall] {
        let (result, requests) = fetch(&[first, Reply::Json(CATALOG)], true).await;
        let CatalogResponse::Payload(value) = result.unwrap() else {
            panic!("expected payload")
        };
        assert_eq!(value["models"][0]["slug"], "fixture-model");
        assert_eq!(requests.len(), 2);
        for headers in requests {
            assert!(headers.starts_with("GET /models?version=fixture HTTP/1.1\r\n"));
            assert!(headers
                .to_ascii_lowercase()
                .contains("\r\nauthorization: bearer fixture\r\n"));
        }
    }
}

#[tokio::test]
async fn catalog_response_bounds_retries_and_respects_provider_policy() {
    for (retry, count) in [(true, 2), (false, 1)] {
        for reply in [Reply::ShortBody, Reply::Json(PREFIX)] {
            let (result, requests) = fetch(&vec![reply; count], retry).await;
            error(result);
            assert_eq!(requests.len(), count);
        }
    }
}

#[tokio::test]
async fn catalog_response_leaves_schema_validation_to_parser() {
    let (result, requests) = fetch(&[Reply::Json(r#"{"other":true}"#)], true).await;
    let CatalogResponse::Payload(value) = result.unwrap() else {
        panic!("expected payload")
    };
    assert_eq!(value, serde_json::json!({"other": true}));
    assert_eq!(requests.len(), 1);
}

#[tokio::test]
async fn catalog_response_preserves_auth_response_after_incomplete_body() {
    for replies in [
        &[Reply::Unauthorized][..],
        &[Reply::ShortBody, Reply::Unauthorized],
    ] {
        let (result, requests) = fetch(replies, true).await;
        let CatalogResponse::Http(response) = result.unwrap() else {
            panic!("expected HTTP response")
        };
        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
        assert_eq!(response.text().await.unwrap(), "denied");
        assert_eq!(requests.len(), replies.len());
    }
}

#[tokio::test]
async fn catalog_response_reports_body_timeout_separately_from_invalid_json() {
    let (timed_out, requests) = fetch(&[Reply::Stall], false).await;
    let timeout = error(timed_out);
    assert!(timeout.contains("超时") || timeout.to_ascii_lowercase().contains("time"));
    assert_eq!(requests.len(), 1);
    for invalid in [
        "<html>private-fixture-marker",
        r#"{"private-fixture-marker":!}"#,
    ] {
        let (malformed, requests) = fetch(&[Reply::Json(invalid)], true).await;
        let syntax = error(malformed);
        assert_ne!(syntax, timeout);
        assert_eq!(requests.len(), 1);
    }
}
