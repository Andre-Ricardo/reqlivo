//! Protocolo interno da aplicação, executado em memória pelo adaptador IPC.
//! Não abre sockets nem serve a interface. Mantém os contratos do núcleo.

use crate::error::{Error, ErrorInfo};
use crate::import_export::{self, ExportFormat, ImportFormat};
use crate::log::LocalLog;
use crate::model::{
    CollectionManifest, CollectionSettings, Environment, SecretProvider, SecretRef, Variable,
};
use crate::runner::{self, ExecuteOptions, RunFilters, RunOptions, RunReport};
use crate::store::{CollectionSnapshot, CollectionStore, RequestPayload};
use axum::extract::{Path, Query, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<RwLock<CollectionStore>>,
    pub root: PathBuf,
    pub log: LocalLog,
    pub cookie_jar: Arc<reqwest::cookie::Jar>,
    tokens: Arc<Mutex<HashMap<String, CancellationToken>>>,
    runs: Arc<Mutex<HashMap<String, StoredRun>>>,
}

impl AppState {
    pub async fn cancel_all(&self) {
        for token in self.tokens.lock().await.values() {
            token.cancel();
        }
    }

    pub fn new(store: CollectionStore) -> Self {
        let root = store.root.clone();
        Self {
            store: Arc::new(RwLock::new(store)),
            log: LocalLog::new(&root),
            root,
            cookie_jar: Arc::new(reqwest::cookie::Jar::default()),
            tokens: Arc::new(Mutex::new(HashMap::new())),
            runs: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum StoredRun {
    Running { started_at: String },
    Finished { report: RunReport },
    Failed { error: ErrorInfo },
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/collection", get(snapshot))
        .route("/api/collection/reload", post(reload))
        .route("/api/requests", post(create_request))
        .route(
            "/api/requests/{id}",
            get(get_request).put(save_request).delete(delete_request),
        )
        .route("/api/folders", post(create_folder))
        .route(
            "/api/folders/{id}",
            put(rename_folder).delete(delete_folder),
        )
        .route(
            "/api/environments",
            get(list_environments).post(create_environment),
        )
        .route(
            "/api/environments/{name}",
            put(save_environment).delete(delete_environment),
        )
        .route("/api/settings", put(save_settings))
        .route("/api/send", post(send))
        .route("/api/send/{id}/cancel", post(cancel_send))
        .route("/api/run", post(start_run))
        .route("/api/run/{id}", get(get_run))
        .route("/api/run/{id}/cancel", post(cancel_run))
        .route("/api/history", get(history))
        .route("/api/logs", get(logs))
        .route("/api/import", post(import))
        .route("/api/export", post(export))
        .route("/api/validate", post(validate))
        .fallback(get(fallback))
        .layer(middleware::from_fn(loopback_host_guard))
        .with_state(state)
}

/// Rejeita requisições cujo `Host` não seja loopback. O bind já é local, mas
/// isso reduz o risco de DNS rebinding em navegadores.
async fn loopback_host_guard(request: Request, next: Next) -> Response {
    if let Some(host) = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    {
        let host = host_name(host);
        if ensure_loopback(host).is_err() {
            return (
                StatusCode::FORBIDDEN,
                "Host não loopback recusado por segurança",
            )
                .into_response();
        }
    }
    next.run(request).await
}

fn host_name(value: &str) -> &str {
    if value.parse::<IpAddr>().is_ok() {
        return value;
    }
    if let Some(rest) = value.strip_prefix('[') {
        if let Some((host, _)) = rest.split_once(']') {
            return host;
        }
    }
    value
        .rsplit_once(':')
        .map(|(host, _)| host)
        .filter(|host| !host.is_empty())
        .unwrap_or(value)
}

// ---------------------------------------------------------------------------
// Erros da API
// ---------------------------------------------------------------------------

pub struct ApiError(Error);

impl From<Error> for ApiError {
    fn from(value: Error) -> Self {
        Self(value)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let info = self.0.info();
        let status = match info.category {
            crate::error::ErrorCategory::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            crate::error::ErrorCategory::Storage => StatusCode::CONFLICT,
            _ => StatusCode::BAD_REQUEST,
        };
        (status, Json(json!({ "error": info }))).into_response()
    }
}

type ApiResult<T> = std::result::Result<Json<T>, ApiError>;

// ---------------------------------------------------------------------------
// Rota desconhecida
// ---------------------------------------------------------------------------

async fn fallback() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "recurso não encontrado")
}

// ---------------------------------------------------------------------------
// Estado da coleção
// ---------------------------------------------------------------------------

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let store = state.store.read().await;
    let manifest = &store.manifest;
    Json(json!({
        "name": "reqlivo",
        "version": crate::VERSION,
        "collection": manifest.info.name,
        "root": state.root,
        "offline": offline_active(&store),
        "warnings": store.warnings,
    }))
}

