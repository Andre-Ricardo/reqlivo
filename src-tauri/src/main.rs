#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use reqlivo::desktop::{IpcResponse, dispatch};
use reqlivo::error::ErrorCategory;
use reqlivo::migration::{self, Inspection};
use reqlivo::server::AppState;
use reqlivo::store::{CollectionStore, init_collection};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_dialog::MessageDialogButtons;
use tokio::sync::RwLock;

/// Coleção selecionada que ainda não pode ser aberta sem confirmação humana.
#[derive(Debug, Clone)]
struct PendingCollection {
    root: PathBuf,
    inspection: Option<Inspection>,
    error: Option<String>,
}

struct DesktopState {
    application: RwLock<Option<AppState>>,
    pending: RwLock<Option<PendingCollection>>,
    last_collection: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CollectionStatus {
    status: &'static str,
    root: Option<String>,
    name: Option<String>,
    inspection: Option<Inspection>,
    error: Option<String>,
    /// Há uma coleção anterior ainda ativa por trás da tela de recuperação.
    previous_active: bool,
}

impl CollectionStatus {
    async fn ready(state: &DesktopState) -> Self {
        let application = state.application.read().await;
        match application.as_ref() {
            Some(application) => {
                let store = application.store.read().await;
                Self {
                    status: "ready",
                    root: Some(store.root.display().to_string()),
                    name: Some(store.manifest.info.name.clone()),
                    inspection: None,
                    error: None,
                    previous_active: true,
                }
            }
            None => Self {
                status: "empty",
                root: None,
                name: None,
                inspection: None,
                error: None,
                previous_active: false,
            },
        }
    }

    async fn pending(state: &DesktopState) -> Option<Self> {
        let pending = state.pending.read().await;
        if let Some(pending) = pending.as_ref() {
            let previous_active = state.application.read().await.is_some();
            return Some(Self {
                status: "pending",
                root: Some(pending.root.display().to_string()),
                name: None,
                inspection: pending.inspection.clone(),
                error: pending.error.clone(),
                previous_active,
            });
        }
        None
    }
}

#[tauri::command]
async fn confirm_action(app: tauri::AppHandle, message: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .message(message)
            .title("Reqlivo")
            .buttons(MessageDialogButtons::OkCancel)
            .blocking_show()
    })
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn close_window(app: tauri::AppHandle, state: State<'_, DesktopState>) -> Result<(), String> {
    if let Some(application) = state.application.read().await.as_ref() {
        application.cancel_all().await;
    }
    if let Some(window) = app.get_webview_window("main") {
        window.destroy().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn api_request(
    state: State<'_, DesktopState>,
    path: String,
    method: String,
    body: Option<Value>,
) -> Result<IpcResponse, String> {
    let application = state.application.read().await;
    let Some(application) = application.as_ref() else {
        return Err(
            "nenhuma coleção ativa; confirme ou cancele a migração antes de continuar".into(),
        );
    };
    dispatch(application.clone(), path, method, body).await
}

#[tauri::command]
async fn collection_status(state: State<'_, DesktopState>) -> Result<CollectionStatus, String> {
    if let Some(status) = CollectionStatus::pending(&state).await {
        return Ok(status);
    }
    Ok(CollectionStatus::ready(&state).await)
}

fn describe_pending(root: PathBuf, error: reqlivo::Error) -> PendingCollection {
    let inspection = migration::inspect(&root).ok();
    PendingCollection {
        root,
        inspection,
        error: Some(error.to_string()),
    }
}

#[tauri::command]
async fn open_collection(
    app: tauri::AppHandle,
    state: State<'_, DesktopState>,
) -> Result<CollectionStatus, String> {
    let folder = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Abrir coleção OpenCollection")
            .blocking_pick_folder()
    })
    .await
    .map_err(|error| error.to_string())?;
    let Some(folder) = folder else {
        return Ok(CollectionStatus::ready(&state).await);
    };
    let path = folder.into_path().map_err(|error| error.to_string())?;
    match CollectionStore::open(&path) {
        Ok(store) => {
            let mut application = state.application.write().await;
            if let Some(previous) = application.as_ref() {
                previous.cancel_all().await;
            }
            *application = Some(AppState::new(store));
            *state.pending.write().await = None;
            drop(application);
            std::fs::write(&state.last_collection, path.to_string_lossy().as_bytes())
                .map_err(|error| error.to_string())?;
            Ok(CollectionStatus::ready(&state).await)
        }
        Err(error) if error.info().category == ErrorCategory::Conflict => {
            let inspection = migration::inspect(&path).ok();
            let pending = PendingCollection {
                root: path,
                inspection,
                error: Some(error.to_string()),
            };
            *state.pending.write().await = Some(pending);
            Ok(CollectionStatus::pending(&state)
                .await
                .unwrap_or(CollectionStatus {
                    status: "pending",
                    root: None,
                    name: None,
                    inspection: None,
                    error: Some(error.to_string()),
                    previous_active: false,
                }))
        }
        Err(error) => {
            *state.pending.write().await = Some(describe_pending(path, error));
            Ok(CollectionStatus::pending(&state)
                .await
                .expect("pending recém-definido"))
        }
    }
}

