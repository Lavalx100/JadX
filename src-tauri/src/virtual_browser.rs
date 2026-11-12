//! A single-tab, ephemeral Chromium session for visual computer use.
//!
//! The browser is separate from the user's Safari/Chrome profiles and is
//! controlled with Chrome DevTools Protocol input events (never macOS mouse
//! events). Downloads are denied and every HTTP(S) request is checked against
//! JadX's public-address policy before Chromium is allowed to send it.

use crate::browser;
use crate::types::ImageInput;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout, Instant};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

const MAX_VIEWPORT_WIDTH: u32 = 1280;
const MAX_VIEWPORT_HEIGHT: u32 = 900;
const MAX_SCREENSHOT_BYTES: usize = 1_500_000;
const MAX_PREVIEW_RESOURCE_BYTES: u64 = 8 * 1024 * 1024;
const CDP_TIMEOUT: Duration = Duration::from_secs(25);

type CdpSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug, Clone, Serialize)]
pub struct VirtualBrowserView {
    pub url: String,
    pub image: ImageInput,
}

#[derive(Default)]
pub struct VirtualBrowser {
    session: Option<BrowserSession>,
}

struct BrowserSession {
    child: Child,
    profile_dir: PathBuf,
    socket: CdpSocket,
    next_id: u64,
    width: u32,
    height: u32,
    load_event_seen: bool,
    workspace_preview_root: Option<PathBuf>,
}

impl Drop for BrowserSession {
    fn drop(&mut self) {
        signal_process_group(self.child.id(), libc::SIGKILL);
        let _ = self.child.start_kill();
        remove_profile_with_retry(&self.profile_dir);
    }
}

#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum InputAction {
    Click { x: u32, y: u32 },
    TypeText { text: String },
    PressKey { key: String },
    Scroll { x: u32, y: u32, delta_y: i32 },
}

impl VirtualBrowser {
    pub fn is_running(&self) -> bool {
        self.session.is_some()
    }

    pub async fn start(
        &mut self,
        url: &str,
        width: u32,
        height: u32,
    ) -> Result<VirtualBrowserView, String> {
        let safe_url = browser::validate_url(url).await?;
        self.start_target(safe_url.as_str(), width, height, None)
            .await
    }

