use crate::AppState;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout, Duration};
use url::Url;

const VERSION: &str = "4.143.1";
const ARCHIVE_SIZE: u64 = 204_850_136;
const ARCHIVE_SHA256: &str = "940bb5ecd567187336ed3608c2404e21e37528465aaddb4d6f5b514d930bd7db";
const ARCHIVE_URL: &str = "https://github.com/coder/code-server/releases/download/v4.143.1/code-server-4.143.1-macos-arm64.tar.gz";
const RUNTIME_DIR: &str = "code-oss";

#[derive(Clone, Default)]
pub struct CodeOssRuntime {
    managed: std::sync::Arc<Mutex<ManagedServer>>,
}

impl CodeOssRuntime {
    pub fn terminate(&self) {
        if let Ok(mut managed) = self.managed.try_lock() {
            if let Some(child) = managed.child.as_mut() {
                if let Some(pid) = child.id() {
                    unsafe {
                        libc::kill(-(pid as libc::pid_t), libc::SIGTERM);
                    }
                }
                let _ = child.start_kill();
            }
        }
    }
}

#[derive(Default)]
struct ManagedServer {
    child: Option<Child>,
    workspace_id: Option<String>,
    url: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeOssStatus {
    installed: bool,
    running: bool,
    url: Option<String>,
    version: &'static str,
    archive_bytes: u64,
}

fn support_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join(RUNTIME_DIR))
        .map_err(|error| format!("Could not locate JadX application data: {error}"))
}

fn install_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(support_dir(app)?.join(format!("code-server-{VERSION}-macos-arm64")))
}

fn server_binary(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(install_dir(app)?.join("bin/code-server"))
}

fn status_for(app: &AppHandle, server: &ManagedServer) -> Result<CodeOssStatus, String> {
    let installed = server_binary(app)?.is_file();
    Ok(CodeOssStatus {
        installed,
        running: server.child.is_some(),
        url: server.url.clone(),
        version: VERSION,
        archive_bytes: ARCHIVE_SIZE,
    })
}

#[tauri::command]
pub async fn ide_code_oss_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CodeOssStatus, String> {
    let mut server = state.runtime.code_oss.managed.lock().await;
    let exited = if let Some(child) = server.child.as_mut() {
        child
            .try_wait()
            .map_err(|error| format!("Could not inspect Code OSS: {error}"))?
            .is_some()
    } else {
        false
    };
    if exited {
        server.child = None;
        server.workspace_id = None;
        server.url = None;
    }
    status_for(&app, &server)
}

