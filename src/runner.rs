use crate::assertions::{TestResult, evaluate_assertions};
use crate::auth::AuthExplanation;
use crate::body::PreparedRequest;
use crate::error::{Error, ErrorInfo, Result};
use crate::model::{RequestFile, ScriptPhase};
use crate::redact::{redact_headers, redact_text};
use crate::scripts::{ScriptLog, ScriptRuntime, apply_request_patch};
use crate::store::CollectionStore;
use crate::transport::{self, HttpResponse, TransportOptions};
use crate::variables::{EffectiveVariable, Resolver};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Executed,
    Passed,
    Failed,
    Skipped,
    Cancelled,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponseSummary {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub content_type: Option<String>,
    pub size_bytes: usize,
    pub declared_size: Option<u64>,
    pub truncated: bool,
    pub time_ms: u128,
    pub final_url: String,
    pub redirected: bool,
    pub body_preview: Option<String>,
}

impl ResponseSummary {
    pub fn from_response(response: &HttpResponse, secrets: &[String]) -> Self {
        let body_preview = match &response.body_text {
            Some(text) => {
                let limit = 2000;
                let truncated = text.len() > limit;
                let mut end = limit.min(text.len());
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                let mut preview = text[..end].to_string();
                if truncated {
                    preview.push_str("\n… (prévia truncada)");
                }
                Some(redact_text(&preview, secrets))
            }
            None => Some(format!(
                "<binário: {} · {} bytes>",
                response.content_type.as_deref().unwrap_or("desconhecido"),
                response.size_bytes
            )),
        };
        Self {
            status: response.status,
            status_text: response.status_text.clone(),
            headers: redact_headers(&response.headers, secrets),
            content_type: response.content_type.clone(),
            size_bytes: response.size_bytes,
            declared_size: response.declared_size,
            truncated: response.truncated,
            time_ms: response.time_ms,
            final_url: crate::redact::redact_url(&response.final_url, secrets),
            redirected: response.redirected,
            body_preview,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestRun {
    pub id: String,
    pub request_id: String,
    pub name: String,
    pub path: String,
    pub method: String,
    pub url: String,
    pub tags: Vec<String>,
    pub status: RunStatus,
    pub http_status: Option<u16>,
    pub duration_ms: u128,
    pub started_at: DateTime<Utc>,
    pub tests: Vec<TestResult>,
    pub logs: Vec<ScriptLog>,
    pub response: Option<ResponseSummary>,
    pub error: Option<ErrorInfo>,
    pub auth: Option<AuthExplanation>,
    pub effective_variables: Vec<EffectiveVariable>,
    pub skip_reason: Option<String>,
}

impl RequestRun {
    fn skipped(store: &CollectionStore, request_id: &str, reason: impl Into<String>) -> Self {
        let entry = store.request(request_id).ok();
        Self {
            id: format!("run_{}", crate::store::short_uuid()),
            request_id: request_id.to_string(),
            name: entry.map(|entry| entry.name.clone()).unwrap_or_default(),
            path: entry
                .map(|entry| entry.rel_path.clone())
                .unwrap_or_default(),
            method: entry
                .map(|entry| entry.file.http.method.clone())
                .unwrap_or_default(),
            url: entry
                .map(|entry| entry.file.http.url.clone())
                .unwrap_or_default(),
            tags: store.request_tags(request_id).unwrap_or_default(),
            status: RunStatus::Skipped,
            http_status: None,
            duration_ms: 0,
            started_at: Utc::now(),
            tests: Vec::new(),
            logs: Vec::new(),
            response: None,
            error: None,
            auth: None,
            effective_variables: Vec::new(),
            skip_reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RunSummary {
    pub total: usize,
    pub executed: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub errors: usize,
    pub cancelled: usize,
}

#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunFilters {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub request_ids: Vec<String>,
    #[serde(default)]
    pub folder_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunReport {
    pub id: String,
    pub collection: String,
    pub environment: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub duration_ms: u128,
    pub filters: RunFilters,
    pub summary: RunSummary,
    pub requests: Vec<RequestRun>,
    pub deterministic_hash: String,
}

#[derive(Debug, Clone)]
pub struct ExecuteOptions {
    pub environment: Option<String>,
    pub cookie_jar: Arc<reqwest::cookie::Jar>,
    pub cancellation: Option<CancellationToken>,
}

#[derive(Debug, Clone)]
pub struct ExecutionOutcome {
    pub run: RequestRun,
    pub response: Option<HttpResponse>,
    pub runtime: BTreeMap<String, String>,
}

/// Executa uma requisição: scripts pré → transporte → pós → asserções/testes.
pub async fn execute_request(
    store: &CollectionStore,
    request_id: &str,
    request: &RequestFile,
    options: ExecuteOptions,
    mut runtime: BTreeMap<String, String>,
) -> Result<ExecutionOutcome> {
    let started = Instant::now();
    let started_at = Utc::now();
    let entry = store.request(request_id)?.clone();
    let environment = options.environment.clone();
    let mut effective_request = request.clone();
    let mut run_id = format!("run_{}", crate::store::short_uuid());
    let mut all_tests: Vec<TestResult> = Vec::new();
    let mut all_logs: Vec<ScriptLog> = Vec::new();
    let mut error: Option<ErrorInfo> = None;
    let mut status = RunStatus::Executed;
    let mut response: Option<HttpResponse> = None;
    let mut prepared_info: Option<(
        Option<AuthExplanation>,
        Vec<EffectiveVariable>,
        String,
        Vec<String>,
    )> = None;
    // Base de variáveis efetivas (inclui segredos apenas em memória).
    let scope = store.scope_input_with(request_id, request, environment.as_deref(), &runtime)?;
    let resolver = Resolver::build(scope);
    let mut secrets = resolver.secret_values();
    let mut base = BTreeMap::new();
    for name in resolver.values().keys() {
        if let Ok(value) = resolver.resolve(&format!("{{{{{name}}}}}")) {
            base.insert(name.clone(), value);
        }
    }
    let mut script_runtime =
        ScriptRuntime::new(base, runtime.clone()).with_secrets(secrets.clone());

    // Pré-requisição: coleção → pastas (raiz→folha) → requisição.
    let pre_scripts =
        store.scoped_scripts_with(request_id, &effective_request, ScriptPhase::BeforeRequest)?;
    for script in &pre_scripts {
        let outcome = script_runtime.run(
            &script.code,
            ScriptPhase::BeforeRequest,
            &script.origin,
            &effective_request,
            None,
            store
                .effective_settings_with(&effective_request)?
                .script_timeout_ms,
        );
        all_logs.extend(outcome.logs);
        all_tests.extend(outcome.tests);
        if let Some(patch) = &outcome.request_patch {
            if let Err(patch_error) = apply_request_patch(&mut effective_request, patch) {
                error = Some(patch_error.info());
                status = RunStatus::Error;
                break;
            }
        }
        if let Some(message) = outcome.error {
            status = RunStatus::Error;
            error = Some(Error::Script(message).info());
            break;
        }
    }
    runtime = script_runtime.runtime.clone();

    if error.is_none() {
        let settings = store.effective_settings_with(&effective_request)?;
        let prepared = PreparedRequest::new(
            store,
            request_id,
            &effective_request,
            environment.as_deref(),
            &runtime,
        );
        match prepared {
            Ok(prepared) => {
                prepared_info = Some((
                    prepared.auth.clone(),
                    prepared.effective_variables.clone(),
                    prepared.display_url.clone(),
                    prepared.secret_values.clone(),
                ));
                secrets.extend(prepared.secret_values.clone());
                let transport_options = TransportOptions {
                    settings,
                    cookie_jar: options.cookie_jar.clone(),
                    cancellation: options.cancellation.clone(),
                };
                match transport::send(prepared, transport_options).await {
                    Ok(http_response) => {
                        response = Some(http_response);
                    }
                    Err(send_error) => {
                        status = if matches!(send_error, Error::Cancelled) {
                            RunStatus::Cancelled
                        } else {
                            RunStatus::Error
                        };
                        error = Some(send_error.info());
                    }
                }
            }
            Err(prepare_error) => {
                status = RunStatus::Error;
                error = Some(prepare_error.info());
            }
        }
    }

    // Pós-resposta: requisição → pastas (folha→raiz) → coleção.
    if let Some(http_response) = &response {
        let post_scripts = store.scoped_scripts_with(
            request_id,
            &effective_request,
            ScriptPhase::AfterResponse,
        )?;
        for script in &post_scripts {
            let outcome = script_runtime.run(
                &script.code,
                ScriptPhase::AfterResponse,
                &script.origin,
                &effective_request,
                Some(http_response),
                store
                    .effective_settings_with(&effective_request)?
                    .script_timeout_ms,
            );
            all_logs.extend(outcome.logs);
            all_tests.extend(outcome.tests);
            if let Some(message) = outcome.error {
                all_tests.push(TestResult::new(
                    format!("script pós-resposta ({})", script.origin),
                    false,
                    message,
                    None,
                    None,
                    format!("script:{}", script.origin),
                ));
            }
        }

        // Asserções declarativas.
        let assertions = store.assertions_with(&effective_request);
        let mut assertion_results = evaluate_assertions(&assertions, http_response)?;
        all_tests.append(&mut assertion_results);

        // Scripts de teste (depois do pós, conforme contrato).
        let test_scripts =
            store.scoped_scripts_with(request_id, &effective_request, ScriptPhase::Tests)?;
        for script in &test_scripts {
            let outcome = script_runtime.run(
                &script.code,
                ScriptPhase::Tests,
                &script.origin,
                &effective_request,
                Some(http_response),
                store
                    .effective_settings_with(&effective_request)?
                    .script_timeout_ms,
            );
            all_logs.extend(outcome.logs);
            all_tests.extend(outcome.tests);
            if let Some(message) = outcome.error {
                all_tests.push(TestResult::new(
                    format!("script de teste ({})", script.origin),
                    false,
                    message,
                    None,
                    None,
                    format!("script:{}", script.origin),
                ));
            }
        }
    }
    runtime = script_runtime.runtime.clone();

    if matches!(status, RunStatus::Executed) {
        if all_tests.iter().any(|test| !test.passed) {
            status = RunStatus::Failed;
        } else if !all_tests.is_empty() {
            status = RunStatus::Passed;
        }
    }

    // Erros de script/asserção são redigidos com os segredos conhecidos.
    if let Some(info) = &mut error {
        info.message = redact_text(&info.message, &secrets);
    }
    for test in &mut all_tests {
        test.message = redact_text(&test.message, &secrets);
        if let Some(expected) = &test.expected {
            test.expected = Some(redact_text(expected, &secrets));
        }
        if let Some(actual) = &test.actual {
            test.actual = Some(redact_text(actual, &secrets));
        }
    }
    for log in &mut all_logs {
        log.message = redact_text(&log.message, &secrets);
    }

    let response_summary = response
        .as_ref()
        .map(|response| ResponseSummary::from_response(response, &secrets));
    let (auth, effective_variables, display_url) = prepared_info
        .map(|(auth, variables, url, _)| (auth, variables, url))
        .unwrap_or((None, Vec::new(), effective_request.http.url.clone()));

    let duration_ms = started.elapsed().as_millis();
    let run = RequestRun {
        id: std::mem::take(&mut run_id),
        request_id: request_id.to_string(),
        name: effective_request.info.name.clone(),
        path: entry.rel_path.clone(),
        method: effective_request.http.method.clone(),
        url: redact_text(&display_url, &secrets),
        tags: store.request_tags(request_id).unwrap_or_default(),
        status,
        http_status: response.as_ref().map(|response| response.status),
        duration_ms,
        started_at,
        tests: all_tests,
        logs: all_logs,
        response: response_summary,
        error,
        auth,
        effective_variables,
        skip_reason: None,
    };

    Ok(ExecutionOutcome {
        run,
        response,
        runtime,
    })
}

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub environment: Option<String>,
    pub filters: RunFilters,
    pub cookie_jar: Arc<reqwest::cookie::Jar>,
    pub cancellation: Option<CancellationToken>,
}

pub async fn run_collection(
    store: &CollectionStore,
    options: RunOptions,
    initial_runtime: BTreeMap<String, String>,
) -> Result<RunReport> {
    let started = Instant::now();
    let started_at = Utc::now();
    let environment = options
        .environment
        .clone()
        .or_else(|| store.default_environment());
    if let Some(name) = &environment {
        store.environment_chain(name)?;
    }
    let requests = store.all_requests_in_order();
    let mut results = Vec::new();
    let mut runtime = initial_runtime;
    let mut cancel_requested = options
        .cancellation
        .as_ref()
        .map(CancellationToken::is_cancelled)
        .unwrap_or(false);

    for request in requests {
        let selected = is_selected(store, request, &options.filters);
        if !selected {
            results.push(RequestRun::skipped(
                store,
                &request.id,
                skip_reason(&options.filters),
            ));
            continue;
        }
        if cancel_requested {
            results.push(RequestRun::skipped(
                store,
                &request.id,
                "execução cancelada pelo usuário",
            ));
            continue;
        }
        if options
            .cancellation
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            cancel_requested = true;
            results.push(RequestRun::skipped(
                store,
                &request.id,
                "execução cancelada pelo usuário",
            ));
            continue;
        }

        let outcome = execute_request(
            store,
            &request.id,
            &request.file,
            ExecuteOptions {
                environment: environment.clone(),
                cookie_jar: options.cookie_jar.clone(),
                cancellation: options.cancellation.clone(),
            },
            runtime.clone(),
        )
        .await?;
        runtime = outcome.runtime;
        if outcome.run.status == RunStatus::Cancelled {
            cancel_requested = true;
        }
        results.push(outcome.run);
    }

    let summary = summarize(&results);
    let deterministic_hash = deterministic_hash(&results);
    let finished_at = Utc::now();
    Ok(RunReport {
        id: format!("run_{}", crate::store::short_uuid()),
        collection: store.manifest.info.name.clone(),
        environment,
        started_at,
        finished_at,
        duration_ms: started.elapsed().as_millis(),
        filters: options.filters,
        summary,
        requests: results,
        deterministic_hash,
    })
}

fn is_selected(
    store: &CollectionStore,
    request: &crate::store::RequestEntry,
    filters: &RunFilters,
) -> bool {
    if let Some(folder_id) = &filters.folder_id {
        if !store.is_in_folder(&request.id, folder_id) {
            return false;
        }
    }
    if !filters.request_ids.is_empty() && !filters.request_ids.contains(&request.id) {
        return false;
    }
    if !filters.tags.is_empty() {
        let tags = store.request_tags(&request.id).unwrap_or_default();
        let matches = filters
            .tags
            .iter()
            .any(|wanted| tags.iter().any(|tag| tag.eq_ignore_ascii_case(wanted)));
        if !matches {
            return false;
        }
    }
    true
}

fn skip_reason(filters: &RunFilters) -> String {
    if !filters.tags.is_empty() {
        format!("filtro de tags: {}", filters.tags.join(", "))
    } else if !filters.request_ids.is_empty() {
        "filtro de requisições".into()
    } else if filters.folder_id.is_some() {
        "fora da pasta selecionada".into()
    } else {
        "não selecionada".into()
    }
}

fn summarize(results: &[RequestRun]) -> RunSummary {
    let mut summary = RunSummary {
        total: results.len(),
        executed: 0,
        passed: 0,
        failed: 0,
        skipped: 0,
        errors: 0,
        cancelled: 0,
    };
    for result in results {
        match result.status {
            RunStatus::Executed => summary.executed += 1,
            RunStatus::Passed => summary.passed += 1,
            RunStatus::Failed => summary.failed += 1,
            RunStatus::Skipped => summary.skipped += 1,
            RunStatus::Error => summary.errors += 1,
            RunStatus::Cancelled => summary.cancelled += 1,
        }
    }
    summary
}

fn deterministic_hash(results: &[RequestRun]) -> String {
    let mut hasher = Sha256::new();
    for result in results {
        hasher.update(result.request_id.as_bytes());
        hasher.update(b"\0");
        hasher.update(format!("{:?}", result.status).as_bytes());
        hasher.update(b"\0");
        if let Some(error) = &result.error {
            hasher.update(error.code.as_bytes());
        }
        hasher.update(b"\0");
        for test in &result.tests {
            hasher.update(test.name.as_bytes());
            hasher.update(b"\0");
            hasher.update(if test.passed { b"1" } else { b"0" });
            hasher.update(b"\0");
        }
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

/// Executa uma única requisição para o envio interativo da UI/CLI.
pub async fn execute_single(
    store: &CollectionStore,
    request_id: &str,
    request: &RequestFile,
    mut options: ExecuteOptions,
) -> Result<ExecutionOutcome> {
    if options.environment.is_none() {
        options.environment = store.default_environment();
    }
    execute_request(store, request_id, request, options, BTreeMap::new()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::init_collection;

    #[test]
    fn skipped_items_are_counted_separately() {
        let summary = summarize(&[
            RequestRun {
                id: "1".into(),
                request_id: "a".into(),
                name: "a".into(),
                path: "a.yml".into(),
                method: "GET".into(),
                url: "".into(),
                tags: vec![],
                status: RunStatus::Executed,
                http_status: Some(404),
                duration_ms: 1,
                started_at: Utc::now(),
                tests: vec![],
                logs: vec![],
                response: None,
                error: None,
                auth: None,
                effective_variables: vec![],
                skip_reason: None,
            },
            RequestRun::skipped(
                &init_collection(tempfile::tempdir().unwrap().path(), "x").unwrap(),
                "missing",
                "filtro",
            ),
        ]);
        assert_eq!(summary.total, 2);
        assert_eq!(summary.executed, 1);
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.passed, 0);
    }

    #[test]
    fn response_summary_redacts_final_url_and_sensitive_values() {
        let response = HttpResponse {
            status: 200,
            status_text: "OK".into(),
            headers: vec![("Set-Cookie".into(), "session=segredo".into())],
            body_text: Some("token=segredo".into()),
            body_base64: None,
            is_binary: false,
            truncated: false,
            declared_size: Some(12),
            size_bytes: 12,
            time_ms: 1,
            final_url: "https://user:pass@example.test/echo?token=segredo".into(),
            requested_url: "https://example.test/echo".into(),
            content_type: Some("application/json".into()),
            redirected: true,
            body_bytes: Vec::new(),
        };
        let summary = ResponseSummary::from_response(&response, &["segredo".into()]);
        assert!(!summary.final_url.contains("segredo"));
        assert!(!summary.final_url.contains("pass"));
        assert!(summary.final_url.contains("token="));
        assert_eq!(
            summary.body_preview.unwrap(),
            format!("token={}", crate::redact::MASK)
        );
        assert_eq!(summary.headers[0].1, crate::redact::MASK);
    }

    #[test]
    fn deterministic_hash_ignores_timestamps() {
        let mut first = RequestRun {
            id: "1".into(),
            request_id: "a".into(),
            name: "a".into(),
            path: "a.yml".into(),
            method: "GET".into(),
            url: "".into(),
            tags: vec![],
            status: RunStatus::Passed,
            http_status: Some(200),
            duration_ms: 5,
            started_at: Utc::now(),
            tests: vec![TestResult::new("t", true, "", None, None, "assertion")],
            logs: vec![],
            response: None,
            error: None,
            auth: None,
            effective_variables: vec![],
            skip_reason: None,
        };
        let first_hash = deterministic_hash(std::slice::from_ref(&first));
        first.duration_ms = 999;
        first.started_at = Utc::now();
        let second_hash = deterministic_hash(std::slice::from_ref(&first));
        assert_eq!(first_hash, second_hash);
    }
}