    /// Render one shareable HTML file from the selected workspace in an
    /// ephemeral browser, then return a screenshot and bounded visibility / JS
    /// error diagnostics. Only files under the canonical workspace root can
    /// load, and all HTTP(S) requests are denied for this preview session.
    pub async fn inspect_workspace_preview(
        &mut self,
        workspace_root: &Path,
        relative_path: &str,
        width: u32,
        height: u32,
    ) -> Result<(VirtualBrowserView, String), String> {
        let (root, target, relative) = workspace_preview_target(workspace_root, relative_path)?;
        let target_url = url::Url::from_file_path(&target)
            .map_err(|_| "preview file path could not be converted to a local URL".to_string())?;
        let result = async {
            let mut view = self
                .start_target(target_url.as_str(), width, height, Some(root))
                .await?;
            view.url = format!("workspace preview · {relative}");
            view.image.name = format!("IDE preview · {relative}");
            let diagnostics = match self.workspace_page_diagnostics().await {
                Ok(value) => value,
                Err(error) => format!("DOM diagnostics unavailable: {error}"),
            };
            Ok((view, diagnostics))
        }
        .await;
        let stop_result = self.stop().await;
        match (result, stop_result) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(format!("preview rendered but cleanup failed: {error}")),
        }
    }

    async fn start_target(
        &mut self,
        url: &str,
        width: u32,
        height: u32,
        workspace_preview_root: Option<PathBuf>,
    ) -> Result<VirtualBrowserView, String> {
        if self.session.is_some() {
            return Err(
                "Contained browser is already running. Use computer_use_action or stop it first."
                    .into(),
            );
        }
        let width = width.clamp(800, MAX_VIEWPORT_WIDTH);
        let height = height.clamp(500, MAX_VIEWPORT_HEIGHT);
        let executable = chromium_path().ok_or_else(|| {
            "Google Chrome or Chromium was not found. Install Chrome to use contained computer control.".to_string()
        })?;
        let profile_dir = create_profile_dir()?;

        let mut command = Command::new(executable);
        #[cfg(unix)]
        command.process_group(0);
        command
            .arg("--headless=new")
            .arg(format!("--window-size={width},{height}"))
            .arg("--remote-debugging-port=0")
            .arg("--remote-debugging-address=127.0.0.1")
            .arg("--remote-allow-origins=http://localhost")
            .arg(format!("--user-data-dir={}", profile_dir.display()))
            .arg("--incognito")
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-extensions")
            .arg("--disable-sync")
            .arg("--disable-background-networking")
            .arg("--disable-component-update")
            .arg("--disable-default-apps")
            .arg("--disable-features=Translate,MediaRouter,OptimizationHints,AutofillServerCommunication")
            .arg("about:blank")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        configure_minimal_environment(&mut command);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                remove_profile_with_retry(&profile_dir);
                return Err(format!("could not launch isolated Chromium: {error}"));
            }
        };

        let port = match wait_for_debug_port(&profile_dir, &mut child).await {
            Ok(port) => port,
            Err(error) => {
                terminate_child(&mut child).await;
                remove_profile_with_retry(&profile_dir);
                return Err(error);
            }
        };
        let target = match find_page_target(port).await {
            Ok(target) => target,
            Err(error) => {
                terminate_child(&mut child).await;
                remove_profile_with_retry(&profile_dir);
                return Err(error);
            }
        };
        let mut request = match target.into_client_request() {
            Ok(request) => request,
            Err(error) => {
                terminate_child(&mut child).await;
                remove_profile_with_retry(&profile_dir);
                return Err(format!("invalid Chromium debugging endpoint: {error}"));
            }
        };
        request
            .headers_mut()
            .insert("Origin", HeaderValue::from_static("http://localhost"));
        let (socket, _) = match connect_async(request).await {
            Ok(connection) => connection,
            Err(error) => {
                terminate_child(&mut child).await;
                remove_profile_with_retry(&profile_dir);
                return Err(format!(
                    "could not connect to the local Chromium session: {error}"
                ));
            }
        };
        let mut session = BrowserSession {
            child,
            profile_dir,
            socket,
            next_id: 1,
            width,
            height,
            load_event_seen: false,
            workspace_preview_root,
        };
        session.command("Page.enable", json!({})).await?;
        session.command("Runtime.enable", json!({})).await?;
        session
            .command(
                "Page.addScriptToEvaluateOnNewDocument",
                json!({"source": VISUAL_ERROR_CAPTURE_SCRIPT}),
            )
            .await?;
        session
            .command(
                "Fetch.enable",
                json!({
                    "patterns": [{"urlPattern": "*", "requestStage": "Request"}]
                }),
            )
            .await?;
        let _ = session
            .command("Browser.setDownloadBehavior", json!({"behavior":"deny"}))
            .await;
        self.session = Some(session);
        match self.navigate_and_capture(url).await {
            Ok(view) => Ok(view),
            Err(error) => {
                let _ = self.stop().await;
                Err(error)
            }
        }
    }

    async fn workspace_page_diagnostics(&mut self) -> Result<String, String> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| "preview browser session was not initialized".to_string())?;
        session.pump(Duration::from_millis(250)).await?;
        let value = session
            .command(
                "Runtime.evaluate",
                json!({
                    "expression": VISUAL_DIAGNOSTICS_SCRIPT,
                    "returnByValue": true,
                    "awaitPromise": true
                }),
            )
            .await?;
        let raw = value
            .get("result")
            .and_then(|result| result.get("value"))
            .and_then(Value::as_str)
            .unwrap_or("Visual diagnostics returned no page summary.")
            .chars()
            .take(3_000)
            .collect::<String>();
        let (safe, _) = crate::ide_intelligence::redact_source(&raw);
        Ok(safe)
    }

    pub async fn act(&mut self, raw_actions: &Value) -> Result<VirtualBrowserView, String> {
        let session = self.session.as_mut().ok_or_else(|| {
            "Contained browser is not running. Call computer_use_start first.".to_string()
        })?;
        let actions = raw_actions
            .as_array()
            .ok_or_else(|| "actions must be a JSON array".to_string())?;
        if actions.is_empty() || actions.len() > 8 {
            return Err("computer_use_action accepts 1–8 input actions per approval".into());
        }
        for raw in actions {
            let action: InputAction = serde_json::from_value(raw.clone())
                .map_err(|error| format!("invalid computer input action: {error}"))?;
            match action {
                InputAction::Click { x, y } => {
                    check_point(x, y, session.width, session.height)?;
                    session
                        .command(
                            "Input.dispatchMouseEvent",
                            json!({"type":"mouseMoved","x":x,"y":y}),
                        )
                        .await?;
                    session.command("Input.dispatchMouseEvent", json!({"type":"mousePressed","x":x,"y":y,"button":"left","clickCount":1})).await?;
                    session.command("Input.dispatchMouseEvent", json!({"type":"mouseReleased","x":x,"y":y,"button":"left","clickCount":1})).await?;
                }
                InputAction::TypeText { text } => {
                    if text.is_empty() || text.len() > 2_000 || text.contains('\0') {
                        return Err("typed text must contain 1–2000 UTF-8 bytes and no NUL".into());
                    }
                    session
                        .command("Input.insertText", json!({"text":text}))
                        .await?;
                }
                InputAction::PressKey { key } => {
                    let (code, key_code, text) =
                        key_mapping(&key).ok_or_else(|| format!("unsupported key: {key}"))?;
                    session.command("Input.dispatchKeyEvent", json!({"type":"keyDown","key":key,"code":code,"windowsVirtualKeyCode":key_code,"text":text})).await?;
                    session.command("Input.dispatchKeyEvent", json!({"type":"keyUp","key":key,"code":code,"windowsVirtualKeyCode":key_code})).await?;
                }
                InputAction::Scroll { x, y, delta_y } => {
                    check_point(x, y, session.width, session.height)?;
                    if !(-1_200..=1_200).contains(&delta_y) {
                        return Err("scroll delta must be between -1200 and 1200".into());
                    }
                    session
                        .command(
                            "Input.dispatchMouseEvent",
                            json!({"type":"mouseWheel","x":x,"y":y,"deltaX":0,"deltaY":delta_y}),
                        )
                        .await?;
                }
            }
        }
        session.pump(Duration::from_secs(2)).await?;
        capture_view(session).await
    }

    async fn navigate_and_capture(&mut self, url: &str) -> Result<VirtualBrowserView, String> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| "browser session was not initialized".to_string())?;
        session.load_event_seen = false;
        session.command("Page.navigate", json!({"url":url})).await?;
        session.pump_until_load(Duration::from_secs(12)).await?;
        capture_view(session).await
    }

    pub async fn stop(&mut self) -> Result<(), String> {
        if let Some(mut session) = self.session.take() {
            let _ = session.socket.close(None).await;
            terminate_child(&mut session.child).await;
            remove_profile_with_retry(&session.profile_dir);
        }
        Ok(())
    }
}

