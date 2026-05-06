use std::time::Duration;

use serde::Deserialize;

/// 单条搜索结果
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub content: String,
}

const SEARCH_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_SEARCH_CONTEXT_RESULTS: usize = 5;
const MAX_SEARCH_CONTEXT_BYTES: usize = 10_000;
const MAX_SEARCH_RESULT_CONTENT_BYTES: usize = 1_600;

// ── Exa MCP（免费，无需 Key）────────────────────────────────────────

/// JSON-RPC 2.0 响应
#[derive(Deserialize)]
struct JsonRpcResponse {
    result: Option<McpResult>,
}

#[derive(Deserialize)]
struct McpResult {
    #[serde(default)]
    content: Vec<McpContent>,
}

#[derive(Deserialize)]
struct McpContent {
    #[serde(default)]
    text: String,
}

pub async fn exa_search(
    http_client: &reqwest::Client,
    query: &str,
    max_results: u8,
) -> Result<Vec<SearchResult>, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "web_search_exa",
            "arguments": {
                "query": query,
                "numResults": max_results,
                "type": "auto",
            }
        }
    });

    let resp = http_client
        .post("https://mcp.exa.ai/mcp")
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .timeout(SEARCH_TIMEOUT)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Exa 搜索请求失败: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!(
            "Exa MCP 返回 HTTP {status}: {}",
            truncate_str(&text, 200)
        ));
    }

    // 响应可能是 SSE（text/event-stream）或纯 JSON
    let raw = resp
        .text()
        .await
        .map_err(|e| format!("Exa 响应读取失败: {e}"))?;

    let json_str = extract_final_json_data_line(&raw).unwrap_or_else(|| raw.trim());

    let rpc: JsonRpcResponse =
        serde_json::from_str(json_str).map_err(|e| format!("Exa 响应解析失败: {e}"))?;

    let content_blocks = rpc.result.map(|r| r.content).unwrap_or_default();

    // Exa MCP 返回的 text 是带标签的纯文本块，多条结果用空行分隔
    let mut results = Vec::new();
    for block in &content_blocks {
        for entry in block.text.split("\n\n") {
            let parsed = parse_exa_text_block(entry);
            if !parsed.title.is_empty() || !parsed.url.is_empty() {
                results.push(parsed);
            }
        }
    }

    Ok(results)
}

/// 解析 Exa MCP 的带标签文本块：
/// ```text
/// Title: ...
/// URL: ...
/// Published Date: ...
/// Text: ...
/// ```
fn parse_exa_text_block(block: &str) -> SearchResult {
    let mut title = String::new();
    let mut url = String::new();
    let mut content = String::new();

    for line in block.lines() {
        if let Some(val) = line.strip_prefix("Title: ") {
            title = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("URL: ") {
            url = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("Text: ") {
            content = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("Highlights:") {
            if content.is_empty() {
                content = val.trim().to_string();
            }
        } else if content.is_empty()
            && !line.starts_with("Published")
            && !line.starts_with("Author")
        {
            // 没有 Text: 前缀的额外内容行
            if !line.trim().is_empty() && !title.is_empty() {
                content = line.trim().to_string();
            }
        }
    }

    SearchResult {
        title,
        url,
        content,
    }
}

fn extract_final_json_data_line(raw: &str) -> Option<&str> {
    if !raw.contains("data:") {
        return None;
    }

    raw.lines()
        .rev()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim))
        .find(|data| {
            !data.is_empty()
                && *data != "[DONE]"
                && (data.starts_with('{') || data.starts_with('['))
        })
}

// ── Tavily（API，需要 Key）─────────────────────────────────────────

#[derive(Deserialize)]
struct TavilyResponse {
    #[serde(default)]
    results: Vec<TavilyHit>,
}

#[derive(Deserialize)]
struct TavilyHit {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    content: String,
}

pub async fn tavily_search(
    http_client: &reqwest::Client,
    api_key: &str,
    query: &str,
    max_results: u8,
) -> Result<Vec<SearchResult>, String> {
    let body = serde_json::json!({
        "query": query,
        "max_results": max_results,
        "include_answer": false,
    });

    let resp = http_client
        .post("https://api.tavily.com/search")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {api_key}"))
        .timeout(SEARCH_TIMEOUT)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Tavily 搜索请求失败: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!(
            "Tavily API 返回 HTTP {status}: {}",
            truncate_str(&text, 200)
        ));
    }

    let parsed: TavilyResponse = resp
        .json()
        .await
        .map_err(|e| format!("Tavily 响应解析失败: {e}"))?;

    Ok(parsed
        .results
        .into_iter()
        .map(|h| SearchResult {
            title: h.title,
            url: h.url,
            content: h.content,
        })
        .collect())
}

