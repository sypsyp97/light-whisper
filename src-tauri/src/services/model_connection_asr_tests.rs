use super::{probe_online_asr, TEST_AUDIO};
use base64::Engine;
use serde_json::Value;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

const KEY: &str = "asr-probe-key-sentinel";
const PRIVATE_BODY: &str = "private-body-sentinel";
const OMNI_OK: &str =
    "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\ndata: [DONE]\n\n";

struct Request {
    line: String,
    headers: String,
    body: Vec<u8>,
}

fn header<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    headers.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

fn position(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes.windows(needle.len()).position(|part| part == needle)
}

async fn server(status: &str, response: &str, sse: bool) -> (String, JoinHandle<Request>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let content_type = if sse {
        "text/event-stream"
    } else {
        "application/json"
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
        response.len()
    );
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let mut chunk = [0; 4096];
        let header_end = loop {
            let read = socket.read(&mut chunk).await.unwrap();
            assert!(read > 0, "request ended before headers");
            bytes.extend_from_slice(&chunk[..read]);
            if let Some(offset) = position(&bytes, b"\r\n\r\n") {
                break offset + 4;
            }
        };
        let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
        let length: usize = header(&headers, "content-length")
            .expect("fixture requires a bounded request body")
            .parse()
            .unwrap();
        while bytes.len() < header_end + length {
            let read = socket.read(&mut chunk).await.unwrap();
            assert!(read > 0, "request ended before body");
            bytes.extend_from_slice(&chunk[..read]);
        }
        socket.write_all(response.as_bytes()).await.unwrap();
        Request {
            line: headers.lines().next().unwrap().into(),
            headers,
            body: bytes[header_end..header_end + length].to_vec(),
        }
    });
    (origin, task)
}

async fn captured(mut task: JoinHandle<Request>) -> Request {
    match tokio::time::timeout(Duration::from_secs(1), &mut task).await {
        Ok(result) => result.expect("local HTTP fixture must finish"),
        Err(_) => {
            task.abort();
            panic!("ASR probe did not send an inference request");
        }
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

fn multipart_field<'a>(request: &'a Request, name: &str) -> &'a [u8] {
    let boundary = header(&request.headers, "content-type")
        .unwrap()
        .split("boundary=")
        .nth(1)
        .unwrap();
    let field = format!("name=\"{name}\"");
    let field_start = position(&request.body, field.as_bytes()).expect("multipart field missing");
    let content_start =
        field_start + position(&request.body[field_start..], b"\r\n\r\n").unwrap() + 4;
    let ending = format!("\r\n--{boundary}");
    let length = position(&request.body[content_start..], ending.as_bytes()).unwrap();
    &request.body[content_start..content_start + length]
}

fn sanitized(result: Result<(), String>) -> String {
    let error = result.expect_err("unusable transcription must fail the connection probe");
    assert!(!error.trim().is_empty());
    for secret in [KEY, PRIVATE_BODY] {
        assert!(
            !error.contains(secret),
            "probe error exposed private content"
        );
    }
    error
}