const VISUAL_ERROR_CAPTURE_SCRIPT: &str = r#"(() => {
  const key = "__JADX_VISUAL_ERRORS";
  const errors = [];
  Object.defineProperty(window, key, { value: errors, configurable: false });
  const record = (value) => {
    const text = String(value ?? "Unhandled page error").replace(/\s+/g, " ").slice(0, 300);
    if (errors.length < 20 && !errors.includes(text)) errors.push(text);
  };
  addEventListener("error", (event) => record(event.message || event.error?.name), true);
  addEventListener("unhandledrejection", (event) => record(event.reason?.message || event.reason), true);
  const original = console.error.bind(console);
  console.error = (...args) => { record(args.map((arg) => arg?.message || arg).join(" ")); original(...args); };
})();"#;

const VISUAL_DIAGNOSTICS_SCRIPT: &str = r#"(() => {
  const root = document.documentElement;
  const body = document.body;
  const rect = body?.getBoundingClientRect();
  const style = body ? getComputedStyle(body) : null;
  const visible = Boolean(body && rect && rect.width > 0 && rect.height > 0 && style?.display !== "none" && style?.visibility !== "hidden" && Number(style?.opacity ?? 1) > 0);
  const candidates = body ? [...body.querySelectorAll("*")].slice(0, 2000) : [];
  const visibleElements = candidates.filter((element) => {
    const box = element.getBoundingClientRect();
    const css = getComputedStyle(element);
    return box.width > 0 && box.height > 0 && css.display !== "none" && css.visibility !== "hidden" && Number(css.opacity) > 0;
  }).length;
  const landmarks = [...(body?.querySelectorAll("main, [role=main], nav, header") || [])].slice(0, 20).map((element) => {
    const box = element.getBoundingClientRect();
    const css = getComputedStyle(element);
    return { tag: element.tagName.toLowerCase(), role: element.getAttribute("role"), visible: box.width > 0 && box.height > 0 && css.display !== "none" && css.visibility !== "hidden" && Number(css.opacity) > 0 };
  });
  const errors = Array.isArray(window.__JADX_VISUAL_ERRORS) ? window.__JADX_VISUAL_ERRORS.slice(0, 20) : [];
  const text = (body?.innerText || "").replace(/\s+/g, " ").trim().slice(0, 1000);
  return JSON.stringify({
    title: document.title || "(untitled)",
    viewport: `${innerWidth}x${innerHeight}`,
    document: `${root?.scrollWidth || 0}x${root?.scrollHeight || 0}`,
    bodyVisible: visible,
    visibleElementCount: visibleElements,
    inspectedElementCount: candidates.length,
    elementInspectionTruncated: Boolean(body && body.querySelectorAll("*").length > 2000),
    landmarks,
    horizontalOverflow: (root?.scrollWidth || 0) > innerWidth + 2,
    visibleText: text,
    javascriptErrors: errors
  });
})()"#;

