//! Ponte IPC sem servidor: reutiliza os contratos do núcleo via Router::oneshot.
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, Uri};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use tower::ServiceExt;

use crate::server::{AppState, router};

/// AppRun altera o cwd. Os runtimes preservam a origem em OWD ou no PWD do shell.
pub fn collection_argument(
    path: PathBuf,
    appimage: bool,
    original_dir: Option<PathBuf>,
    shell_dir: Option<PathBuf>,
) -> PathBuf {
    if appimage && path.is_relative() {
        if let Some(base) = original_dir
            .filter(|path| path.is_absolute())
            .or_else(|| shell_dir.filter(|path| path.is_absolute()))
        {
            return base.join(path);
        }
    }
    path
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcResponse {
    pub status: u16,
    pub data: Value,
}

/// Executa uma operação local. Nunca transforma uma URI em acesso à rede.
pub async fn dispatch(
    state: AppState,
    path: String,
    method: String,
    body: Option<Value>,
) -> Result<IpcResponse, String> {
    let uri: Uri = path.parse().map_err(|_| "caminho IPC inválido")?;
    if uri.scheme().is_some() || uri.authority().is_some() || !uri.path().starts_with("/api/") {
        return Err("somente operações internas /api/ são permitidas".into());
    }
    let method: Method = method.parse().map_err(|_| "método IPC inválido")?;
    if !matches!(
        method,
        Method::GET | Method::POST | Method::PUT | Method::DELETE
    ) {
        return Err("método IPC não permitido".into());
    }
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(body.map_or(Body::empty(), |value| Body::from(value.to_string())))
        .map_err(|error| error.to_string())?;
    let response = router(state)
        .oneshot(request)
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    // O limite inclui respostas codificadas como base64 e o overhead JSON.
    let bytes = to_bytes(response.into_body(), 64 * 1024 * 1024)
        .await
        .map_err(|error| error.to_string())?;
    let data = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    Ok(IpcResponse { status, data })
}
