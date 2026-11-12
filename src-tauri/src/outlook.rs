//! Local Outlook verification-code listener.
//!
//! JadX uses Microsoft Graph delegated read-only access.  OAuth tokens live in
//! the macOS Keychain; message bodies and verification codes are kept in
//! memory only.  The listener intentionally uses bounded polling rather than
//! a public webhook because a desktop app should not expose a mailbox endpoint
//! to the internet.

use crate::types::{OutlookAccount, OutlookStatus};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{Duration as ChronoDuration, Utc};
use keyring::Entry;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::select;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const GRAPH_URL: &str = "https://graph.microsoft.com/v1.0";
const KEYCHAIN_SERVICE: &str = "com.jadx.desktop";
const TOKEN_KEY: &str = "outlook.oauth.tokens";
const DEFAULT_TENANT: &str = "common";
const DEFAULT_FOLDER: &str = "inbox";
const DEFAULT_POLL_SECONDS: u64 = 5;
const MAX_MESSAGES_PER_POLL: u8 = 25;
// Microsoft consumer sign-in rejected JadX's ephemeral loopback port even
// though Entra normally ignores localhost ports. Keep this deterministic and
// register the exact desktop redirect URI in the app registration.
const OAUTH_CALLBACK_PORT: u16 = 53682;

