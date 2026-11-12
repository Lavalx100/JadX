//! Authenticated loopback bridge for the bundled Code OSS extension.
//! Only editor metadata/selection and navigation/theme commands cross this boundary.
use crate::{db, AppState};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tauri::State;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub struct Bridge {
    workspace_id: String,
    pub url: String,
    pub token: String,
    cancel: CancellationToken,
    context: EditorContext,
    seen: Instant,
    navigation: Option<Value>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorContext {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub selection_start: Option<usize>,
    #[serde(default)]
    pub selection_end: Option<usize>,
    #[serde(default)]
    pub selection_text: String,
    #[serde(default)]
    pub dirty: bool,
    #[serde(default)]
    pub connected: bool,
}

pub async fn ensure(state: &AppState, workspace_id: &str) -> Result<(String, String), String> {
    let mut current = state.runtime.intelligence.bridge.lock().await;
    if let Some(b) = current.as_ref() {
        if b.workspace_id == workspace_id && !b.cancel.is_cancelled() {
            return Ok((b.url.clone(), b.token.clone()));
        }
        b.cancel.cancel();
    }
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| e.to_string())?;
    let url = format!(
        "http://127.0.0.1:{}",
        listener.local_addr().map_err(|e| e.to_string())?.port()
    );
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let cancel = CancellationToken::new();
    *current = Some(Bridge {
        workspace_id: workspace_id.into(),
        url: url.clone(),
        token: token.clone(),
        cancel: cancel.clone(),
        context: EditorContext::default(),
        seen: Instant::now(),
        navigation: None,
    });
    let state = state.clone();
    let auth = token.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let accepted = tokio::select! {_ = cancel.cancelled()=>break, r=listener.accept()=>r};
            let Ok((mut stream, peer)) = accepted else {
                break;
            };
            if !peer.ip().is_loopback() {
                continue;
            }
            let state = state.clone();
            let auth = auth.clone();
            tauri::async_runtime::spawn(async move {
                let response = tokio::time::timeout(
                    Duration::from_secs(3),
                    handle(&state, &auth, &mut stream),
                )
                .await
                .unwrap_or_else(|_| Err("Request timed out".into()));
                let (status, body) = match response {
                    Ok(v) => ("200 OK", v.to_string()),
                    Err(_) => ("403 Forbidden", "{}".into()),
                };
                let response=format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",body.len());
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    Ok((url, token))
}

async fn handle(
    state: &AppState,
    auth: &str,
    stream: &mut tokio::net::TcpStream,
) -> Result<Value, String> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let n = stream.read(&mut chunk).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("Incomplete request".into());
        }
        buffer.extend_from_slice(&chunk[..n]);
        if buffer.len() > 48_000 {
            return Err("Request too large".into());
        }
        if let Some(pos) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let headers = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    if !authorized(&headers, auth) {
        return Err("Forbidden".into());
    }
    let size = headers
        .lines()
        .find_map(|l| {
            l.split_once(':')
                .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    if size > 32_000 {
        return Err("Request too large".into());
    }
    while buffer.len() < header_end + size {
        let n = stream.read(&mut chunk).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("Incomplete request".into());
        }
        buffer.extend_from_slice(&chunk[..n]);
        if buffer.len() > 48_000 {
            return Err("Request too large".into());
        }
    }
    let mut current = state.runtime.intelligence.bridge.lock().await;
    let b = current
        .as_mut()
        .filter(|b| b.token == auth && !b.cancel.is_cancelled())
        .ok_or("Bridge expired")?;
    if headers.starts_with("POST /context HTTP/1.") {
        let mut context: EditorContext =
            serde_json::from_slice(&buffer[header_end..header_end + size])
                .map_err(|e| e.to_string())?;
        let root = crate::workspace_root_for(state, &b.workspace_id)?;
        let path = context.path.take().filter(|p| {
            crate::ide_intelligence::shareable_path(p)
                && crate::ide_workspace_path(&root, p, false).is_ok()
        });
        if path.is_none() {
            context.selection_text.clear();
        } else {
            context.selection_text =
                crate::ide_intelligence::redact_source(&context.selection_text).0;
        }
        context.path = path;
        context.connected = true;
        b.context = context;
        b.seen = Instant::now();
        Ok(json!({"ok":true}))
    } else if headers.starts_with("GET /state HTTP/1.") {
        let settings = db::get_settings(&state.db)?;
        Ok(
            json!({"engineeringPrinciples":crate::ide_intelligence::mode_enabled(&settings),"navigation":b.navigation}),
        )
    } else {
        Err("Unknown request".into())
    }
}

fn authorized(headers: &str, auth: &str) -> bool {
    let expected = format!("Bearer {auth}");
    !headers.lines().any(|l| {
        l.split_once(':')
            .map(|(k, _)| k.eq_ignore_ascii_case("origin"))
            .unwrap_or(false)
    }) && headers.lines().any(|l| {
        l.split_once(':')
            .map(|(k, v)| k.eq_ignore_ascii_case("authorization") && v.trim() == expected)
            .unwrap_or(false)
    })
}

pub async fn stop(state: &AppState) {
    let mut b = state.runtime.intelligence.bridge.lock().await;
    if let Some(b) = b.take() {
        b.cancel.cancel();
    }
}

pub async fn has_dirty_documents(state: &AppState, workspace_id: &str) -> bool {
    let b = state.runtime.intelligence.bridge.lock().await;
    b.as_ref()
        .map(|b| {
            b.workspace_id == workspace_id
                && (b.context.dirty
                    || !b.context.connected
                    || b.seen.elapsed() > Duration::from_secs(6))
        })
        .unwrap_or(false)
}

#[tauri::command]
pub async fn ide_editor_context(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<EditorContext, String> {
    let current = state.runtime.intelligence.bridge.lock().await;
    Ok(current
        .as_ref()
        .filter(|b| b.workspace_id == workspace_id && b.seen.elapsed() < Duration::from_secs(6))
        .map(|b| b.context.clone())
        .unwrap_or_default())
}

#[tauri::command]
pub async fn ide_editor_navigate(
    state: State<'_, AppState>,
    workspace_id: String,
    path: String,
    line: usize,
) -> Result<(), String> {
    let root = crate::workspace_root_for(&state, &workspace_id)?;
    crate::ide_workspace_path(&root, &path, false)?;
    let mut current = state.runtime.intelligence.bridge.lock().await;
    let b = current
        .as_mut()
        .filter(|b| b.workspace_id == workspace_id)
        .ok_or("Code OSS bridge unavailable")?;
    b.navigation = Some(json!({"id":Uuid::new_v4().to_string(),"path":path,"line":line.max(1)}));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bridge_requires_header_auth_and_rejects_browser_origins() {
        assert!(authorized(
            "GET /state HTTP/1.1\r\nAuthorization: Bearer abc\r\n",
            "abc"
        ));
        assert!(!authorized("GET /state?token=abc HTTP/1.1\r\n", "abc"));
        assert!(!authorized(
            "GET /state HTTP/1.1\r\nAuthorization: Bearer abc\r\nOrigin: https://example.com\r\n",
            "abc"
        ));
    }
}
