#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use reqlivo::desktop::{IpcResponse, dispatch};
use reqlivo::server::AppState;
use reqlivo::store::{CollectionStore, init_collection};
use serde_json::Value;
use std::path::PathBuf;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_dialog::MessageDialogButtons;
use tokio::sync::RwLock;

struct DesktopState {
    application: RwLock<AppState>,
    last_collection: PathBuf,
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
    state.application.read().await.cancel_all().await;
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
    dispatch(application.clone(), path, method, body).await
}

#[tauri::command]
async fn open_collection(
    app: tauri::AppHandle,
    state: State<'_, DesktopState>,
) -> Result<bool, String> {
    let folder = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Abrir coleção OpenCollection")
            .blocking_pick_folder()
    })
    .await
    .map_err(|error| error.to_string())?;
    let Some(folder) = folder else {
        return Ok(false);
    };
    let path = folder.into_path().map_err(|error| error.to_string())?;
    let store = CollectionStore::open(&path).map_err(|error| error.to_string())?;
    let mut application = state.application.write().await;
    std::fs::write(
        &state.last_collection,
        store.root.to_string_lossy().as_bytes(),
    )
    .map_err(|error| error.to_string())?;
    application.cancel_all().await;
    *application = AppState::new(store);
    Ok(true)
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
            let store = if let Some(path) = explicit {
                let path = reqlivo::desktop::collection_argument(
                    path,
                    std::env::var_os("APPIMAGE").is_some(),
                    std::env::var_os("OWD").map(PathBuf::from),
                    std::env::var_os("PWD").map(PathBuf::from),
                );
                CollectionStore::open(&path)?
            } else if let Some(store) = std::fs::read_to_string(&last_collection)
                .ok()
                .and_then(|path| CollectionStore::open(std::path::Path::new(&path)).ok())
            {
                store
            } else {
                let root = data.join("colecao");
                if root.join("opencollection.yml").exists() {
                    CollectionStore::open(&root)?
                } else {
                    init_collection(&root, "Minha API")?
                }
            };
            app.manage(DesktopState {
                application: RwLock::new(AppState::new(store)),
                last_collection,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            api_request,
            open_collection,
            save_export,
            confirm_action,
            close_window
        ])
        .run(tauri::generate_context!())
        .expect("não foi possível iniciar o Reqlivo");
}