#[tauri::command]
async fn migration_inspect(state: State<'_, DesktopState>) -> Result<Value, String> {
    let pending = state.pending.read().await;
    if let Some(pending) = pending.as_ref() {
        let inspection = migration::inspect(&pending.root).map_err(|error| error.to_string())?;
        return Ok(json!({
            "root": pending.root.display().to_string(),
            "inspection": inspection,
            "operations": migration::list_operations(&pending.root).map_err(|error| error.to_string())?,
            "previousError": pending.error,
        }));
    }
    drop(pending);
    let application = state.application.read().await;
    let Some(application) = application.as_ref() else {
        return Err("nenhuma coleção selecionada".into());
    };
    Ok(json!({
        "root": application.root.display().to_string(),
        "inspection": migration::inspect(&application.root).map_err(|error| error.to_string())?,
        "operations": migration::list_operations(&application.root).map_err(|error| error.to_string())?,
    }))
}

#[tauri::command]
async fn migration_apply(
    state: State<'_, DesktopState>,
    plan_id: String,
    revision: Option<String>,
) -> Result<CollectionStatus, String> {
    let root = {
        let pending = state.pending.read().await;
        match pending.as_ref() {
            Some(pending) => pending.root.clone(),
            None => {
                return Err("nenhuma migração pendente para aplicar".into());
            }
        }
    };
    if let Some(expected) = revision.as_deref() {
        let operations = migration::list_operations(&root).map_err(|error| error.to_string())?;
        let current = operations
            .iter()
            .find(|operation| operation.id == plan_id)
            .and_then(|operation| operation.revision.as_deref());
        if current != Some(expected) {
            return Err("o plano mudou desde a inspeção; inspecione novamente".into());
        }
    }
    migration::apply(&root, &plan_id).map_err(|error| error.to_string())?;
    let store = CollectionStore::open(&root).map_err(|error| error.to_string())?;
    {
        let mut application = state.application.write().await;
        if let Some(previous) = application.as_ref() {
            previous.cancel_all().await;
        }
        *application = Some(AppState::new(store));
    }
    *state.pending.write().await = None;
    std::fs::write(&state.last_collection, root.to_string_lossy().as_bytes())
        .map_err(|error| error.to_string())?;
    Ok(CollectionStatus::ready(&state).await)
}

