use std::collections::HashSet;
use std::time::Duration;

/// Convert a raw HTML string into plain text by stripping tags and collapsing
/// whitespace. This is a pure function so it can be unit-tested without any
/// network access.
pub(super) fn html_to_text(html: &str) -> String {
    let text = html
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<p>", "\n")
        .replace("</p>", "\n")
        .replace("<div>", "\n")
        .replace("</div>", "\n");

    let mut cleaned = String::new();
    let mut in_tag = false;
    for ch in text.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
            cleaned.push('\n');
        } else if !in_tag {
            cleaned.push(ch);
        }
    }

    let lines: Vec<&str> = cleaned
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    lines.join("\n")
}

pub(super) const MAX_NETWORK_RESPONSE_CHARS: usize = 100_000;
pub(super) const MAX_SEARCH_RESULTS: usize = 5;
pub(super) const MAX_SEARCH_SNIPPET_CHARS: usize = 240;
pub(super) const MAX_SEARCH_TITLE_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SearchResult {
    pub(super) title: String,
    pub(super) snippet: String,
    pub(super) url: String,
}

pub(super) fn validate_http_url(url: &str) -> anyhow::Result<reqwest::Url> {
    let parsed = reqwest::Url::parse(url).map_err(|err| anyhow::anyhow!("invalid url: {err}"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        scheme => anyhow::bail!("unsupported url scheme: {scheme}"),
    }
    if parsed.host_str().is_none() {
        anyhow::bail!("url must include a host");
    }
    Ok(parsed)
}

pub(super) fn parse_headers(arguments: &serde_json::Value) -> Vec<(String, String)> {
    arguments
        .get("headers")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(super) fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated = text.chars().take(max_chars).collect::<String>();
    format!("{truncated}\n...[truncated]")
}

pub(super) fn proxy_from_arguments(arguments: &serde_json::Value) -> Option<String> {
    arguments
        .get("proxy")
        .and_then(|value| value.as_str())
        .and_then(non_empty_trimmed)
        .map(str::to_string)
}

pub(super) fn configured_network_proxy(request_proxy: Option<&str>) -> Option<String> {
    if let Some(value) = request_proxy {
        let value = value.trim();
        if value.eq_ignore_ascii_case("direct") || value.eq_ignore_ascii_case("none") {
            return None;
        }
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }

    [
        "NIGHT24_NETWORK_PROXY",
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ]
    .into_iter()
    .find_map(|name| {
        std::env::var(name)
            .ok()
            .and_then(|value| non_empty_trimmed(&value).map(str::to_string))
    })
}

pub(super) fn non_empty_trimmed(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

pub(super) fn http_client(proxy: Option<&str>) -> anyhow::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent("Night24/0.1 (+https://github.com/night24)");

    if let Some(proxy) = configured_network_proxy(proxy) {
        builder =
            builder
                .proxy(reqwest::Proxy::all(&proxy).map_err(|err| {
                    anyhow::anyhow!("invalid network proxy `{}`: {}", proxy, err)
                })?);
    }

    builder.build().map_err(Into::into)
}

pub(super) async fn send_network_request(
    url: &str,
    method: &str,
    headers: Vec<(String, String)>,
    body: Option<String>,
    proxy: Option<&str>,
) -> anyhow::Result<String> {
    let url = validate_http_url(url)?;
    let client = http_client(proxy)?;
    let method = method.to_uppercase();
    let mut request = match method.as_str() {
        "GET" => client.get(url),
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "PATCH" => client.patch(url),
        "DELETE" => client.delete(url),
        "HEAD" => client.head(url),
        _ => anyhow::bail!("unsupported http method: {}", method),
    };

    for (key, value) in headers {
        request = request.header(&key, value);
    }
    if let Some(body) = body {
        request = request.body(body);
    }

    let response = request.send().await?;
    let status = response.status();
    let final_url = response.url().to_string();
    let text = response.text().await.unwrap_or_default();
    let text = truncate_chars(&text, MAX_NETWORK_RESPONSE_CHARS);

    if status.is_success() {
        Ok(format!(
            "status: {}\nurl: {}\n\n{}",
            status, final_url, text
        ))
    } else {
        anyhow::bail!("http request failed {} at {}:\n{}", status, final_url, text);
    }
}

pub(super) async fn fetch_network_body(url: &str, proxy: Option<&str>) -> anyhow::Result<String> {
    let url = validate_http_url(url)?;
    let response = http_client(proxy)?.get(url).send().await?;
    let status = response.status();
    let final_url = response.url().to_string();
    let text = response.text().await.unwrap_or_default();
    if status.is_success() {
        Ok(text)
    } else {
        anyhow::bail!(
            "http request failed {} at {}:\n{}",
            status,
            final_url,
            truncate_chars(&text, 4_000)
        );
    }
}

pub(super) async fn search_web(query: &str, proxy: Option<&str>) -> anyhow::Result<String> {
    let query = query.trim();
    if query.is_empty() {
        anyhow::bail!("missing `query` for web search");
    }

    let url = reqwest::Url::parse_with_params(
        "https://api.duckduckgo.com/",
        &[
            ("q", query),
            ("format", "json"),
            ("no_redirect", "1"),
            ("no_html", "1"),
            ("skip_disambig", "1"),
        ],
    )?;

    let client = http_client(proxy)?;
    let response = client.get(url).send().await?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!(
            "web search failed {}:\n{}",
            status,
            truncate_chars(&text, 4_000)
        );
    }

    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|err| anyhow::anyhow!("web search returned invalid json: {err}"))?;
    Ok(format_duckduckgo_results(query, &value))
}

