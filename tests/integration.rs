use reqlivo::import_export::{self, ExportFormat, ImportFormat};
use reqlivo::model::{
    Assertion, AuthConfig, CollectionSettings, Param, ParamKind, RequestDefaults, Script,
    ScriptPhase, SecretProvider, SecretRef, Variable,
};
use reqlivo::runner::{self, ExecuteOptions, RunFilters, RunOptions, RunStatus};
use reqlivo::server::{self, AppState};
use reqlivo::store::{CollectionStore, init_collection};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Servidor HTTP sintético de teste
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
#[allow(dead_code)]
struct Recorded {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: String,
}

#[derive(Clone)]
struct TestState {
    recorded: Arc<Mutex<Vec<Recorded>>>,
}

impl TestState {
    fn record(&self, request: Recorded) {
        self.recorded.lock().unwrap().push(request);
    }

    fn last(&self) -> Recorded {
        self.recorded.lock().unwrap().last().cloned().unwrap()
    }
}

struct TestHttp {
    base: String,
    state: TestState,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for TestHttp {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn start_test_http() -> TestHttp {
    use axum::Router;
    use axum::body::to_bytes;
    use axum::extract::{Request, State};
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::{IntoResponse, Json, Response};
    use axum::routing::get;

    async fn record(State(state): State<TestState>, request: Request, response: Value) -> Response {
        let (parts, body) = request.into_parts();
        let body = to_bytes(body, 2 * 1024 * 1024)
            .await
            .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
            .unwrap_or_default();
        let headers: HeaderMap = parts.headers;
        state.record(Recorded {
            method: parts.method.to_string(),
            path: parts
                .uri
                .path_and_query()
                .map(|value| value.as_str().to_string())
                .unwrap_or_default(),
            headers: headers
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|value| (name.as_str().to_string(), value.to_string()))
                })
                .collect(),
            body,
        });
        Json(response).into_response()
    }

    async fn ok(State(state): State<TestState>, request: Request) -> Response {
        record(
            State(state),
            request,
            json!({"ok": true, "items": [{"name": "alpha"}]}),
        )
        .await
    }

    async fn echo(State(state): State<TestState>, request: Request) -> Response {
        let method = request.method().to_string();
        let path = request
            .uri()
            .path_and_query()
            .map(|value| value.as_str().to_string())
            .unwrap_or_default();
        record(
            State(state),
            request,
            json!({"ok": true, "method": method, "path": path}),
        )
        .await
    }

    async fn not_found(State(state): State<TestState>, request: Request) -> Response {
        let response = record(
            State(state),
            request,
            json!({"error": "not_found", "synthetic": true}),
        )
        .await;
        (StatusCode::NOT_FOUND, response).into_response()
    }

    async fn slow(State(state): State<TestState>, request: Request) -> Response {
        tokio::time::sleep(Duration::from_millis(200)).await;
        record(State(state), request, json!({"slow": true})).await
    }

    async fn redirect_external() -> axum::response::Redirect {
        axum::response::Redirect::temporary("https://example.com/nao-deve-sair")
    }

    async fn redirect_local() -> axum::response::Redirect {
        axum::response::Redirect::temporary("/ok")
    }

    let state = TestState {
        recorded: Arc::new(Mutex::new(Vec::new())),
    };
    let app = Router::new()
        .route("/ok", get(ok))
        .route("/echo", get(echo).post(echo))
        .route("/status/404", get(not_found))
        .route("/slow", get(slow))
        .route("/redirect-external", get(redirect_external))
        .route("/redirect-local", get(redirect_local))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    TestHttp {
        base: format!("http://{address}"),
        state,
        handle,
    }
}

// ---------------------------------------------------------------------------
// Ajudantes de coleção
// ---------------------------------------------------------------------------

fn test_collection(base_url: &str) -> (TempDir, CollectionStore) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = init_collection(dir.path(), "Teste de integração").unwrap();
    store
        .upsert_environment(
            "dev",
            vec![Variable::plain("baseUrl", base_url)],
            Vec::new(),
            None,
        )
        .unwrap();
    (dir, store)
}

fn add_assertion(request: &mut reqlivo::model::RequestFile, expression: &str, value: &str) {
    request.runtime.assertions.push(Assertion {
        expression: expression.to_string(),
        operator: "eq".into(),
        value: Some(serde_yaml::Value::String(value.to_string())),
        disabled: false,
        description: None,
        extra: Default::default(),
    });
}

fn add_script(request: &mut reqlivo::model::RequestFile, phase: ScriptPhase, code: &str) {
    request.runtime.scripts.push(Script {
        phase,
        code: code.to_string(),
        extra: Default::default(),
    });
}