#[tauri::command]
async fn migration_rollback(
    state: State<'_, DesktopState>,
    plan_id: String,
    revision: Option<String>,
) -> Result<CollectionStatus, String> {
    let root = {
        let pending = state.pending.read().await;
        match pending.as_ref() {
            Some(pending) => pending.root.clone(),
            None => {
                let application = state.application.read().await;
                match application.as_ref() {
                    Some(application) => application.root.clone(),
                    None => return Err("nenhuma coleção ativa".into()),
                }
            }
        }
    };
    if let Some(expected) = revision.as_deref() {
        let operations = migration::list_operations(&root).map_err(|error| error.to_string())?;
        let current = operations
            .iter()
            .find(|operation| operation.id == plan_id)
            .and_then(|operation| operation.revision.as_deref());
        if current != Some(expected) {
            return Err("o plano mudou desde a inspeção; inspecione novamente".into());
        }
    }
    migration::rollback(&root, &plan_id).map_err(|error| error.to_string())?;
    *state.application.write().await = None;
    *state.pending.write().await = Some(PendingCollection {
        inspection: migration::inspect(&root).ok(),
        root,
        error: Some("rollback concluído; confirme a nova migração para reabrir".into()),
    });
    Ok(CollectionStatus::pending(&state)
        .await
        .expect("pending recém-definido"))
}

#[tauri::command]
async fn cancel_pending(state: State<'_, DesktopState>) -> Result<CollectionStatus, String> {
    *state.pending.write().await = None;
    Ok(CollectionStatus::ready(&state).await)
}

#[tauri::command]
async fn save_export(app: tauri::AppHandle, content: String) -> Result<bool, String> {
    let file = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Salvar exportação")
            .set_file_name("export.txt")
            .blocking_save_file()
    })
    .await
    .map_err(|error| error.to_string())?;
    let Some(file) = file else { return Ok(false) };
    let path = file.into_path().map_err(|error| error.to_string())?;
    std::fs::write(path, content).map_err(|error| error.to_string())?;
    Ok(true)
}

fn boot_collection(
    explicit: Option<PathBuf>,
    last_collection: &Path,
    data: &Path,
) -> (Option<AppState>, Option<PendingCollection>) {
    let candidate = if let Some(path) = explicit {
        Some(reqlivo::desktop::collection_argument(
            path,
            std::env::var_os("APPIMAGE").is_some(),
            std::env::var_os("OWD").map(PathBuf::from),
            std::env::var_os("PWD").map(PathBuf::from),
        ))
    } else {
        std::fs::read_to_string(last_collection)
            .ok()
            .map(|path| PathBuf::from(path.trim()))
            .filter(|path| !path.as_os_str().is_empty())
    };

    if let Some(path) = candidate {
        return match CollectionStore::open(&path) {
            Ok(store) => (Some(AppState::new(store)), None),
            Err(error) => (None, Some(describe_pending(path, error))),
        };
    }

    let root = data.join("colecao");
    let store = if root.join("opencollection.yml").exists() {
        CollectionStore::open(&root)
    } else {
        init_collection(&root, "Minha API")
    };
    match store {
        Ok(store) => (Some(AppState::new(store)), None),
        Err(error) => (None, Some(describe_pending(root, error))),
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data)?;
            let last_collection = data.join("last-collection.txt");
            let mut args = std::env::args().skip(1);
            let explicit = match args.next().as_deref() {
                None => None,
                Some("--collection") => Some(PathBuf::from(
                    args.next().ok_or("--collection exige um diretório")?,
                )),
                Some(_) => return Err("uso: reqlivo-desktop [--collection <diretório>]".into()),
            };
            let (application, pending) = boot_collection(explicit, &last_collection, &data);
            if let Some(store) = application.as_ref().and_then(|application| {
                application
                    .store
                    .try_read()
                    .ok()
                    .map(|store| store.root.to_string_lossy().to_string())
            }) {
                let _ = std::fs::write(&last_collection, store.as_bytes());
            }
            app.manage(DesktopState {
                application: RwLock::new(application),
                pending: RwLock::new(pending),
                last_collection,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            api_request,
            collection_status,
            open_collection,
            migration_inspect,
            migration_apply,
            migration_rollback,
            cancel_pending,
            save_export,
            confirm_action,
            close_window
        ])
        .run(tauri::generate_context!())
        .expect("não foi possível iniciar o Reqlivo");
}