async fn snapshot(State(state): State<AppState>) -> Json<CollectionSnapshot> {
    let store = state.store.read().await;
    Json(store.snapshot())
}

async fn reload(State(state): State<AppState>) -> ApiResult<CollectionSnapshot> {
    let store = CollectionStore::open(&state.root)?;
    let snapshot = store.snapshot();
    *state.store.write().await = store;
    Ok(Json(snapshot))
}

fn offline_active(store: &CollectionStore) -> bool {
    store.manifest.settings().offline_only
        || std::env::var("REQLIVO_OFFLINE")
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateRequestBody {
    parent_id: Option<String>,
    name: String,
    method: String,
    url: String,
}

async fn create_request(
    State(state): State<AppState>,
    Json(body): Json<CreateRequestBody>,
) -> ApiResult<RequestPayload> {
    let mut store = state.store.write().await;
    let payload =
        store.create_request(body.parent_id.as_deref(), body.name, body.method, body.url)?;
    state.log.event(
        "info",
        "request.create",
        "requisição criada",
        json!({ "requestId": payload.id, "path": payload.path }),
        &[],
    );
    Ok(Json(payload))
}

#[derive(Debug, Serialize)]
struct RequestView {
    payload: RequestPayload,
    tags: Vec<String>,
    folder_path: Vec<String>,
}

async fn get_request(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<RequestView> {
    let store = state.store.read().await;
    let entry = store.request(&id)?;
    let payload = store.request_payload(&id)?;
    let folder_path = store
        .folder_chain(entry.parent.as_deref())
        .into_iter()
        .map(|folder| folder.name.clone())
        .collect();
    let tags = store.request_tags(&id).unwrap_or_default();
    Ok(Json(RequestView {
        payload,
        tags,
        folder_path,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveRequestBody {
    request: crate::model::RequestFile,
    expected_revision: Option<String>,
}

async fn save_request(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SaveRequestBody>,
) -> ApiResult<RequestPayload> {
    let mut store = state.store.write().await;
    let payload = store.save_request(&id, body.request, body.expected_revision.as_deref())?;
    state.log.event(
        "info",
        "request.save",
        "requisição salva",
        json!({ "requestId": id, "path": payload.path }),
        &[],
    );
    Ok(Json(payload))
}

async fn delete_request(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<serde_json::Value> {
    let mut store = state.store.write().await;
    let trash = store.delete_request(&id)?;
    state.log.event(
        "info",
        "request.delete",
        "requisição movida para a lixeira",
        json!({ "requestId": id, "trash": trash }),
        &[],
    );
    Ok(Json(json!({ "trash": trash })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateFolderBody {
    parent_id: Option<String>,
    name: String,
}

async fn create_folder(
    State(state): State<AppState>,
    Json(body): Json<CreateFolderBody>,
) -> ApiResult<serde_json::Value> {
    let mut store = state.store.write().await;
    let id = store.add_folder(body.parent_id.as_deref(), body.name)?;
    Ok(Json(json!({ "id": id })))
}

#[derive(Debug, Deserialize)]
struct RenameFolderBody {
    name: String,
}

async fn rename_folder(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RenameFolderBody>,
) -> ApiResult<serde_json::Value> {
    let mut store = state.store.write().await;
    store.rename_folder(&id, body.name)?;
    Ok(Json(json!({ "id": id })))
}

async fn delete_folder(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<serde_json::Value> {
    let mut store = state.store.write().await;
    let trash = store.delete_folder(&id)?;
    Ok(Json(json!({ "trash": trash })))
}

// ---------------------------------------------------------------------------
// Ambientes e configurações
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SecretRefView {
    name: String,
    environment: Option<String>,
    provider: SecretProvider,
    key: String,
    available: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EnvironmentsView {
    environments: Vec<Environment>,
    secret_refs: Vec<SecretRefView>,
    default_environment: Option<String>,
}

async fn list_environments(State(state): State<AppState>) -> Json<EnvironmentsView> {
    let store = state.store.read().await;
    let secret_refs = store
        .manifest
        .secret_refs()
        .iter()
        .map(|reference| SecretRefView {
            name: reference.name.clone(),
            environment: reference.environment.clone(),
            provider: reference.provider,
            key: reference.key.clone(),
            available: reference.provider == SecretProvider::Env
                && std::env::var(&reference.key).is_ok(),
        })
        .collect();
    Json(EnvironmentsView {
        environments: store.manifest.environments().to_vec(),
        secret_refs,
        default_environment: store.default_environment(),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvironmentVariableInput {
    name: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    secret: bool,
    #[serde(default)]
    disabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SecretRefInput {
    name: String,
    key: String,
    #[serde(default)]
    provider: Option<SecretProvider>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveEnvironmentBody {
    name: Option<String>,
    #[serde(default)]
    variables: Vec<EnvironmentVariableInput>,
    #[serde(default, alias = "secretRefs")]
    secret_references: Vec<SecretRefInput>,
    #[serde(default)]
    extends: Option<String>,
    #[serde(default)]
    make_default: bool,
}

async fn create_environment(
    State(state): State<AppState>,
    Json(body): Json<SaveEnvironmentBody>,
) -> ApiResult<serde_json::Value> {
    let name = body
        .name
        .clone()
        .ok_or_else(|| Error::Validation("o nome do ambiente é obrigatório".into()))?;
    save_environment_inner(state, name, body).await
}

async fn save_environment(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<SaveEnvironmentBody>,
) -> ApiResult<serde_json::Value> {
    save_environment_inner(state, name, body).await
}

async fn save_environment_inner(
    state: AppState,
    name: String,
    body: SaveEnvironmentBody,
) -> ApiResult<serde_json::Value> {
    let mut store = state.store.write().await;
    let mut variables = Vec::new();
    let mut references = Vec::new();
    for variable in &body.variables {
        if variable.name.trim().is_empty() {
            return Err(
                Error::Validation("toda variável de ambiente precisa de nome".into()).into(),
            );
        }
        if variable.secret {
            let mut secret = Variable::secret(variable.name.clone());
            secret.disabled = variable.disabled;
            variables.push(secret);
            let reference = body
                .secret_references
                .iter()
                .find(|reference| reference.name == variable.name);
            let Some(reference) = reference else {
                return Err(Error::Validation(format!(
                    "a variável secreta '{}' exige uma referência (provider/key)",
                    variable.name
                ))
                .into());
            };
            if reference.key.trim().is_empty() {
                return Err(Error::Validation(format!(
                    "a referência da variável secreta '{}' está sem chave",
                    variable.name
                ))
                .into());
            }
            references.push(SecretRef {
                name: reference.name.clone(),
                environment: Some(name.clone()),
                provider: reference.provider.unwrap_or(SecretProvider::Env),
                key: reference.key.clone(),
                description: None,
                extra: BTreeMap::new(),
            });
        } else {
            let mut plain = Variable::plain(variable.name.clone(), variable.value.clone());
            plain.disabled = variable.disabled;
            variables.push(plain);
        }
    }
    store.upsert_environment(&name, variables, references, body.extends)?;
    if body.make_default {
        let settings = store.manifest.settings();
        store.update_settings(settings, Some(name.clone()))?;
    }
    state.log.event(
        "info",
        "environment.save",
        "ambiente salvo",
        json!({ "environment": name, "variables": body.variables.len() }),
        &[],
    );
    Ok(Json(json!({ "name": name })))
}

async fn delete_environment(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<serde_json::Value> {
    let mut store = state.store.write().await;
    store.delete_environment(&name)?;
    Ok(Json(json!({ "deleted": name })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveSettingsBody {
    settings: CollectionSettings,
    default_environment: Option<String>,
}

async fn save_settings(
    State(state): State<AppState>,
    Json(body): Json<SaveSettingsBody>,
) -> ApiResult<serde_json::Value> {
    if body.settings.max_redirects > 50 {
        return Err(Error::Validation("max_redirects deve ser no máximo 50".into()).into());
    }
    if body.settings.max_response_bytes < 1024
        || body.settings.max_response_bytes > 100 * 1024 * 1024
    {
        return Err(Error::Validation(
            "max_response_bytes deve ficar entre 1 KiB e 100 MiB".into(),
        )
        .into());
    }
    if body.settings.script_timeout_ms < 50 || body.settings.script_timeout_ms > 60_000 {
        return Err(
            Error::Validation("script_timeout_ms deve ficar entre 50 e 60000".into()).into(),
        );
    }
    let mut store = state.store.write().await;
    store.update_settings(body.settings, body.default_environment)?;
    Ok(Json(json!({ "saved": true })))
}

// ---------------------------------------------------------------------------
// Envio e runner
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendBody {
    request_id: String,
    #[serde(default)]
    request: Option<crate::model::RequestFile>,
    #[serde(default)]
    environment: Option<String>,
    /// ID opcional gerado pelo cliente para permitir cancelamento antes da
    /// resposta HTTP terminar.
    #[serde(default)]
    send_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SendResponse {
    send_id: String,
    run: crate::runner::RequestRun,
    response: Option<crate::transport::HttpResponse>,
}

async fn send(
    State(state): State<AppState>,
    Json(body): Json<SendBody>,
) -> ApiResult<SendResponse> {
    let (store, request) = {
        let store = state.store.read().await;
        let request = match body.request {
            Some(request) => request,
            None => store.request(&body.request_id)?.file.clone(),
        };
        request.validate()?;
        (store.clone(), request)
    };
    let send_id = body
        .send_id
        .as_deref()
        .filter(|id| {
            id.len() == 21
                && id.starts_with("send_")
                && id[5..]
                    .chars()
                    .all(|character| character.is_ascii_hexdigit())
        })
        .map(str::to_string)
        .unwrap_or_else(|| format!("send_{}", crate::store::short_uuid()));
    let token = CancellationToken::new();
    {
        let mut tokens = state.tokens.lock().await;
        if tokens.contains_key(&send_id) {
            return Err(
                Error::Validation(format!("envio '{send_id}' já está em andamento")).into(),
            );
        }
        tokens.insert(send_id.clone(), token.clone());
    }
    let outcome = runner::execute_single(
        &store,
        &body.request_id,
        &request,
        ExecuteOptions {
            environment: body.environment.clone(),
            cookie_jar: state.cookie_jar.clone(),
            cancellation: Some(token.clone()),
        },
    )
    .await;
    state.tokens.lock().await.remove(&send_id);
    let outcome = outcome?;
    state.log.event(
        "info",
        "request.send",
        "requisição enviada",
        json!({
            "requestId": body.request_id,
            "environment": body.environment,
            "status": outcome.run.status,
            "httpStatus": outcome.run.http_status,
            "durationMs": outcome.run.duration_ms,
            "error": outcome.run.error,
        }),
        &[],
    );
    Ok(Json(SendResponse {
        send_id,
        run: outcome.run,
        response: outcome.response,
    }))
}

async fn cancel_send(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<serde_json::Value> {
    let tokens = state.tokens.lock().await;
    let Some(token) = tokens.get(&id) else {
        return Err(Error::Validation(format!("envio '{id}' não está em andamento")).into());
    };
    token.cancel();
    Ok(Json(json!({ "cancelled": id })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartRunBody {
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    filters: RunFilters,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartRunResponse {
    run_id: String,
}

async fn start_run(
    State(state): State<AppState>,
    Json(body): Json<StartRunBody>,
) -> ApiResult<StartRunResponse> {
    let store = state.store.read().await.clone();
    let run_id = format!("run_{}", crate::store::short_uuid());
    let token = CancellationToken::new();
    state
        .tokens
        .lock()
        .await
        .insert(run_id.clone(), token.clone());
    state.runs.lock().await.insert(
        run_id.clone(),
        StoredRun::Running {
            started_at: chrono::Utc::now().to_rfc3339(),
        },
    );
    let state_for_task = state.clone();
    let run_id_for_task = run_id.clone();
    let environment = body.environment.clone();
    tokio::spawn(async move {
        let options = RunOptions {
            environment: body.environment,
            filters: body.filters,
            cookie_jar: state_for_task.cookie_jar.clone(),
            cancellation: Some(token.clone()),
        };
        let result = runner::run_collection(&store, options, BTreeMap::new()).await;
        state_for_task.tokens.lock().await.remove(&run_id_for_task);
        let mut runs = state_for_task.runs.lock().await;
        match result {
            Ok(report) => {
                state_for_task.log.history(&report);
                state_for_task.log.event(
                    "info",
                    "run.finish",
                    "runner concluído",
                    json!({
                        "runId": report.id,
                        "environment": environment,
                        "summary": report.summary,
                        "deterministicHash": report.deterministic_hash,
                    }),
                    &[],
                );
                runs.insert(run_id_for_task, StoredRun::Finished { report });
            }
            Err(error) => {
                state_for_task.log.event(
                    "error",
                    "run.error",
                    &error.to_string(),
                    json!({ "runId": run_id_for_task }),
                    &[],
                );
                runs.insert(
                    run_id_for_task,
                    StoredRun::Failed {
                        error: error.info(),
                    },
                );
            }
        }
        // Mantém apenas os 30 runs mais recentes em memória.
        if runs.len() > 30 {
            let keys: Vec<String> = runs.keys().cloned().collect();
            for key in keys.into_iter().take(runs.len() - 30) {
                runs.remove(&key);
            }
        }
    });
    Ok(Json(StartRunResponse { run_id }))
}

async fn get_run(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<StoredRun> {
    let runs = state.runs.lock().await;
    let Some(run) = runs.get(&id) else {
        return Err(Error::Validation(format!("execução '{id}' não encontrada")).into());
    };
    Ok(Json(run.clone()))
}

async fn cancel_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<serde_json::Value> {
    let tokens = state.tokens.lock().await;
    let Some(token) = tokens.get(&id) else {
        return Err(Error::Validation(format!("execução '{id}' não está em andamento")).into());
    };
    token.cancel();
    Ok(Json(json!({ "cancelled": id })))
}

#[derive(Debug, Deserialize)]
struct LimitQuery {
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    50
}

async fn history(
    State(state): State<AppState>,
    Query(query): Query<LimitQuery>,
) -> Json<serde_json::Value> {
    Json(json!({ "runs": state.log.read_history(query.limit.min(500)) }))
}

async fn logs(
    State(state): State<AppState>,
    Query(query): Query<LimitQuery>,
) -> Json<serde_json::Value> {
    Json(json!({ "events": state.log.read_events(query.limit.min(1000)) }))
}

// ---------------------------------------------------------------------------
// Importação, exportação e validação
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportBody {
    format: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    filename: Option<String>,
}

async fn import(
    State(state): State<AppState>,
    Json(body): Json<ImportBody>,
) -> ApiResult<import_export::ImportReport> {
    let format = ImportFormat::parse(&body.format)?;
    let mut store = state.store.write().await;
    let report = if let Some(path) = body.path {
        import_export::import_path(
            &mut store,
            body.parent_id.as_deref(),
            format,
            &PathBuf::from(path),
        )?
    } else {
        let content = body
            .content
            .ok_or_else(|| Error::Import("informe 'content' ou 'path' para importar".into()))?;
        import_export::import_content(
            &mut store,
            body.parent_id.as_deref(),
            format,
            &content,
            body.filename.as_deref(),
        )?
    };
    state.log.event(
        "info",
        "import.finish",
        "importação concluída",
        json!({
            "format": report.format,
            "requests": report.requests_imported,
            "folders": report.folders_imported,
            "losses": report.losses.len(),
        }),
        &[],
    );
    Ok(Json(report))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportBody {
    format: String,
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    destination: Option<String>,
}

async fn export(
    State(state): State<AppState>,
    Json(body): Json<ExportBody>,
) -> ApiResult<import_export::ExportReport> {
    let format = ExportFormat::parse(&body.format)?;
    let store = state.store.read().await;
    let report = if format == ExportFormat::Project {
        let destination = body
            .destination
            .ok_or_else(|| Error::Export("exportação de projeto exige 'destination'".into()))?;
        import_export::export_project(&store, &PathBuf::from(destination))?
    } else {
        import_export::export(&store, format, body.request_id.as_deref())?
    };
    Ok(Json(report))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ValidateBody {
    #[serde(default)]
    request_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ValidateResponse {
    valid: bool,
    issues: Vec<String>,
    warnings: Vec<String>,
    unsupported: Vec<crate::store::UnsupportedItem>,
}

async fn validate(
    State(state): State<AppState>,
    Json(body): Json<ValidateBody>,
) -> ApiResult<ValidateResponse> {
    let store = state.store.read().await;
    let mut issues = store.validate();
    if let Some(id) = body.request_id {
        let request = &store.request(&id)?.file;
        if let Err(error) = request.validate() {
            issues.push(error.to_string());
        }
        if let Some(crate::model::Body::Json { data }) = &request.http.body {
            if !data.trim().is_empty() && serde_json::from_str::<serde_json::Value>(data).is_err() {
                issues.push("corpo JSON inválido".into());
            }
        }
    }
    Ok(Json(ValidateResponse {
        valid: issues.is_empty() && store.unsupported.is_empty(),
        issues,
        warnings: store.warnings.clone(),
        unsupported: store.unsupported.clone(),
    }))
}

/// Garante que uma interface informada é loopback.
pub fn ensure_loopback(host: &str) -> crate::Result<()> {
    let host = host.trim_matches(['[', ']']);
    let allowed = match host.parse::<IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => host.eq_ignore_ascii_case("localhost"),
    };
    if allowed {
        Ok(())
    } else {
        Err(Error::Config(format!(
            "a interface '{host}' não é loopback; por segurança o Reqlivo só escuta localmente"
        )))
    }
}

/// Mantém a API de manifesto disponível para consumidores externos do crate.
pub fn manifest_summary(manifest: &CollectionManifest) -> serde_json::Value {
    json!({
        "name": manifest.info.name,
        "version": manifest.opencollection,
        "environments": manifest.environments().iter().map(|env| env.name.clone()).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_check_rejects_external_interfaces() {
        assert!(ensure_loopback("127.0.0.1").is_ok());
        assert!(ensure_loopback("localhost").is_ok());
        assert!(ensure_loopback("::1").is_ok());
        assert!(ensure_loopback("0.0.0.0").is_err());
        assert!(ensure_loopback("192.168.0.1").is_err());
    }

    #[test]
    fn host_header_parsing_handles_ports_and_ipv6() {
        assert_eq!(host_name("127.0.0.1:8787"), "127.0.0.1");
        assert_eq!(host_name("localhost:8787"), "localhost");
        assert_eq!(host_name("[::1]:8787"), "::1");
        assert_eq!(host_name("::1"), "::1");
        assert_eq!(host_name("example.com:80"), "example.com");
    }
}