#[derive(Debug, Clone, Default)]
pub struct OutlookRuntime {
    pub account: Option<OutlookAccount>,
    pub listener_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct DetectedEmailCode {
    pub code: String,
    pub sender: String,
    pub subject: String,
    pub received_at: String,
}

#[derive(Debug)]
pub struct BrowserOAuthLogin {
    pub authorization_url: String,
    redirect_uri: String,
    state: String,
    code_verifier: String,
    listener: TcpListener,
    ipv6_listener: Option<TcpListener>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TokenStore {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: i64,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GraphMessageCollection {
    value: Vec<GraphMessage>,
}

#[derive(Debug, Deserialize)]
struct GraphMessage {
    id: Option<String>,
    subject: Option<String>,
    from: Option<GraphFrom>,
    #[serde(rename = "receivedDateTime")]
    received_date_time: Option<String>,
    #[serde(rename = "bodyPreview")]
    body_preview: Option<String>,
    body: Option<GraphBody>,
}

#[derive(Debug, Deserialize)]
struct GraphFrom {
    #[serde(rename = "emailAddress")]
    email_address: Option<GraphEmailAddress>,
}

#[derive(Debug, Deserialize)]
struct GraphEmailAddress {
    #[serde(rename = "address")]
    address: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphBody {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthError {
    error: Option<String>,
    error_description: Option<String>,
}

fn tenant_name(tenant: Option<&str>) -> String {
    let value = tenant.unwrap_or(DEFAULT_TENANT).trim();
    if value.is_empty() {
        DEFAULT_TENANT.into()
    } else {
        value.to_string()
    }
}

fn client() -> Client {
    Client::builder()
        .user_agent("JadX local Outlook listener")
        .build()
        .unwrap_or_else(|_| Client::new())
}

fn keychain() -> Result<Entry, String> {
    Entry::new(KEYCHAIN_SERVICE, TOKEN_KEY).map_err(|error| error.to_string())
}

fn save_token_store(store: &TokenStore) -> Result<(), String> {
    let entry = keychain()?;
    let value = serde_json::to_string(store).map_err(|error| error.to_string())?;
    entry
        .set_password(&value)
        .map_err(|error| error.to_string())
}

fn load_token_store() -> Result<TokenStore, String> {
    let value = keychain()?
        .get_password()
        .map_err(|error| error.to_string())?;
    serde_json::from_str(&value)
        .map_err(|error| format!("stored Outlook token is invalid: {error}"))
}

pub fn clear_tokens() -> Result<(), String> {
    match keychain()?.delete_credential() {
        Ok(()) => Ok(()),
        Err(error) if error.to_string().to_ascii_lowercase().contains("not found") => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

pub fn has_tokens() -> bool {
    load_token_store().is_ok()
}

pub async fn begin_browser_login(
    client_id: &str,
    tenant: Option<&str>,
    email_hint: Option<&str>,
) -> Result<BrowserOAuthLogin, String> {
    let client_id = client_id.trim();
    if client_id.is_empty() {
        return Err("Enter a Microsoft Entra public client ID before connecting Outlook.".into());
    }
    let listener = TcpListener::bind(("127.0.0.1", OAUTH_CALLBACK_PORT))
        .await
        .map_err(|error| {
            format!(
                "could not reserve the Outlook callback at http://localhost:{OAUTH_CALLBACK_PORT} ({error}); close any other JadX sign-in window and retry"
            )
        })?;
    let port = OAUTH_CALLBACK_PORT;
    // Browsers may resolve `localhost` to ::1 before 127.0.0.1. Bind both
    // loopback families when possible so the callback works either way.
    let ipv6_listener = TcpListener::bind(format!("[::1]:{port}")).await.ok();
    let redirect_uri = format!("http://localhost:{port}");
    let state = Uuid::new_v4().to_string();
    let code_verifier = format!("{}{}", new_pkce_verifier(), new_pkce_verifier());
    let challenge = pkce_challenge(&code_verifier);
    let mut authorization_url = url::Url::parse(&format!(
        "https://login.microsoftonline.com/{}/oauth2/v2.0/authorize",
        tenant_name(tenant)
    ))
    .map_err(|error| format!("could not prepare Microsoft sign-in URL: {error}"))?;
    {
        let mut query = authorization_url.query_pairs_mut();
        query
            .append_pair("client_id", client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("response_mode", "query")
            .append_pair(
                "scope",
                "openid profile offline_access https://graph.microsoft.com/Mail.Read",
            )
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");
        if let Some(hint) = email_hint.map(str::trim).filter(|value| !value.is_empty()) {
            query.append_pair("login_hint", hint);
        }
    }
    Ok(BrowserOAuthLogin {
        authorization_url: authorization_url.to_string(),
        redirect_uri,
        state,
        code_verifier,
        listener,
        ipv6_listener,
    })
}

pub async fn finish_browser_login(
    login: BrowserOAuthLogin,
    client_id: &str,
    tenant: Option<&str>,
    account_hint: Option<&str>,
) -> Result<OutlookAccount, String> {
    let code =
        wait_for_authorization_code(login.listener, login.ipv6_listener, &login.state).await?;
    let response = client()
        .post(format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
            tenant_name(tenant)
        ))
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id.trim()),
            ("code", code.as_str()),
            ("redirect_uri", login.redirect_uri.as_str()),
            ("code_verifier", login.code_verifier.as_str()),
            (
                "scope",
                "openid profile offline_access https://graph.microsoft.com/Mail.Read",
            ),
        ])
        .send()
        .await
        .map_err(|error| format!("could not complete Microsoft sign-in: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("could not read Microsoft token response: {error}"))?;
    if !status.is_success() {
        return Err(format_oauth_error(&body, status.as_u16()));
    }
    let token: TokenResponse = serde_json::from_str(&body)
        .map_err(|error| format!("Microsoft returned an invalid token response: {error}"))?;
    save_token_store(&TokenStore {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at: Utc::now().timestamp() + token.expires_in.unwrap_or(3600) as i64,
    })?;
    // Mail.Read is the only Microsoft Graph permission JadX requests. Do not
    // call /me here: that endpoint requires the separate User.Read permission.
    // The optional hint is presentation-only; mailbox access always follows
    // the account that Microsoft actually authenticated in the access token.
    Ok(account_from_hint(tenant, account_hint))
}

async fn wait_for_authorization_code(
    listener: TcpListener,
    ipv6_listener: Option<TcpListener>,
    expected_state: &str,
) -> Result<String, String> {
    tokio::time::timeout(Duration::from_secs(600), async move {
        loop {
            let accepted = if let Some(ipv6_listener) = ipv6_listener.as_ref() {
                tokio::select! {
                    result = listener.accept() => result,
                    result = ipv6_listener.accept() => result,
                }
            } else {
                listener.accept().await
            };
            let (mut stream, _) = accepted
                .map_err(|error| format!("could not accept Microsoft sign-in callback: {error}"))?;
            let mut request = Vec::with_capacity(2048);
            loop {
                let mut chunk = [0_u8; 1024];
                let count = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk))
                    .await
                    .map_err(|_| "timed out reading Microsoft sign-in callback".to_string())?
                    .map_err(|error| {
                        format!("could not read Microsoft sign-in callback: {error}")
                    })?;
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..count]);
                if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    break;
                }
                if request.len() > 8192 {
                    reply_to_callback(&mut stream, false).await;
                    return Err(
                        "Microsoft sign-in callback exceeded the local request limit.".into(),
                    );
                }
            }
            let request = String::from_utf8_lossy(&request);
            let target = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/");
            let callback = url::Url::parse(&format!("http://localhost{target}"))
                .map_err(|error| format!("Microsoft returned an invalid callback URL: {error}"))?;
            let params = callback
                .query_pairs()
                .collect::<std::collections::HashMap<_, _>>();
            if let Some(error) = params.get("error") {
                let description = params
                    .get("error_description")
                    .map(|value| value.as_ref())
                    .unwrap_or("Microsoft sign-in was declined or could not be completed.");
                reply_to_callback(&mut stream, false).await;
                return Err(format!("Microsoft sign-in failed ({error}): {description}"));
            }
            if params.get("state").map(|value| value.as_ref()) != Some(expected_state) {
                reply_to_callback(&mut stream, false).await;
                return Err(
                    "Microsoft sign-in state did not match; start Outlook connection again.".into(),
                );
            }
            let Some(code) = params.get("code") else {
                reply_to_callback(&mut stream, false).await;
                continue;
            };
            let code = code.to_string();
            reply_to_callback(&mut stream, true).await;
            return Ok(code);
        }
    })
    .await
    .map_err(|_| {
        "Microsoft sign-in timed out after 10 minutes. Connect Outlook again.".to_string()
    })?
}

async fn reply_to_callback(stream: &mut tokio::net::TcpStream, success: bool) {
    let (status, title, message) = if success {
        (
            "200 OK",
            "JadX connected",
            "You can close this tab and return to JadX.",
        )
    } else {
        (
            "400 Bad Request",
            "Sign-in not completed",
            "Return to JadX and start Outlook connection again.",
        )
    };
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>{title}</title><main style=\"font:16px -apple-system;max-width:520px;margin:15vh auto;padding:24px;color:#202124\"><h1>{title}</h1><p>{message}</p></main>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
}

async fn refresh_access_token(
    store: &TokenStore,
    client_id: &str,
    tenant: Option<&str>,
) -> Result<String, String> {
    let Some(refresh_token) = &store.refresh_token else {
        return Err("Outlook session expired; reconnect the account.".into());
    };
    let response = client()
        .post(format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
            tenant_name(tenant)
        ))
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id.trim()),
            ("refresh_token", refresh_token.as_str()),
            (
                "scope",
                "openid profile offline_access https://graph.microsoft.com/Mail.Read",
            ),
        ])
        .send()
        .await
        .map_err(|error| format!("could not refresh Outlook access: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("could not read Outlook refresh response: {error}"))?;
    if !status.is_success() {
        return Err(format_oauth_error(&body, status.as_u16()));
    }
    let token: TokenResponse = serde_json::from_str(&body)
        .map_err(|error| format!("Microsoft returned an invalid refresh response: {error}"))?;
    save_token_store(&TokenStore {
        access_token: token.access_token.clone(),
        refresh_token: token.refresh_token.or_else(|| store.refresh_token.clone()),
        expires_at: Utc::now().timestamp() + token.expires_in.unwrap_or(3600) as i64,
    })?;
    Ok(token.access_token)
}

async fn access_token(client_id: &str, tenant: Option<&str>) -> Result<String, String> {
    let store = load_token_store().map_err(|_| "Connect an Outlook account first.".to_string())?;
    if store.expires_at > Utc::now().timestamp() + 90 {
        return Ok(store.access_token);
    }
    refresh_access_token(&store, client_id, tenant).await
}

pub fn account_from_hint(tenant: Option<&str>, account_hint: Option<&str>) -> OutlookAccount {
    OutlookAccount {
        id: String::new(),
        email: account_hint
            .map(str::trim)
            .filter(|hint| !hint.is_empty())
            .unwrap_or("Microsoft account")
            .to_string(),
        display_name: String::new(),
        tenant: tenant_name(tenant),
        connected_at: Utc::now().to_rfc3339(),
        listener_enabled: false,
    }
}

pub async fn wait_for_code(
    client_id: &str,
    tenant: Option<&str>,
    folder: Option<&str>,
    sender_hint: Option<&str>,
    subject_hint: Option<&str>,
    timeout_seconds: u64,
    cancel: CancellationToken,
) -> Result<DetectedEmailCode, String> {
    let folder = folder.unwrap_or(DEFAULT_FOLDER).trim();
    if folder.is_empty() || folder.contains('/') || folder.contains('\\') {
        return Err("Outlook folder must be a single folder identifier such as inbox.".into());
    }
    let sender_hint = sender_hint.map(|value| value.trim().to_ascii_lowercase());
    let subject_hint = subject_hint.map(|value| value.trim().to_ascii_lowercase());
    let since = (Utc::now() - ChronoDuration::seconds(120)).to_rfc3339();
    let deadline = Instant::now() + Duration::from_secs(timeout_seconds.clamp(15, 600));
    let mut seen = std::collections::HashSet::new();
    loop {
        let messages = list_recent_messages(client_id, tenant, folder, &since).await?;
        for message in messages {
            let id = message.id.clone().unwrap_or_default();
            if !id.is_empty() && !seen.insert(id) {
                continue;
            }
            let subject = message.subject.clone().unwrap_or_default();
            let sender = message
                .from
                .as_ref()
                .and_then(|value| value.email_address.as_ref())
                .and_then(|value| value.address.clone())
                .unwrap_or_default();
            let sender_text = sender.to_ascii_lowercase();
            let subject_text = subject.to_ascii_lowercase();
            if sender_hint
                .as_deref()
                .is_some_and(|hint| !hint.is_empty() && !sender_text.contains(hint))
                || subject_hint
                    .as_deref()
                    .is_some_and(|hint| !hint.is_empty() && !subject_text.contains(hint))
            {
                continue;
            }
            let mut content = message.body_preview.unwrap_or_default();
            if let Some(body) = message.body.and_then(|value| value.content) {
                content.push('\n');
                content.push_str(&strip_markup(&body));
            }
            if let Some(code) = extract_code(&format!("{subject}\n{content}")) {
                return Ok(DetectedEmailCode {
                    code,
                    sender,
                    subject,
                    received_at: message.received_date_time.unwrap_or_default(),
                });
            }
        }
        if Instant::now() >= deadline {
            return Err("No matching Outlook verification code arrived before the timeout.".into());
        }
        select! {
            _ = cancel.cancelled() => return Err("Outlook verification-code wait cancelled.".into()),
            _ = sleep(Duration::from_secs(DEFAULT_POLL_SECONDS)) => {}
        }
    }
}

async fn list_recent_messages(
    client_id: &str,
    tenant: Option<&str>,
    folder: &str,
    since: &str,
) -> Result<Vec<GraphMessage>, String> {
    let token = access_token(client_id, tenant).await?;
    let url = format!("{GRAPH_URL}/me/mailFolders/{folder}/messages");
    let response = client()
        .get(url)
        .query(&[
            (
                "$select",
                "id,subject,from,receivedDateTime,bodyPreview,body",
            ),
            ("$orderby", "receivedDateTime desc"),
            ("$top", &MAX_MESSAGES_PER_POLL.to_string()),
            ("$filter", &format!("receivedDateTime ge {since}")),
        ])
        .header("Prefer", "outlook.body-content-type=\"text\"")
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| format!("could not read Outlook messages: {error}"))?;
    let status = response.status();
    let body = response.text().await.map_err(|error| error.to_string())?;
    if !status.is_success() {
        return Err(format_graph_error(&body, status.as_u16()));
    }
    serde_json::from_str::<GraphMessageCollection>(&body)
        .map(|collection| collection.value)
        .map_err(|error| format!("Microsoft returned an invalid message list: {error}"))
}

