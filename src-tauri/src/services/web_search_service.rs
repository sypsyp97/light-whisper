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

    // Exa MCP 返回的 text 是带标签的纯文本块。单条结果的 Highlights/Text
    // 内部也可能有空行，所以只能在新的 Title: 行处切分结果。
    let mut results = Vec::new();
    for block in &content_blocks {
        for entry in split_exa_result_blocks(&block.text) {
            let parsed = parse_exa_text_block(entry);
            if !parsed.title.is_empty() || !parsed.url.is_empty() {
                results.push(parsed);
            }
        }
    }

    Ok(results)
}

fn split_exa_result_blocks(text: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut start = None;
    let mut offset = 0;

    for line in text.split_inclusive('\n') {
        let line_start = offset;
        let line_without_newline = line.trim_end_matches(['\r', '\n']);
        if line_without_newline.starts_with("Title: ") {
            if let Some(current_start) = start {
                let block = text[current_start..line_start].trim();
                if !block.is_empty() {
                    blocks.push(block);
                }
            }
            start = Some(line_start);
        } else if start.is_none() && !line_without_newline.trim().is_empty() {
            start = Some(line_start);
        }
        offset += line.len();
    }

    if let Some(current_start) = start {
        let block = text[current_start..].trim();
        if !block.is_empty() {
            blocks.push(block);
        }
    }

    blocks
}

fn labeled_value<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    line.strip_prefix(label).map(str::trim)
}

fn push_content_line(content: &mut String, line: &str) {
    let line = line.trim_end();
    if content.is_empty() {
        let first = line.trim();
        if !first.is_empty() {
            content.push_str(first);
        }
        return;
    }
    content.push('\n');
    content.push_str(line);
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
    let mut reading_content = false;

    for line in block.lines() {
        if let Some(val) = labeled_value(line, "Title:") {
            title = val.to_string();
            reading_content = false;
        } else if let Some(val) = labeled_value(line, "URL:") {
            url = val.to_string();
            reading_content = false;
        } else if labeled_value(line, "Published Date:").is_some()
            || labeled_value(line, "Published:").is_some()
            || labeled_value(line, "Author:").is_some()
        {
            reading_content = false;
        } else if let Some(val) = labeled_value(line, "Text:") {
            reading_content = true;
            push_content_line(&mut content, val);
        } else if let Some(val) = labeled_value(line, "Highlights:") {
            reading_content = true;
            push_content_line(&mut content, val);
        } else if reading_content {
            push_content_line(&mut content, line);
        } else if !title.is_empty() && !line.trim().is_empty() {
            reading_content = true;
            push_content_line(&mut content, line);
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
    out.push_str("<status>已经执行过联网查询；下面是本次第三方搜索返回的全部可用结果。</status>\n");
    out.push_str("<instruction>如果用户问题涉及天气、新闻、价格、时间、政策、事实核验等实时信息，优先根据这些搜索结果作答，并自然标注来源；有搜索结果时不要回答无法实时查询。如果搜索结果没有给出用户要的具体实时数值，明确说明“搜索结果没有给出具体实时数值”，再概括已有线索并给出最有用的下一步或最相关来源。如果用户请求是创作、改写、翻译、润色、闲聊或不需要外部信息的任务，忽略搜索结果即可。</instruction>\n");
    out.push_str(&format!("<result_count>{}</result_count>\n", results.len()));
    let closing = "</web_search_results>";
    if results.is_empty() {
        out.push_str("<empty>本次联网搜索完成，但没有返回可用结果。</empty>\n");
        out.push_str(closing);
        return out;
    }
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
    fn parse_exa_block_keeps_multiline_highlights_after_blank_lines() {
        let block = "Title: Wetter und Klima - Deutscher Wetterdienst   -  Nürnberg (Flugh.)\nURL: https://www.dwd.de/DE/wetter/wetterundklima_vorort/bayern/nuernberg/_node.html\nPublished: N/A\nAuthor: N/A\nHighlights:\nWetter und Klima - Deutscher Wetterdienst - Nürnberg (Flugh.)\n\n# Nürnberg (Flugh.)\n\n| Wetterwerte | 7.06.2026 | 07 Uhr |\n| --- | --- | --- |\n| Temperatur | 14 Grad C |\n| rel. Feuchte | 85 % |";

        let result = parse_exa_text_block(block);

        assert_eq!(
            result.title,
            "Wetter und Klima - Deutscher Wetterdienst   -  Nürnberg (Flugh.)"
        );
        assert_eq!(
            result.url,
            "https://www.dwd.de/DE/wetter/wetterundklima_vorort/bayern/nuernberg/_node.html"
        );
        assert!(
            result.content.contains("Temperatur | 14 Grad C"),
            "weather values after blank lines must reach the assistant prompt: {}",
            result.content
        );
    }

    #[test]
    fn exa_content_blocks_split_only_at_new_result_titles() {
        let text = "Title: First\nURL: https://one.example\nHighlights:\nFirst line\n\nstill first result\n\nTitle: Second\nURL: https://two.example\nText: Second line";

        let blocks = split_exa_result_blocks(text);

        assert_eq!(blocks.len(), 2);
        assert!(
            blocks[0].contains("still first result"),
            "blank lines inside highlights belong to the same Exa result"
        );
        assert!(blocks[1].starts_with("Title: Second"));
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
    fn render_search_context_marks_third_party_results_as_completed_web_lookup() {
        let results = vec![SearchResult {
            title: "Nuremberg Weather".to_string(),
            url: "https://example.com/weather".to_string(),
            content: "Current temperature is 18 C with light rain.".to_string(),
        }];

        let rendered = render_search_context(&results);

        assert!(
            rendered.contains("已经执行过联网查询"),
            "assistant prompt should tell the model that Exa/Tavily results came from an already-completed web lookup: {rendered}"
        );
        assert!(
            rendered.contains("不得回答无法实时查询") || rendered.contains("不要回答无法实时查询"),
            "real-time questions with search results should not trigger a generic inability-to-browse answer: {rendered}"
        );
        assert!(
            rendered.contains("创作") && rendered.contains("改写") && rendered.contains("忽略搜索结果"),
            "creative or rewrite tasks should remain allowed to ignore web search results: {rendered}"
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