#[tokio::test]
async fn glm_posts_complete_synthetic_speech_as_multipart_with_fixed_model() {
    let audio = hound::WavReader::new(std::io::Cursor::new(TEST_AUDIO)).unwrap();
    assert_eq!(
        (
            audio.spec().channels,
            audio.spec().sample_rate,
            audio.spec().bits_per_sample
        ),
        (1, 16_000, 16)
    );
    assert_eq!(audio.spec().sample_format, hound::SampleFormat::Int);
    assert_eq!(audio.duration(), 61_040);
    let (origin, task) = server(
        "200 OK",
        r#"{"text":"Hello. This is a model connection test."}"#,
        false,
    )
    .await;
    assert_eq!(
        probe_online_asr(&client(), "glm-asr", &origin, "ignored-draft-model", KEY).await,
        Ok(())
    );
    let request = captured(task).await;
    assert_eq!(
        request.line,
        "POST /api/paas/v4/audio/transcriptions HTTP/1.1"
    );
    assert_eq!(
        header(&request.headers, "authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    assert!(header(&request.headers, "content-type")
        .unwrap()
        .starts_with("multipart/form-data;"));
    assert_eq!(multipart_field(&request, "model"), b"glm-asr-2512");
    assert_eq!(multipart_field(&request, "file"), TEST_AUDIO);
    assert!(
        position(&request.body, b"hotwords").is_none(),
        "probe must not use private vocabulary"
    );
}

#[tokio::test]
async fn alibaba_posts_current_model_auth_and_complete_speech_in_each_supported_format() {
    for (model, path, audio_pointer, response, sse) in [
        (
            "qwen3-asr-flash",
            "/api/v1/services/aigc/multimodal-generation/generation",
            "/input/messages/1/content/0/audio",
            r#"{"output":{"text":"Hello"}}"#,
            false,
        ),
        (
            "qwen-audio-3.5-asr-flash",
            "/api/v1/services/aigc/multimodal-generation/generation",
            "/input/messages/0/content/0/input_audio/data",
            r#"{"output":{"output":{"sentence":{"text":"Hello"}}}}"#,
            false,
        ),
        (
            "qwen3-omni-flash",
            "/compatible-mode/v1/chat/completions",
            "/messages/1/content/0/input_audio/data",
            OMNI_OK,
            true,
        ),
    ] {
        let (origin, task) = server("200 OK", response, sse).await;
        assert_eq!(
            probe_online_asr(&client(), "alibaba-asr", &origin, model, KEY).await,
            Ok(()),
            "{model}"
        );
        let request = captured(task).await;
        assert_eq!(request.line, format!("POST {path} HTTP/1.1"));
        assert_eq!(
            header(&request.headers, "authorization"),
            Some(format!("Bearer {KEY}").as_str())
        );
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["model"], model);
        let audio = body
            .pointer(audio_pointer)
            .and_then(Value::as_str)
            .expect("request must contain audio");
        let (prefix, encoded) = audio.split_once("base64,").expect("audio must be embedded");
        assert!(prefix.starts_with("data:"));
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap(),
            TEST_AUDIO
        );
    }
}

#[tokio::test]
async fn online_asr_rejects_empty_failed_malformed_and_incomplete_transcriptions() {
    for (engine, model, response, sse) in [
        ("glm-asr", "glm-asr-2512", r#"{"text":"   "}"#, false),
        ("glm-asr", "glm-asr-2512", r#"{"code":1001,"message":"private-body-sentinel","text":"not a valid success"}"#, false),
        ("glm-asr", "glm-asr-2512", "private-body-sentinel", false),
        ("alibaba-asr", "qwen3-asr-flash", r#"{"output":{"text":""}}"#, false),
        ("alibaba-asr", "qwen3-asr-flash", r#"{"code":"InvalidApiKey","message":"private-body-sentinel","output":{"text":"not a valid success"}}"#, false),
        ("alibaba-asr", "qwen3-asr-flash", "{}", false),
        ("alibaba-asr", "qwen3-asr-flash", r#"{"output":{"choices":[{"finish_reason":"length","message":{"content":[{"text":"partial private-body-sentinel"}]}}]}}"#, false),
        ("alibaba-asr", "qwen3-asr-flash", r#"{"output":{"choices":[{"finish_reason":"content_filter","message":{"content":[{"text":"partial private-body-sentinel"}]}}]}}"#, false),
        ("alibaba-asr", "qwen3-omni-flash", "data: [DONE]\n\n", true),
        ("alibaba-asr", "qwen3-omni-flash", "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n", true),
        ("alibaba-asr", "qwen3-omni-flash", "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\ndata: {\"code\":\"InvalidApiKey\",\"message\":\"private-body-sentinel\"}\n\ndata: [DONE]\n\n", true),
        ("alibaba-asr", "qwen3-omni-flash", "data: {broken-json\n\ndata: [DONE]\n\n", true),
    ] {
        let (origin, task) = server("200 OK", response, sse).await;
        let result = probe_online_asr(&client(), engine, &origin, model, KEY).await;
        captured(task).await;
        sanitized(result);
    }
}

#[tokio::test]
async fn online_asr_http_errors_keep_status_without_credentials_or_body() {
    for (engine, model) in [
        ("glm-asr", "glm-asr-2512"),
        ("alibaba-asr", "qwen3-asr-flash"),
        ("alibaba-asr", "qwen3-omni-flash"),
    ] {
        for (status, code) in [
            ("401 Unauthorized", "401"),
            ("503 Service Unavailable", "503"),
        ] {
            let (origin, task) = server(
                status,
                r#"{"error":"private-body-sentinel asr-probe-key-sentinel"}"#,
                false,
            )
            .await;
            let error = sanitized(probe_online_asr(&client(), engine, &origin, model, KEY).await);
            assert!(
                error.contains(code),
                "HTTP status must remain visible: {error}"
            );
            captured(task).await;
        }
    }
}
