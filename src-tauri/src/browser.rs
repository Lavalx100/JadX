use crate::system;
use crate::types::ResourcePolicy;
use futures_util::StreamExt;
use reqwest::header::{CONTENT_DISPOSITION, CONTENT_TYPE, LOCATION};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Semaphore;
use url::Url;

pub const MAX_BROWSER_TABS: usize = 8;
pub const MAX_BROWSER_TABS_UNDER_PRESSURE: usize = 3;
const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_PAGE_TEXT: usize = 80_000;
const MAX_LINKS: usize = 120;
const MAX_REDIRECTS: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserLink {
    pub text: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserTab {
    pub id: String,
    pub title: String,
    pub url: String,
    pub text: String,
    pub links: Vec<BrowserLink>,
    pub status: String,
    pub bytes: usize,
    #[serde(default)]
    pub content_type: String,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserSessionStatus {
    pub tabs: Vec<BrowserTab>,
    pub active_tab_id: Option<String>,
    pub max_tabs: usize,
    pub max_page_bytes: usize,
    pub scripts_enabled: bool,
    pub downloads_enabled: bool,
    pub fetch_in_flight: bool,
}

#[derive(Clone)]
pub struct BrowserSession {
    pub(crate) tabs: BTreeMap<String, BrowserTab>,
    pub(crate) active_tab_id: Option<String>,
    pub(crate) next_id: u64,
    pub(crate) client: Client,
    pub(crate) fetch_gate: Arc<Semaphore>,
}

impl BrowserSession {
    pub fn new() -> Result<Self, String> {
        let client = Client::builder()
            .timeout(Duration::from_secs(25))
            .connect_timeout(Duration::from_secs(8))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("JadX Safe Reader/1.0")
            .build()
            .map_err(|error| format!("browser client setup failed: {error}"))?;
        Ok(Self {
            tabs: BTreeMap::new(),
            active_tab_id: None,
            next_id: 1,
            client,
            // A single shared permit prevents several model/browser actions from
            // multiplying network, parsing, and memory pressure.
            fetch_gate: Arc::new(Semaphore::new(1)),
        })
    }

    pub fn status(&self) -> BrowserSessionStatus {
        BrowserSessionStatus {
            tabs: self.tabs.values().cloned().collect(),
            active_tab_id: self.active_tab_id.clone(),
            max_tabs: MAX_BROWSER_TABS,
            max_page_bytes: MAX_PAGE_BYTES,
            scripts_enabled: false,
            downloads_enabled: false,
            fetch_in_flight: self
                .tabs
                .values()
                .any(|tab| tab.status.as_str() == "loading"),
        }
    }

    pub fn activate(&mut self, tab_id: &str) -> Result<BrowserTab, String> {
        if !self.tabs.contains_key(tab_id) {
            return Err("browser tab not found".into());
        }
        self.active_tab_id = Some(tab_id.to_string());
        self.tabs
            .get(tab_id)
            .cloned()
            .ok_or_else(|| "browser tab not found".into())
    }

    pub fn close(&mut self, tab_id: &str) -> Result<(), String> {
        if self.tabs.remove(tab_id).is_none() {
            return Err("browser tab not found".into());
        }
        if self.active_tab_id.as_deref() == Some(tab_id) {
            self.active_tab_id = self.tabs.keys().next_back().cloned();
        }
        Ok(())
    }

    fn new_tab(&mut self, url: &Url) -> Result<String, String> {
        if self.tabs.len() >= MAX_BROWSER_TABS {
            return Err(format!(
                "browser tab limit reached ({MAX_BROWSER_TABS}); close an unused tab first"
            ));
        }
        let id = format!("tab-{}", self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        let now = chrono::Utc::now().to_rfc3339();
        self.tabs.insert(
            id.clone(),
            BrowserTab {
                id: id.clone(),
                title: "Loading…".into(),
                url: url.to_string(),
                status: "loading".into(),
                created_at: now.clone(),
                updated_at: now,
                ..Default::default()
            },
        );
        self.active_tab_id = Some(id.clone());
        Ok(id)
    }
}

pub async fn open_tab(
    session: Arc<Mutex<BrowserSession>>,
    raw_url: &str,
    resource_policy: ResourcePolicy,
) -> Result<BrowserTab, String> {
    let url = validate_url(raw_url).await?;
    let (tab_id, client, gate) = {
        let mut session = session
            .lock()
            .map_err(|_| "browser session lock poisoned".to_string())?;
        let telemetry = system::resource_telemetry(resource_policy);
        let available = telemetry.available_memory_bytes.unwrap_or_default();
        let under_pressure = telemetry.memory_pressure.eq_ignore_ascii_case("critical")
            || telemetry.memory_pressure.eq_ignore_ascii_case("warning")
            || available < telemetry.emergency_floor_bytes;
        let tab_limit = if under_pressure {
            MAX_BROWSER_TABS_UNDER_PRESSURE
        } else {
            MAX_BROWSER_TABS
        };
        if session.tabs.len() >= tab_limit {
            return Err(format!(
                "browser is resource-limited to {tab_limit} tabs while memory pressure is {}; close a tab or wait for pressure to fall",
                telemetry.memory_pressure
            ));
        }
        let tab_id = session.new_tab(&url)?;
        (tab_id, session.client.clone(), session.fetch_gate.clone())
    };
    finish_navigation(session, tab_id, url, client, gate).await
}

pub async fn navigate_tab(
    session: Arc<Mutex<BrowserSession>>,
    tab_id: &str,
    raw_url: &str,
) -> Result<BrowserTab, String> {
    let (url, client, gate) = {
        let session = session
            .lock()
            .map_err(|_| "browser session lock poisoned".to_string())?;
        if !session.tabs.contains_key(tab_id) {
            return Err("browser tab not found".into());
        }
        let current = session
            .tabs
            .get(tab_id)
            .map(|tab| tab.url.clone())
            .unwrap_or_default();
        let url = Url::parse(raw_url)
            .or_else(|_| Url::parse(&current).and_then(|base| base.join(raw_url)))
            .map_err(|error| format!("invalid browser URL: {error}"))?;
        (url, session.client.clone(), session.fetch_gate.clone())
    };
    validate_url(url.as_str()).await?;
    {
        let mut session = session
            .lock()
            .map_err(|_| "browser session lock poisoned".to_string())?;
        let tab = session
            .tabs
            .get_mut(tab_id)
            .ok_or_else(|| "browser tab not found".to_string())?;
        if tab.status == "loading" {
            return Err("this tab is already loading; wait for it to finish".into());
        }
        tab.url = url.to_string();
        tab.status = "loading".into();
        tab.error = None;
    }
    finish_navigation(session, tab_id.to_string(), url, client, gate).await
}

async fn finish_navigation(
    session: Arc<Mutex<BrowserSession>>,
    tab_id: String,
    url: Url,
    client: Client,
    gate: Arc<Semaphore>,
) -> Result<BrowserTab, String> {
    let _permit = gate
        .acquire_owned()
        .await
        .map_err(|_| "browser fetch gate closed".to_string())?;
    let result = fetch_page(&client, url).await;
    let mut session = session
        .lock()
        .map_err(|_| "browser session lock poisoned".to_string())?;
    let tab = session
        .tabs
        .get_mut(&tab_id)
        .ok_or_else(|| "browser tab was closed while loading".to_string())?;
    tab.updated_at = chrono::Utc::now().to_rfc3339();
    match result {
        Ok(page) => {
            tab.title = page.title;
            tab.url = page.url;
            tab.text = page.text;
            tab.links = page.links;
            tab.bytes = page.bytes;
            tab.content_type = page.content_type;
            tab.status = "ready".into();
            tab.error = None;
            Ok(tab.clone())
        }
        Err(error) => {
            tab.status = "error".into();
            tab.error = Some(error.clone());
            Err(error)
        }
    }
}

#[derive(Debug)]
struct PageData {
    title: String,
    url: String,
    text: String,
    links: Vec<BrowserLink>,
    bytes: usize,
    content_type: String,
    is_html: bool,
}

async fn fetch_page(client: &Client, mut url: Url) -> Result<PageData, String> {
    for redirect_count in 0..=MAX_REDIRECTS {
        validate_url(url.as_str()).await?;
        let response = client
            .get(url.clone())
            .header(
                "accept",
                "text/html, text/plain, application/json, application/xhtml+xml",
            )
            .send()
            .await
            .map_err(|error| format!("browser request failed: {error}"))?;
        if response.status().is_redirection() {
            if redirect_count == MAX_REDIRECTS {
                return Err("too many redirects; navigation stopped".into());
            }
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| "redirect did not provide a valid location".to_string())?;
            url = url
                .join(location)
                .map_err(|error| format!("invalid redirect target: {error}"))?;
            continue;
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if response.headers().contains_key(CONTENT_DISPOSITION)
            || content_type.contains("application/octet-stream")
            || content_type.contains("application/zip")
            || content_type.contains("application/x-7z")
            || content_type.contains("application/x-rar")
            || content_type.contains("application/x-msdownload")
            || content_type.starts_with("image/")
            || content_type.starts_with("audio/")
            || content_type.starts_with("video/")
            || content_type == "application/pdf"
        {
            return Err("download or binary content blocked by Safe Reader".into());
        }
        if let Some(length) = response.content_length() {
            if length as usize > MAX_PAGE_BYTES {
                return Err(format!(
                    "page exceeds the {} MB reader limit",
                    MAX_PAGE_BYTES / 1024 / 1024
                ));
            }
        }
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| format!("browser response failed: {error}"))?;
            if body.len().saturating_add(chunk.len()) > MAX_PAGE_BYTES {
                return Err(format!(
                    "page exceeds the {} MB reader limit",
                    MAX_PAGE_BYTES / 1024 / 1024
                ));
            }
            body.extend_from_slice(&chunk);
        }
        let raw = String::from_utf8(body.clone())
            .map_err(|_| "non-text response blocked by Safe Reader".to_string())?;
        let is_html = content_type.contains("text/html")
            || content_type.contains("application/xhtml+xml")
            || raw.trim_start().starts_with("<!doctype html")
            || raw.contains("<html");
        let (title, text, links) = if is_html {
            parse_html(&raw, &url)?
        } else if content_type.starts_with("text/") || content_type.contains("json") {
            (
                url.host_str().unwrap_or("Untitled").into(),
                bound_text(&raw),
                Vec::new(),
            )
        } else {
            return Err("response is not a readable text page".into());
        };
        return Ok(PageData {
            title,
            url: url.to_string(),
            text,
            links,
            bytes: body.len(),
            content_type,
            is_html,
        });
    }
    Err("browser navigation failed".into())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchResult {
    pub title: String,
    pub url: String,
}

/// Search the public web through the same bounded, script-free reader used for
/// ordinary pages. Search output is untrusted and should never be treated as
/// instructions.
pub async fn search_web(
    session: Arc<Mutex<BrowserSession>>,
    query: &str,
    resource_policy: ResourcePolicy,
) -> Result<Vec<WebSearchResult>, String> {
    let query = query.trim();
    if query.is_empty() || query.len() > 400 {
        return Err("search query must contain 1–400 characters".into());
    }
    let mut url =
        Url::parse("https://html.duckduckgo.com/html/").map_err(|error| error.to_string())?;
    url.query_pairs_mut().append_pair("q", query);
    let tab = open_tab(session.clone(), url.as_str(), resource_policy).await?;
    let results = search_result_links(&tab.links);
    if let Ok(mut session) = session.lock() {
        let _ = session.close(&tab.id);
    }
    if results.is_empty() {
        return Err("the search page loaded, but no external results were found; try a more specific query or open a known URL".into());
    }
    Ok(results)
}

fn search_result_links(links: &[BrowserLink]) -> Vec<WebSearchResult> {
    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for link in links {
        let Ok(mut url) = Url::parse(&link.url) else {
            continue;
        };
        if url
            .host_str()
            .is_some_and(|host| host.ends_with("duckduckgo.com"))
        {
            let destination = url
                .query_pairs()
                .find(|(key, _)| key == "uddg")
                .map(|(_, value)| value.into_owned());
            let Some(destination) = destination else {
                continue;
            };
            let Ok(parsed) = Url::parse(&destination) else {
                continue;
            };
            url = parsed;
        }
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url
                .host_str()
                .is_none_or(|host| host.ends_with("duckduckgo.com"))
        {
            continue;
        }
        url.set_fragment(None);
        let canonical = url.to_string();
        if seen.insert(canonical.clone()) {
            results.push(WebSearchResult {
                title: if link.text.trim().is_empty() {
                    canonical.clone()
                } else {
                    link.text.trim().chars().take(300).collect()
                },
                url: canonical,
            });
            if results.len() == 8 {
                break;
            }
        }
    }
    results
}

/// Fetch source text using Safe Reader transport rules. Common repository
/// permalink formats are converted to their raw-text equivalents first.
pub async fn read_source(
    session: Arc<Mutex<BrowserSession>>,
    raw_url: &str,
    start_line: usize,
    max_lines: usize,
) -> Result<String, String> {
    let url = normalize_source_url(raw_url)?;
    validate_url(url.as_str()).await?;
    let (client, gate) = {
        let session = session
            .lock()
            .map_err(|_| "browser session lock poisoned".to_string())?;
        (session.client.clone(), session.fetch_gate.clone())
    };
    let _permit = gate
        .acquire_owned()
        .await
        .map_err(|_| "browser fetch gate closed".to_string())?;
    let page = fetch_page(&client, url.clone()).await?;
    let content_type = page.content_type.to_ascii_lowercase();
    if page.is_html
        || content_type.contains("text/html")
        || content_type.contains("application/xhtml+xml")
        || !(content_type.starts_with("text/")
            || content_type.contains("json")
            || content_type.contains("javascript")
            || content_type.contains("xml"))
    {
        return Err("the URL did not return a readable raw text/source response; use a repository file permalink or raw URL".into());
    }
    let start_line = start_line.max(1);
    let max_lines = max_lines.clamp(1, 500);
    let lines: Vec<&str> = page.text.lines().collect();
    if start_line > lines.len().max(1) {
        return Err(format!(
            "start_line {start_line} is past the end of the {}-line source",
            lines.len()
        ));
    }
    let end = start_line
        .saturating_sub(1)
        .saturating_add(max_lines)
        .min(lines.len());
    let mut output = String::new();
    for (index, line) in lines[start_line.saturating_sub(1)..end].iter().enumerate() {
        let numbered = format!("{:>6}  {}\n", start_line + index, line);
        if output.len().saturating_add(numbered.len()) > 60_000 {
            output.push_str("[source output truncated at 60 KB]\n");
            break;
        }
        output.push_str(&numbered);
    }
    if end < lines.len() {
        output.push_str(&format!(
            "\n[showing lines {start_line}–{end} of {}]",
            lines.len()
        ));
    }
    let display_url = page.url.split(['?', '#']).next().unwrap_or(url.as_str());
    Ok(format!(
        "Source: {display_url}\nContent-Type: {}\n\n{}",
        page.content_type, output
    ))
}

fn normalize_source_url(raw: &str) -> Result<Url, String> {
    let mut url = Url::parse(raw).map_err(|error| format!("invalid source URL: {error}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("only public HTTP(S) source URLs are allowed".into());
    }
    url.set_fragment(None);
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let segments: Vec<String> = url
        .path_segments()
        .map(|segments| segments.map(str::to_string).collect())
        .unwrap_or_default();
    if host == "github.com" && segments.len() >= 5 && segments[2] == "blob" {
        url.set_host(Some("raw.githubusercontent.com"))
            .map_err(|_| "invalid GitHub source URL".to_string())?;
        let mut raw_segments = segments[..2].to_vec();
        raw_segments.extend_from_slice(&segments[3..]);
        set_path_segments(&mut url, &raw_segments)?;
        url.set_query(None);
    } else if host == "gitlab.com" {
        if let Some(marker) = segments.iter().position(|segment| segment == "-") {
            if segments
                .get(marker + 1)
                .is_some_and(|segment| segment == "blob")
            {
                let mut raw_segments = segments;
                raw_segments[marker + 1] = "raw".into();
                set_path_segments(&mut url, &raw_segments)?;
                url.set_query(None);
            }
        }
    } else if host == "codeberg.org"
        && segments.len() >= 6
        && segments[2] == "src"
        && segments[3] == "branch"
    {
        let mut raw_segments = segments;
        raw_segments[2] = "raw".into();
        set_path_segments(&mut url, &raw_segments)?;
        url.set_query(None);
    }
    Ok(url)
}

fn set_path_segments(url: &mut Url, segments: &[String]) -> Result<(), String> {
    let mut path = url
        .path_segments_mut()
        .map_err(|_| "invalid source URL path".to_string())?;
    path.clear().extend(segments.iter().map(String::as_str));
    Ok(())
}

fn parse_html(raw: &str, base: &Url) -> Result<(String, String, Vec<BrowserLink>), String> {
    let title = regex::Regex::new(r"(?is)<title[^>]*>(.*?)</title>")
        .map_err(|error| error.to_string())?
        .captures(raw)
        .and_then(|capture| capture.get(1).map(|value| clean_text(value.as_str())))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| base.host_str().unwrap_or("Untitled").to_string());
    let link_re =
        regex::Regex::new(r#"(?is)<a\b[^>]*href\s*=\s*[\"']([^\"']+)[\"'][^>]*>(.*?)</a>"#)
            .map_err(|error| error.to_string())?;
    let mut links = Vec::new();
    for capture in link_re.captures_iter(raw).take(MAX_LINKS) {
        let href = capture
            .get(1)
            .map(|value| value.as_str().trim())
            .unwrap_or_default();
        let Ok(url) = base.join(href) else { continue };
        if !matches!(url.scheme(), "http" | "https")
            || url.username() != ""
            || url.password().is_some()
        {
            continue;
        }
        let text = capture
            .get(2)
            .map(|value| clean_text(value.as_str()))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| url.to_string());
        links.push(BrowserLink {
            text,
            url: url.to_string(),
        });
    }
    let mut without_code = raw.to_string();
    for tag in ["script", "style", "noscript", "template"] {
        let pattern = format!(r"(?is)<{tag}\b[^>]*>.*?</{tag}\s*>");
        let code_re = regex::Regex::new(&pattern).map_err(|error| error.to_string())?;
        without_code = code_re.replace_all(&without_code, " ").to_string();
    }
    let without_tags = regex::Regex::new(r"(?is)<[^>]+>")
        .map_err(|error| error.to_string())?
        .replace_all(&without_code, " ");
    Ok((title, bound_text(&clean_text(&without_tags)), links))
}

fn clean_text(value: &str) -> String {
    let value = value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn bound_text(value: &str) -> String {
    let mut text = value.to_string();
    if text.len() > MAX_PAGE_TEXT {
        text.truncate(MAX_PAGE_TEXT);
        text.push_str("\n\n[Reader output truncated]");
    }
    text
}

pub async fn validate_url(raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw).map_err(|error| format!("invalid browser URL: {error}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("only http and https browser URLs are allowed".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("browser URLs cannot contain credentials".into());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "browser URL must include a host".to_string())?;
    if host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".local")
        || host.eq_ignore_ascii_case("0.0.0.0")
    {
        return Err("localhost and local-network browser targets are blocked".into());
    }
    let port = url.port_or_known_default().unwrap_or(443);
    let addresses: Vec<SocketAddr> = if let Ok(ip) = host.parse::<IpAddr>() {
        vec![SocketAddr::new(ip, port)]
    } else {
        tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| "browser host could not be resolved".to_string())?
            .collect()
    };
    if addresses.is_empty() {
        return Err("browser host did not resolve".into());
    }
    if addresses.iter().any(|address| is_private_ip(address.ip())) {
        return Err(
            "private, link-local, loopback, or metadata browser targets are blocked".into(),
        );
    }
    Ok(url)
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(value) => {
            value.is_private()
                || value.is_loopback()
                || value.is_link_local()
                || value.is_multicast()
                || value.is_unspecified()
                || value.octets()[0] == 169 && value.octets()[1] == 254
        }
        IpAddr::V6(value) => {
            value.is_loopback()
                || value.is_unique_local()
                || value.is_unicast_link_local()
                || value.is_multicast()
                || value.is_unspecified()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn blocks_local_and_non_http_urls() {
        assert!(validate_url("file:///tmp/a").await.is_err());
        assert!(validate_url("http://127.0.0.1:1234").await.is_err());
        assert!(validate_url("http://localhost").await.is_err());
        assert!(validate_url("https://example.com").await.is_ok());
    }

    #[test]
    fn session_stays_bounded() {
        let session = BrowserSession::new().unwrap();
        assert_eq!(session.status().max_tabs, MAX_BROWSER_TABS);
        assert!(!session.status().scripts_enabled);
        assert!(!session.status().downloads_enabled);
    }

    #[test]
    fn repository_permalinks_map_to_raw_source_urls() {
        assert_eq!(
            normalize_source_url("https://github.com/owner/repo/blob/main/src/lib.rs")
                .unwrap()
                .as_str(),
            "https://raw.githubusercontent.com/owner/repo/main/src/lib.rs"
        );
        assert_eq!(
            normalize_source_url("https://gitlab.com/group/sub/repo/-/blob/main/src/lib.rs")
                .unwrap()
                .as_str(),
            "https://gitlab.com/group/sub/repo/-/raw/main/src/lib.rs"
        );
        assert_eq!(
            normalize_source_url("https://codeberg.org/owner/repo/src/branch/main/src/lib.rs")
                .unwrap()
                .as_str(),
            "https://codeberg.org/owner/repo/raw/branch/main/src/lib.rs"
        );
    }

    #[test]
    fn search_results_unwrap_engine_redirects_and_deduplicate() {
        let links = vec![
            BrowserLink {
                text: "Example docs".into(),
                url: "https://duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fdocs%23top".into(),
            },
            BrowserLink {
                text: "Duplicate".into(),
                url: "https://example.com/docs".into(),
            },
            BrowserLink {
                text: "Search home".into(),
                url: "https://duckduckgo.com/about".into(),
            },
        ];
        let results = search_result_links(&links);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/docs");
    }

    #[tokio::test]
    #[ignore = "live public-web smoke test; reads DuckDuckGo and a public GitHub source file"]
    async fn live_search_and_source_reader_work() {
        let session = Arc::new(Mutex::new(BrowserSession::new().unwrap()));
        let results = search_web(
            session.clone(),
            "Microsoft Playwright MCP GitHub",
            ResourcePolicy::Adaptive,
        )
        .await
        .unwrap();
        assert!(!results.is_empty());

        let source = read_source(
            session,
            "https://github.com/microsoft/playwright-mcp/blob/main/README.md",
            1,
            30,
        )
        .await
        .unwrap();
        assert!(source.contains("Playwright MCP"));
        assert!(source.contains("Content-Type: text/"));
    }
}