async fn execute(store: &CollectionStore, id: &str) -> runner::ExecutionOutcome {
    let request = store.request(id).unwrap().file.clone();
    runner::execute_single(
        store,
        id,
        &request,
        ExecuteOptions {
            environment: Some("dev".into()),
            cookie_jar: Arc::new(reqwest::cookie::Jar::default()),
            cancellation: None,
        },
    )
    .await
    .unwrap()
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn http_success_metadata_assertions_and_scripts() {
    let http = start_test_http().await;
    let (_dir, mut store) = test_collection(&http.base);
    let payload = store
        .create_request(None, "OK", "GET", "{{baseUrl}}/ok")
        .unwrap();
    let mut request = payload.request;
    add_assertion(&mut request, "res.status", "200");
    add_assertion(&mut request, "res.body.ok", "true");
    add_script(
        &mut request,
        ScriptPhase::Tests,
        "test('itens', function () { expect(rq.response.json().items.length).toBe(1); });",
    );
    store.save_request(&payload.id, request, None).unwrap();

    let outcome = execute(&store, &payload.id).await;
    let run = outcome.run;
    let response = outcome.response.unwrap();
    assert_eq!(run.status, RunStatus::Passed);
    assert_eq!(run.http_status, Some(200));
    assert_eq!(response.status, 200);
    assert_eq!(response.status_text, "OK");
    assert_eq!(run.tests.len(), 3);
    assert!(run.tests.iter().all(|test| test.passed));
    assert_eq!(response.size_bytes, response.body_bytes.len());
    assert!(
        response
            .body_text
            .as_deref()
            .unwrap()
            .contains("\"ok\":true")
    );
    assert!(!response.truncated);
    assert!(!response.redirected);
}

#[tokio::test]
async fn no_tests_is_executed_not_passed_and_404_is_preserved() {
    let http = start_test_http().await;
    let (_dir, mut store) = test_collection(&http.base);
    let payload = store
        .create_request(None, "404", "GET", "{{baseUrl}}/status/404")
        .unwrap();
    let outcome = execute(&store, &payload.id).await;
    assert_eq!(outcome.run.status, RunStatus::Executed);
    assert_eq!(outcome.run.http_status, Some(404));
    assert!(outcome.run.tests.is_empty());
    assert!(outcome.run.error.is_none());
}

#[tokio::test]
async fn connection_error_is_classified_without_panicking() {
    let (_dir, mut store) = test_collection("http://127.0.0.1:9");
    let payload = store
        .create_request(None, "Falha", "GET", "{{baseUrl}}/nao-existe")
        .unwrap();
    let outcome = execute(&store, &payload.id).await;
    assert_eq!(outcome.run.status, RunStatus::Error);
    assert_eq!(outcome.run.http_status, None);
    let error = outcome.run.error.expect("erro");
    assert_eq!(error.category, reqlivo::error::ErrorCategory::Connect);
    assert!(!error.message.is_empty());
}

#[tokio::test]
async fn variable_precedence_request_then_folder_then_environment() {
    let http = start_test_http().await;
    let (_dir, mut store) = test_collection(&http.base);
    store
        .upsert_environment(
            "dev",
            vec![
                Variable::plain("baseUrl", &http.base),
                Variable::plain("precedencia", "ambiente"),
            ],
            Vec::new(),
            None,
        )
        .unwrap();
    let folder = store.add_folder(None, "Pasta").unwrap();
    store
        .set_folder_defaults(
            &folder,
            Some(RequestDefaults {
                variables: vec![Variable::plain("precedencia", "pasta")],
                ..RequestDefaults::default()
            }),
            None,
        )
        .unwrap();
    let payload = store
        .create_request(
            Some(&folder),
            "Precedência",
            "GET",
            "{{baseUrl}}/echo?p={{precedencia}}",
        )
        .unwrap();
    let mut request = payload.request;
    request
        .runtime
        .variables
        .push(Variable::plain("precedencia", "requisicao"));
    store.save_request(&payload.id, request, None).unwrap();

    execute(&store, &payload.id).await;
    assert!(http.state.last().path.contains("p=requisicao"));

    let mut request = store.request(&payload.id).unwrap().file.clone();
    request.runtime.variables.clear();
    store.save_request(&payload.id, request, None).unwrap();
    execute(&store, &payload.id).await;
    assert!(http.state.last().path.contains("p=pasta"));

    let root = store
        .create_request(None, "Raiz", "GET", "{{baseUrl}}/echo?p={{precedencia}}")
        .unwrap();
    execute(&store, &root.id).await;
    assert!(http.state.last().path.contains("p=ambiente"));
}

#[tokio::test]
async fn auth_inherits_from_folder_and_redacts_secret_values() {
    let http = start_test_http().await;
    let (_dir, mut store) = test_collection(&http.base);
    let key = format!("REQLIVO_TEST_TOKEN_{}", std::process::id());
    let key_for_ref = key.clone();
    unsafe { std::env::set_var(&key, "segredo-sintetico-123") };
    store
        .upsert_environment(
            "dev",
            vec![
                Variable::plain("baseUrl", &http.base),
                Variable::secret("token"),
            ],
            vec![SecretRef {
                name: "token".into(),
                environment: Some("dev".into()),
                provider: SecretProvider::Env,
                key: key_for_ref,
                description: None,
                extra: Default::default(),
            }],
            None,
        )
        .unwrap();
    let folder = store.add_folder(None, "Autenticada").unwrap();
    store
        .set_folder_defaults(
            &folder,
            Some(RequestDefaults {
                auth: Some(AuthConfig::bearer("{{token}}")),
                ..RequestDefaults::default()
            }),
            None,
        )
        .unwrap();
    let payload = store
        .create_request(Some(&folder), "Bearer herdado", "GET", "{{baseUrl}}/echo")
        .unwrap();

    let outcome = execute(&store, &payload.id).await;
    assert_eq!(outcome.run.status, RunStatus::Executed);
    let recorded = http.state.last();
    assert_eq!(
        recorded.headers.get("authorization").map(String::as_str),
        Some("Bearer segredo-sintetico-123")
    );
    let auth = outcome.run.auth.as_ref().unwrap();
    assert_eq!(auth.mode, "bearer");
    assert!(auth.origin.contains("Autenticada"));
    assert!(auth.secret_used);
    let token = outcome
        .run
        .effective_variables
        .iter()
        .find(|variable| variable.name == "token")
        .unwrap();
    assert_eq!(token.value, None);
    assert_eq!(token.display_value.as_deref(), Some("••••••"));
    let json = serde_json::to_string(&outcome.run).unwrap();
    assert!(!json.contains("segredo-sintetico-123"));
    unsafe { std::env::remove_var(&key) };
}

#[tokio::test]
async fn auth_secret_flag_only_tracks_secrets_used_by_auth() {
    let http = start_test_http().await;
    let (_dir, mut store) = test_collection(&http.base);
    store
        .upsert_environment(
            "dev",
            vec![
                Variable::plain("baseUrl", &http.base),
                Variable::plain("plainToken", "token-comum"),
                Variable::secret("unusedSecret"),
            ],
            vec![SecretRef {
                name: "unusedSecret".into(),
                environment: Some("dev".into()),
                provider: SecretProvider::Env,
                key: format!("REQLIVO_UNUSED_SECRET_{}", std::process::id()),
                description: None,
                extra: Default::default(),
            }],
            None,
        )
        .unwrap();
    let payload = store
        .create_request(None, "Auth comum", "GET", "{{baseUrl}}/ok")
        .unwrap();
    let mut request = payload.request;
    request.http.auth = Some(AuthConfig::bearer("{{plainToken}}"));
    store.save_request(&payload.id, request, None).unwrap();

    let outcome = execute(&store, &payload.id).await;
    assert_eq!(outcome.run.status, RunStatus::Executed);
    let auth = outcome.run.auth.as_ref().unwrap();
    assert!(!auth.secret_used);
    assert_eq!(
        http.state
            .last()
            .headers
            .get("authorization")
            .map(String::as_str),
        Some("Bearer token-comum")
    );
}

#[tokio::test]
async fn execute_single_uses_the_collection_default_environment() {
    let http = start_test_http().await;
    let (_dir, mut store) = test_collection(&http.base);
    store
        .update_settings(CollectionSettings::default(), Some("dev".into()))
        .unwrap();
    let payload = store
        .create_request(None, "Ambiente padrão", "GET", "{{baseUrl}}/ok")
        .unwrap();
    let request = store.request(&payload.id).unwrap().file.clone();
    let outcome = runner::execute_single(
        &store,
        &payload.id,
        &request,
        ExecuteOptions {
            environment: None,
            cookie_jar: Arc::new(reqwest::cookie::Jar::default()),
            cancellation: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(outcome.run.status, RunStatus::Executed);
    assert_eq!(outcome.response.unwrap().status, 200);
}

#[tokio::test]
async fn offline_mode_blocks_external_hosts_without_dns() {
    let (_dir, mut store) = test_collection("https://example.com");
    let mut settings = CollectionSettings::default();
    settings.offline_only = true;
    store.update_settings(settings, Some("dev".into())).unwrap();
    let payload = store
        .create_request(None, "Externo", "GET", "{{baseUrl}}/bloqueado")
        .unwrap();
    let started = Instant::now();
    let outcome = execute(&store, &payload.id).await;
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(outcome.run.status, RunStatus::Error);
    let error = outcome.run.error.expect("erro offline");
    assert_eq!(error.category, reqlivo::error::ErrorCategory::Offline);
    assert!(error.message.contains("offline") || error.message.contains("local"));
}

#[tokio::test]
async fn offline_mode_follows_local_redirects_but_blocks_external_redirects() {
    let http = start_test_http().await;
    let (_dir, mut store) = test_collection(&http.base);
    let mut settings = CollectionSettings::default();
    settings.offline_only = true;
    settings.follow_redirects = true;
    settings.max_redirects = 5;
    store.update_settings(settings, Some("dev".into())).unwrap();

    let local = store
        .create_request(None, "Local", "GET", "{{baseUrl}}/redirect-local")
        .unwrap();
    let outcome = execute(&store, &local.id).await;
    let response = outcome.response.unwrap();
    assert_eq!(outcome.run.status, RunStatus::Executed);
    assert_eq!(response.status, 200);
    assert!(response.redirected);
    assert!(response.final_url.ends_with("/ok"));

    let external = store
        .create_request(None, "Externo", "GET", "{{baseUrl}}/redirect-external")
        .unwrap();
    let outcome = execute(&store, &external.id).await;
    let response = outcome.response.unwrap();
    assert_eq!(outcome.run.status, RunStatus::Executed);
    assert_eq!(response.status, 307);
    assert!(!response.redirected);
    assert!(response.final_url.contains("/redirect-external"));
}

#[tokio::test]
async fn deterministic_hash_is_stable_across_runs() {
    let http = start_test_http().await;
    let (_dir, mut store) = test_collection(&http.base);
    let payload = store
        .create_request(None, "OK", "GET", "{{baseUrl}}/ok")
        .unwrap();
    let options = || RunOptions {
        environment: Some("dev".into()),
        filters: RunFilters::default(),
        cookie_jar: Arc::new(reqwest::cookie::Jar::default()),
        cancellation: None,
    };
    let first = runner::run_collection(&store, options(), BTreeMap::new())
        .await
        .unwrap();
    let second = runner::run_collection(&store, options(), BTreeMap::new())
        .await
        .unwrap();
    assert_eq!(first.deterministic_hash, second.deterministic_hash);
    assert_eq!(first.summary.executed, 1);
    assert_eq!(first.summary.total, 1);
    assert!(store.request(&payload.id).is_ok());
}

#[tokio::test]
async fn script_timeout_is_enforced() {
    let http = start_test_http().await;
    let (_dir, mut store) = test_collection(&http.base);
    let mut settings = CollectionSettings::default();
    settings.script_timeout_ms = 100;
    store.update_settings(settings, Some("dev".into())).unwrap();
    let payload = store
        .create_request(None, "Loop", "GET", "{{baseUrl}}/ok")
        .unwrap();
    let mut request = payload.request;
    add_script(&mut request, ScriptPhase::BeforeRequest, "while (true) {}");
    store.save_request(&payload.id, request, None).unwrap();
    let started = Instant::now();
    let outcome = execute(&store, &payload.id).await;
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(outcome.run.status, RunStatus::Error);
    let error = outcome.run.error.expect("erro de script");
    assert_eq!(error.category, reqlivo::error::ErrorCategory::Script);
    assert!(http.state.recorded.lock().unwrap().is_empty());
}

#[tokio::test]
async fn import_curl_export_and_project_roundtrip() {
    let (_dir, mut store) = test_collection("http://127.0.0.1:18080");
    let report = import_export::import_content(
        &mut store,
        None,
        ImportFormat::Curl,
        "curl -X POST 'http://127.0.0.1:18080/echo' -H 'X-Test: 1' -d '{\"a\":1}'",
        Some("inline.sh"),
    )
    .unwrap();
    assert_eq!(report.requests_imported, 1);
    assert_eq!(store.requests.len(), 1);
    let request = store.all_requests_in_order()[0].file.clone();
    assert_eq!(request.http.method, "POST");
    assert!(request.http.body.is_some());

    let id = store.all_requests_in_order()[0].id.clone();
    let exported = import_export::export(&store, ExportFormat::Curl, Some(&id)).unwrap();
    let content = exported.content.unwrap();
    assert!(content.contains("curl"));
    assert!(content.contains("POST"));

    let target = tempfile::tempdir().unwrap();
    let destination = target.path().join("projeto-exportado");
    let project = import_export::export_project(&store, &destination).unwrap();
    assert!(project.files >= 2);
    let reopened = CollectionStore::open(&destination).unwrap();
    assert_eq!(reopened.info_name(), "Teste de integração");
    assert_eq!(reopened.requests.len(), 1);
}

#[tokio::test]
async fn exports_omit_literal_credentials_by_default() {
    let (_dir, mut store) = test_collection("http://127.0.0.1:18080");
    let payload = store
        .create_request(None, "Credencial literal", "GET", "{{baseUrl}}/echo")
        .unwrap();
    let mut request = payload.request;
    request.http.headers.push(reqlivo::model::Header {
        name: "Authorization".into(),
        value: "Bearer cabecalho-literal".into(),
        description: None,
        disabled: false,
        extra: Default::default(),
    });
    request.http.auth = Some(AuthConfig::bearer("token-literal"));
    request.http.url = "http://usuario:senha-url@127.0.0.1:18080/echo?token=query-literal".into();
    request.http.params.push(Param {
        name: "api_key".into(),
        value: "param-literal".into(),
        kind: ParamKind::Query,
        description: None,
        disabled: false,
        extra: Default::default(),
    });
    store.save_request(&payload.id, request, None).unwrap();
    let templated = store
        .create_request(
            None,
            "URL com template",
            "GET",
            "{{baseUrl}}/echo?token=template-literal",
        )
        .unwrap();

    let postman = import_export::export(&store, ExportFormat::Postman, None)
        .unwrap()
        .content
        .unwrap();
    assert!(!postman.contains("cabecalho-literal"));
    assert!(!postman.contains("token-literal"));
    assert!(!postman.contains("senha-url"));
    assert!(!postman.contains("query-literal"));
    assert!(!postman.contains("param-literal"));
    assert!(!postman.contains("template-literal"));

    let opencollection = import_export::export(&store, ExportFormat::OpenCollection, None)
        .unwrap()
        .content
        .unwrap();
    assert!(!opencollection.contains("cabecalho-literal"));
    assert!(!opencollection.contains("token-literal"));
    assert!(!opencollection.contains("senha-url"));
    assert!(!opencollection.contains("query-literal"));
    assert!(!opencollection.contains("param-literal"));
    assert!(!opencollection.contains("template-literal"));

    let curl = import_export::export(&store, ExportFormat::Curl, Some(&templated.id))
        .unwrap()
        .content
        .unwrap();
    assert!(!curl.contains("template-literal"));

    let target = tempfile::tempdir().unwrap();
    let destination = target.path().join("projeto-seguro");
    let report = import_export::export_project(&store, &destination).unwrap();
    assert!(!report.warnings.is_empty());
    let mut found = Vec::new();
    fn walk(directory: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, found);
            } else {
                found.push(path);
            }
        }
    }
    walk(&destination, &mut found);
    for path in found {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(!text.contains("cabecalho-literal"), "{}", path.display());
        assert!(!text.contains("token-literal"), "{}", path.display());
        assert!(!text.contains("senha-url"), "{}", path.display());
        assert!(!text.contains("query-literal"), "{}", path.display());
        assert!(!text.contains("param-literal"), "{}", path.display());
        assert!(!text.contains("template-literal"), "{}", path.display());
    }
}