fn workspace_preview_target(
    workspace_root: &Path,
    relative_path: &str,
) -> Result<(PathBuf, PathBuf, String), String> {
    if relative_path.is_empty() || relative_path.len() > 512 {
        return Err("preview path must contain 1–512 bytes".into());
    }
    let requested = Path::new(relative_path);
    if requested.is_absolute()
        || requested
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("preview path must be a workspace-relative path without traversal".into());
    }
    let root = std::fs::canonicalize(workspace_root)
        .map_err(|error| format!("selected workspace is unavailable: {error}"))?;
    let target = std::fs::canonicalize(root.join(requested))
        .map_err(|error| format!("preview file is unavailable: {error}"))?;
    if !target.starts_with(&root) {
        return Err("preview file is outside the selected workspace".into());
    }
    if !target.is_file() {
        return Err("preview path must name a file".into());
    }
    let extension = target
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !matches!(extension.to_ascii_lowercase().as_str(), "html" | "htm") {
        return Err("visual preview currently supports HTML entry files".into());
    }
    let relative = target
        .strip_prefix(&root)
        .map_err(|_| "preview file is outside the selected workspace".to_string())?;
    let relative = relative.to_string_lossy().replace('\\', "/");
    if !crate::ide_intelligence::shareable_path(&relative) {
        return Err("sensitive, generated, or unsupported preview paths are excluded".into());
    }
    let metadata = std::fs::metadata(&target).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_PREVIEW_RESOURCE_BYTES {
        return Err("HTML preview entry exceeds the 8 MB inspection limit".into());
    }
    let content = std::fs::read_to_string(&target)
        .map_err(|_| "HTML preview must be a UTF-8 text file".to_string())?;
    if crate::ide_intelligence::redact_source(&content).1 {
        return Err(
            "visual preview is withheld because the HTML contains credential-like content".into(),
        );
    }
    Ok((root, target, relative))
}

fn workspace_preview_request_allowed(raw_url: &str, root: &Path) -> bool {
    let Ok(url) = url::Url::parse(raw_url) else {
        return false;
    };
    match url.scheme() {
        "data" | "blob" => true,
        "about" => url.as_str() == "about:blank",
        "file" => {
            let Ok(path) = url.to_file_path() else {
                return false;
            };
            workspace_preview_file_allowed(&path, root)
        }
        _ => false,
    }
}

