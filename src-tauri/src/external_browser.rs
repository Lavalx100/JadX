//! Lightweight control of the user's existing Safari or Chrome session.
//!
//! JadX deliberately does not embed another browser process. The adapter uses
//! macOS automation for tabs and an opt-in, fixed script for narrowly scoped
//! visible form inspection/fill plus a distinct, one-shot, approval-gated
//! submission action. It never reads existing field values, cookies,
//! keystrokes, or downloads. User-supplied secrets must be exact; an explicitly
//! requested random password is generated locally after approval. Credentials
//! require normal approval and loopback inference.

use crate::browser;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

pub const MAX_EXTERNAL_TABS: usize = 32;
pub const MAX_FORM_FIELDS: usize = 20;
pub const MAX_FORM_VALUE_BYTES: usize = 1_000;
pub const MAX_FORM_VALUES_BYTES: usize = 8_000;
pub const MAX_EXTERNAL_HTML_BYTES: usize = 48 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExternalBrowser {
    Chrome,
    Safari,
}

impl ExternalBrowser {
    pub fn app_name(self) -> &'static str {
        match self {
            Self::Chrome => "Google Chrome",
            Self::Safari => "Safari",
        }
    }

    pub fn app_path(self) -> &'static str {
        match self {
            Self::Chrome => "/Applications/Google Chrome.app",
            Self::Safari => "/Applications/Safari.app",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "chrome" | "google chrome" => Ok(Self::Chrome),
            "safari" => Ok(Self::Safari),
            _ => Err("browser must be Chrome or Safari".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExternalBrowserTab {
    pub index: usize,
    pub title: String,
    pub url: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExternalBrowserStatus {
    pub browser: String,
    pub installed: bool,
    pub running: bool,
    pub active_index: Option<usize>,
    pub tabs: Vec<ExternalBrowserTab>,
    pub max_tabs: usize,
    pub credentials_stay_in_browser: bool,
    pub downloads_managed_by_browser: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserFormOption {
    pub label: String,
    pub value: String,
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserFormField {
    pub field_id: String,
    pub label: String,
    pub kind: String,
    pub name: String,
    pub placeholder: String,
    pub required: bool,
    pub is_empty: bool,
    #[serde(default)]
    pub checked: bool,
    #[serde(default)]
    pub options: Vec<BrowserFormOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserFormInspection {
    pub origin: String,
    pub title: String,
    pub fields: Vec<BrowserFormField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserHtmlSnapshot {
    pub origin: String,
    pub title: String,
    pub html: String,
    pub bytes_returned: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FormFieldFill {
    pub field_id: String,
    pub label: String,
    pub kind: String,
    pub name: String,
    pub placeholder: String,
    pub required: bool,
    pub checked: bool,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserFormFillResult {
    pub origin: String,
    pub filled_fields: Vec<String>,
    pub submitted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserFormSubmission {
    pub origin: String,
    pub submit_button: String,
    pub submitted: bool,
    #[serde(default)]
    pub result_url: Option<String>,
    #[serde(default)]
    pub location_changed: bool,
}

const APPLESCRIPT: &str = r#"
property fieldSeparator : (ASCII character 30)
property recordSeparator : (ASCII character 31)
property embeddedScript : "__JADX_JS_LITERAL__"

on textValue(value)
    try
        return value as text
    on error
        return ""
    end try
end textValue

on tabRecord(tabIndex, tabTitle, tabUrl, isActive)
    return (tabIndex as text) & fieldSeparator & textValue(tabTitle) & fieldSeparator & textValue(tabUrl) & fieldSeparator & (isActive as text) & recordSeparator
end tabRecord

on run argv
    set browserName to item 1 of argv
    set actionName to item 2 of argv
    set targetIndex to 0
    set targetUrl to ""
    if (count of argv) is greater than 2 then set targetIndex to (item 3 of argv) as integer
    if (count of argv) is greater than 3 then set targetUrl to item 4 of argv
    set injectedScript to ""
    set injectedScript to embeddedScript

    if browserName is "Google Chrome" then
        tell application "Google Chrome"
            if actionName is "list" then
                if not running then return "NOT_RUNNING"
                if (count of windows) is 0 then return "NO_WINDOWS"
                set tabCount to count of tabs of window 1
                if tabCount is greater than 32 then set tabCount to 32
                set activeIndex to active tab index of window 1
                set output to ""
                repeat with i from 1 to tabCount
                    set currentTab to tab i of window 1
                    set output to output & my tabRecord(i, title of currentTab, URL of currentTab, i is activeIndex)
                end repeat
                return output
            else if actionName is "open" then
                activate
                if (count of windows) is 0 then make new window
                set newTab to make new tab at end of tabs of window 1 with properties {URL:targetUrl}
                set active tab index of window 1 to (count of tabs of window 1)
                return "OPENED" & fieldSeparator & (count of tabs of window 1 as text)
            else if actionName is "navigate" then
                if (count of windows) is 0 then error "Google Chrome has no window"
                if targetIndex < 1 or targetIndex > (count of tabs of window 1) then error "Chrome tab index is out of range"
                set URL of tab targetIndex of window 1 to targetUrl
                set active tab index of window 1 to targetIndex
                activate
                return "NAVIGATED" & fieldSeparator & targetIndex
            else if actionName is "focus" then
                if (count of windows) is 0 then make new window
                if targetIndex > 0 and targetIndex ≤ (count of tabs of window 1) then set active tab index of window 1 to targetIndex
                activate
                return "FOCUSED"
            else if actionName is "close" then
                if (count of windows) is 0 then error "Google Chrome has no window"
                if targetIndex < 1 or targetIndex > (count of tabs of window 1) then error "Chrome tab index is out of range"
                close tab targetIndex of window 1
                return "CLOSED"
            else if actionName is "paste-code" then
                activate
                delay 0.1
                tell application "System Events" to keystroke targetUrl
                return "PASTED"
            else if actionName is "inspect-form" or actionName is "fill-form" or actionName is "submit-form" or actionName is "read-html" then
                if (count of windows) is 0 then error "Google Chrome has no window"
                if targetIndex < 1 or targetIndex > (count of tabs of window 1) then error "Chrome tab index is out of range"
                return execute tab targetIndex of window 1 javascript injectedScript
            end if
        end tell
    else if browserName is "Safari" then
        tell application "Safari"
            if actionName is "list" then
                if not running then return "NOT_RUNNING"
                if (count of windows) is 0 then return "NO_WINDOWS"
                set tabCount to count of tabs of window 1
                if tabCount is greater than 32 then set tabCount to 32
                set activeIndex to index of current tab of window 1
                set output to ""
                repeat with i from 1 to tabCount
                    set currentTab to tab i of window 1
                    set output to output & my tabRecord(i, name of currentTab, URL of currentTab, i is activeIndex)
                end repeat
                return output
            else if actionName is "open" then
                activate
                if (count of windows) is 0 then make new document
                set newTab to make new tab at end of tabs of window 1 with properties {URL:targetUrl}
                set current tab of window 1 to newTab
                return "OPENED" & fieldSeparator & (count of tabs of window 1 as text)
            else if actionName is "navigate" then
                if (count of windows) is 0 then error "Safari has no window"
                if targetIndex < 1 or targetIndex > (count of tabs of window 1) then error "Safari tab index is out of range"
                set URL of tab targetIndex of window 1 to targetUrl
                set current tab of window 1 to tab targetIndex of window 1
                activate
                return "NAVIGATED" & fieldSeparator & targetIndex
            else if actionName is "focus" then
                if (count of windows) is 0 then make new document
                if targetIndex > 0 and targetIndex ≤ (count of tabs of window 1) then set current tab of window 1 to tab targetIndex of window 1
                activate
                return "FOCUSED"
            else if actionName is "close" then
                if (count of windows) is 0 then error "Safari has no window"
                if targetIndex < 1 or targetIndex > (count of tabs of window 1) then error "Safari tab index is out of range"
                close tab targetIndex of window 1
                return "CLOSED"
            else if actionName is "paste-code" then
                activate
                delay 0.1
                tell application "System Events" to keystroke targetUrl
                return "PASTED"
            else if actionName is "inspect-form" or actionName is "fill-form" or actionName is "submit-form" or actionName is "read-html" then
                if (count of windows) is 0 then error "Safari has no window"
                if targetIndex < 1 or targetIndex > (count of tabs of window 1) then error "Safari tab index is out of range"
                return do JavaScript injectedScript in tab targetIndex of window 1
            end if
        end tell
    end if
    error "unsupported browser action"
end run
"#;

pub async fn status(browser: ExternalBrowser) -> Result<ExternalBrowserStatus, String> {
    let installed = Path::new(browser.app_path()).exists();
    if !installed {
        return Ok(ExternalBrowserStatus {
            browser: browser.app_name().into(),
            installed: false,
            running: false,
            max_tabs: MAX_EXTERNAL_TABS,
            credentials_stay_in_browser: true,
            downloads_managed_by_browser: true,
            ..Default::default()
        });
    }
    let output = run_script(browser, "list", None, None).await?;
    if output.trim() == "NOT_RUNNING" || output.trim() == "NO_WINDOWS" {
        return Ok(ExternalBrowserStatus {
            browser: browser.app_name().into(),
            installed,
            running: output.trim() == "NO_WINDOWS",
            max_tabs: MAX_EXTERNAL_TABS,
            credentials_stay_in_browser: true,
            downloads_managed_by_browser: true,
            ..Default::default()
        });
    }
    let tabs = parse_tabs(&output)?;
    Ok(ExternalBrowserStatus {
        browser: browser.app_name().into(),
        installed,
        running: true,
        active_index: tabs.iter().find(|tab| tab.active).map(|tab| tab.index),
        tabs,
        max_tabs: MAX_EXTERNAL_TABS,
        credentials_stay_in_browser: true,
        downloads_managed_by_browser: true,
    })
}

pub async fn open(browser: ExternalBrowser, raw_url: &str) -> Result<ExternalBrowserTab, String> {
    let url = browser::validate_url(raw_url).await?;
    let output = run_script(browser, "open", None, Some(url.as_str())).await?;
    let index = parse_result_index(&output).unwrap_or(1);
    let _ = focus(browser, index).await?;
    let state = status(browser).await?;
    state
        .tabs
        .into_iter()
        .find(|tab| tab.index == index || tab.active)
        .ok_or_else(|| "browser opened the tab but JadX could not read its tab state".into())
}

pub async fn navigate(
    browser: ExternalBrowser,
    index: usize,
    raw_url: &str,
) -> Result<ExternalBrowserTab, String> {
    let url = browser::validate_url(raw_url).await?;
    run_script(browser, "navigate", Some(index), Some(url.as_str())).await?;
    let state = status(browser).await?;
    state
        .tabs
        .into_iter()
        .find(|tab| tab.index == index)
        .ok_or_else(|| "browser navigated the tab but JadX could not read its tab state".into())
}

pub async fn focus(
    browser: ExternalBrowser,
    index: usize,
) -> Result<ExternalBrowserStatus, String> {
    run_script(browser, "focus", (index > 0).then_some(index), None).await?;
    status(browser).await
}

pub async fn close(
    browser: ExternalBrowser,
    index: usize,
) -> Result<ExternalBrowserStatus, String> {
    run_script(browser, "close", Some(index), None).await?;
    status(browser).await
}

/// Type a validated one-time verification code into the focused browser
/// field. The page is never clicked or submitted; this only runs after a
/// visible JadX approval.
pub async fn paste_code(browser: ExternalBrowser, code: &str) -> Result<(), String> {
    let code = crate::outlook::validate_verification_code(code)?;
    let output = run_script(browser, "paste-code", None, Some(&code)).await?;
    if output.trim() == "PASTED" {
        Ok(())
    } else {
        Err("macOS did not confirm the verification code was entered".into())
    }
}

/// Inspect only visible, enabled, non-sensitive form controls in one public
/// browser tab. Values already typed by the user are never returned.
pub async fn inspect_form(
    browser: ExternalBrowser,
    index: usize,
    expected_origin: &str,
) -> Result<BrowserFormInspection, String> {
    let origin = validate_public_origin(expected_origin).await?;
    ensure_tab_origin(browser, index, &origin).await?;
    let payload = json!({ "origin": origin });
    let script = form_script(&payload, FormScriptAction::Inspect)?;
    let raw = run_script_with_javascript(browser, "inspect-form", Some(index), None, Some(&script))
        .await?;
    let inspection: BrowserFormInspection = serde_json::from_str(&raw).map_err(|_| {
        "browser returned an invalid form inspection; refresh the page and retry".to_string()
    })?;
    if inspection.origin != origin {
        return Err(
            "the browser tab changed sites during form inspection; inspect it again".into(),
        );
    }
    if inspection.fields.len() > 30 {
        return Err("browser returned too many form fields".into());
    }
    Ok(inspection)
}

/// Read a bounded snapshot of the selected tab's live DOM markup. This is
/// deliberately read-only: the page is cloned, executable script bodies,
/// event-handler attributes, hidden inputs, and existing form values are
/// removed/redacted before serialization. The returned page content is still
/// untrusted and may contain private visible text, so the tool is approval-gated.
pub async fn read_html(
    browser: ExternalBrowser,
    index: usize,
    expected_origin: &str,
) -> Result<BrowserHtmlSnapshot, String> {
    let origin = validate_public_origin(expected_origin).await?;
    ensure_tab_origin(browser, index, &origin).await?;
    let payload = json!({ "origin": origin, "max_bytes": MAX_EXTERNAL_HTML_BYTES });
    let script = form_script(&payload, FormScriptAction::ReadHtml)?;
    let raw =
        run_script_with_javascript(browser, "read-html", Some(index), None, Some(&script)).await?;
    let response: Value = serde_json::from_str(&raw).map_err(|_| {
        "browser returned an invalid HTML snapshot; refresh the page and retry".to_string()
    })?;
    if let Some(error) = response.get("error").and_then(Value::as_str) {
        return Err(error.to_string());
    }
    let snapshot: BrowserHtmlSnapshot = serde_json::from_value(response)
        .map_err(|_| "browser returned an invalid HTML snapshot".to_string())?;
    if snapshot.origin != origin || snapshot.html.len() > MAX_EXTERNAL_HTML_BYTES {
        return Err("the tab changed origin or returned an oversized HTML snapshot".into());
    }
    Ok(snapshot)
}

/// Fill only explicitly named, currently-empty, visible and non-sensitive
/// controls. This never clicks buttons or submits a form.
pub async fn fill_form(
    browser: ExternalBrowser,
    index: usize,
    expected_origin: &str,
    fields: &[FormFieldFill],
) -> Result<BrowserFormFillResult, String> {
    validate_form_fill_fields(fields)?;
    let origin = validate_public_origin(expected_origin).await?;
    ensure_tab_origin(browser, index, &origin).await?;
    let payload = json!({ "origin": origin, "fields": fields });
    let script = form_script(&payload, FormScriptAction::Fill)?;
    let raw =
        run_script_with_javascript(browser, "fill-form", Some(index), None, Some(&script)).await?;
    let response: Value = serde_json::from_str(&raw).map_err(|_| {
        "browser returned an invalid form-fill result; no submission was attempted".to_string()
    })?;
    if let Some(error) = response.get("error").and_then(Value::as_str) {
        return Err(error.to_string());
    }
    let result: BrowserFormFillResult = serde_json::from_value(response)
        .map_err(|_| "browser returned an invalid form-fill result".to_string())?;
    if result.origin != origin || result.submitted || result.filled_fields.len() != fields.len() {
        return Err("the page changed while filling; review every field before continuing".into());
    }
    Ok(result)
}

/// Submit exactly one visible same-origin form after a separate explicit
/// JadX approval. The page must have one unambiguous visible submit button,
/// pass native browser validity checks, and contain no visible CAPTCHA or
/// payment controls. This does not inspect or return any field values.
pub async fn submit_form(
    browser: ExternalBrowser,
    index: usize,
    expected_origin: &str,
) -> Result<BrowserFormSubmission, String> {
    let origin = validate_public_origin(expected_origin).await?;
    ensure_tab_origin(browser, index, &origin).await?;
    let before = status(browser)
        .await?
        .tabs
        .into_iter()
        .find(|tab| tab.index == index)
        .ok_or_else(|| format!("{} tab {index} is not open", browser.app_name()))?;
    let payload = json!({ "origin": origin });
    let script = form_script(&payload, FormScriptAction::Submit)?;
    let raw = run_script_with_javascript(browser, "submit-form", Some(index), None, Some(&script))
        .await?;
    let response: Value = serde_json::from_str(&raw).map_err(|_| {
        "browser returned no verifiable form-submission result; inspect the page before retrying".to_string()
    })?;
    if let Some(error) = response.get("error").and_then(Value::as_str) {
        return Err(error.to_string());
    }
    let mut result: BrowserFormSubmission = serde_json::from_value(response)
        .map_err(|_| "browser returned an invalid form-submission result".to_string())?;
    if result.origin != origin || !result.submitted {
        return Err("the browser did not confirm the approved form submission".into());
    }
    // Submission can trigger an immediate navigation. Poll only this tab's
    // bounded metadata; never inspect or return its query values or page body.
    for _ in 0..16 {
        if let Ok(current) = status(browser).await {
            if let Some(tab) = current.tabs.into_iter().find(|tab| tab.index == index) {
                result.location_changed = tab.url != before.url || tab.title != before.title;
                result.result_url = sanitized_result_url(&tab.url);
                if result.location_changed {
                    break;
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    Ok(result)
}

fn sanitized_result_url(raw: &str) -> Option<String> {
    let mut url = url::Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let has_query = url.query().is_some();
    url.set_query(None);
    url.set_fragment(None);
    let mut result = url.to_string();
    if has_query {
        result.push_str("?[query redacted]");
    }
    Some(result)
}

async fn ensure_tab_origin(
    browser: ExternalBrowser,
    index: usize,
    expected_origin: &str,
) -> Result<(), String> {
    if index == 0 {
        return Err("browser tab index must be positive".into());
    }
    let status = status(browser).await?;
    let tab = status
        .tabs
        .iter()
        .find(|tab| tab.index == index)
        .ok_or_else(|| format!("{} tab {index} is not open", browser.app_name()))?;
    require_web_tab_url(&tab.url)?;
    let actual = browser::validate_url(&tab.url).await?;
    if actual.origin().ascii_serialization() != expected_origin {
        return Err(
            "the selected browser tab is not at the approved site; inspect it again".into(),
        );
    }
    Ok(())
}

fn require_web_tab_url(raw: &str) -> Result<(), String> {
    let url = url::Url::parse(raw)
        .map_err(|_| "the selected browser tab has no valid public web URL".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(
            "the selected browser tab is an internal or blank page, not a public website; navigate it to the approved HTTP(S) site and inspect again".into(),
        );
    }
    Ok(())
}

async fn validate_public_origin(raw: &str) -> Result<String, String> {
    let url = browser::validate_url(raw).await?;
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err("form tools accept only a site origin, not a path, query, or fragment".into());
    }
    let origin = url.origin().ascii_serialization();
    // Model-generated tool arguments commonly append the root slash. Accept
    // that harmless spelling while still rejecting any path, query, fragment,
    // or non-normalized origin.
    if !is_normalized_public_origin_spelling(raw, &origin) {
        return Err("form origin must be a normalized public HTTP(S) origin".into());
    }
    Ok(origin)
}

fn is_normalized_public_origin_spelling(raw: &str, origin: &str) -> bool {
    raw == origin || raw == format!("{origin}/")
}

fn javascript_automation_is_disabled(detail: &str) -> bool {
    [
        "JavaScript from Apple Events",
        "Executing JavaScript through AppleScript",
        "JavaScript Apple Events",
        "Access not allowed",
        "-1723",
    ]
    .iter()
    .any(|marker| detail.contains(marker))
}

pub fn validate_form_fill_fields(fields: &[FormFieldFill]) -> Result<(), String> {
    if fields.is_empty() || fields.len() > MAX_FORM_FIELDS {
        return Err(format!(
            "form fill must include between 1 and {MAX_FORM_FIELDS} fields"
        ));
    }
    let mut seen = std::collections::HashSet::new();
    let mut total_bytes = 0usize;
    for field in fields {
        let suffix = field.field_id.strip_prefix('f').unwrap_or_default();
        if suffix.is_empty()
            || suffix.len() > 6
            || !suffix.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err("field_id must be an ID returned by the latest form inspection".into());
        }
        if !seen.insert(field.field_id.as_str()) {
            return Err("form fill contains a duplicate field_id".into());
        }
        if !matches!(
            field.kind.as_str(),
            "text"
                | "email"
                | "tel"
                | "url"
                | "search"
                | "textarea"
                | "select"
                | "password"
                | "one-time-code"
                | "terms-checkbox"
        ) {
            return Err(format!("{} is not an allowed form field type", field.kind));
        }
        if field.label.trim().is_empty() || field.label.len() > 160 {
            return Err("sensitive or invalid form fields cannot be filled by JadX".into());
        }
        if field.name.len() > 80 || field.placeholder.len() > 120 {
            return Err("sensitive or invalid form fields cannot be filled by JadX".into());
        }
        let hints = format!("{} {} {}", field.label, field.name, field.placeholder);
        if is_always_forbidden_form_hint(&hints) {
            return Err("CAPTCHA, payment, and highly sensitive personal-data fields must be completed manually".into());
        }
        match field.kind.as_str() {
            "password" if !has_any_form_hint(&hints, &["password", "passcode"]) => {
                return Err(
                    "password fields must be explicitly identified by the inspected page".into(),
                );
            }
            "terms-checkbox"
                if !has_any_form_hint(
                    &hints,
                    &["terms", "conditions", "agreement", "eula", "license"],
                ) || field.value != "true" =>
            {
                return Err("terms checkboxes require explicit user acceptance".into());
            }
            _ => {}
        }
        if !matches!(
            field.kind.as_str(),
            "password" | "one-time-code" | "terms-checkbox"
        ) && is_sensitive_form_hint(&hints)
        {
            return Err("sensitive or legal fields require their dedicated approval flow".into());
        }
        if field.value.trim().is_empty()
            || field.value.as_bytes().len() > MAX_FORM_VALUE_BYTES
            || field.value.contains('\0')
        {
            return Err("form values must be non-empty and at most 1000 bytes".into());
        }
        if field.kind != "textarea" && (field.value.contains('\n') || field.value.contains('\r')) {
            return Err("multi-line values are allowed only for textarea fields".into());
        }
        if field.kind == "terms-checkbox" && field.checked {
            return Err("already-accepted terms are never modified".into());
        }
        total_bytes += field.value.len();
        if total_bytes > MAX_FORM_VALUES_BYTES {
            return Err("combined form values exceed the 8000-byte safety limit".into());
        }
    }
    Ok(())
}

fn is_sensitive_form_hint(value: &str) -> bool {
    let lower = value
        .to_ascii_lowercase()
        .replace('-', " ")
        .replace('_', " ");
    [
        "password",
        "passcode",
        "one time",
        "otp",
        "verification",
        "captcha",
        "credit card",
        "card number",
        "cvv",
        "cvc",
        "security code",
        "recovery code",
        "secret",
        "api key",
        "access token",
        "payment",
        "billing",
        "bank",
        "routing",
        "iban",
        "swift",
        "social security",
        "ssn",
        "birth date",
        "birthday",
        "gender",
        "ethnicity",
        "religion",
        "medical",
        "health",
        "disability",
        "terms",
        "consent",
        "agreement",
        "accept terms",
        "cookie",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn is_always_forbidden_form_hint(value: &str) -> bool {
    let lower = value
        .to_ascii_lowercase()
        .replace('-', " ")
        .replace('_', " ");
    [
        "captcha",
        "security code",
        "credit card",
        "card number",
        "cvv",
        "cvc",
        "payment",
        "billing",
        "bank",
        "routing",
        "iban",
        "swift",
        "social security",
        "ssn",
        "birth date",
        "birthday",
        "gender",
        "ethnicity",
        "religion",
        "medical",
        "health",
        "disability",
        "cookie",
        "marketing",
        "newsletter",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn has_any_form_hint(value: &str, needles: &[&str]) -> bool {
    let lower = value
        .to_ascii_lowercase()
        .replace('-', " ")
        .replace('_', " ");
    needles.iter().any(|needle| lower.contains(needle))
}

#[derive(Clone, Copy)]
enum FormScriptAction {
    Inspect,
    Fill,
    Submit,
    ReadHtml,
}

fn form_script(payload: &Value, action: FormScriptAction) -> Result<String, String> {
    let encoded = BASE64.encode(
        serde_json::to_vec(payload)
            .map_err(|error| format!("could not prepare form request: {error}"))?,
    );
    let mut script =
        String::from("(()=>{const p=JSON.parse(new TextDecoder().decode(Uint8Array.from(atob('");
    script.push_str(&encoded);
    script.push_str("'),c=>c.charCodeAt(0))));");
    script.push_str(FORM_FIELD_HELPERS);
    match action {
        FormScriptAction::Inspect => script.push_str(FORM_INSPECT_BODY),
        FormScriptAction::Fill => script.push_str(FORM_FILL_BODY),
        FormScriptAction::Submit => script.push_str(FORM_SUBMIT_BODY),
        FormScriptAction::ReadHtml => script.push_str(HTML_SNAPSHOT_BODY),
    }
    script.push_str("})()");
    Ok(script)
}

const FORM_FIELD_HELPERS: &str = r#"
const risky=/(password|passcode|one.?time|\botp\b|verification|captcha|credit.?card|card.?number|\bcvv\b|\bcvc\b|security.?code|recovery.?code|secret|api.?key|access.?token|payment|billing|bank|routing|iban|swift|social.?security|\bssn\b|birth.?date|birthday|gender|ethnicity|religion|medical|health|disability|terms|consent|agreement|accept.?terms|cookie)/i;
const otpHint=/(one.?time|verification.?code|confirm.?code|\botp\b)/i;
const legalHint=/(terms|conditions|agreement|\beula\b|license)/i;
const blockedConsent=/(cookie|marketing|newsletter|subscribe)/i;
const hardBlocked=/(captcha|security.?code|credit.?card|card.?number|\bcvv\b|\bcvc\b|payment|billing|bank|routing|iban|swift|social.?security|\bssn\b|birth.?date|birthday|gender|ethnicity|religion|medical|health|disability|recovery.?code|api.?key|access.?token|secret)/i;
const short=(v,n=160)=>String(v||'').replace(/\s+/g,' ').trim().slice(0,n);
const visible=e=>{const r=e.getBoundingClientRect();if(r.width<=0||r.height<=0||e.matches(':disabled')||e.readOnly)return false;for(let n=e;n&&n!==document.documentElement;n=n.parentElement){const s=getComputedStyle(n);if(s.display==='none'||s.visibility==='hidden'||s.opacity==='0')return false}return true};
const labelOf=e=>{const labels=[...(e.labels||[])].map(x=>x.textContent||'').join(' ');const labelled=(e.getAttribute('aria-labelledby')||'').split(/\s+/).map(id=>document.getElementById(id)?.textContent||'').join(' ');return short(labels||labelled||e.getAttribute('aria-label')||e.placeholder||e.name||e.id)};
const controls=()=>[...document.querySelectorAll('input,textarea,select')].slice(0,500);
const rawKind=e=>e.tagName==='TEXTAREA'?'textarea':e.tagName==='SELECT'?'select':String(e.type||'text').toLowerCase();
const kindOf=e=>{const raw=rawKind(e),hints=[labelOf(e),e.name,e.id,e.placeholder,e.autocomplete].join(' ');if(raw==='password')return 'password';if(raw==='checkbox'&&legalHint.test(hints)&&!blockedConsent.test(hints))return 'terms-checkbox';if(['text','tel','number'].includes(raw)&&otpHint.test(hints))return 'one-time-code';return raw};
const safeField=e=>{const kind=kindOf(e);if(!visible(e))return false;const hints=[labelOf(e),e.name,e.id,e.placeholder,String(e.autocomplete||'')].join(' ');if(hardBlocked.test(hints))return false;if(kind==='password')return e.tagName==='INPUT';if(kind==='one-time-code')return !/(cookie|marketing|newsletter|subscribe)/i.test(hints);if(kind==='terms-checkbox')return !blockedConsent.test(hints);if(!['text','email','tel','url','search','textarea','select'].includes(kind))return false;return !risky.test(hints)&&!/(current-password|new-password|one-time-code|cc-|webauthn|transaction-amount|bday|sex)/i.test(String(e.autocomplete||''))};
const describe=(e,i)=>{const kind=kindOf(e),checked=kind==='terms-checkbox'?!!e.checked:false;return {field_id:'f'+i,label:labelOf(e)||('Field '+(i+1)),kind,name:short(e.name,80),placeholder:short(e.placeholder,120),required:!!e.required,checked,is_empty:kind==='terms-checkbox'?!checked:String(e.value||'').trim()==='',options:e.tagName==='SELECT'?[...e.options].slice(0,50).map(o=>({label:short(o.label||o.textContent,120),value:short(o.value,120),disabled:!!o.disabled})):[]}};
"#;

const FORM_INSPECT_BODY: &str = r#"
if(location.origin!==p.origin)return JSON.stringify({error:'The selected tab changed sites. Inspect it again.'});
const fields=controls().map((e,i)=>safeField(e)?describe(e,i):null).filter(Boolean).slice(0,30);
return JSON.stringify({origin:location.origin,title:short(document.title,160),fields});
"#;

const HTML_SNAPSHOT_BODY: &str = r#"
if(location.origin!==p.origin)return JSON.stringify({error:'The selected tab changed sites. Inspect it again.'});
if(!document.documentElement)return JSON.stringify({error:'The selected page has no readable HTML document.'});
const sensitive=/(password|passcode|one.?time|verification|captcha|secret|token|authorization|cookie|session|csrf|nonce|api.?key|access.?key|credential|recovery|security.?code|otp)/i;
const encoder=new TextEncoder(),limit=Math.max(1024,Math.min(Number(p.max_bytes)||0,49152)),maxNodes=20000,chunks=[];
let bytes_returned=0,truncated=false,visited=0;
const add=s=>{if(truncated||!s)return;for(const ch of s){const n=encoder.encode(ch).length;if(bytes_returned+n>limit){truncated=true;return}chunks.push(ch);bytes_returned+=n}};
const addEscaped=(s,attribute=false)=>{for(const ch of String(s)){const escaped=ch==='&'?'&amp;':ch==='<'?'&lt;':ch==='>'?'&gt;':attribute&&ch==='"'?'&quot;':ch;add(escaped);if(truncated)return}};
const voidTags=new Set(['area','base','br','col','embed','hr','img','input','link','meta','param','source','track','wbr']);
const walker=document.createTreeWalker(document.documentElement,NodeFilter.SHOW_ELEMENT|NodeFilter.SHOW_TEXT,{acceptNode:n=>n.nodeType===1&&((n.tagName==='SCRIPT')||(n.tagName==='INPUT'&&String(n.getAttribute('type')||'').toLowerCase()==='hidden'))?NodeFilter.FILTER_REJECT:NodeFilter.FILTER_ACCEPT});
const open=[];add('<!doctype html>');
const emitStart=node=>{const tag=node.tagName.toLowerCase();add('<'+tag);for(const attr of [...node.attributes]){const name=attr.name.toLowerCase();let value=attr.value;if(name.startsWith('on')||['value','checked','selected'].includes(name))continue;if(sensitive.test(name)||sensitive.test(value))value='[redacted]';if(['href','src','action','formaction','cite','xlink:href'].includes(name)){try{const url=new URL(value,location.href);if(!['http:','https:'].includes(url.protocol))continue;url.search='';url.hash='';value=url.href}catch{continue}}add(' '+name+'="');addEscaped(value,true);add('"');if(truncated)break}if(!truncated){add('>');if(!voidTags.has(tag))open.push(node)}};
emitStart(document.documentElement);
let node;
while(!truncated&&visited<maxNodes&&(node=walker.nextNode())){
 visited++;
 const parent=node.parentElement;
 while(open.length&&open[open.length-1]!==parent){const closed=open.pop();add('</'+closed.tagName.toLowerCase()+'>');if(truncated)break}
 if(truncated)break;
 if(node.nodeType===3){
  if(parent&&parent.tagName==='TEXTAREA')add('[existing editable content redacted]');
  else if(parent&&parent.isContentEditable)add('');
  else addEscaped(node.nodeValue||'');
  continue;
 }
 emitStart(node);
}
if(!truncated&&visited>=maxNodes&&walker.nextNode())truncated=true;
while(!truncated&&open.length){const closed=open.pop();add('</'+closed.tagName.toLowerCase()+'>')}
const html=chunks.join(''),title=String(document.title||'');
return JSON.stringify({origin:location.origin,title:sensitive.test(title)?'[redacted sensitive title]':title.slice(0,160),html,bytes_returned,truncated});
"#;

const FORM_FILL_BODY: &str = r#"
if(location.origin!==p.origin)return JSON.stringify({error:'The selected tab changed sites. Inspect it again.'});
if(!Array.isArray(p.fields)||p.fields.length<1||p.fields.length>20)return JSON.stringify({error:'Invalid field list.'});
const all=controls(),prepared=[],seen=new Set();
for(const f of p.fields){
 if(!f||typeof f.field_id!=='string'||!/^f\d+$/.test(f.field_id)||seen.has(f.field_id))return JSON.stringify({error:'Invalid or repeated field ID.'});
 seen.add(f.field_id);
 const i=Number(f.field_id.slice(1)),e=all[i];
 if(!e||!safeField(e))return JSON.stringify({error:'A requested field is no longer visible or allowed.'});
 const d=describe(e,i);
 if(d.label!==f.label||d.kind!==f.kind||d.name!==f.name||d.placeholder!==f.placeholder||d.required!==f.required||d.checked!==f.checked||!d.is_empty)return JSON.stringify({error:'A field changed or is no longer empty. Review the form again.'});
 if(typeof f.value!=='string'||!f.value.trim()||new TextEncoder().encode(f.value).length>1000||f.value.includes('\0'))return JSON.stringify({error:'A proposed value is invalid.'});
 if(f.kind==='terms-checkbox'&&(e.type!=='checkbox'||f.value!=='true'||e.checked))return JSON.stringify({error:'Terms require explicit review and must still be unchecked.'});
 if(f.kind==='password'&&(e.type!=='password'||f.value.length<1))return JSON.stringify({error:'Password target is no longer valid.'});
 if(f.kind==='one-time-code'&&!otpHint.test([labelOf(e),e.name,e.id,e.placeholder,e.autocomplete].join(' ')))return JSON.stringify({error:'Verification-code target is no longer valid.'});
 if(e.tagName==='SELECT'&&!([...e.options].some(o=>!o.disabled&&o.value===f.value)))return JSON.stringify({error:'A selected option is no longer available.'});
 prepared.push({e,f,previous:String(e.value||''),previousChecked:!!e.checked});
}
const setValue=(item,v)=>{const e=item.e;if(item.f.kind==='terms-checkbox'){e.checked=v==='true';}else{const proto=e.tagName==='TEXTAREA'?HTMLTextAreaElement.prototype:e.tagName==='SELECT'?HTMLSelectElement.prototype:HTMLInputElement.prototype;const setter=Object.getOwnPropertyDescriptor(proto,'value')?.set;if(!setter)throw new Error('The browser field cannot be safely updated.');setter.call(e,v)}e.dispatchEvent(new Event('input',{bubbles:true}));e.dispatchEvent(new Event('change',{bubbles:true}));};
try{for(const item of prepared)setValue(item,item.f.value);if(prepared.some(item=>item.f.kind==='terms-checkbox'?!item.e.checked:String(item.e.value)!==item.f.value))throw new Error('The page changed a field while JadX was filling it.');}catch(error){for(const item of prepared){try{if(item.f.kind==='terms-checkbox'){item.e.checked=item.previousChecked;item.e.dispatchEvent(new Event('input',{bubbles:true}));item.e.dispatchEvent(new Event('change',{bubbles:true}));}else setValue(item,item.previous)}catch{}}return JSON.stringify({error:'Form fill did not complete cleanly; review the visible fields before continuing.'});}
return JSON.stringify({origin:location.origin,filled_fields:prepared.map(item=>item.f.label),submitted:false});
"#;

const FORM_SUBMIT_BODY: &str = r#"
if(location.origin!==p.origin)return JSON.stringify({error:'The selected tab changed sites. Review it again.'});
const isVisible=e=>{const r=e.getBoundingClientRect();if(r.width<=0||r.height<=0||e.matches(':disabled'))return false;for(let n=e;n&&n!==document.documentElement;n=n.parentElement){const s=getComputedStyle(n);if(s.display==='none'||s.visibility==='hidden'||s.opacity==='0')return false}return true};
const forms=[...document.forms].filter(isVisible);
if(forms.length!==1)return JSON.stringify({error:'Submission paused: JadX requires exactly one visible form on the page.'});
const form=forms[0],target=new URL(form.getAttribute('action')||location.href,location.href);
if(target.origin!==p.origin)return JSON.stringify({error:'Submission paused: the form destination differs from the approved site.'});
if(form.target&&form.target!=='_self')return JSON.stringify({error:'Submission paused: the form opens a separate browsing context.'});
const captcha=form.querySelector('iframe[src*="captcha" i],iframe[title*="captcha" i],[id*="captcha" i],[name*="captcha" i],[class*="captcha" i]');
if(captcha)return JSON.stringify({error:'Submission paused: this form contains CAPTCHA controls. Complete and submit it yourself in the browser.'});
const riskyControls=[...form.querySelectorAll('input,textarea,select')].filter(isVisible).filter(e=>hardBlocked.test([labelOf(e),e.name,e.id,e.placeholder,e.autocomplete].join(' ')));
if(riskyControls.length)return JSON.stringify({error:'Submission paused: visible payment, recovery, CAPTCHA, or highly sensitive fields must remain user-controlled.'});
const legalAcceptance=[...form.querySelectorAll('input[type="checkbox"]')].filter(isVisible).some(e=>e.checked&&legalHint.test([labelOf(e),e.name,e.id,e.value].join(' ')));
if(legalAcceptance)return JSON.stringify({error:'Submission paused: JadX cannot submit a form that accepts terms or another legal agreement.'});
const chosenFiles=[...form.querySelectorAll('input[type="file"]')].some(e=>e.files&&e.files.length>0);
if(chosenFiles)return JSON.stringify({error:'Submission paused: a file is attached. Review and submit it yourself in the browser.'});
const buttons=[...form.querySelectorAll('button,input[type="submit"]')].filter(isVisible).filter(e=>String(e.type||'submit').toLowerCase()==='submit');
if(buttons.length!==1)return JSON.stringify({error:'Submission paused: JadX requires exactly one visible submit button; choose the action yourself in the browser.'});
if(!form.checkValidity())return JSON.stringify({error:'Submission paused: the form has missing or invalid required fields. Review the form in the browser.'});
if(form.dataset.jadxSubmissionAttempted==='1')return JSON.stringify({error:'This form submission was already attempted in the current page. Inspect the result; JadX will not submit it twice.'});
const button=buttons[0],buttonLabel=short(button.innerText||button.value||button.getAttribute('aria-label')||'Submit',80);
form.dataset.jadxSubmissionAttempted='1';try{form.requestSubmit(button)}catch{delete form.dataset.jadxSubmissionAttempted;return JSON.stringify({error:'Browser did not accept the reviewed form submission.'})}
return JSON.stringify({origin:location.origin,submit_button:buttonLabel,submitted:true});
"#;

async fn run_script(
    browser: ExternalBrowser,
    action: &str,
    index: Option<usize>,
    url: Option<&str>,
) -> Result<String, String> {
    run_script_with_javascript(browser, action, index, url, None).await
}

async fn run_script_with_javascript(
    browser: ExternalBrowser,
    action: &str,
    index: Option<usize>,
    url: Option<&str>,
    javascript: Option<&str>,
) -> Result<String, String> {
    if !Path::new(browser.app_path()).exists() {
        return Err(format!("{} is not installed", browser.app_name()));
    }
    let script_source = javascript
        .unwrap_or_default()
        .replace('\n', " ")
        .replace('\r', " ");
    // JavaScript is fixed JadX code, and caller data is JSON encoded inside it.
    // It travels to osascript over stdin (never argv) and is escaped as one
    // AppleScript string so page values cannot alter the automation script.
    let escaped_script = script_source.replace('\\', "\\\\").replace('"', "\\\"");
    let apple_script = APPLESCRIPT.replace("__JADX_JS_LITERAL__", &escaped_script);
    let mut command = Command::new("/usr/bin/osascript");
    command
        .arg("-")
        .arg(browser.app_name())
        .arg(action)
        .arg(index.unwrap_or(0).to_string())
        .arg(url.unwrap_or_default())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Apple Events can hang indefinitely when automation permission is
        // missing or a browser script stops responding. Never leave an
        // approved JadX tool marked as running forever.
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start macOS browser automation: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(apple_script.as_bytes())
            .await
            .map_err(|error| format!("could not send browser automation request: {error}"))?;
    }
    let output = timeout(Duration::from_secs(30), child.wait_with_output())
        .await
        .map_err(|_| {
            format!(
                "{} did not answer the approved browser action within 30 seconds. The automation process was stopped; check browser automation permission and retry.",
                browser.app_name()
            )
        })?
        .map_err(|error| format!("browser automation failed: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if detail.contains("Not authorized") || detail.contains("not authorized") {
            "macOS denied browser automation. Enable JadX under System Settings → Privacy & Security → Automation, then retry.".into()
        } else if javascript_automation_is_disabled(&detail) {
            format!(
                "{} blocks form assistance until you enable ‘Allow JavaScript from Apple Events’ yourself. In Chrome, use View → Developer → Allow JavaScript from Apple Events. In Safari, enable the Develop menu in Safari Settings → Advanced, then choose Develop → Allow JavaScript from Apple Events. JadX does not change this browser security setting.",
                browser.app_name()
            )
        } else if action == "submit-form" {
            "JadX could not verify the form submission. Inspect the browser page before retrying; the action may already have been sent.".into()
        } else if matches!(action, "fill-form" | "inspect-form" | "read-html") {
            // These scripts may contain user-entered secrets (base64 encoded
            // in the payload). Never surface osascript stderr for form tasks.
            "Browser inspection failed. Review the page and retry; no page action or form submission was attempted."
                .into()
        } else if detail.is_empty() {
            "macOS browser automation failed".into()
        } else {
            detail
        };
        return Err(detail);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn parse_tabs(raw: &str) -> Result<Vec<ExternalBrowserTab>, String> {
    let mut tabs = Vec::new();
    for record in raw.split('\u{1f}').filter(|record| !record.is_empty()) {
        let fields = record.split('\u{1e}').collect::<Vec<_>>();
        if fields.len() != 4 {
            return Err("macOS returned an invalid browser tab record".into());
        }
        let index = fields[0]
            .parse::<usize>()
            .map_err(|_| "macOS returned an invalid browser tab index".to_string())?;
        tabs.push(ExternalBrowserTab {
            index,
            title: fields[1].to_string(),
            url: fields[2].to_string(),
            active: fields[3].eq_ignore_ascii_case("true"),
        });
    }
    Ok(tabs)
}

fn parse_result_index(raw: &str) -> Option<usize> {
    raw.split('\u{1e}').nth(1)?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "live browser smoke test; opens, fills, and submits Selenium's official demo exactly once"]
    async fn live_selenium_demo_form_fill_and_submit_once() {
        let browser = ExternalBrowser::Chrome;
        let tab = open(
            browser,
            "https://www.selenium.dev/selenium/web/web-form.html",
        )
        .await
        .expect("open Selenium's public WebDriver demo form");
        let result = async {
            let mut loaded = false;
            for _ in 0..20 {
                let status = status(browser).await?;
                if status.tabs.iter().any(|candidate| {
                    candidate.index == tab.index
                        && candidate.url.contains("/selenium/web/web-form.html")
                        && candidate.title == "Web form"
                }) {
                    loaded = true;
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            assert!(loaded, "Selenium demo page should finish loading");
            let form = inspect_form(browser, tab.index, "https://www.selenium.dev")
                .await
                .map_err(|error| format!("form inspection failed: {error}"))?;
            assert_eq!(form.origin, "https://www.selenium.dev");
            let text_field = form
                .fields
                .iter()
                .find(|field| field.name == "my-text")
                .expect("demo text input is visible");
            assert_eq!(text_field.kind, "text");
            assert!(text_field.is_empty, "only use the blank demo input");
            let fill = FormFieldFill {
                field_id: text_field.field_id.clone(),
                label: text_field.label.clone(),
                kind: text_field.kind.clone(),
                name: text_field.name.clone(),
                placeholder: text_field.placeholder.clone(),
                required: text_field.required,
                checked: text_field.checked,
                value: "JadX demo test".into(),
            };
            let filled = fill_form(browser, tab.index, "https://www.selenium.dev", &[fill])
                .await
                .map_err(|error| format!("form fill failed: {error}"))?;
            assert_eq!(filled.filled_fields.len(), 1);
            assert!(!filled.submitted, "filling itself must not submit");
            let submitted = submit_form(browser, tab.index, "https://www.selenium.dev").await?;
            assert!(
                submitted.submitted,
                "the single approved submit should be sent"
            );
            assert!(
                submitted.location_changed,
                "the demo should navigate after submit"
            );
            assert!(submitted
                .result_url
                .as_deref()
                .is_some_and(|url| url.contains("/selenium/web/submitted-form.html")
                    && url.contains("[query redacted]")));
            let mut confirmed = false;
            for _ in 0..20 {
                let status = status(browser).await?;
                if status.tabs.iter().any(|candidate| {
                    candidate.index == tab.index
                        && candidate.url.contains("/selenium/web/submitted-form.html")
                        && candidate.url.contains("my-text=JadX+demo+test")
                }) {
                    confirmed = true;
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            assert!(
                confirmed,
                "Selenium should show its documented submitted-form result"
            );
            Ok::<(), String>(())
        }
        .await;
        let _ = close(browser, tab.index).await;
        result
            .expect("inspect, fill one dummy text value, submit once, and confirm the result page");
    }

    #[test]
    fn parses_bounded_tab_records() {
        let raw = format!("1\u{1e}Login\u{1e}https://example.com\u{1e}true\u{1f}");
        let tabs = parse_tabs(&raw).expect("valid tab record");
        assert_eq!(tabs[0].index, 1);
        assert!(tabs[0].active);
        assert_eq!(tabs[0].url, "https://example.com");
    }

    #[test]
    fn rejects_malformed_tab_records() {
        assert!(parse_tabs("1\u{1e}only-two-fields").is_err());
    }

    #[test]
    fn accepts_only_supported_browsers() {
        assert_eq!(
            ExternalBrowser::parse("chrome").unwrap(),
            ExternalBrowser::Chrome
        );
        assert_eq!(
            ExternalBrowser::parse("Safari").unwrap(),
            ExternalBrowser::Safari
        );
        assert!(ExternalBrowser::parse("Firefox").is_err());
    }

    #[test]
    fn normalized_origin_accepts_optional_root_slash_but_not_paths() {
        assert!(is_normalized_public_origin_spelling(
            "https://www.selenium.dev",
            "https://www.selenium.dev"
        ));
        assert!(is_normalized_public_origin_spelling(
            "https://www.selenium.dev/",
            "https://www.selenium.dev"
        ));
        assert!(!is_normalized_public_origin_spelling(
            "https://www.selenium.dev/selenium/web/web-form.html",
            "https://www.selenium.dev"
        ));
    }

    #[test]
    fn internal_tabs_get_a_recoverable_web_navigation_error() {
        assert!(require_web_tab_url("chrome://newtab")
            .unwrap_err()
            .contains("navigate it to the approved HTTP(S) site"));
        assert!(require_web_tab_url("about:blank").is_err());
        assert!(require_web_tab_url("https://www.selenium.dev/").is_ok());
    }

    #[test]
    fn submission_result_url_redacts_query_and_fragment_values() {
        let result = sanitized_result_url(
            "https://www.selenium.dev/selenium/web/submitted-form.html?my-text=private-value#receipt",
        )
        .unwrap();
        assert_eq!(
            result,
            "https://www.selenium.dev/selenium/web/submitted-form.html?[query redacted]"
        );
        assert!(!result.contains("private-value"));
        assert!(sanitized_result_url("chrome://newtab").is_none());
    }

    #[test]
    fn chrome_javascript_setting_error_is_recognized_without_exposing_script_details() {
        assert!(javascript_automation_is_disabled(
            "Can’t get value in tab. Access not allowed. (-1723)"
        ));
        assert!(javascript_automation_is_disabled(
            "JavaScript from Apple Events is disabled"
        ));
        assert!(!javascript_automation_is_disabled(
            "Not authorized to send Apple events"
        ));
    }

    #[test]
    fn form_fill_accepts_only_bounded_standard_fields() {
        let valid = FormFieldFill {
            field_id: "f2".into(),
            label: "Email address".into(),
            kind: "email".into(),
            name: "email".into(),
            placeholder: "name@example.com".into(),
            required: true,
            checked: false,
            value: "user@example.com".into(),
        };
        assert!(validate_form_fill_fields(std::slice::from_ref(&valid)).is_ok());

        let mut password = valid.clone();
        password.label = "Account password".into();
        password.kind = "password".into();
        password.name = "password".into();
        password.placeholder = "Create password".into();
        password.value = "user-provided-test-password".into();
        assert!(validate_form_fill_fields(&[password.clone()]).is_ok());

        let mut blank_password = password;
        blank_password.value.clear();
        assert!(validate_form_fill_fields(&[blank_password]).is_err());

        let mut otp = valid.clone();
        otp.label = "Verification code".into();
        otp.kind = "one-time-code".into();
        otp.name = "verification_code".into();
        otp.placeholder = "Enter code".into();
        otp.value = "492105".into();
        assert!(validate_form_fill_fields(&[otp]).is_ok());

        let mut terms = valid.clone();
        terms.label = "I agree to the Terms and Conditions".into();
        terms.kind = "terms-checkbox".into();
        terms.name = "terms".into();
        terms.placeholder.clear();
        terms.value = "true".into();
        assert!(validate_form_fill_fields(&[terms]).is_ok());

        let mut captcha = valid.clone();
        captcha.label = "CAPTCHA verification".into();
        captcha.name = "captcha".into();
        assert!(validate_form_fill_fields(&[captcha]).is_err());

        let mut submit = valid.clone();
        submit.kind = "submit".into();
        assert!(validate_form_fill_fields(&[submit]).is_err());

        let mut multi_line = valid.clone();
        multi_line.value = "one\ntwo".into();
        assert!(validate_form_fill_fields(&[multi_line]).is_err());

        let duplicate = valid.clone();
        assert!(validate_form_fill_fields(&[valid.clone(), duplicate]).is_err());
    }

    #[test]
    fn form_fill_limits_field_count_and_value_size() {
        let too_many = (0..=MAX_FORM_FIELDS)
            .map(|index| FormFieldFill {
                field_id: format!("f{index}"),
                label: format!("Profile field {index}"),
                kind: "text".into(),
                name: format!("field{index}"),
                placeholder: String::new(),
                required: false,
                checked: false,
                value: "value".into(),
            })
            .collect::<Vec<_>>();
        assert!(validate_form_fill_fields(&too_many).is_err());

        let too_large = FormFieldFill {
            field_id: "f0".into(),
            label: "Display name".into(),
            kind: "text".into(),
            name: "display_name".into(),
            placeholder: String::new(),
            required: false,
            checked: false,
            value: "x".repeat(MAX_FORM_VALUE_BYTES + 1),
        };
        assert!(validate_form_fill_fields(&[too_large]).is_err());
    }

    #[test]
    fn generated_form_scripts_keep_payload_encoded_and_never_submit() {
        let payload = json!({
            "origin": "https://example.com",
            "fields": [{"field_id":"f0","label":"Display name","kind":"text","name":"display_name","placeholder":"","required":false,"value":"User\"x"}]
        });
        let inspect = form_script(
            &json!({"origin":"https://example.com"}),
            FormScriptAction::Inspect,
        )
        .unwrap();
        let fill = form_script(&payload, FormScriptAction::Fill).unwrap();
        assert!(inspect.contains("location.origin!==p.origin"));
        assert!(fill.contains("location.origin!==p.origin"));
        assert!(fill.contains("d.is_empty"));
        assert!(fill.contains("filled_fields"));
        assert!(FORM_FIELD_HELPERS.contains("if(hardBlocked.test(hints))return false"));
        assert!(FORM_FIELD_HELPERS.contains("captcha"));
        assert!(!fill.contains("User\"x"));
        assert!(!fill.contains("submit()"));
        assert!(!fill.contains("requestSubmit"));
    }

    #[test]
    fn submit_script_is_explicit_one_shot_and_refuses_sensitive_actions() {
        let script = form_script(
            &json!({ "origin": "https://example.com" }),
            FormScriptAction::Submit,
        )
        .unwrap();
        assert!(script.contains("location.origin!==p.origin"));
        assert!(script.contains("form.requestSubmit(button)"));
        assert!(script.contains("form.dataset.jadxSubmissionAttempted"));
        assert!(script.contains("form.checkValidity()"));
        assert!(script.contains("legalAcceptance"));
        assert!(script.contains("chosenFiles"));
        assert!(script.contains("captcha"));
        assert!(script.contains("target.origin!==p.origin"));
        assert!(!script.contains("form.submit()"));
        assert!(APPLESCRIPT.contains("actionName is \"submit-form\""));
    }

    #[test]
    fn html_snapshot_is_origin_scoped_bounded_and_non_executing() {
        let script = form_script(
            &json!({ "origin": "https://example.com", "max_bytes": MAX_EXTERNAL_HTML_BYTES }),
            FormScriptAction::ReadHtml,
        )
        .unwrap();
        assert!(script.contains("location.origin!==p.origin"));
        assert!(script.contains("createTreeWalker"));
        assert!(script.contains("n.tagName==='SCRIPT'"));
        assert!(script.contains("[existing editable content redacted]"));
        assert!(script.contains("name.startsWith('on')"));
        assert!(script.contains("url.search='';url.hash=''"));
        assert!(script.contains("encoder.encode(ch)"));
        assert!(script.contains("JSON.stringify({origin:location.origin"));
        assert!(!script.contains("eval("));
        assert!(!script.contains("fetch("));
        assert!(!script.contains("requestSubmit"));
        assert!(APPLESCRIPT.contains("actionName is \"read-html\""));
        assert_eq!(MAX_EXTERNAL_HTML_BYTES, 48 * 1024);
    }
}