pub fn masked_code(code: &str) -> String {
    if code.len() <= 2 {
        return "••".into();
    }
    format!(
        "{}{}",
        &code[..1],
        "•".repeat(code.chars().count().saturating_sub(2))
    ) + &code[code.len().saturating_sub(1)..]
}

pub fn validate_verification_code(raw: &str) -> Result<String, String> {
    let code = raw.trim();
    if !(4..=16).contains(&code.len())
        || !code
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Err("verification code must be 4–16 letters, numbers, or hyphens".into());
    }
    Ok(code.to_string())
}

pub fn status(
    account: Option<OutlookAccount>,
    listener_enabled: bool,
    folder: Option<&str>,
    token_present: bool,
) -> OutlookStatus {
    OutlookStatus {
        connected: account.is_some() && token_present,
        account,
        listener_enabled,
        folder: folder.unwrap_or(DEFAULT_FOLDER).to_string(),
        permission: "Mail.Read · delegated read-only".into(),
    }
}

fn extract_code(text: &str) -> Option<String> {
    let labeled = Regex::new(
        r"(?ix)(?:verification|confirmation|security|one[- ]time|otp|activation|passcode|code)\D{0,36}([A-Z0-9][A-Z0-9-]{3,15})",
    )
    .ok()?;
    for capture in labeled.captures_iter(text) {
        if let Some(value) = capture
            .get(1)
            .and_then(|value| validate_verification_code(value.as_str()).ok())
        {
            return Some(value);
        }
    }
    let digits = Regex::new(r"\b\d{4,8}\b").ok()?;
    let fallback = digits
        .find_iter(text)
        .find_map(|value| validate_verification_code(value.as_str()).ok());
    fallback
}