// ── 公共工具 ────────────────────────────────────────────────────────

/// UTF-8 安全的字符串截断
fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// 将搜索结果渲染为 XML 片段，注入 system prompt（使用 CDATA 转义）
pub fn render_search_context(results: &[SearchResult]) -> String {
    use crate::utils::foreground::wrap_xml_cdata;

    let mut out = String::from("<web_search_results>\n");
    out.push_str("<instruction>以下是联网搜索返回的参考信息。根据用户问题自行判断是否需要引用：如果问题涉及实时信息、新闻、事实查询，请参考搜索结果并在行文中自然标注来源；如果问题是创作、闲聊或不需要外部信息的任务，直接忽略搜索结果即可。</instruction>\n");
    let closing = "</web_search_results>";
    for (i, r) in results.iter().take(MAX_SEARCH_CONTEXT_RESULTS).enumerate() {
        let fixed_parts = format!(
            "<result index=\"{}\">\n{}\n{}\n",
            i + 1,
            wrap_xml_cdata("title", truncate_str(&r.title, 240)),
            wrap_xml_cdata("url", truncate_str(&r.url, 600)),
        );
        let suffix = "</result>\n";
        let available = MAX_SEARCH_CONTEXT_BYTES
            .saturating_sub(out.len())
            .saturating_sub(fixed_parts.len())
            .saturating_sub(suffix.len())
            .saturating_sub(closing.len());
        if available == 0 {
            break;
        }
        let content_limit = available.min(MAX_SEARCH_RESULT_CONTENT_BYTES);
        let content = truncate_str(&r.content, content_limit);
        let block = format!(
            "{}{}\n{}",
            fixed_parts,
            wrap_xml_cdata("content", content),
            suffix
        );
        if out.len() + block.len() + closing.len() > MAX_SEARCH_CONTEXT_BYTES {
            break;
        }
        out.push_str(&block);
    }
    out.push_str(closing);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_exa_block_extracts_fields() {
        let block = "Title: Rust Programming\nURL: https://rust-lang.org\nPublished Date: 2024-01-01\nText: Rust is a systems programming language.";
        let result = parse_exa_text_block(block);
        assert_eq!(result.title, "Rust Programming");
        assert_eq!(result.url, "https://rust-lang.org");
        assert_eq!(result.content, "Rust is a systems programming language.");
    }

    #[test]
    fn render_search_context_caps_prompt_contribution() {
        let results = vec![SearchResult {
            title: "large".to_string(),
            url: "https://example.com".to_string(),
            content: "x".repeat(80_000),
        }];

        let rendered = render_search_context(&results);

        assert!(
            rendered.len() <= 12_000,
            "web search context should cap prompt contribution; rendered {} bytes",
            rendered.len()
        );
    }

    #[test]
    fn exa_sse_parser_uses_final_json_data_line_not_done_marker() {
        let source = include_str!("web_search_service.rs");

        assert!(
            source.contains("extract_final_json_data_line"),
            "Exa SSE parsing should use a helper that scans data: lines from the end and skips [DONE]/empty/non-JSON lines"
        );
        assert!(
            !source.contains(".rev()\n            .find_map(|line| line.strip_prefix(\"data:\")"),
            "selecting the last data: line treats trailing [DONE] or keepalive data as the JSON-RPC payload"
        );
    }
}