#[tauri::command]
pub async fn ide_code_oss_install(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CodeOssStatus, String> {
    if std::env::consts::ARCH != "aarch64" {
        return Err(
            "The Code OSS runtime package is currently available for Apple silicon only.".into(),
        );
    }

    let _server = state.runtime.code_oss.managed.lock().await;
    let support = support_dir(&app)?;
    let destination = install_dir(&app)?;
    let binary = destination.join("bin/code-server");
    if binary.is_file() {
        return Ok(CodeOssStatus {
            installed: true,
            running: false,
            url: None,
            version: VERSION,
            archive_bytes: ARCHIVE_SIZE,
        });
    }

    tokio::fs::create_dir_all(&support)
        .await
        .map_err(|error| format!("Could not prepare the Code OSS install folder: {error}"))?;
    let archive = support.join(format!("code-server-{VERSION}.tar.gz"));
    let partial = support.join(format!("code-server-{VERSION}.tar.gz.part"));
    let hash = if tokio::fs::metadata(&archive).await.is_ok() {
        sha256_file(&archive).await?
    } else {
        download_archive(&app, &partial).await?
    };
    if hash != ARCHIVE_SHA256 {
        let _ = tokio::fs::remove_file(&partial).await;
        let _ = tokio::fs::remove_file(&archive).await;
        return Err(
            "The downloaded Code OSS package did not match its published checksum. It was removed."
                .into(),
        );
    }

    let staging = support.join(format!("code-server-{VERSION}.installing"));
    if tokio::fs::metadata(&staging).await.is_ok() {
        tokio::fs::remove_dir_all(&staging)
            .await
            .map_err(|error| format!("Could not clear an incomplete Code OSS install: {error}"))?;
    }
    tokio::fs::create_dir_all(&staging)
        .await
        .map_err(|error| format!("Could not prepare Code OSS extraction: {error}"))?;
    let output = Command::new("/usr/bin/tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(&staging)
        .arg("--strip-components=1")
        .output()
        .await
        .map_err(|error| format!("Could not extract Code OSS: {error}"))?;
    if !output.status.success() {
        let _ = tokio::fs::remove_dir_all(&staging).await;
        return Err(format!(
            "Could not extract Code OSS: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    if !staging.join("bin/code-server").is_file() || !staging.join("lib/node").is_file() {
        let _ = tokio::fs::remove_dir_all(&staging).await;
        return Err("The Code OSS package is missing its server or Node runtime.".into());
    }
    tokio::fs::rename(&staging, &destination)
        .await
        .map_err(|error| format!("Could not finish installing Code OSS: {error}"))?;
    let _ = tokio::fs::remove_file(&archive).await;

    Ok(CodeOssStatus {
        installed: true,
        running: false,
        url: None,
        version: VERSION,
        archive_bytes: ARCHIVE_SIZE,
    })
}

async fn download_archive(app: &AppHandle, partial: &Path) -> Result<String, String> {
    let response = reqwest::Client::builder()
        .https_only(true)
        .build()
        .map_err(|error| format!("Could not prepare a secure Code OSS download: {error}"))?
        .get(ARCHIVE_URL)
        .send()
        .await
        .map_err(|error| format!("Could not download Code OSS: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Could not download Code OSS: {error}"))?;
    if response
        .content_length()
        .is_some_and(|length| length != ARCHIVE_SIZE)
    {
        return Err("The Code OSS download had an unexpected size and was rejected.".into());
    }

    let mut response = response;
    let mut file = tokio::fs::File::create(partial)
        .await
        .map_err(|error| format!("Could not write the Code OSS download: {error}"))?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0_u64;
    loop {
        let chunk = response
            .chunk()
            .await
            .map_err(|error| format!("Code OSS download interrupted: {error}"))?;
        let Some(chunk) = chunk else { break };
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded > ARCHIVE_SIZE {
            let _ = tokio::fs::remove_file(partial).await;
            return Err(
                "The Code OSS download exceeded its expected size and was rejected.".into(),
            );
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|error| format!("Could not write the Code OSS download: {error}"))?;
        let _ = app.emit(
            "ide-code-oss-download-progress",
            serde_json::json!({ "downloaded": downloaded, "total": ARCHIVE_SIZE }),
        );
    }
    file.flush()
        .await
        .map_err(|error| format!("Could not finish the Code OSS download: {error}"))?;
    drop(file);
    if downloaded != ARCHIVE_SIZE {
        let _ = tokio::fs::remove_file(partial).await;
        return Err("The Code OSS download was incomplete and was removed.".into());
    }
    let hash = format!("{:x}", hasher.finalize());
    if hash == ARCHIVE_SHA256 {
        let archive = partial.with_extension("");
        tokio::fs::rename(partial, &archive)
            .await
            .map_err(|error| format!("Could not finish the Code OSS download: {error}"))?;
    }
    Ok(hash)
}

async fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| format!("Could not verify the Code OSS package: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|error| format!("Could not verify the Code OSS package: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

async fn stop_process_group(mut child: Child) -> Result<(), String> {
    let Some(pid) = child.id() else {
        return Ok(());
    };
    unsafe {
        libc::kill(-(pid as libc::pid_t), libc::SIGTERM);
    }
    if timeout(Duration::from_secs(3), child.wait()).await.is_err() {
        unsafe {
            libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
        }
        child
            .wait()
            .await
            .map_err(|error| format!("Could not finish stopping Code OSS: {error}"))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn ide_code_oss_start(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<CodeOssStatus, String> {
    let root = crate::workspace_root_for(&state, &workspace_id)?;
    let mut managed = state.runtime.code_oss.managed.lock().await;
    let binary = server_binary(&app)?;
    if !binary.is_file() {
        return Err("Install Code OSS before starting the IDE runtime.".into());
    }
    let process_running = if let Some(child) = managed.child.as_mut() {
        child
            .try_wait()
            .map_err(|error| format!("Could not inspect Code OSS: {error}"))?
            .is_none()
    } else {
        false
    };
    if process_running {
        if managed.workspace_id.as_deref() == Some(workspace_id.as_str()) {
            return status_for(&app, &managed);
        }
        if let Some(child) = managed.child.take() {
            stop_process_group(child).await?;
        }
    }
    managed.child = None;
    managed.url = None;
    managed.workspace_id = None;

    let port = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|error| format!("Could not reserve a local Code OSS port: {error}"))?
        .local_addr()
        .map_err(|error| format!("Could not read the local Code OSS port: {error}"))?
        .port();
    let support = support_dir(&app)?;
    let user_data = support.join("user-data");
    let extensions = support.join("extensions");
    tokio::fs::create_dir_all(&user_data)
        .await
        .map_err(|error| format!("Could not prepare Code OSS settings: {error}"))?;
    tokio::fs::create_dir_all(&extensions)
        .await
        .map_err(|error| format!("Could not prepare Code OSS extensions: {error}"))?;
    let bridge_extension = extensions.join("jadx.jadx-intelligence-1.0.0");
    tokio::fs::create_dir_all(&bridge_extension)
        .await
        .map_err(|e| e.to_string())?;
    for (name, content) in [
        (
            "package.json",
            include_str!("../../resources/code-oss-extension/package.json"),
        ),
        (
            "extension.js",
            include_str!("../../resources/code-oss-extension/extension.js"),
        ),
        (
            "theme.json",
            include_str!("../../resources/code-oss-extension/theme.json"),
        ),
    ] {
        tokio::fs::write(bridge_extension.join(name), content)
            .await
            .map_err(|e| e.to_string())?;
    }
    let (bridge_url, bridge_token) = crate::ide_bridge::ensure(&state, &workspace_id).await?;
    let user_settings = user_data.join("User/settings.json");
    if !user_settings
        .try_exists()
        .map_err(|error| format!("Could not inspect Code OSS preferences: {error}"))?
    {
        if let Some(parent) = user_settings.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| format!("Could not prepare Code OSS preferences: {error}"))?;
        }
        tokio::fs::write(
            &user_settings,
            "{\n  \"workbench.colorTheme\": \"Default Dark Modern\",\n  \"workbench.startupEditor\": \"none\"\n}\n",
        )
        .await
        .map_err(|error| format!("Could not save Code OSS preferences: {error}"))?;
    }

    let child = Command::new(binary)
        .env("JADX_IDE_BRIDGE_URL", bridge_url)
        .env("JADX_IDE_BRIDGE_TOKEN", bridge_token)
        .arg(format!("--bind-addr=127.0.0.1:{port}"))
        .arg("--auth=none")
        .arg("--disable-telemetry")
        .arg("--disable-update-check")
        .arg("--user-data-dir")
        .arg(&user_data)
        .arg("--extensions-dir")
        .arg(&extensions)
        .arg(&root)
        .current_dir(&root)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("Could not start Code OSS: {error}"))?;
    managed.child = Some(child);
    managed.workspace_id = Some(workspace_id);

    let mut editor_url = Url::parse(&format!("http://127.0.0.1:{port}/"))
        .map_err(|error| format!("Could not create the Code OSS address: {error}"))?;
    editor_url.query_pairs_mut().append_pair(
        "folder",
        root.to_str()
            .ok_or_else(|| "The selected workspace path is not valid Unicode.".to_string())?,
    );
    let editor_url = editor_url.to_string();
    for _ in 0..80 {
        if let Some(child) = managed.child.as_mut() {
            if let Some(status) = child
                .try_wait()
                .map_err(|error| format!("Could not inspect Code OSS startup: {error}"))?
            {
                managed.child = None;
                managed.workspace_id = None;
                return Err(format!("Code OSS stopped during startup ({status})."));
            }
        }
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            managed.url = Some(editor_url);
            return status_for(&app, &managed);
        }
        sleep(Duration::from_millis(250)).await;
    }
    if let Some(child) = managed.child.take() {
        let _ = stop_process_group(child).await;
    }
    managed.workspace_id = None;
    Err("Code OSS did not start listening on its private local port.".into())
}

#[tauri::command]
pub async fn ide_code_oss_stop(state: State<'_, AppState>) -> Result<CodeOssStatus, String> {
    crate::ide_bridge::stop(&state).await;
    let mut managed = state.runtime.code_oss.managed.lock().await;
    if let Some(mut child) = managed.child.take() {
        let still_running = child
            .try_wait()
            .map_err(|error| format!("Could not inspect Code OSS: {error}"))?
            .is_none();
        if still_running {
            stop_process_group(child).await?;
        }
    }
    managed.workspace_id = None;
    managed.url = None;
    Ok(CodeOssStatus {
        installed: true,
        running: false,
        url: None,
        version: VERSION,
        archive_bytes: ARCHIVE_SIZE,
    })
}