pub(super) fn format_duckduckgo_results(query: &str, value: &serde_json::Value) -> String {
    let results = clean_duckduckgo_results(query, value);

    if results.is_empty() {
        return format!("No clean search results for: {query}");
    }

    let mut output = format!("Search results for: {query}\n");
    for (index, result) in results.into_iter().enumerate() {
        output.push_str(&format!("{}. {}\n", index + 1, result.title));
        output.push_str(&result.snippet);
        output.push('\n');
        if !result.url.is_empty() {
            output.push_str(&result.url);
            output.push('\n');
        }
    }
    output.trim_end().to_string()
}

pub(super) fn clean_duckduckgo_results(
    query: &str,
    value: &serde_json::Value,
) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();
    let heading = value
        .get("Heading")
        .and_then(|value| value.as_str())
        .unwrap_or(query);
    let abstract_text = clean_search_text(
        value
            .get("AbstractText")
            .and_then(|value| value.as_str())
            .unwrap_or(""),
        MAX_SEARCH_SNIPPET_CHARS,
    );
    let abstract_url = clean_url(
        value
            .get("AbstractURL")
            .and_then(|value| value.as_str())
            .unwrap_or(""),
    );
    if !abstract_text.is_empty() {
        push_search_result(
            &mut results,
            &mut seen,
            clean_search_text(heading, MAX_SEARCH_TITLE_CHARS),
            abstract_text,
            abstract_url,
        );
    }

    collect_duckduckgo_topics(value.get("Results"), &mut results, &mut seen);
    collect_duckduckgo_topics(value.get("RelatedTopics"), &mut results, &mut seen);

    results.truncate(MAX_SEARCH_RESULTS);
    results
}

pub(super) fn collect_duckduckgo_topics(
    value: Option<&serde_json::Value>,
    results: &mut Vec<SearchResult>,
    seen: &mut HashSet<String>,
) {
    let Some(serde_json::Value::Array(items)) = value else {
        return;
    };

    for item in items {
        if results.len() >= MAX_SEARCH_RESULTS {
            return;
        }
        if let Some(nested) = item.get("Topics") {
            collect_duckduckgo_topics(Some(nested), results, seen);
            continue;
        }
        let text = clean_search_text(
            item.get("Text")
                .and_then(|value| value.as_str())
                .unwrap_or(""),
            MAX_SEARCH_SNIPPET_CHARS,
        );
        if text.is_empty() {
            continue;
        }
        let (title, snippet) = split_search_topic_text(&text);
        let url = clean_url(
            item.get("FirstURL")
                .and_then(|value| value.as_str())
                .unwrap_or(""),
        );
        push_search_result(results, seen, title, snippet, url);
    }
}

pub(super) fn push_search_result(
    results: &mut Vec<SearchResult>,
    seen: &mut HashSet<String>,
    title: String,
    snippet: String,
    url: String,
) {
    let snippet = clean_search_text(&snippet, MAX_SEARCH_SNIPPET_CHARS);
    if snippet.is_empty() {
        return;
    }
    let title = if title.trim().is_empty() {
        clean_search_text(&snippet, MAX_SEARCH_TITLE_CHARS)
    } else {
        clean_search_text(&title, MAX_SEARCH_TITLE_CHARS)
    };
    let key = if url.is_empty() {
        snippet.to_ascii_lowercase()
    } else {
        url.to_ascii_lowercase()
    };
    if !seen.insert(key) {
        return;
    }
    results.push(SearchResult {
        title,
        snippet,
        url,
    });
}

pub(super) fn split_search_topic_text(text: &str) -> (String, String) {
    if let Some((title, rest)) = text.split_once(" - ") {
        let title = clean_search_text(title, MAX_SEARCH_TITLE_CHARS);
        let snippet = clean_search_text(rest, MAX_SEARCH_SNIPPET_CHARS);
        if !title.is_empty() && !snippet.is_empty() {
            return (title, snippet);
        }
    }
    (
        clean_search_text(text, MAX_SEARCH_TITLE_CHARS),
        clean_search_text(text, MAX_SEARCH_SNIPPET_CHARS),
    )
}

pub(super) fn clean_search_text(text: &str, max_chars: usize) -> String {
    let decoded = text
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    let compact = decoded
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    limit_single_line(&compact, max_chars)
}

pub(super) fn clean_url(url: &str) -> String {
    limit_single_line(url.trim(), 500)
}

pub(super) fn limit_single_line(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut value = text.chars().take(max_chars).collect::<String>();
    value.push_str("...");
    value
}