fn workspace_preview_file_allowed(path: &Path, root: &Path) -> bool {
    let Ok(path) = std::fs::canonicalize(path) else {
        return false;
    };
    if !path.starts_with(root) || !path.is_file() {
        return false;
    }
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let relative = relative.to_string_lossy().replace('\\', "/");
    if !shareable_preview_resource(&relative) {
        return false;
    }
    let Ok(metadata) = std::fs::metadata(&path) else {
        return false;
    };
    if metadata.len() > MAX_PREVIEW_RESOURCE_BYTES {
        return false;
    }
    if let Ok(source) = std::fs::read_to_string(path) {
        if crate::ide_intelligence::redact_source(&source).1 {
            return false;
        }
    }
    true
}

fn shareable_preview_resource(relative: &str) -> bool {
    if crate::ide_intelligence::shareable_path(relative) {
        return true;
    }
    let path = Path::new(relative);
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(
        extension.as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "avif"
            | "bmp"
            | "ico"
            | "woff"
            | "woff2"
            | "ttf"
            | "otf"
            | "eot"
    ) {
        return false;
    }
    let mut source_like = path.to_path_buf();
    source_like.set_extension("html");
    crate::ide_intelligence::shareable_path(&source_like.to_string_lossy())
}

async fn terminate_child(child: &mut Child) {
    let pid = child.id();
    signal_process_group(pid, libc::SIGTERM);
    if timeout(Duration::from_secs(2), child.wait()).await.is_err() {
        signal_process_group(pid, libc::SIGKILL);
        let _ = child.start_kill();
        let _ = timeout(Duration::from_secs(2), child.wait()).await;
    } else {
        // Chromium can leave helper processes behind after its main process exits.
        signal_process_group(pid, libc::SIGKILL);
    }
}

#[cfg(unix)]
fn signal_process_group(pid: Option<u32>, signal: i32) {
    if let Some(pid) = pid.and_then(|pid| libc::pid_t::try_from(pid).ok()) {
        // Each contained browser is launched into a fresh process group.
        // A negative PID targets only Chromium and its own helper processes.
        unsafe {
            libc::kill(-pid, signal);
        }
    }
}

#[cfg(not(unix))]
fn signal_process_group(_pid: Option<u32>, _signal: i32) {}

fn remove_profile_with_retry(profile_dir: &Path) {
    for attempt in 0..10 {
        match std::fs::remove_dir_all(profile_dir) {
            Ok(()) => return,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(_) if attempt < 9 => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return,
        }
    }
}