fn strip_markup(value: &str) -> String {
    Regex::new(r"(?is)<[^>]+>")
        .map(|regex| regex.replace_all(value, " ").into_owned())
        .unwrap_or_else(|_| value.to_string())
}

fn format_oauth_error(body: &str, status: u16) -> String {
    if let Ok(error) = serde_json::from_str::<OAuthError>(body) {
        if let Some(description) = error.error_description {
            return format!("Microsoft sign-in failed ({status}): {description}");
        }
        if let Some(code) = error.error {
            return format!("Microsoft sign-in failed ({status}): {code}");
        }
    }
    format!("Microsoft sign-in failed with HTTP {status}.")
}

fn format_graph_error(body: &str, status: u16) -> String {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| value["error"]["message"].as_str().map(str::to_string));
    detail
        .map(|message| format!("Microsoft Graph failed ({status}): {message}"))
        .unwrap_or_else(|| format!("Microsoft Graph failed with HTTP {status}."))
}

fn pkce_challenge(verifier: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest.finalize())
}

fn new_pkce_verifier() -> String {
    Uuid::new_v4().to_string().replace('-', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_labeled_and_fallback_codes() {
        assert_eq!(
            extract_code("Your verification code: 482913"),
            Some("482913".into())
        );
        assert_eq!(
            extract_code("Use 123456 to continue"),
            Some("123456".into())
        );
    }

    #[test]
    fn rejects_unbounded_or_unsafe_codes() {
        assert!(validate_verification_code("123").is_err());
        assert!(validate_verification_code("12345678901234567").is_err());
        assert!(validate_verification_code("1234 secret").is_err());
        assert_eq!(validate_verification_code("AB-1234").unwrap(), "AB-1234");
    }

    #[test]
    fn masks_codes_without_retaining_the_middle() {
        assert_eq!(masked_code("482913"), "4••••3");
    }

    #[test]
    fn pkce_is_url_safe() {
        let challenge = pkce_challenge("test-verifier");
        assert!(!challenge.contains('+'));
        assert!(!challenge.contains('/'));
        assert!(!challenge.contains('='));
    }

    #[test]
    fn oauth_callback_uri_is_stable_and_matches_desktop_registration() {
        assert_eq!(
            format!("http://localhost:{OAUTH_CALLBACK_PORT}"),
            "http://localhost:53682"
        );
    }

    #[test]
    fn account_display_uses_only_the_optional_hint_without_graph_profile_scope() {
        let hinted = account_from_hint(Some("common"), Some("  user@example.com  "));
        assert_eq!(hinted.email, "user@example.com");
        assert!(hinted.id.is_empty());
        let unknown = account_from_hint(None, None);
        assert_eq!(unknown.email, "Microsoft account");
    }
}
