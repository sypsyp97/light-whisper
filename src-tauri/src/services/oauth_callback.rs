use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::net::TcpStream;

// A loopback client gets two seconds and 8 KiB to send one HTTP request line.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_REQUEST_LINE_BYTES: usize = 8 * 1024;

pub(super) async fn read_callback_url(stream: &mut TcpStream) -> Option<reqwest::Url> {
    let mut line = Vec::new();
    let mut reader = BufReader::new(stream).take((MAX_REQUEST_LINE_BYTES + 1) as u64);
    let count = tokio::time::timeout(REQUEST_TIMEOUT, reader.read_until(b'\n', &mut line))
        .await
        .ok()?
        .ok()?;
    if count > MAX_REQUEST_LINE_BYTES || !line.ends_with(b"\n") {
        return None;
    }
    let mut parts = std::str::from_utf8(&line).ok()?.split_whitespace();
    if parts.next()? != "GET" {
        return None;
    }
    let path = parts.next()?;
    if !path.starts_with('/') || path.starts_with("//") {
        return None;
    }
    if !matches!(parts.next()?, "HTTP/1.0" | "HTTP/1.1") || parts.next().is_some() {
        return None;
    }
    reqwest::Url::parse(&format!("http://localhost{path}")).ok()
}