impl BrowserSession {
    async fn command(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.socket
            .send(Message::Text(
                json!({"id":id,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .await
            .map_err(|error| format!("Chromium command transport failed: {error}"))?;
        let deadline = Instant::now() + CDP_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(format!(
                    "Chromium did not complete {method} within {} seconds",
                    CDP_TIMEOUT.as_secs()
                ));
            }
            let message = timeout(remaining, self.socket.next())
                .await
                .map_err(|_| {
                    format!(
                        "Chromium did not complete {method} within {} seconds",
                        CDP_TIMEOUT.as_secs()
                    )
                })?
                .ok_or_else(|| "contained Chromium session closed unexpectedly".to_string())?
                .map_err(|error| format!("Chromium debugging stream failed: {error}"))?;
            let Message::Text(text) = message else {
                continue;
            };
            let value: Value = serde_json::from_str(text.as_str())
                .map_err(|error| format!("Chromium sent an invalid protocol event: {error}"))?;
            if value.get("method").and_then(Value::as_str) == Some("Page.loadEventFired") {
                self.load_event_seen = true;
            }
            if value.get("method").and_then(Value::as_str) == Some("Fetch.requestPaused") {
                self.handle_paused_request(&value).await?;
            }
            if value.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = value.get("error") {
                    return Err(format!(
                        "Chromium rejected {method}: {}",
                        error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("protocol error")
                    ));
                }
                return Ok(value.get("result").cloned().unwrap_or_else(|| json!({})));
            }
        }
    }

    async fn send_unawaited(&mut self, method: &str, params: Value) -> Result<(), String> {
        let id = self.next_id;
        self.next_id += 1;
        self.socket
            .send(Message::Text(
                json!({"id":id,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .await
            .map_err(|error| format!("could not apply browser request policy: {error}"))
    }

    async fn handle_paused_request(&mut self, event: &Value) -> Result<(), String> {
        let Some(request_id) = event
            .get("params")
            .and_then(|p| p.get("requestId"))
            .and_then(Value::as_str)
        else {
            return Ok(());
        };
        let url = event
            .get("params")
            .and_then(|p| p.get("request"))
            .and_then(|r| r.get("url"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let allowed = match &self.workspace_preview_root {
            Some(root) => workspace_preview_request_allowed(url, root),
            None => browser::validate_url(url).await.is_ok(),
        };
        if allowed {
            self.send_unawaited("Fetch.continueRequest", json!({"requestId":request_id}))
                .await
        } else {
            self.send_unawaited(
                "Fetch.failRequest",
                json!({"requestId":request_id,"errorReason":"BlockedByClient"}),
            )
            .await
        }
    }

    async fn pump_until_load(&mut self, max_wait: Duration) -> Result<(), String> {
        if self.load_event_seen {
            return Ok(());
        }
        let deadline = Instant::now() + max_wait;
        while Instant::now() < deadline && !self.load_event_seen {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let result = timeout(remaining, self.socket.next()).await;
            let Ok(Some(Ok(Message::Text(text)))) = result else {
                break;
            };
            let Ok(value) = serde_json::from_str::<Value>(text.as_str()) else {
                continue;
            };
            if value.get("method").and_then(Value::as_str) == Some("Page.loadEventFired") {
                self.load_event_seen = true;
            }
            if value.get("method").and_then(Value::as_str) == Some("Fetch.requestPaused") {
                self.handle_paused_request(&value).await?;
            }
        }
        Ok(())
    }

    async fn pump(&mut self, duration: Duration) -> Result<(), String> {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let result = timeout(remaining, self.socket.next()).await;
            let Ok(Some(Ok(Message::Text(text)))) = result else {
                break;
            };
            let Ok(value) = serde_json::from_str::<Value>(text.as_str()) else {
                continue;
            };
            if value.get("method").and_then(Value::as_str) == Some("Page.loadEventFired") {
                self.load_event_seen = true;
            }
            if value.get("method").and_then(Value::as_str) == Some("Fetch.requestPaused") {
                self.handle_paused_request(&value).await?;
            }
        }
        Ok(())
    }
}

async fn capture_view(session: &mut BrowserSession) -> Result<VirtualBrowserView, String> {
    let metadata = session
        .command(
            "Runtime.evaluate",
            json!({
                "expression":"location.href",
                "returnByValue":true,
                "awaitPromise":true
            }),
        )
        .await?;
    let raw_url = metadata
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(Value::as_str)
        .unwrap_or("about:blank");
    let url = redact_url(raw_url);
    let screenshot = session
        .command(
            "Page.captureScreenshot",
            json!({"format":"jpeg","quality":58,"captureBeyondViewport":false}),
        )
        .await?;
    let encoded = screenshot
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| "Chromium returned no screenshot".to_string())?;
    let bytes = BASE64
        .decode(encoded)
        .map_err(|_| "Chromium returned an invalid screenshot".to_string())?;
    if bytes.is_empty() || bytes.len() > MAX_SCREENSHOT_BYTES {
        return Err("browser screenshot exceeded the 1.5 MB safety limit".into());
    }
    Ok(VirtualBrowserView {
        url,
        image: ImageInput {
            name: "Contained browser screenshot".into(),
            mime_type: "image/jpeg".into(),
            data_url: format!("data:image/jpeg;base64,{encoded}"),
            size_bytes: bytes.len() as u64,
        },
    })
}

fn redact_url(raw: &str) -> String {
    match url::Url::parse(raw) {
        Ok(mut url) if matches!(url.scheme(), "http" | "https") => {
            url.set_query(None);
            url.set_fragment(None);
            url.to_string()
        }
        _ => "about:blank".into(),
    }
}

fn chromium_path() -> Option<PathBuf> {
    [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/opt/homebrew/bin/chromium",
        "/usr/local/bin/chromium",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|path| path.is_file())
}

fn create_profile_dir() -> Result<PathBuf, String> {
    let base = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("JadX")
        .join("computer-use");
    std::fs::create_dir_all(&base)
        .map_err(|error| format!("could not create browser cache: {error}"))?;
    let profile = base.join(Uuid::new_v4().to_string());
    std::fs::create_dir(&profile)
        .map_err(|error| format!("could not create isolated browser profile: {error}"))?;
    Ok(profile)
}

fn configure_minimal_environment(command: &mut Command) {
    command.env_clear();
    command.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    if let Some(home) = dirs::home_dir() {
        command.env("HOME", home);
    }
    if let Ok(tmp) = std::env::var("TMPDIR") {
        command.env("TMPDIR", tmp);
    }
    if let Ok(locale) = std::env::var("LANG") {
        command.env("LANG", locale);
    }
}

async fn wait_for_debug_port(profile: &Path, child: &mut Child) -> Result<u16, String> {
    let active_port = profile.join("DevToolsActivePort");
    for _ in 0..120 {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Err(format!("Chromium exited during startup ({status})."));
        }
        if let Ok(contents) = tokio::fs::read_to_string(&active_port).await {
            if let Some(port) = contents
                .lines()
                .next()
                .and_then(|line| line.parse::<u16>().ok())
            {
                if port > 0 {
                    return Ok(port);
                }
            }
        }
        sleep(Duration::from_millis(100)).await;
    }
    Err("Chromium did not start its loopback-only debugging endpoint within 12 seconds.".into())
}

async fn find_page_target(port: u16) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(format!("http://127.0.0.1:{port}/json/list"))
        .send()
        .await
        .map_err(|error| format!("could not query local Chromium page target: {error}"))?;
    let targets: Vec<Value> = response
        .json()
        .await
        .map_err(|error| format!("invalid Chromium target list: {error}"))?;
    targets
        .iter()
        .find(|target| target.get("type").and_then(Value::as_str) == Some("page"))
        .and_then(|target| target.get("webSocketDebuggerUrl"))
        .and_then(Value::as_str)
        .filter(|url| url.starts_with(&format!("ws://127.0.0.1:{port}/")))
        .map(str::to_string)
        .ok_or_else(|| "Chromium did not expose a page on its loopback debugging endpoint.".into())
}

fn check_point(x: u32, y: u32, width: u32, height: u32) -> Result<(), String> {
    if x >= width || y >= height {
        Err(format!(
            "input point ({x}, {y}) is outside the {width}×{height} virtual browser viewport"
        ))
    } else {
        Ok(())
    }
}

fn key_mapping(key: &str) -> Option<(&'static str, u32, &'static str)> {
    Some(match key {
        "Enter" => ("Enter", 13, "\r"),
        "Tab" => ("Tab", 9, "\t"),
        "Escape" => ("Escape", 27, ""),
        "Backspace" => ("Backspace", 8, ""),
        "Delete" => ("Delete", 46, ""),
        "ArrowUp" => ("ArrowUp", 38, ""),
        "ArrowDown" => ("ArrowDown", 40, ""),
        "ArrowLeft" => ("ArrowLeft", 37, ""),
        "ArrowRight" => ("ArrowRight", 39, ""),
        "Home" => ("Home", 36, ""),
        "End" => ("End", 35, ""),
        "Space" => (" ", 32, " "),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_query_and_fragment_from_visible_url() {
        assert_eq!(
            redact_url("https://example.com/path?token=secret#section"),
            "https://example.com/path"
        );
        assert_eq!(redact_url("file:///etc/passwd"), "about:blank");
    }

    #[test]
    fn only_safe_navigation_keys_are_mappable() {
        assert!(key_mapping("Enter").is_some());
        assert!(key_mapping("ArrowDown").is_some());
        assert!(key_mapping("Meta").is_none());
        assert!(key_mapping("F12").is_none());
    }

    #[test]
    fn input_actions_are_strictly_typed_and_reject_extra_fields() {
        assert!(serde_json::from_value::<InputAction>(json!({
            "type":"click","x":15,"y":20
        }))
        .is_ok());
        assert!(serde_json::from_value::<InputAction>(json!({
            "type":"click","x":15,"y":20,"command":"rm -rf ~"
        }))
        .is_err());
        assert!(serde_json::from_value::<InputAction>(json!({
            "type":"press_key","key":"Meta"
        }))
        .is_ok());
        assert!(key_mapping("Meta").is_none());
    }

    #[test]
    fn viewport_input_is_bounded() {
        assert!(check_point(800, 1, 800, 500).is_err());
        assert!(check_point(799, 499, 800, 500).is_ok());
    }

    #[test]
    fn workspace_preview_rejects_network_and_paths_outside_the_root() {
        let root = std::env::temp_dir().canonicalize().unwrap();
        assert!(!workspace_preview_request_allowed(
            "https://example.com/app.css",
            &root
        ));
        assert!(!workspace_preview_request_allowed(
            "http://127.0.0.1:3000/",
            &root
        ));
        assert!(workspace_preview_request_allowed("about:blank", &root));
        assert!(workspace_preview_request_allowed(
            "data:text/plain,ok",
            &root
        ));
    }

    #[test]
    fn workspace_preview_entry_requires_shareable_html_inside_workspace() {
        let root = std::env::temp_dir().join(format!("jadx-preview-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("index.html"), "<main>Preview</main>").unwrap();
        std::fs::write(root.join(".env"), "PRIVATE=1").unwrap();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("assets/logo.png"), b"image").unwrap();
        std::fs::write(
            root.join("assets/app.js"),
            "const api_key = \"placeholder\";",
        )
        .unwrap();
        assert_eq!(
            workspace_preview_target(&root, "index.html").unwrap().2,
            "index.html"
        );
        assert!(workspace_preview_target(&root, ".env").is_err());
        assert!(workspace_preview_target(&root, "../index.html").is_err());
        assert!(workspace_preview_target(&root, "/etc/passwd").is_err());
        assert!(workspace_preview_target(&root, "package.json").is_err());
        let file_url = url::Url::from_file_path(root.join("index.html")).unwrap();
        assert!(workspace_preview_request_allowed(
            file_url.as_str(),
            &root.canonicalize().unwrap()
        ));
        let image_url = url::Url::from_file_path(root.join("assets/logo.png")).unwrap();
        assert!(workspace_preview_request_allowed(
            image_url.as_str(),
            &root.canonicalize().unwrap()
        ));
        let secret_script_url = url::Url::from_file_path(root.join("assets/app.js")).unwrap();
        assert!(!workspace_preview_request_allowed(
            secret_script_url.as_str(),
            &root.canonicalize().unwrap()
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    #[ignore = "requires Google Chrome or Chromium"]
    async fn workspace_visual_preview_returns_screenshot_and_page_errors() {
        let root = std::env::temp_dir().join(format!("jadx-live-preview-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("index.html"),
            "<!doctype html><html><head><title>JadX visual check</title></head><body><main>Visible page</main><script>console.error('visual-error-check')</script></body></html>",
        )
        .unwrap();
        let mut browser = VirtualBrowser::default();
        let (view, diagnostics) = browser
            .inspect_workspace_preview(&root, "index.html", 1024, 700)
            .await
            .unwrap();
        assert!(view.image.data_url.starts_with("data:image/jpeg;base64,"));
        assert!(
            diagnostics.contains("\"bodyVisible\":true"),
            "{diagnostics}"
        );
        assert!(diagnostics.contains("visual-error-check"), "{diagnostics}");
        assert!(!browser.is_running());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    #[ignore = "requires Google Chrome and public internet access"]
    async fn live_isolated_browser_navigates_screenshots_and_cleans_up() {
        let mut browser = VirtualBrowser::default();
        let view = browser
            .start("https://example.com", 1024, 700)
            .await
            .unwrap();
        assert!(view.url.starts_with("https://example.com"));
        assert!(view.image.data_url.starts_with("data:image/jpeg;base64,"));
        assert!(view.image.size_bytes > 0 && view.image.size_bytes < MAX_SCREENSHOT_BYTES as u64);
        let next = browser
            .act(&json!([{"type":"scroll","x":900,"y":650,"delta_y":250}]))
            .await
            .unwrap();
        assert_eq!(next.url, view.url);
        let profile = browser.session.as_ref().unwrap().profile_dir.clone();
        browser.stop().await.unwrap();
        assert!(!profile.exists());
    }
}
