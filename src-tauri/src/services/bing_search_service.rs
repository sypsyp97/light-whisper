use super::web_search_service::SearchResult;

use base64::Engine;
use scraper::{Html, Selector};
use std::time::Duration;

/// Read public search snippets without opening a browser or fetching result URLs.
pub async fn search(
    client: &reqwest::Client,
    query: &str,
    max: u8,
) -> Result<Vec<SearchResult>, String> {
    let mut response = client
        .get("https://www.bing.com/search")
        .query(&[("q", query), ("count", &max.clamp(1, 10).to_string())])
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|error| super::web_search_service::search_request_error(&error).to_string())?;
    if !response.status().is_success() {
        return Err(super::web_search_service::search_http_error(response.status()).to_string());
    }
    // Search pages can contain large scripts; cap input rather than retaining an unbounded response.
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| super::web_search_service::search_request_error(&error).to_string())?
    {
        if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
            return Err("SEARCH_INVALID_RESPONSE".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    parse_results(&String::from_utf8_lossy(&bytes), max.clamp(1, 10) as usize)
}

fn destination(href: &str) -> Option<String> {
    let url = reqwest::Url::parse(href).ok()?;
    let decoded = if url
        .host_str()
        .is_some_and(|host| host == "bing.com" || host.ends_with(".bing.com"))
        && url.path() == "/ck/a"
    {
        let value = url
            .query_pairs()
            .find(|(name, _)| name == "u")?
            .1
            .into_owned();
        let encoded = value.strip_prefix("a1")?.trim_end_matches('=');
        String::from_utf8(
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(encoded)
                .ok()?,
        )
        .ok()?
    } else {
        href.to_string()
    };
    let target = reqwest::Url::parse(&decoded).ok()?;
    (matches!(target.scheme(), "http" | "https")
        && target.host_str().is_some()
        && target.username().is_empty()
        && target.password().is_none())
    .then(|| target.to_string())
}

fn parse_results(html: &str, max: usize) -> Result<Vec<SearchResult>, String> {
    let doc = Html::parse_document(html);
    let selector = |value| Selector::parse(value).expect("static Bing selector");
    if doc
        .select(&selector(
            "#b_captcha, #b_captcha_container, #challenge-form, iframe[src*='captcha']",
        ))
        .next()
        .is_some()
    {
        return Err("SEARCH_ACCESS_DENIED".into());
    }
    let anchor = selector("h2 a");
    let snippet = selector(".b_caption p, .b_snippet, p");
    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in doc.select(&selector("#b_results .b_algo")) {
        let Some(link) = item.select(&anchor).next() else {
            continue;
        };
        let Some(url) = link.value().attr("href").and_then(destination) else {
            continue;
        };
        let title = link.text().collect::<String>().trim().to_string();
        if title.is_empty() || !seen.insert(url.clone()) {
            continue;
        }
        let content = item
            .select(&snippet)
            .next()
            .map(|node| node.text().collect::<String>())
            .unwrap_or_default();
        results.push(SearchResult {
            title,
            url,
            content: content.trim().to_string(),
            published_date: None,
        });
        if results.len() >= max {
            break;
        }
    }
    if results.is_empty() && doc.select(&selector("#b_results .b_no")).next().is_none() {
        return Err("SEARCH_INVALID_RESPONSE".into());
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_only_organic_results_and_decodes_links() {
        let html = r#"<ol id="b_results"><li class="b_ad"><h2><a href="https://ads.example/">Ad</a></h2></li><li class="b_algo"><h2><a href="https://www.bing.com/ck/a?u=a1aHR0cHM6Ly9ydXN0LWxhbmcub3JnLw">Rust &amp; tools</a></h2><div class="b_caption"><p>Fast &amp; safe.</p></div></li><li class="b_algo"><h2><a href="javascript:alert(1)">Bad</a></h2></li><li class="b_algo"><h2><a href="https://second.example/">Second</a></h2><p>Next result</p></li></ol>"#;
        let result = parse_results(html, 1).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].url, "https://rust-lang.org/");
        assert_eq!(result[0].title, "Rust & tools");
        assert_eq!(result[0].content, "Fast & safe.");
    }
    #[test]
    fn distinguishes_empty_results_from_challenges_and_page_changes() {
        assert!(parse_results(
            r#"<ol id="b_results"><li class="b_no">No results</li></ol>"#,
            5
        )
        .unwrap()
        .is_empty());
        assert!(parse_results(r#"<div id="b_captcha">Verify</div>"#, 5)
            .unwrap_err()
            .contains("SEARCH_ACCESS_DENIED"));
        assert!(parse_results("<html>unexpected layout</html>", 5)
            .unwrap_err()
            .contains("SEARCH_INVALID_RESPONSE"));
    }
}

#[cfg(test)]
mod live_tests {
    #[tokio::test]
    #[ignore = "Requires the public Exa endpoint; run manually"]
    async fn exa_live_status_smoke() {
        let result = crate::services::web_search_service::exa_search(
            &reqwest::Client::new(),
            "",
            "Exa MCP official documentation",
            3,
        )
        .await;
        match result {
            Ok(results) => {
                assert!(!results.is_empty());
                println!("Exa returned {} results", results.len());
            }
            Err(error) => {
                assert!(
                    error.contains("SEARCH_RATE_LIMITED") || error.contains("SEARCH_ACCESS_DENIED"),
                    "unexpected Exa response: {error}"
                );
                println!("Exa correctly reported: {error}");
            }
        }
    }
    #[tokio::test]
    #[ignore = "Requires the public Bing endpoint; run manually"]
    async fn bing_live_smoke() {
        let client = reqwest::Client::new();
        for query in [
            "Rust programming language official",
            "Rust 编程语言 官方文档",
        ] {
            let start = std::time::Instant::now();
            let results = super::search(&client, query, 3)
                .await
                .expect("live Bing search");
            assert!(!results.is_empty());
            println!(
                "{}: {} results in {}ms; first={} {}",
                query,
                results.len(),
                start.elapsed().as_millis(),
                results[0].title,
                results[0].url
            );
        }
    }
}