#[tokio::test]
async fn postman_import_preserves_scripts_as_loss_warning() {
    let (_dir, mut store) = test_collection("http://127.0.0.1:18080");
    let postman = json!({
        "info": {"name": "Postman teste", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
        "item": [{
            "name": "Echo",
            "request": {"method": "GET", "header": [], "url": {"raw": "http://127.0.0.1:18080/echo"}},
            "event": [{"listen": "test", "script": {"exec": ["console.log('x')"]}}]
        }]
    })
    .to_string();
    let report = import_export::import_content(
        &mut store,
        None,
        ImportFormat::Postman,
        &postman,
        Some("collection.json"),
    )
    .unwrap();
    assert_eq!(report.requests_imported, 1);
    assert!(!report.warnings.is_empty() || !report.losses.is_empty());
    let request = store.all_requests_in_order()[0].file.clone();
    assert!(request.extensions.extra.contains_key("postmanScripts"));
}

#[tokio::test]
async fn desktop_send_can_be_cancelled_with_client_supplied_id() {
    use reqlivo::desktop::dispatch;

    let http = start_test_http().await;
    let (_dir, mut store) = test_collection(&http.base);
    let payload = store
        .create_request(None, "Lenta", "GET", "{{baseUrl}}/slow")
        .unwrap();
    let state = AppState::new(store);
    let send_id = format!("send_{}", "a".repeat(16));
    let body = json!({ "sendId": send_id, "requestId": payload.id, "environment": "dev" });
    let send = dispatch(state.clone(), "/api/send".into(), "POST".into(), Some(body));
    let cancel_id = send_id.clone();
    let cancel = async move {
        tokio::time::sleep(Duration::from_millis(40)).await;
        dispatch(
            state,
            format!("/api/send/{cancel_id}/cancel"),
            "POST".into(),
            None,
        )
        .await
        .unwrap()
    };
    let (send_response, cancel_response) = tokio::join!(send, cancel);
    let send_response = send_response.unwrap();
    assert_eq!(cancel_response.status, 200);
    assert_eq!(send_response.status, 200);
    assert_eq!(send_response.data["run"]["status"], "cancelled");
    assert_eq!(send_response.data["sendId"], send_id);
}

#[tokio::test]
async fn server_api_exposes_collection_and_environment_editor() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let (_dir, store) = test_collection("http://127.0.0.1:18080");
    let state = AppState::new(store);
    let app = server::router(state.clone());

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = json!({
        "name": "teste",
        "variables": [
            {"name": "baseUrl", "value": "http://127.0.0.1:18080"},
            {"name": "segredo", "secret": true}
        ],
        "secretRefs": [
            {"name": "segredo", "key": "REQLIVO_TEST_API_KEY", "provider": "env"}
        ]
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/environments")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let store = state.store.read().await;
    let environment = store
        .manifest
        .environments()
        .iter()
        .find(|environment| environment.name == "teste")
        .unwrap();
    assert!(environment.variables.iter().any(|variable| variable.secret));
    assert!(store
        .manifest
        .secret_refs()
        .iter()
        .any(|reference| reference.name == "segredo" && reference.key == "REQLIVO_TEST_API_KEY"));
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (axum::http::StatusCode, Value) {
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

    let builder = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

fn error_code(value: &Value) -> &str {
    value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

#[tokio::test]
async fn drafts_api_roundtrips_without_touching_canonical_yaml() {
    use axum::http::StatusCode;

    let (_dir, mut store) = test_collection("http://127.0.0.1:18080");
    let payload = store
        .create_request(None, "Rascunho", "GET", "http://127.0.0.1/echo")
        .unwrap();
    let canonical_path = store.request(&payload.id).unwrap().path.clone();
    let canonical_before = std::fs::read(&canonical_path).unwrap();
    let state = AppState::new(store);
    let app = server::router(state);

    let mut edited = payload.request.clone();
    edited.http.url = "http://127.0.0.1/editado".into();
    let (status, saved) = call(
        &app,
        "PUT",
        &format!("/api/drafts/{}", payload.id),
        Some(json!({
            "request": edited,
            "baseRevision": payload.revision,
            "expectedGeneration": 0,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["generation"], 1);
    assert_eq!(std::fs::read(&canonical_path).unwrap(), canonical_before);

    let (status, detail) = call(&app, "GET", &format!("/api/drafts/{}", payload.id), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["summary"]["state"], "current");
    assert_eq!(
        detail["record"]["request"]["http"]["url"],
        "http://127.0.0.1/editado"
    );

    let (status, listed) = call(&app, "GET", "/api/drafts", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["drafts"].as_array().unwrap().len(), 1);

    let (status, conflict) = call(
        &app,
        "PUT",
        &format!("/api/drafts/{}", payload.id),
        Some(json!({
            "request": payload.request,
            "baseRevision": payload.revision,
            "expectedGeneration": 0,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&conflict), "draft_generation");

    let (status, missing) = call(&app, "GET", "/api/drafts/req_0000000000000000", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(missing["summary"]["state"], "absent");
    assert!(missing["record"].is_null());
}

#[tokio::test]
async fn changes_endpoint_detects_external_edits_and_reload_rejects_stale_generation() {
    use axum::http::StatusCode;

    let (_dir, mut store) = test_collection("http://127.0.0.1:18080");
    let payload = store
        .create_request(None, "Externa", "GET", "http://127.0.0.1/echo")
        .unwrap();
    let path = store.request(&payload.id).unwrap().path.clone();
    let state = AppState::new(store);
    let app = server::router(state);

    let (status, snapshot) = call(&app, "GET", "/api/collection", None).await;
    assert_eq!(status, StatusCode::OK);
    let confirmed = snapshot["generation"].as_str().unwrap().to_string();

    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("http://127.0.0.1/echo", "http://127.0.0.1/externo");
    std::fs::write(&path, text).unwrap();

    let (status, changes) = call(
        &app,
        "GET",
        &format!("/api/collection/changes?generation={confirmed}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(changes["changed"], true);
    assert_eq!(changes["cursorStale"], true);
    assert_eq!(changes["reconcile"], true);
    assert!(
        changes["paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path.as_str() == Some(payload.path.as_str()))
    );
    let external_generation = changes["generation"].as_str().unwrap().to_string();

    let (status, conflict) = call(
        &app,
        "POST",
        &format!("/api/collection/reload?generation={confirmed}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&conflict), "generation_conflict");

    let (status, unchanged) = call(&app, "GET", "/api/collection", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unchanged["generation"], confirmed);

    let (status, reloaded) = call(
        &app,
        "POST",
        &format!("/api/collection/reload?generation={external_generation}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reloaded["generation"], external_generation);
}

#[tokio::test]
async fn trash_api_deletes_reports_and_restores_with_content_revision() {
    use axum::http::StatusCode;

    let (dir, mut store) = test_collection("http://127.0.0.1:18080");
    let payload = store
        .create_request(None, "Descartável", "GET", "http://127.0.0.1/echo")
        .unwrap();
    let path = store.request(&payload.id).unwrap().path.clone();
    let state = AppState::new(store);
    let app = server::router(state);

    let (status, deleted) = call(
        &app,
        "DELETE",
        &format!(
            "/api/requests/{}?expectedRevision={}",
            payload.id, payload.revision
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let entry_id = deleted["trash"].as_str().unwrap().to_string();
    assert!(!path.exists());

    let (status, listed) = call(&app, "GET", "/api/trash", None).await;
    assert_eq!(status, StatusCode::OK);
    let entry = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == entry_id)
        .unwrap()
        .clone();
    assert_eq!(entry["state"], "trashed");
    assert_eq!(entry["kind"], "request");
    let content_revision = entry["entryRevision"].as_str().unwrap().to_string();

    let (status, restored) = call(
        &app,
        "POST",
        &format!("/api/trash/{entry_id}/restore"),
        Some(json!({ "revision": content_revision, "name": "Restaurada" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(restored["alreadyRestored"], false);
    let restored_path = dir.path().join(restored["path"].as_str().unwrap());
    assert!(restored_path.exists());
    assert_eq!(
        restored_path.file_name().and_then(|name| name.to_str()),
        Some("restaurada.yml")
    );
    assert!(
        std::fs::read_to_string(&restored_path)
            .unwrap()
            .contains("Descartável")
    );

    let (status, again) = call(
        &app,
        "POST",
        &format!("/api/trash/{entry_id}/restore"),
        Some(json!({ "revision": content_revision })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&again), "restore_conflict");

    let (status, stale) = call(
        &app,
        "POST",
        &format!("/api/trash/{entry_id}/restore"),
        Some(json!({ "revision": "revisao-antiga" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&stale), "revision_conflict");
}

#[tokio::test]
async fn manifest_save_rejects_stale_expected_revision_and_preserves_form() {
    use axum::http::StatusCode;

    let (_dir, mut store) = test_collection("http://127.0.0.1:18080");
    let payload = store
        .create_request(None, "Edição", "GET", "http://127.0.0.1/echo")
        .unwrap();
    let state = AppState::new(store);
    let app = server::router(state);

    let (status, view) = call(&app, "GET", &format!("/api/requests/{}", payload.id), None).await;
    assert_eq!(status, StatusCode::OK);
    let revision = view["payload"]["revision"].as_str().unwrap().to_string();
    let mut request = view["payload"]["request"].clone();
    request["http"]["url"] = json!("http://127.0.0.1/alterado");

    let (status, conflict) = call(
        &app,
        "PUT",
        &format!("/api/requests/{}", payload.id),
        Some(json!({ "request": request, "expectedRevision": "revisao-antiga" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&conflict), "revision_conflict");

    let (status, saved) = call(
        &app,
        "PUT",
        &format!("/api/requests/{}", payload.id),
        Some(json!({ "request": request, "expectedRevision": revision })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["request"]["http"]["url"], "http://127.0.0.1/alterado");
}

#[tokio::test]
async fn migration_api_requires_confirmation_and_blocks_writes_after_rollback() {
    use axum::http::StatusCode;

    let (dir, mut store) = test_collection("http://127.0.0.1:18080");
    let _payload = store
        .create_request(None, "Migrável", "GET", "http://127.0.0.1/echo")
        .unwrap();
    let manifest_path = reqlivo::store::find_manifest(dir.path()).unwrap();
    let text = std::fs::read_to_string(&manifest_path)
        .unwrap()
        .replace("schemaVersion: 1", "schemaVersion: 0");
    std::fs::write(&manifest_path, text).unwrap();

    let state = AppState::new(store);
    let app = server::router(state);

    let (status, inspected) = call(&app, "POST", "/api/migrations/inspect", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(inspected["inspection"]["required"], true);
    let plan_id = inspected["inspection"]["planId"]
        .as_str()
        .unwrap()
        .to_string();
    let revision = inspected["inspection"]["revision"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, unconfirmed) = call(
        &app,
        "POST",
        "/api/migrations/apply",
        Some(json!({ "planId": plan_id, "revision": revision })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&unconfirmed), "confirmation_required");

    let (status, stale) = call(
        &app,
        "POST",
        "/api/migrations/apply",
        Some(json!({
            "planId": plan_id,
            "revision": "revisao-antiga",
            "confirm": true,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&stale), "revision_conflict");

    let (status, applied) = call(
        &app,
        "POST",
        "/api/migrations/apply",
        Some(json!({
            "planId": plan_id,
            "revision": revision,
            "confirm": true,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(applied["outcome"]["state"], "applied");

    let (status, listed) = call(&app, "GET", "/api/migrations", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        listed["operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|operation| operation["id"] == plan_id && operation["rollbackEligible"] == true)
    );

    let (status, rolled_back) = call(
        &app,
        "POST",
        &format!("/api/migrations/{plan_id}/rollback"),
        Some(json!({ "revision": revision, "confirm": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rolled_back["state"], "rolled_back");

    let (status, blocked) = call(
        &app,
        "POST",
        "/api/requests",
        Some(json!({
            "name": "Bloqueada",
            "method": "GET",
            "url": "http://127.0.0.1/echo",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&blocked), "collection_unavailable");

    let (status, _still_readable) = call(&app, "GET", "/api/collection", None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, reinspected) = call(&app, "POST", "/api/migrations/inspect", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reinspected["inspection"]["required"], true);
    let next_plan = reinspected["inspection"]["planId"]
        .as_str()
        .unwrap()
        .to_string();
    let next_revision = reinspected["inspection"]["revision"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, reapplied) = call(
        &app,
        "POST",
        "/api/migrations/apply",
        Some(json!({
            "planId": next_plan,
            "revision": next_revision,
            "confirm": true,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reapplied["outcome"]["state"], "applied");

    let (status, created) = call(
        &app,
        "POST",
        "/api/requests",
        Some(json!({
            "name": "Liberada",
            "method": "GET",
            "url": "http://127.0.0.1/echo",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(created["request"]["info"]["name"], "Liberada");
    assert!(CollectionStore::open(dir.path()).is_ok());
}

#[tokio::test]
async fn project_export_and_trash_listing_do_not_leak_deleted_content() {
    use axum::http::StatusCode;

    let (dir, mut store) = test_collection("http://127.0.0.1:18080");
    let marker = "conteudo-sintetico-exclusivo-da-lixeira";
    let payload = store
        .create_request(None, "Item descartado", "POST", "http://127.0.0.1/echo")
        .unwrap();
    let mut request = payload.request;
    request.http.headers.push(reqlivo::model::Header {
        name: "X-Marcador".into(),
        value: marker.into(),
        description: None,
        disabled: false,
        extra: Default::default(),
    });
    store
        .save_request(&payload.id, request, Some(&payload.revision))
        .unwrap();
    let revision = store.request(&payload.id).unwrap().revision.clone();
    let state = AppState::new(store);
    let app = server::router(state.clone());

    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/api/requests/{}?expectedRevision={revision}", payload.id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, listed) = call(&app, "GET", "/api/trash", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!listed.to_string().contains(marker));
    let log = std::fs::read_to_string(dir.path().join(".reqlivo/logs/events.jsonl")).unwrap();
    assert!(!log.contains(marker));

    let store = state.store.read().await;
    let target = tempfile::tempdir().unwrap();
    let destination = target.path().join("exportado");
    import_export::export_project(&store, &destination).unwrap();
    assert!(!destination.join(".reqlivo").exists());

    fn walk(directory: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, files);
            } else {
                files.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(&destination, &mut files);
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(!text.contains(marker), "{}", path.display());
    }
}

fn tree_has_id(nodes: &Value, id: &str) -> bool {
    nodes
        .as_array()
        .map(|items| {
            items
                .iter()
                .any(|item| item["id"] == id || tree_has_id(&item["children"], id))
        })
        .unwrap_or(false)
}

#[tokio::test]
async fn failed_canonical_save_keeps_the_draft_and_both_versions() {
    use axum::http::StatusCode;

    let (dir, mut store) = test_collection("http://127.0.0.1:18080");
    let payload = store
        .create_request(None, "Conflito", "GET", "http://127.0.0.1/echo")
        .unwrap();
    let path = store.request(&payload.id).unwrap().path.clone();
    let state = AppState::new(store);
    let app = server::router(state);

    let mut edited = payload.request.clone();
    edited.http.url = "http://127.0.0.1/rascunho".into();
    let (status, draft) = call(
        &app,
        "PUT",
        &format!("/api/drafts/{}", payload.id),
        Some(json!({
            "request": edited,
            "baseRevision": payload.revision,
            "expectedGeneration": 0,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(draft["generation"], 1);

    let external = std::fs::read_to_string(&path)
        .unwrap()
        .replace("http://127.0.0.1/echo", "http://127.0.0.1/externo");
    std::fs::write(&path, &external).unwrap();

    let mut local = payload.request.clone();
    local.http.url = "http://127.0.0.1/rascunho".into();
    let (status, conflict) = call(
        &app,
        "PUT",
        &format!("/api/requests/{}", payload.id),
        Some(json!({ "request": local, "expectedRevision": payload.revision })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&conflict), "revision_conflict");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), external);

    let (status, detail) = call(&app, "GET", &format!("/api/drafts/{}", payload.id), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["summary"]["generation"], 1);
    assert_eq!(
        detail["record"]["request"]["http"]["url"],
        "http://127.0.0.1/rascunho"
    );
    assert!(std::fs::read_to_string(&path).unwrap().contains("/externo"));
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains("/rascunho")
    );
    // A versão do disco é consultada explicitamente para a comparação.
    let (status, disk) = call(
        &app,
        "GET",
        &format!("/api/requests/{}/disk", payload.id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        disk["payload"]["request"]["http"]["url"],
        "http://127.0.0.1/externo"
    );
    assert_ne!(disk["payload"]["revision"], payload.revision);
    let _ = dir;
}

#[tokio::test]
async fn draft_survives_process_restart_without_sending_anything() {
    use axum::http::StatusCode;

    let http = start_test_http().await;
    let (dir, mut store) = test_collection(&http.base);
    let payload = store
        .create_request(None, "Recuperável", "GET", &format!("{}/echo", http.base))
        .unwrap();
    let canonical = store.request(&payload.id).unwrap().path.clone();
    let canonical_before = std::fs::read(&canonical).unwrap();

    {
        let state = AppState::new(store);
        let app = server::router(state);
        let mut edited = payload.request.clone();
        edited.http.url = format!("{}/editado", http.base);
        let (status, saved) = call(
            &app,
            "PUT",
            &format!("/api/drafts/{}", payload.id),
            Some(json!({
                "request": edited,
                "baseRevision": payload.revision,
                "expectedGeneration": 0,
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(saved["generation"], 1);
    }

    // Processo encerrado: o estado em memória desaparece; só o disco permanece.
    let reopened = CollectionStore::open(dir.path()).unwrap();
    let state = AppState::new(reopened);
    let app = server::router(state);

    let (status, listed) = call(&app, "GET", "/api/drafts", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["drafts"].as_array().unwrap().len(), 1);
    assert_eq!(listed["drafts"][0]["state"], "current");

    let (status, detail) = call(&app, "GET", &format!("/api/drafts/{}", payload.id), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["summary"]["state"], "current");
    assert_eq!(
        detail["record"]["request"]["http"]["url"],
        format!("{}/editado", http.base)
    );
    assert_eq!(std::fs::read(&canonical).unwrap(), canonical_before);
    let (status, view) = call(&app, "GET", &format!("/api/requests/{}", payload.id), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        view["payload"]["request"]["http"]["url"],
        format!("{}/echo", http.base)
    );
    assert!(http.state.recorded.lock().unwrap().is_empty());
}

#[tokio::test]
async fn external_create_move_atomic_rename_and_remove_converge() {
    use axum::http::StatusCode;

    let (dir, mut store) = test_collection("http://127.0.0.1:18080");
    let payload = store
        .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
        .unwrap();
    let path = store.request(&payload.id).unwrap().path.clone();
    let original = std::fs::read_to_string(&path).unwrap();
    let external_id = "req_00000000000000aa";
    let external = original
        .replace(&payload.id, external_id)
        .replace("name: Echo", "name: Externo");
    let external_path = dir.path().join("externo.yml");
    std::fs::write(&external_path, &external).unwrap();

    let state = AppState::new(store);
    let app = server::router(state);

    let mut generation = call(&app, "GET", "/api/collection", None).await.1["generation"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, changes) = call(
        &app,
        "GET",
        &format!("/api/collection/changes?generation={generation}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(changes["changed"], true);
    assert!(
        changes["paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path.as_str() == Some("externo.yml"))
    );
    generation = changes["generation"].as_str().unwrap().to_string();
    let (status, snapshot) = call(
        &app,
        "POST",
        &format!("/api/collection/reload?generation={generation}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(tree_has_id(&snapshot["tree"], external_id));

    // Gravação atômica por rename, como editores externos costumam fazer.
    let edited = external.replace("http://127.0.0.1/echo", "http://127.0.0.1/editado");
    let temporary = dir.path().join(".externo-tmp.yml");
    std::fs::write(&temporary, edited).unwrap();
    std::fs::rename(&temporary, &external_path).unwrap();
    let changes = call(
        &app,
        "GET",
        &format!("/api/collection/changes?generation={generation}"),
        None,
    )
    .await
    .1;
    assert_eq!(changes["changed"], true);
    generation = changes["generation"].as_str().unwrap().to_string();
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/collection/reload?generation={generation}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, view) = call(&app, "GET", &format!("/api/requests/{external_id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        view["payload"]["request"]["http"]["url"],
        "http://127.0.0.1/editado"
    );

    // Mover o arquivo para uma pasta criada pelo produto.
    let (status, folder) = call(
        &app,
        "POST",
        "/api/folders",
        Some(json!({ "name": "Movida" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let folder_dir = dir.path().join("movida");
    std::fs::rename(&external_path, folder_dir.join("externo.yml")).unwrap();
    let changes = call(
        &app,
        "GET",
        &format!("/api/collection/changes?generation={generation}"),
        None,
    )
    .await
    .1;
    generation = changes["generation"].as_str().unwrap().to_string();
    let (status, snapshot) = call(
        &app,
        "POST",
        &format!("/api/collection/reload?generation={generation}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(tree_has_id(&snapshot["tree"], external_id));
    assert!(tree_has_id(
        &snapshot["tree"],
        folder["id"].as_str().unwrap()
    ));

    // Remoção externa.
    std::fs::remove_file(folder_dir.join("externo.yml")).unwrap();
    let changes = call(
        &app,
        "GET",
        &format!("/api/collection/changes?generation={generation}"),
        None,
    )
    .await
    .1;
    generation = changes["generation"].as_str().unwrap().to_string();
    let (status, snapshot) = call(
        &app,
        "POST",
        &format!("/api/collection/reload?generation={generation}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!tree_has_id(&snapshot["tree"], external_id));
}

#[tokio::test]
async fn invalid_yaml_keeps_last_valid_snapshot_and_recovers_after_fix() {
    use axum::http::StatusCode;

    let (dir, mut store) = test_collection("http://127.0.0.1:18080");
    let payload = store
        .create_request(None, "Instável", "GET", "http://127.0.0.1/echo")
        .unwrap();
    let path = store.request(&payload.id).unwrap().path.clone();
    let original = std::fs::read(&path).unwrap();
    let state = AppState::new(store);
    let app = server::router(state);
    let confirmed = call(&app, "GET", "/api/collection", None).await.1["generation"]
        .as_str()
        .unwrap()
        .to_string();

    std::fs::write(&path, b"info: [\n").unwrap();
    let changes = call(
        &app,
        "GET",
        &format!("/api/collection/changes?generation={confirmed}"),
        None,
    )
    .await
    .1;
    assert_eq!(changes["changed"], true);
    let generation = changes["generation"].as_str().unwrap().to_string();

    let (status, conflict) = call(
        &app,
        "POST",
        &format!("/api/collection/reload?generation={generation}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&conflict), "invalid_collection");
    assert!(
        conflict["error"]["message"]
            .as_str()
            .unwrap()
            .contains(&payload.path)
    );

    let (status, snapshot) = call(&app, "GET", "/api/collection", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(snapshot["generation"], confirmed);
    assert!(tree_has_id(&snapshot["tree"], &payload.id));
    let (status, _) = call(&app, "GET", &format!("/api/requests/{}", payload.id), None).await;
    assert_eq!(status, StatusCode::OK);

    std::fs::write(&path, &original).unwrap();
    let changes = call(
        &app,
        "GET",
        &format!("/api/collection/changes?generation={generation}"),
        None,
    )
    .await
    .1;
    let generation = changes["generation"].as_str().unwrap().to_string();
    let (status, snapshot) = call(
        &app,
        "POST",
        &format!("/api/collection/reload?generation={generation}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(tree_has_id(&snapshot["tree"], &payload.id));
}

#[tokio::test]
async fn own_writes_and_draft_saves_do_not_request_reconciliation() {
    use axum::http::StatusCode;

    let (_dir, mut store) = test_collection("http://127.0.0.1:18080");
    let payload = store
        .create_request(None, "Autoral", "GET", "http://127.0.0.1/echo")
        .unwrap();
    let state = AppState::new(store);
    let app = server::router(state);
    let snapshot = call(&app, "GET", "/api/collection", None).await.1;
    let confirmed = snapshot["generation"].as_str().unwrap().to_string();

    let mut request = payload.request.clone();
    request.http.url = "http://127.0.0.1/autoral".into();
    let (status, saved) = call(
        &app,
        "PUT",
        &format!("/api/requests/{}", payload.id),
        Some(json!({ "request": request, "expectedRevision": payload.revision })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, changes) = call(
        &app,
        "GET",
        &format!("/api/collection/changes?generation={confirmed}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(changes["changed"], false);
    assert_eq!(changes["cursorStale"], true);
    assert_eq!(changes["reconcile"], false);

    let (status, _) = call(
        &app,
        "PUT",
        &format!("/api/drafts/{}", payload.id),
        Some(json!({
            "request": saved["request"],
            "baseRevision": saved["revision"],
            "expectedGeneration": 0,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let current = call(&app, "GET", "/api/collection", None).await.1["generation"]
        .as_str()
        .unwrap()
        .to_string();
    let changes = call(
        &app,
        "GET",
        &format!("/api/collection/changes?generation={current}"),
        None,
    )
    .await
    .1;
    assert_eq!(changes["changed"], false);
    assert_eq!(changes["reconcile"], false);
}

#[tokio::test]
async fn stale_generation_from_another_collection_does_not_reconcile() {
    use axum::http::StatusCode;

    let (_dir_a, mut store_a) = test_collection("http://127.0.0.1:18080");
    let payload_a = store_a
        .create_request(None, "Anterior", "GET", "http://127.0.0.1/a")
        .unwrap();
    let generation_a = store_a.snapshot().generation;
    assert!(!payload_a.id.is_empty());

    let (_dir_b, mut store_b) = test_collection("http://127.0.0.1:18080");
    store_b
        .create_request(None, "Atual", "GET", "http://127.0.0.1/b")
        .unwrap();
    let confirmed_b = store_b.snapshot().generation;
    let state = AppState::new(store_b);
    let app = server::router(state);

    let (status, changes) = call(
        &app,
        "GET",
        &format!("/api/collection/changes?generation={generation_a}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(changes["changed"], false);
    assert_eq!(changes["cursorStale"], true);
    assert_eq!(changes["reconcile"], false);

    let (status, conflict) = call(
        &app,
        "POST",
        &format!("/api/collection/reload?generation={generation_a}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&conflict), "generation_conflict");
    let unchanged = call(&app, "GET", "/api/collection", None).await.1;
    assert_eq!(unchanged["generation"], confirmed_b);
}

#[tokio::test]
async fn rename_keeps_id_and_duplicate_id_is_rejected_without_losing_snapshot() {
    use axum::http::StatusCode;

    let (dir, mut store) = test_collection("http://127.0.0.1:18080");
    let payload = store
        .create_request(None, "Renomeável", "GET", "http://127.0.0.1/echo")
        .unwrap();
    let path = store.request(&payload.id).unwrap().path.clone();
    let state = AppState::new(store);
    let app = server::router(state);
    let confirmed = call(&app, "GET", "/api/collection", None).await.1["generation"]
        .as_str()
        .unwrap()
        .to_string();

    let renamed = dir.path().join("nome-novo.yml");
    std::fs::rename(&path, &renamed).unwrap();
    let changes = call(
        &app,
        "GET",
        &format!("/api/collection/changes?generation={confirmed}"),
        None,
    )
    .await
    .1;
    let generation = changes["generation"].as_str().unwrap().to_string();
    let (status, snapshot) = call(
        &app,
        "POST",
        &format!("/api/collection/reload?generation={generation}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(tree_has_id(&snapshot["tree"], &payload.id));
    let (status, view) = call(&app, "GET", &format!("/api/requests/{}", payload.id), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["payload"]["path"], "nome-novo.yml");

    let duplicate = dir.path().join("duplicado.yml");
    std::fs::copy(&renamed, &duplicate).unwrap();
    let changes = call(
        &app,
        "GET",
        &format!("/api/collection/changes?generation={generation}"),
        None,
    )
    .await
    .1;
    let duplicate_generation = changes["generation"].as_str().unwrap().to_string();
    let (status, conflict) = call(
        &app,
        "POST",
        &format!("/api/collection/reload?generation={duplicate_generation}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&conflict), "duplicate_id");
    let (status, snapshot) = call(&app, "GET", "/api/collection", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(snapshot["generation"], generation);
    assert!(tree_has_id(&snapshot["tree"], &payload.id));

    std::fs::remove_file(&duplicate).unwrap();
    let changes = call(
        &app,
        "GET",
        &format!("/api/collection/changes?generation={duplicate_generation}"),
        None,
    )
    .await
    .1;
    let recovered = changes["generation"].as_str().unwrap().to_string();
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/collection/reload?generation={recovered}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn environment_save_conflicts_when_cli_changed_manifest_and_preserves_state() {
    use axum::http::StatusCode;

    let (dir, mut store) = test_collection("http://127.0.0.1:18080");
    store
        .create_request(None, "Formulário", "GET", "http://127.0.0.1/echo")
        .unwrap();
    let state = AppState::new(store);
    let app = server::router(state);
    let snapshot = call(&app, "GET", "/api/collection", None).await.1;
    let confirmed = snapshot["generation"].as_str().unwrap().to_string();
    let stale_revision = snapshot["manifest_revision"].as_str().unwrap().to_string();

    // Escritor externo (equivalente ao CLI) altera o manifesto.
    let mut cli = CollectionStore::open(dir.path()).unwrap();
    cli.upsert_environment(
        "externo",
        vec![Variable::plain("origem", "cli")],
        Vec::new(),
        None,
    )
    .unwrap();

    let (status, changes) = call(
        &app,
        "GET",
        &format!("/api/collection/changes?generation={confirmed}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(changes["changed"], true);
    assert!(
        changes["paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path.as_str() == Some("opencollection.yml"))
    );

    let (status, conflict) = call(
        &app,
        "PUT",
        "/api/environments/dev",
        Some(json!({
            "variables": [{ "name": "naoDeveEntrar", "value": "x" }],
            "expectedRevision": stale_revision,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&conflict), "revision_conflict");

    // O snapshot ativo continua sendo o anterior; o corpo rejeitado não vaza
    // para a memória nem para o disco.
    let (status, environments) = call(&app, "GET", "/api/environments", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!environments.to_string().contains("naoDeveEntrar"));
    assert!(!environments.to_string().contains("externo"));
    let (_, current) = call(&app, "GET", "/api/collection", None).await;
    assert_eq!(current["generation"], confirmed);
    let disk = CollectionStore::open(dir.path()).unwrap();
    assert!(
        disk.manifest
            .environments()
            .iter()
            .any(|environment| environment.name == "externo")
    );
    assert!(
        disk.manifest
            .environments()
            .iter()
            .all(|environment| environment.name != "naoDeveEntrar")
    );

    // Após recarregar, o valor externo aparece e o formulário pode ser reenviado.
    let changes = call(
        &app,
        "GET",
        &format!("/api/collection/changes?generation={confirmed}"),
        None,
    )
    .await
    .1;
    let generation = changes["generation"].as_str().unwrap().to_string();
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/collection/reload?generation={generation}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, environments) = call(&app, "GET", "/api/environments", None).await;
    assert!(environments.to_string().contains("externo"));
    assert!(!environments.to_string().contains("naoDeveEntrar"));
}

#[tokio::test]
async fn runner_uses_the_initial_snapshot_while_the_file_changes() {
    let http = start_test_http().await;
    let (dir, mut store) = test_collection(&http.base);
    let payload = store
        .create_request(None, "Concorrente", "GET", &format!("{}/slow", http.base))
        .unwrap();
    let path = store.request(&payload.id).unwrap().path.clone();
    let request = payload.request.clone();
    let options = ExecuteOptions {
        environment: Some("dev".into()),
        cookie_jar: Arc::new(reqwest::cookie::Jar::default()),
        cancellation: None,
    };
    let first = runner::execute_single(&store, &payload.id, &request, options.clone());
    tokio::pin!(first);
    tokio::select! {
        outcome = &mut first => panic!("execução terminou cedo demais: {outcome:?}"),
        _ = tokio::time::sleep(Duration::from_millis(50)) => {}
    }
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("/slow", "/ok");
    std::fs::write(&path, text).unwrap();
    let outcome = first.await.unwrap();
    assert_eq!(outcome.run.url, format!("{}/slow", http.base));
    let recorded = http.state.recorded.lock().unwrap().clone();
    assert_eq!(recorded.last().unwrap().path, "/slow");

    let reopened = CollectionStore::open(dir.path()).unwrap();
    let second = runner::execute_single(
        &reopened,
        &payload.id,
        &reopened.request(&payload.id).unwrap().file.clone(),
        options,
    )
    .await
    .unwrap();
    assert_eq!(second.run.url, format!("{}/ok", http.base));
    let recorded = http.state.recorded.lock().unwrap().clone();
    assert_eq!(recorded.last().unwrap().path, "/ok");
}

trait InfoName {
    fn info_name(&self) -> &str;
}

impl InfoName for CollectionStore {
    fn info_name(&self) -> &str {
        &self.manifest.info.name
    }
}
