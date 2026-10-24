use crate::auth::effective_auth;
use crate::error::{Error, Result};
use crate::model::*;
use crate::redact::{is_sensitive_name, is_sensitive_query};
use crate::store::{CollectionStore, new_id, write_yaml};
use serde::Serialize;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    Curl,
    Postman,
    OpenCollection,
}

impl ImportFormat {
    pub fn parse(input: &str) -> Result<Self> {
        match input.to_ascii_lowercase().as_str() {
            "curl" | "cURL" => Ok(Self::Curl),
            "postman" => Ok(Self::Postman),
            "opencollection" | "open-collection" | "oc" => Ok(Self::OpenCollection),
            other => Err(Error::Import(format!(
                "formato de importação desconhecido: '{other}' (use curl, postman ou opencollection)"
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Curl => "curl",
            Self::Postman => "postman",
            Self::OpenCollection => "opencollection",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Curl,
    Postman,
    OpenCollection,
    Project,
}

impl ExportFormat {
    pub fn parse(input: &str) -> Result<Self> {
        match input.to_ascii_lowercase().as_str() {
            "curl" => Ok(Self::Curl),
            "postman" => Ok(Self::Postman),
            "opencollection" | "open-collection" | "oc" => Ok(Self::OpenCollection),
            "project" | "projeto" => Ok(Self::Project),
            other => Err(Error::Export(format!(
                "formato de exportação desconhecido: '{other}' (use curl, postman, opencollection ou project)"
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Curl => "curl",
            Self::Postman => "postman",
            Self::OpenCollection => "opencollection",
            Self::Project => "project",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Loss {
    pub item: String,
    pub field: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    pub format: String,
    pub source: String,
    pub requests_imported: usize,
    pub folders_imported: usize,
    pub environments_imported: usize,
    pub imported_ids: Vec<String>,
    pub warnings: Vec<String>,
    pub losses: Vec<Loss>,
    pub original_saved: Option<String>,
}

impl ImportReport {
    fn new(format: ImportFormat, source: impl Into<String>) -> Self {
        Self {
            format: format.as_str().to_string(),
            source: source.into(),
            requests_imported: 0,
            folders_imported: 0,
            environments_imported: 0,
            imported_ids: Vec::new(),
            warnings: Vec::new(),
            losses: Vec::new(),
            original_saved: None,
        }
    }

    fn loss(
        &mut self,
        item: impl Into<String>,
        field: impl Into<String>,
        reason: impl Into<String>,
    ) {
        self.losses.push(Loss {
            item: item.into(),
            field: field.into(),
            reason: reason.into(),
        });
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportReport {
    pub format: String,
    pub destination: Option<String>,
    pub requests: usize,
    pub folders: usize,
    pub files: usize,
    pub warnings: Vec<String>,
    pub losses: Vec<Loss>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

impl ExportReport {
    fn new(format: ExportFormat) -> Self {
        Self {
            format: format.as_str().to_string(),
            destination: None,
            requests: 0,
            folders: 0,
            files: 0,
            warnings: Vec::new(),
            losses: Vec::new(),
            content: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Entrada
// ---------------------------------------------------------------------------

pub fn import_content(
    store: &mut CollectionStore,
    parent: Option<&str>,
    format: ImportFormat,
    content: &str,
    filename: Option<&str>,
) -> Result<ImportReport> {
    let source_name = filename.unwrap_or("entrada");
    if let Some(parent) = parent {
        store.folder(parent)?;
    }
    let mut report = ImportReport::new(format, source_name);
    report.original_saved = save_original(store, source_name, content.as_bytes())?;
    match format {
        ImportFormat::Curl => import_curl_into(store, parent, content, &mut report)?,
        ImportFormat::Postman => import_postman_into(store, parent, content, &mut report)?,
        ImportFormat::OpenCollection => {
            let value: serde_yaml::Value = serde_yaml::from_str(content)?;
            import_opencollection_value(store, parent, &value, &mut report)?;
        }
    }
    Ok(report)
}

pub fn import_path(
    store: &mut CollectionStore,
    parent: Option<&str>,
    format: ImportFormat,
    path: &Path,
) -> Result<ImportReport> {
    if let Some(parent) = parent {
        store.folder(parent)?;
    }
    if path.is_dir() {
        if format != ImportFormat::OpenCollection {
            return Err(Error::Import(
                "diretórios só são aceitos na importação OpenCollection".into(),
            ));
        }
        let mut report = ImportReport::new(format, path.display().to_string());
        report.original_saved = save_original_directory(store, path)?;
        import_opencollection_dir(store, parent, path, &mut report)?;
        return Ok(report);
    }
    let bytes = fs::read(path)?;
    let content = String::from_utf8_lossy(&bytes).to_string();
    import_content(
        store,
        parent,
        format,
        &content,
        path.file_name().and_then(|name| name.to_str()),
    )
}

fn save_original(store: &CollectionStore, filename: &str, bytes: &[u8]) -> Result<Option<String>> {
    let directory = store.root.join(".reqlivo").join("imports");
    fs::create_dir_all(&directory)?;
    let safe = crate::store::slugify(filename);
    let target = directory.join(format!(
        "{}-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        safe
    ));
    fs::write(&target, bytes)?;
    Ok(Some(target.display().to_string()))
}

fn save_original_directory(store: &CollectionStore, source: &Path) -> Result<Option<String>> {
    let directory = store.root.join(".reqlivo").join("imports");
    fs::create_dir_all(&directory)?;
    let target = directory.join(format!(
        "{}-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        source
            .file_name()
            .and_then(|name| name.to_str())
            .map(crate::store::slugify)
            .unwrap_or_else(|| "opencollection".into())
    ));
    copy_directory(source, &target)?;
    Ok(Some(target.display().to_string()))
}

fn copy_directory(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let from = entry.path();
        let to = target.join(&name);
        if from.is_dir() {
            copy_directory(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// cURL
// ---------------------------------------------------------------------------

fn import_curl_into(
    store: &mut CollectionStore,
    parent: Option<&str>,
    input: &str,
    report: &mut ImportReport,
) -> Result<()> {
    let request = parse_curl(input, report)?;
    let payload = store.add_request(parent, request)?;
    report.requests_imported += 1;
    report.imported_ids.push(payload.id);
    Ok(())
}

pub fn parse_curl(input: &str, report: &mut ImportReport) -> Result<RequestFile> {
    let tokens = shell_words::split(input)
        .map_err(|error| Error::Import(format!("não foi possível interpretar o cURL: {error}")))?;
    if tokens.is_empty() {
        return Err(Error::Import("comando cURL vazio".into()));
    }
    let mut method: Option<String> = None;
    let mut url: Option<String> = None;
    let mut headers: Vec<Header> = Vec::new();
    let mut data_parts: Vec<String> = Vec::new();
    let mut form_fields: Vec<FormField> = Vec::new();
    let mut basic: Option<(String, String)> = None;
    let mut get_mode = false;
    let mut follow_redirects = false;
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        let next = |index: usize| tokens.get(index + 1).cloned();
        match token.as_str() {
            "-X" | "--request" => {
                method = next(index);
                index += 2;
            }
            "-H" | "--header" => {
                if let Some(header) = next(index) {
                    if let Some((name, value)) = header.split_once(':') {
                        headers.push(Header {
                            name: name.trim().to_string(),
                            value: value.trim().to_string(),
                            description: None,
                            disabled: false,
                            extra: Extra::new(),
                        });
                    } else {
                        report.loss(
                            "cURL",
                            "header",
                            format!("cabeçalho sem ':' ignorado: {header}"),
                        );
                    }
                }
                index += 2;
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" | "--data-urlencode" => {
                if let Some(data) = next(index) {
                    data_parts.push(data);
                }
                index += 2;
            }
            "-F" | "--form" => {
                if let Some(field) = next(index) {
                    if let Some((name, value)) = field.split_once('=') {
                        let is_file = value.starts_with('@');
                        form_fields.push(FormField {
                            name: name.to_string(),
                            value: value.trim_start_matches('@').to_string(),
                            kind: Some(if is_file {
                                "file".into()
                            } else {
                                "text".into()
                            }),
                            description: None,
                            disabled: false,
                            extra: Extra::new(),
                        });
                    } else {
                        report.loss("cURL", "form", format!("campo -F inválido: {field}"));
                    }
                }
                index += 2;
            }
            "-u" | "--user" => {
                if let Some(user) = next(index) {
                    let (username, password) = user.split_once(':').unwrap_or((&user, ""));
                    basic = Some((username.to_string(), password.to_string()));
                }
                index += 2;
            }
            "-G" | "--get" => {
                get_mode = true;
                index += 1;
            }
            "-L" | "--location" => {
                follow_redirects = true;
                index += 1;
            }
            "-A" | "--user-agent" => {
                if let Some(agent) = next(index) {
                    headers.push(Header {
                        name: "User-Agent".into(),
                        value: agent,
                        description: None,
                        disabled: false,
                        extra: Extra::new(),
                    });
                }
                index += 2;
            }
            "-b" | "--cookie" => {
                if let Some(cookie) = next(index) {
                    headers.push(Header {
                        name: "Cookie".into(),
                        value: cookie,
                        description: None,
                        disabled: false,
                        extra: Extra::new(),
                    });
                }
                index += 2;
            }
            "--compressed" | "-s" | "--silent" | "-S" | "--show-error" | "-i" | "--include"
            | "-v" | "--verbose" => {
                index += 1;
            }
            "-k" | "--insecure" => {
                report.loss("cURL", "tls", "flag -k/--insecure não é suportada no MVP");
                index += 1;
            }
            "--url" => {
                url = next(index);
                index += 2;
            }
            other if other.starts_with("--") => {
                report.loss("cURL", "flag", format!("flag não suportada: {other}"));
                index += 1;
            }
            other if other.starts_with('-') => {
                report.loss("cURL", "flag", format!("flag não suportada: {other}"));
                index += 1;
            }
            other => {
                if url.is_none() {
                    url = Some(other.to_string());
                }
                index += 1;
            }
        }
    }
    let url = url.ok_or_else(|| Error::Import("o cURL não contém URL".into()))?;
    let url = if url.contains("://") {
        url
    } else {
        format!("http://{url}")
    };
    let mut final_url = url.clone();
    let mut params = Vec::new();
    if get_mode && !data_parts.is_empty() {
        for pair in data_parts.join("&").split('&') {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            params.push(Param {
                name: name.to_string(),
                value: urlencoding_decode(value),
                kind: ParamKind::Query,
                description: None,
                disabled: false,
                extra: Extra::new(),
            });
        }
        data_parts.clear();
    }
    if let Some(position) = final_url.find('?') {
        let query = final_url[position + 1..].to_string();
        final_url.truncate(position);
        for pair in query.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            params.push(Param {
                name: urlencoding_decode(name),
                value: urlencoding_decode(value),
                kind: ParamKind::Query,
                description: None,
                disabled: false,
                extra: Extra::new(),
            });
        }
    }
    let body = if !form_fields.is_empty() {
        Some(Body::Multipart { data: form_fields })
    } else if !data_parts.is_empty() {
        let data = data_parts.join("&");
        if serde_json::from_str::<Value>(&data).is_ok() {
            Some(Body::Json { data })
        } else {
            Some(Body::Text { data })
        }
    } else {
        None
    };
    let method = method
        .or_else(|| {
            if body.is_some() && !get_mode {
                Some("POST".into())
            } else {
                Some("GET".into())
            }
        })
        .unwrap_or_else(|| "GET".into());
    let auth = basic
        .map(|(user, password)| AuthConfig::basic(user, password))
        .or_else(|| {
            headers
                .iter()
                .find(|header| header.name.eq_ignore_ascii_case("authorization"))
                .and_then(|header| {
                    header
                        .value
                        .strip_prefix("Bearer ")
                        .map(|token| AuthConfig::bearer(token.trim()))
                })
        });
    let name = request_name_from_url(&final_url);
    let mut request = RequestFile {
        info: RequestInfo {
            name,
            kind: "http".into(),
            seq: 0,
            tags: vec!["importado".into(), "curl".into()],
            description: None,
            extra: Extra::new(),
        },
        http: HttpDetails {
            method,
            url: final_url,
            headers,
            params,
            body,
            auth,
        },
        runtime: RequestRuntime::default(),
        settings: HttpSettings {
            follow_redirects: Some(follow_redirects),
            ..HttpSettings::default()
        },
        docs: None,
        extensions: Extensions::default(),
        extra: Extra::new(),
    };
    if report.format == "curl" {
        // nome e tags são preenchidos pelo importador; parse_curl também é
        // usado isoladamente pela CLI.
        request.info.tags = vec!["curl".into()];
    }
    Ok(request)
}

fn request_name_from_url(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .map(|url| {
            let path = url.path().trim_matches('/');
            if path.is_empty() {
                url.host_str().unwrap_or("Requisição").to_string()
            } else {
                path.rsplit('/').next().unwrap_or("Requisição").to_string()
            }
        })
        .unwrap_or_else(|| "Requisição importada".into())
}

fn urlencoding_decode(input: &str) -> String {
    url::form_urlencoded::parse(input.as_bytes())
        .next()
        .map(|(_, value)| value.to_string())
        .unwrap_or_else(|| input.to_string())
}

// ---------------------------------------------------------------------------
// Postman
// ---------------------------------------------------------------------------

fn import_postman_into(
    store: &mut CollectionStore,
    parent: Option<&str>,
    content: &str,
    report: &mut ImportReport,
) -> Result<()> {
    let root: Value = serde_json::from_str(content)
        .map_err(|error| Error::Import(format!("Postman JSON inválido: {error}")))?;
    if let Some(variables) = root.get("variable").and_then(Value::as_array) {
        let mut imported = Vec::new();
        let mut secret_refs = Vec::new();
        for variable in variables {
            let Some(name) = variable.get("key").and_then(Value::as_str) else {
                continue;
            };
            let value = variable.get("value").and_then(Value::as_str).unwrap_or("");
            let is_secret = variable
                .get("type")
                .and_then(Value::as_str)
                .map(|kind| kind.eq_ignore_ascii_case("secret"))
                .unwrap_or(false);
            if is_secret {
                report.loss(
                    format!("variável {name}"),
                    "value",
                    "valor secreto não importado; configure REQLIVO_SECRET_* ou uma referência de segredo",
                );
                imported.push(Variable::secret(name));
                secret_refs.push(SecretRef {
                    name: name.to_string(),
                    environment: None,
                    provider: SecretProvider::Env,
                    key: format!(
                        "REQLIVO_{}",
                        name.to_ascii_uppercase().replace(['-', ' '], "_")
                    ),
                    description: None,
                    extra: Extra::new(),
                });
            } else {
                imported.push(Variable::plain(name, value));
            }
        }
        if !imported.is_empty() {
            let mut defaults = store.manifest.request.clone().unwrap_or_default();
            defaults.variables.extend(imported);
            store.set_collection_defaults(Some(defaults))?;
            if !secret_refs.is_empty() {
                let mut manifest = store.manifest.clone();
                let ext = crate::store::ensure_reqlivo_ext(&mut manifest.extensions);
                ext.secret_refs.extend(secret_refs);
                store.manifest = manifest;
                store.save_manifest()?;
            }
            report
                .warnings
                .push("variáveis de coleção importadas para o escopo da coleção".into());
        }
    }
    if let Some(items) = root.get("item").and_then(Value::as_array) {
        import_postman_items(store, parent, items, report)?;
    } else {
        report
            .warnings
            .push("o arquivo Postman não contém itens".into());
    }
    Ok(())
}

fn import_postman_items(
    store: &mut CollectionStore,
    parent: Option<&str>,
    items: &[Value],
    report: &mut ImportReport,
) -> Result<()> {
    for (index, item) in items.iter().enumerate() {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Item")
            .to_string();
        if let Some(children) = item.get("item").and_then(Value::as_array) {
            let folder_id = store.add_folder(parent, &name)?;
            report.folders_imported += 1;
            report.imported_ids.push(folder_id.clone());
            let auth = item
                .get("auth")
                .and_then(|auth| postman_auth(auth, report, &name));
            if auth.is_some() {
                let defaults = RequestDefaults {
                    auth,
                    ..RequestDefaults::default()
                };
                store.set_folder_defaults(&folder_id, Some(defaults), None)?;
            }
            import_postman_items(store, Some(&folder_id), children, report)?;
        } else if let Some(request) = item.get("request") {
            match postman_request(request, item, &name, report) {
                Ok(mut request) => {
                    request.info.seq = (index + 1) as i64;
                    let payload = store.add_request(parent, request)?;
                    report.requests_imported += 1;
                    report.imported_ids.push(payload.id);
                }
                Err(error) => {
                    report.loss(&name, "request", error.to_string());
                }
            }
        } else {
            report.loss(&name, "item", "item sem request ou subitens; ignorado");
        }
    }
    Ok(())
}

fn postman_request(
    request: &Value,
    item: &Value,
    name: &str,
    report: &mut ImportReport,
) -> Result<RequestFile> {
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("GET")
        .to_uppercase();
    let url_value = request.get("url");
    let (url, mut params) = postman_url(url_value, name, report);
    let mut headers = Vec::new();
    if let Some(list) = request.get("header").and_then(Value::as_array) {
        for header in list {
            let Some(header_name) = header.get("key").and_then(Value::as_str) else {
                continue;
            };
            if header_name.starts_with(':') {
                continue;
            }
            headers.push(Header {
                name: header_name.to_string(),
                value: header
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                description: header
                    .get("description")
                    .map(|value| serde_yaml::to_value(value).unwrap_or(serde_yaml::Value::Null)),
                disabled: header
                    .get("disabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                extra: Extra::new(),
            });
        }
    }
    let body = postman_body(request.get("body"), name, report);
    let auth = request
        .get("auth")
        .and_then(|auth| postman_auth(auth, report, name));
    let docs = item
        .get("description")
        .or_else(|| request.get("description"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut extensions = Extensions::default();
    let scripts: Vec<Value> = item
        .get("event")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !scripts.is_empty() {
        extensions
            .extra
            .insert("postmanScripts".into(), serde_yaml::to_value(&scripts)?);
        report.loss(
            name,
            "event",
            "scripts Postman preservados em extensions.postmanScripts, mas não executados",
        );
    }
    // Query params do Postman só entram se a URL não os tiver embutido.
    if url.contains('?') {
        if !params.is_empty() {
            report.warnings.push(format!(
                "{name}: parâmetros de query já presentes na URL; lista duplicada foi ignorada"
            ));
            params.clear();
        }
    }
    Ok(RequestFile {
        info: RequestInfo {
            name: name.to_string(),
            kind: "http".into(),
            seq: 0,
            tags: vec!["importado".into(), "postman".into()],
            description: None,
            extra: Extra::new(),
        },
        http: HttpDetails {
            method,
            url,
            headers,
            params,
            body,
            auth,
        },
        runtime: RequestRuntime::default(),
        settings: HttpSettings {
            follow_redirects: Some(true),
            ..HttpSettings::default()
        },
        docs,
        extensions,
        extra: Extra::new(),
    })
}

fn postman_url(
    value: Option<&Value>,
    name: &str,
    report: &mut ImportReport,
) -> (String, Vec<Param>) {
    match value {
        Some(Value::String(url)) => (url.clone(), Vec::new()),
        Some(Value::Object(object)) => {
            let raw = object
                .get("raw")
                .and_then(Value::as_str)
                .map(str::to_string);
            let mut params = Vec::new();
            if let Some(query) = object.get("query").and_then(Value::as_array) {
                for pair in query {
                    let Some(key) = pair.get("key").and_then(Value::as_str) else {
                        continue;
                    };
                    params.push(Param {
                        name: key.to_string(),
                        value: pair
                            .get("value")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        kind: ParamKind::Query,
                        description: None,
                        disabled: pair
                            .get("disabled")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        extra: Extra::new(),
                    });
                }
            }
            let url = raw.unwrap_or_else(|| {
                let protocol = object
                    .get("protocol")
                    .and_then(Value::as_str)
                    .unwrap_or("http");
                let host = object.get("host").map(value_to_string).unwrap_or_default();
                let path = object.get("path").map(value_to_string).unwrap_or_default();
                format!("{protocol}://{host}/{path}")
            });
            if url.is_empty() {
                report.loss(name, "url", "URL ausente no Postman");
            }
            (url, params)
        }
        None => {
            report.loss(name, "url", "URL ausente no Postman");
            (String::new(), Vec::new())
        }
        Some(other) => (value_to_string(other), Vec::new()),
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(value_to_string)
            .collect::<Vec<_>>()
            .join("/"),
        other => other.to_string(),
    }
}

fn postman_body(value: Option<&Value>, name: &str, report: &mut ImportReport) -> Option<Body> {
    let body = value?;
    let mode = body.get("mode").and_then(Value::as_str).unwrap_or("raw");
    match mode {
        "raw" => {
            let raw = body
                .get("raw")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let language = body
                .get("options")
                .and_then(|options| options.get("raw"))
                .and_then(|raw| raw.get("language"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if language.eq_ignore_ascii_case("json") || serde_json::from_str::<Value>(&raw).is_ok()
            {
                Some(Body::Json { data: raw })
            } else if language.eq_ignore_ascii_case("xml") {
                Some(Body::Xml { data: raw })
            } else {
                Some(Body::Text { data: raw })
            }
        }
        "urlencoded" => Some(Body::FormUrlEncoded {
            data: body
                .get("urlencoded")
                .and_then(Value::as_array)
                .map(|fields| {
                    fields
                        .iter()
                        .filter_map(|field| {
                            Some(FormField {
                                name: field.get("key")?.as_str()?.to_string(),
                                value: field
                                    .get("value")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string(),
                                kind: None,
                                description: None,
                                disabled: field
                                    .get("disabled")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false),
                                extra: Extra::new(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }),
        "formdata" => Some(Body::Multipart {
            data: body
                .get("formdata")
                .and_then(Value::as_array)
                .map(|fields| {
                    fields
                        .iter()
                        .filter_map(|field| {
                            let kind = field.get("type").and_then(Value::as_str).unwrap_or("text");
                            if kind == "file" {
                                report.loss(name, "formdata", "arquivo multipart não é importado");
                                return None;
                            }
                            Some(FormField {
                                name: field.get("key")?.as_str()?.to_string(),
                                value: field
                                    .get("value")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string(),
                                kind: None,
                                description: None,
                                disabled: field
                                    .get("disabled")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false),
                                extra: Extra::new(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }),
        "graphql" => {
            report.loss(name, "body", "corpo GraphQL não suportado no MVP");
            None
        }
        "file" => {
            report.loss(name, "body", "corpo de arquivo não suportado no MVP");
            None
        }
        other => {
            report.loss(
                name,
                "body",
                format!("modo de corpo não suportado: {other}"),
            );
            None
        }
    }
}

fn postman_auth(value: &Value, report: &mut ImportReport, item: &str) -> Option<AuthConfig> {
    let kind = value.get("type").and_then(Value::as_str)?;
    let field = |name: &str| -> String {
        value
            .get(kind)
            .and_then(Value::as_array)
            .and_then(|list| {
                list.iter().find(|entry| {
                    entry
                        .get("key")
                        .and_then(Value::as_str)
                        .map(|key| key.eq_ignore_ascii_case(name))
                        .unwrap_or(false)
                })
            })
            .and_then(|entry| entry.get("value"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    match kind.to_ascii_lowercase().as_str() {
        "bearer" => Some(AuthConfig::bearer(field("token"))),
        "basic" => Some(AuthConfig::basic(field("username"), field("password"))),
        "apikey" => {
            let placement = if field("in").eq_ignore_ascii_case("query") {
                ApiKeyPlacement::Query
            } else {
                ApiKeyPlacement::Header
            };
            Some(AuthConfig::api_key(field("key"), field("value"), placement))
        }
        "noauth" => None,
        "inherit" => Some(AuthConfig::inherit()),
        other => {
            report.loss(
                item,
                "auth",
                format!("tipo de auth Postman não suportado: {other}"),
            );
            None
        }
    }
}

// ---------------------------------------------------------------------------
// OpenCollection
// ---------------------------------------------------------------------------

fn import_opencollection_value(
    store: &mut CollectionStore,
    parent: Option<&str>,
    root: &serde_yaml::Value,
    report: &mut ImportReport,
) -> Result<()> {
    let manifest: CollectionManifest = serde_yaml::from_value(root.clone())
        .map_err(|error| Error::Import(format!("manifesto OpenCollection inválido: {error}")))?;
    import_opencollection_environments(store, &manifest, report)?;
    if let Some(defaults) = &manifest.request {
        let mut current = store.manifest.request.clone().unwrap_or_default();
        current.headers.extend(defaults.headers.clone());
        current.variables.extend(defaults.variables.clone());
        current.scripts.extend(defaults.scripts.clone());
        if defaults.auth.is_some() {
            current.auth = defaults.auth.clone();
        }
        store.set_collection_defaults(Some(current))?;
    }
    if let Some(items) = root.get("items").and_then(|items| items.as_sequence()) {
        import_oc_items(store, parent, items, report)?;
    } else {
        report.warnings.push(
            "OpenCollection sem 'items'; use a importação de diretório para coleções não empacotadas"
                .into(),
        );
    }
    Ok(())
}

fn import_opencollection_dir(
    store: &mut CollectionStore,
    parent: Option<&str>,
    path: &Path,
    report: &mut ImportReport,
) -> Result<()> {
    let manifest_path = ["opencollection.yml", "opencollection.yaml"]
        .into_iter()
        .map(|name| path.join(name))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            Error::Import(format!(
                "diretório '{}' não contém opencollection.yml",
                path.display()
            ))
        })?;
    let bytes = fs::read(&manifest_path)?;
    let root: serde_yaml::Value = serde_yaml::from_slice(&bytes)?;
    let manifest: CollectionManifest = serde_yaml::from_value(root.clone())
        .map_err(|error| Error::Import(format!("manifesto OpenCollection inválido: {error}")))?;
    import_opencollection_environments(store, &manifest, report)?;
    if let Some(defaults) = &manifest.request {
        let mut current = store.manifest.request.clone().unwrap_or_default();
        current.headers.extend(defaults.headers.clone());
        current.variables.extend(defaults.variables.clone());
        current.scripts.extend(defaults.scripts.clone());
        if defaults.auth.is_some() {
            current.auth = defaults.auth.clone();
        }
        store.set_collection_defaults(Some(current))?;
    }
    import_oc_directory(store, parent, path, report)
}

fn import_oc_directory(
    store: &mut CollectionStore,
    parent: Option<&str>,
    directory: &Path,
    report: &mut ImportReport,
) -> Result<()> {
    let mut entries: Vec<PathBuf> = fs::read_dir(directory)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            !path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with('.'))
                .unwrap_or(false)
        })
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            let folder_file = ["folder.yml", "folder.yaml"]
                .into_iter()
                .map(|name| path.join(name))
                .find(|candidate| candidate.is_file());
            let Some(folder_file) = folder_file else {
                report.warnings.push(format!(
                    "diretório sem folder.yml ignorado: {}",
                    path.display()
                ));
                continue;
            };
            let bytes = fs::read(&folder_file)?;
            let folder: FolderFile = serde_yaml::from_slice(&bytes)
                .map_err(|error| Error::Import(format!("pasta inválida: {error}")))?;
            let folder_id = store.add_folder(parent, &folder.info.name)?;
            report.folders_imported += 1;
            report.imported_ids.push(folder_id.clone());
            if folder.request.is_some() || folder.docs.is_some() {
                store.set_folder_defaults(&folder_id, folder.request, folder.docs)?;
            }
            import_oc_directory(store, Some(&folder_id), &path, report)?;
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if matches!(
            name,
            "opencollection.yml" | "opencollection.yaml" | "folder.yml" | "folder.yaml"
        ) || !matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("yml") | Some("yaml")
        ) {
            continue;
        }
        let bytes = fs::read(&path)?;
        let value: serde_yaml::Value = match serde_yaml::from_slice(&bytes) {
            Ok(value) => value,
            Err(error) => {
                report.loss(name, "yaml", format!("YAML inválido: {error}"));
                continue;
            }
        };
        let kind = value
            .get("info")
            .and_then(|info| info.get("type"))
            .and_then(|kind| kind.as_str())
            .unwrap_or("http");
        if !kind.eq_ignore_ascii_case("http") {
            report.loss(name, "type", format!("tipo não suportado no MVP: {kind}"));
            continue;
        }
        match serde_yaml::from_value::<RequestFile>(value) {
            Ok(request) => {
                let payload = store.add_request(parent, request)?;
                report.requests_imported += 1;
                report.imported_ids.push(payload.id);
            }
            Err(error) => report.loss(name, "request", format!("estrutura inválida: {error}")),
        }
    }
    Ok(())
}

fn import_oc_items(
    store: &mut CollectionStore,
    parent: Option<&str>,
    items: &[serde_yaml::Value],
    report: &mut ImportReport,
) -> Result<()> {
    for item in items {
        let kind = item
            .get("info")
            .and_then(|info| info.get("type"))
            .and_then(|kind| kind.as_str())
            .unwrap_or("http");
        if kind.eq_ignore_ascii_case("folder") {
            let folder: FolderFile = match serde_yaml::from_value(item.clone()) {
                Ok(folder) => folder,
                Err(error) => {
                    report.loss("pasta", "folder", format!("estrutura inválida: {error}"));
                    continue;
                }
            };
            let folder_id = store.add_folder(parent, &folder.info.name)?;
            report.folders_imported += 1;
            report.imported_ids.push(folder_id.clone());
            if folder.request.is_some() || folder.docs.is_some() {
                store.set_folder_defaults(&folder_id, folder.request, folder.docs)?;
            }
            if let Some(children) = item.get("items").and_then(|items| items.as_sequence()) {
                import_oc_items(store, Some(&folder_id), children, report)?;
            }
        } else if kind.eq_ignore_ascii_case("http") {
            match serde_yaml::from_value::<RequestFile>(item.clone()) {
                Ok(request) => {
                    let payload = store.add_request(parent, request)?;
                    report.requests_imported += 1;
                    report.imported_ids.push(payload.id);
                }
                Err(error) => report.loss(
                    item.get("info")
                        .and_then(|info| info.get("name"))
                        .and_then(|name| name.as_str())
                        .unwrap_or("requisição"),
                    "request",
                    format!("estrutura inválida: {error}"),
                ),
            }
        } else {
            report.loss(
                item.get("info")
                    .and_then(|info| info.get("name"))
                    .and_then(|name| name.as_str())
                    .unwrap_or("item"),
                "type",
                format!("tipo não suportado no MVP: {kind}"),
            );
        }
    }
    Ok(())
}

fn import_opencollection_environments(
    store: &mut CollectionStore,
    manifest: &CollectionManifest,
    report: &mut ImportReport,
) -> Result<()> {
    let secret_refs = manifest.secret_refs().to_vec();
    for environment in manifest.environments() {
        let vars = environment.variables.clone();
        let refs: Vec<SecretRef> = secret_refs
            .iter()
            .filter(|reference| {
                reference
                    .environment
                    .as_deref()
                    .map(|name| name == environment.name)
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        let mut missing: Vec<Variable> = Vec::new();
        for variable in &vars {
            if variable.secret && !refs.iter().any(|reference| reference.name == variable.name) {
                missing.push(variable.clone());
            }
        }
        if !missing.is_empty() {
            report.warnings.push(format!(
                "ambiente '{}': {} segredo(s) sem referência; crie extensions.reqlivo.secretRefs",
                environment.name,
                missing.len()
            ));
        }
        store.upsert_environment(&environment.name, vars, refs, environment.extends.clone())?;
        report.environments_imported += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Exportação
// ---------------------------------------------------------------------------

pub fn export(
    store: &CollectionStore,
    format: ExportFormat,
    request_id: Option<&str>,
) -> Result<ExportReport> {
    match format {
        ExportFormat::Curl => export_curl(store, request_id),
        ExportFormat::Postman => export_postman(store),
        ExportFormat::OpenCollection => export_opencollection(store),
        ExportFormat::Project => Err(Error::Export(
            "exportação de projeto exige um diretório de destino; use export_project".into(),
        )),
    }
}

pub fn export_project(store: &CollectionStore, destination: &Path) -> Result<ExportReport> {
    let files = store.export_project(destination)?;
    let mut report = ExportReport::new(ExportFormat::Project);
    report.destination = Some(destination.display().to_string());
    report.files = files.len();
    report.requests = store.requests.len();
    report.folders = store.folders.len();
    sanitize_exported_project(destination, &mut report)?;
    Ok(report)
}

fn auth_has_literal_secret(auth: &AuthConfig) -> bool {
    let literal = |value: &str| !value.trim().is_empty() && !value.contains("{{");
    match auth.mode {
        AuthMode::Basic => literal(&auth.password),
        AuthMode::Bearer => literal(&auth.token),
        AuthMode::ApiKey => literal(&auth.value),
        AuthMode::Inherit => false,
    }
}

fn sanitize_auth(auth: &mut Option<AuthConfig>, item: &str, report: &mut ExportReport) {
    if auth.as_ref().is_some_and(auth_has_literal_secret) {
        *auth = None;
        report.warnings.push(format!(
            "{item}: autenticação literal omitida na exportação; use variável ou segredo referenciado"
        ));
    }
}

fn sanitize_headers(headers: &mut Vec<Header>, item: &str, report: &mut ExportReport) {
    let before = headers.len();
    headers.retain(|header| !(is_sensitive_name(&header.name) && !header.value.contains("{{")));
    let removed = before.saturating_sub(headers.len());
    if removed > 0 {
        report.warnings.push(format!(
            "{item}: {removed} cabeçalho(s) sensível(is) literal(is) omitido(s) na exportação"
        ));
    }
}

fn sanitize_url(url: &mut String, item: &str, report: &mut ExportReport) {
    let original = url.clone();
    let mut changed = false;

    let (before_fragment, fragment) = match original.find('#') {
        Some(index) => (&original[..index], Some(&original[index..])),
        None => (original.as_str(), None),
    };
    let (base, query) = match before_fragment.find('?') {
        Some(index) => (
            &before_fragment[..index],
            Some(&before_fragment[index + 1..]),
        ),
        None => (before_fragment, None),
    };

    let mut cleaned_base = base.to_string();
    if let Some(scheme_end) = cleaned_base.find("://") {
        let authority_start = scheme_end + 3;
        let authority_end = cleaned_base[authority_start..]
            .find('/')
            .map(|index| authority_start + index)
            .unwrap_or(cleaned_base.len());
        if let Some(at) = cleaned_base[authority_start..authority_end].rfind('@') {
            let userinfo = &cleaned_base[authority_start..authority_start + at];
            if !userinfo.contains("{{") {
                cleaned_base.replace_range(authority_start..authority_start + at + 1, "");
                changed = true;
            }
        }
    }

    let query_out = query.map(|query| {
        let mut kept = Vec::new();
        for pair in query.split('&') {
            let (raw_key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let key = url::form_urlencoded::parse(raw_key.as_bytes())
                .next()
                .map(|(key, _)| key.into_owned())
                .unwrap_or_else(|| raw_key.to_string());
            if is_sensitive_query(&key) && !value.contains("{{") {
                changed = true;
            } else {
                kept.push(pair.to_string());
            }
        }
        kept.join("&")
    });

    if !changed {
        return;
    }

    let mut rebuilt = cleaned_base;
    if let Some(query) = query_out {
        if !query.is_empty() {
            rebuilt.push('?');
            rebuilt.push_str(&query);
        } else if query.is_empty() && fragment.is_none() {
            // Uma query vazia explícita é preservada.
            if before_fragment.ends_with('?') {
                rebuilt.push('?');
            }
        }
    }
    if let Some(fragment) = fragment {
        rebuilt.push_str(fragment);
    }
    *url = rebuilt;
    report.warnings.push(format!(
        "{item}: credenciais/parâmetros sensíveis da URL omitidos na exportação"
    ));
}

fn sanitize_params(params: &mut Vec<Param>, item: &str, report: &mut ExportReport) {
    let before = params.len();
    params.retain(|param| {
        !(param.kind == ParamKind::Query
            && is_sensitive_query(&param.name)
            && !param.value.contains("{{"))
    });
    if params.len() != before {
        report.warnings.push(format!(
            "{item}: parâmetro(s) de query sensível(is) literal(is) omitido(s) na exportação"
        ));
    }
}

fn sanitize_variables(variables: &mut [Variable]) {
    for variable in variables {
        if variable.secret {
            variable.value = None;
        }
    }
}

fn sanitize_defaults(defaults: &mut RequestDefaults, item: &str, report: &mut ExportReport) {
    sanitize_headers(&mut defaults.headers, item, report);
    sanitize_auth(&mut defaults.auth, item, report);
    sanitize_variables(&mut defaults.variables);
}

fn sanitize_request_for_export(request: &mut RequestFile, item: &str, report: &mut ExportReport) {
    sanitize_url(&mut request.http.url, item, report);
    sanitize_params(&mut request.http.params, item, report);
    sanitize_headers(&mut request.http.headers, item, report);
    sanitize_auth(&mut request.http.auth, item, report);
}

fn find_export_manifest(directory: &Path, stem: &str) -> Option<PathBuf> {
    ["yml", "yaml"]
        .iter()
        .map(|extension| directory.join(format!("{stem}.{extension}")))
        .find(|path| path.is_file())
}

fn sanitize_exported_project(destination: &Path, report: &mut ExportReport) -> Result<()> {
    let store = CollectionStore::open(destination)?;
    let mut manifest = store.manifest.clone();
    if let Some(defaults) = &mut manifest.request {
        sanitize_defaults(defaults, "coleção", report);
    }
    for environment in &mut manifest.config.environments {
        sanitize_variables(&mut environment.variables);
    }
    if let Some(path) = find_export_manifest(destination, "opencollection") {
        write_yaml(&path, &manifest)?;
    }
    for folder in store.folders.values() {
        let mut file = folder.file.clone();
        if let Some(defaults) = &mut file.request {
            sanitize_defaults(defaults, &format!("pasta '{}'", folder.name), report);
        }
        if let Some(path) = find_export_manifest(&folder.dir, "folder") {
            write_yaml(&path, &file)?;
        }
    }
    for entry in store.requests.values() {
        let mut file = entry.file.clone();
        sanitize_request_for_export(&mut file, &entry.name, report);
        write_yaml(&destination.join(&entry.rel_path), &file)?;
    }
    let internal = destination.join(".reqlivo");
    if internal.exists() {
        fs::remove_dir_all(internal)?;
    }
    Ok(())
}

fn export_curl(store: &CollectionStore, request_id: Option<&str>) -> Result<ExportReport> {
    let request_id = request_id
        .ok_or_else(|| Error::Export("exportação cURL exige a requisição de origem".into()))?;
    let entry = store.request(request_id)?;
    let mut file = entry.file.clone();
    let mut report = ExportReport::new(ExportFormat::Curl);
    sanitize_request_for_export(&mut file, &entry.name, &mut report);
    let request = &file;
    report.requests = 1;

    // Exporta a URL com path/query params declarados e, quando seguro,
    // a auth efetiva herdada. Templates permanecem intactos.
    let mut url = request.http.url.clone();
    for param in request
        .http
        .params
        .iter()
        .filter(|param| !param.disabled && param.kind == ParamKind::Path)
    {
        url = url.replace(&format!(":{}", param.name), &param.value);
    }
    let mut query_parts: Vec<String> = request
        .http
        .params
        .iter()
        .filter(|param| !param.disabled && param.kind == ParamKind::Query)
        .filter(|param| !param.name.trim().is_empty())
        .map(|param| format!("{}={}", param.name, param.value))
        .collect();
    let (request_auth, folders, collection_auth) =
        store.auth_inputs_with(request_id, &entry.file)?;
    let mut auth = effective_auth(request_auth.as_ref(), &folders, collection_auth.as_ref())
        .map(|source| source.auth);
    sanitize_auth(&mut auth, &entry.name, &mut report);
    if let Some(auth) = &auth {
        if auth.mode == AuthMode::ApiKey && auth.placement == ApiKeyPlacement::Query {
            query_parts.push(format!("{}={}", auth.key, auth.value));
        }
    }
    if !query_parts.is_empty() {
        url.push(if url.contains('?') { '&' } else { '?' });
        url.push_str(&query_parts.join("&"));
    }

    let mut lines = vec![format!(
        "curl --request {} {}",
        shell_quote(&request.http.method.to_uppercase()),
        shell_quote(&url)
    )];
    let effective_headers = store.effective_headers_with(request_id, &entry.file)?;
    let has_content_type = effective_headers
        .iter()
        .any(|header| !header.disabled && header.name.eq_ignore_ascii_case("content-type"));
    for header in effective_headers {
        if header.disabled || header.name.trim().is_empty() {
            continue;
        }
        if is_sensitive_name(&header.name) && !header.value.contains("{{") {
            report.warnings.push(format!(
                "cabeçalho sensível '{}' omitido; use uma variável ou segredo",
                header.name
            ));
            continue;
        }
        lines.push(format!(
            "  --header {}",
            shell_quote(&format!("{}: {}", header.name, header.value))
        ));
    }
    if !has_content_type {
        let content_type = match &request.http.body {
            Some(Body::Json { .. }) => Some("application/json"),
            Some(Body::Text { .. }) => Some("text/plain; charset=utf-8"),
            Some(Body::Xml { .. }) => Some("application/xml"),
            Some(Body::Sparql { .. }) => Some("application/sparql-query"),
            Some(Body::FormUrlEncoded { .. }) => Some("application/x-www-form-urlencoded"),
            Some(Body::Multipart { .. }) | Some(Body::Unsupported) | None => None,
        };
        if let Some(content_type) = content_type {
            lines.push(format!(
                "  --header {}",
                shell_quote(&format!("Content-Type: {content_type}"))
            ));
        }
    }
    if let Some(auth) = &auth {
        match auth.mode {
            AuthMode::Bearer => lines.push(format!(
                "  --header {}",
                shell_quote(&format!("Authorization: Bearer {}", auth.token))
            )),
            AuthMode::Basic => lines.push(format!(
                "  --user {}",
                shell_quote(&format!("{}:{}", auth.username, auth.password))
            )),
            AuthMode::ApiKey if auth.placement == ApiKeyPlacement::Header => {
                lines.push(format!(
                    "  --header {}",
                    shell_quote(&format!("{}: {}", auth.key, auth.value))
                ));
            }
            _ => {}
        }
    }
    match &request.http.body {
        Some(Body::Json { data })
        | Some(Body::Text { data })
        | Some(Body::Xml { data })
        | Some(Body::Sparql { data }) => {
            lines.push(format!("  --data-raw {}", shell_quote(data)));
        }
        Some(Body::FormUrlEncoded { data }) => {
            for field in data.iter().filter(|field| !field.disabled) {
                lines.push(format!(
                    "  --data-urlencode {}",
                    shell_quote(&format!("{}={}", field.name, field.value))
                ));
            }
        }
        Some(Body::Multipart { data }) => {
            for field in data.iter().filter(|field| !field.disabled) {
                let prefix = if field.kind.as_deref() == Some("file") {
                    "@"
                } else {
                    ""
                };
                lines.push(format!(
                    "  --form {}",
                    shell_quote(&format!("{}={}{}", field.name, prefix, field.value))
                ));
            }
        }
        Some(Body::Unsupported) => {
            report.losses.push(Loss {
                item: entry.name.clone(),
                field: "body".into(),
                reason: "corpo não suportado".into(),
            });
        }
        None => {}
    }
    report.content = Some(format!("{}\n", lines.join(" \\\n")));
    Ok(report)
}

fn export_postman(store: &CollectionStore) -> Result<ExportReport> {
    let mut report = ExportReport::new(ExportFormat::Postman);
    report.requests = store.requests.len();
    report.folders = store.folders.len();
    let mut item = Vec::new();
    item.extend(postman_export_items(store, None, &mut report));
    let mut variables = Vec::new();
    if let Some(defaults) = &store.manifest.request {
        for variable in &defaults.variables {
            if variable.secret {
                report
                    .warnings
                    .push(format!("variável secreta '{}' omitida", variable.name));
                continue;
            }
            variables.push(json!({
                "key": variable.name,
                "value": variable.text_value().unwrap_or_default(),
            }));
        }
    }
    let collection = json!({
        "info": {
            "name": store.manifest.info.name,
            "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json",
        },
        "item": item,
        "variable": variables,
    });
    report.content = Some(serde_json::to_string_pretty(&collection)?);
    Ok(report)
}

fn postman_export_items(
    store: &CollectionStore,
    parent: Option<&str>,
    report: &mut ExportReport,
) -> Vec<Value> {
    let mut items = Vec::new();
    let tree = if parent.is_none() {
        store.tree()
    } else {
        Vec::new()
    };
    if parent.is_none() {
        for node in &tree {
            items.push(postman_node(store, node, report));
        }
        return items;
    }
    items
}

fn postman_node(
    store: &CollectionStore,
    node: &crate::store::TreeNode,
    report: &mut ExportReport,
) -> Value {
    match node {
        crate::store::TreeNode::Folder { id, name, .. } => {
            let children = postman_children(store, Some(id), report);
            let mut auth = store
                .folder(id)
                .ok()
                .and_then(|folder| folder.auth().cloned())
                .filter(|auth| auth.mode != AuthMode::Inherit);
            sanitize_auth(&mut auth, name, report);
            let auth = auth.as_ref().map(postman_auth_value);
            let mut object = json!({ "name": name, "item": children });
            if let Some(auth) = auth {
                object["auth"] = auth;
            }
            object
        }
        crate::store::TreeNode::Request { id, .. } => postman_export_request(store, id, report),
    }
}

fn postman_children(
    store: &CollectionStore,
    parent: Option<&str>,
    report: &mut ExportReport,
) -> Vec<Value> {
    let mut items = Vec::new();
    let nodes = if parent.is_none() {
        store.tree()
    } else {
        let Some(id) = parent else { return items };
        // Reconstrói os filhos a partir da árvore completa.
        find_children(store, id)
    };
    for node in nodes {
        items.push(postman_node(store, &node, report));
    }
    items
}

fn find_children(store: &CollectionStore, folder_id: &str) -> Vec<crate::store::TreeNode> {
    fn walk(nodes: &[crate::store::TreeNode], target: &str) -> Option<Vec<crate::store::TreeNode>> {
        for node in nodes {
            if let crate::store::TreeNode::Folder { id, children, .. } = node {
                if id == target {
                    return Some(children.clone());
                }
                if let Some(found) = walk(children, target) {
                    return Some(found);
                }
            }
        }
        None
    }
    walk(&store.tree(), folder_id).unwrap_or_default()
}

fn postman_export_request(store: &CollectionStore, id: &str, report: &mut ExportReport) -> Value {
    let Ok(entry) = store.request(id) else {
        return json!({"name": "requisição ausente"});
    };
    let mut file = entry.file.clone();
    sanitize_request_for_export(&mut file, &entry.name, report);
    let request = &file;
    let headers: Vec<Value> = request
        .http
        .headers
        .iter()
        .map(|header| {
            json!({
                "key": header.name,
                "value": header.value,
                "disabled": header.disabled,
            })
        })
        .collect();
    let url = if request.http.params.is_empty() {
        json!({ "raw": request.http.url })
    } else {
        let query: Vec<Value> = request
            .http
            .params
            .iter()
            .filter(|param| param.kind == ParamKind::Query)
            .map(|param| {
                json!({
                    "key": param.name,
                    "value": param.value,
                    "disabled": param.disabled,
                })
            })
            .collect();
        let mut raw = request.http.url.clone();
        let query_parts: Vec<String> = query
            .iter()
            .filter(|item| item["disabled"].as_bool() != Some(true))
            .map(|item| {
                format!(
                    "{}={}",
                    item["key"].as_str().unwrap_or(""),
                    item["value"].as_str().unwrap_or("")
                )
            })
            .collect();
        if !query_parts.is_empty() {
            raw.push(if raw.contains('?') { '&' } else { '?' });
            raw.push_str(&query_parts.join("&"));
        }
        json!({ "raw": raw, "query": query })
    };
    let body = match &request.http.body {
        Some(Body::Json { data }) => json!({
            "mode": "raw",
            "raw": data,
            "options": { "raw": { "language": "json" } }
        }),
        Some(Body::Text { data }) => json!({ "mode": "raw", "raw": data }),
        Some(Body::Xml { data }) => json!({
            "mode": "raw",
            "raw": data,
            "options": { "raw": { "language": "xml" } }
        }),
        Some(Body::Sparql { data }) => json!({ "mode": "raw", "raw": data }),
        Some(Body::FormUrlEncoded { data }) => json!({
            "mode": "urlencoded",
            "urlencoded": data.iter().map(|field| json!({
                "key": field.name, "value": field.value, "disabled": field.disabled
            })).collect::<Vec<_>>()
        }),
        Some(Body::Multipart { data }) => json!({
            "mode": "formdata",
            "formdata": data.iter().filter(|field| field.kind.as_deref() != Some("file")).map(|field| json!({
                "key": field.name, "value": field.value, "type": "text", "disabled": field.disabled
            })).collect::<Vec<_>>()
        }),
        Some(Body::Unsupported) | None => Value::Null,
    };
    let mut object = json!({
        "name": entry.name,
        "request": {
            "method": request.http.method,
            "header": headers,
            "url": url,
        }
    });
    if !body.is_null() {
        object["request"]["body"] = body;
    }
    if let Some(auth) = &request.http.auth {
        if auth.mode != AuthMode::Inherit {
            object["request"]["auth"] = postman_auth_value(auth);
        }
    }
    let events = postman_events(request, report, &entry.name);
    if !events.is_empty() {
        object["event"] = Value::Array(events);
    }
    if let Some(docs) = &request.docs {
        object["request"]["description"] = Value::String(docs.clone());
    }
    object
}

fn postman_auth_value(auth: &AuthConfig) -> Value {
    match auth.mode {
        AuthMode::Inherit => json!({ "type": "inherit" }),
        AuthMode::Basic => json!({
            "type": "basic",
            "basic": [
                {"key": "username", "value": auth.username},
                {"key": "password", "value": auth.password},
            ]
        }),
        AuthMode::Bearer => json!({
            "type": "bearer",
            "bearer": [{"key": "token", "value": auth.token}]
        }),
        AuthMode::ApiKey => json!({
            "type": "apikey",
            "apikey": [
                {"key": "key", "value": auth.key},
                {"key": "value", "value": auth.value},
                {"key": "in", "value": if auth.placement == ApiKeyPlacement::Query { "query" } else { "header" }},
            ]
        }),
    }
}

fn postman_events(request: &RequestFile, report: &mut ExportReport, name: &str) -> Vec<Value> {
    let mut events = Vec::new();
    for script in &request.runtime.scripts {
        let listen = match script.phase {
            ScriptPhase::BeforeRequest => "prerequest",
            ScriptPhase::AfterResponse => {
                report.warnings.push(format!(
                    "{name}: script after-response exportado como teste Postman (semântica difere)"
                ));
                "test"
            }
            ScriptPhase::Tests => "test",
            ScriptPhase::Hooks => {
                report.losses.push(Loss {
                    item: name.into(),
                    field: "hooks".into(),
                    reason: "hooks não têm equivalente Postman".into(),
                });
                continue;
            }
        };
        events.push(json!({
            "listen": listen,
            "script": {
                "type": "text/javascript",
                "exec": script.code.lines().map(str::to_string).collect::<Vec<_>>()
            }
        }));
    }
    if !request.runtime.assertions.is_empty() {
        report.losses.push(Loss {
            item: name.into(),
            field: "assertions".into(),
            reason: "asserções declarativas não são convertidas para Postman".into(),
        });
    }
    events
}

fn export_opencollection(store: &CollectionStore) -> Result<ExportReport> {
    let mut report = ExportReport::new(ExportFormat::OpenCollection);
    report.requests = store.requests.len();
    report.folders = store.folders.len();
    let mut manifest = store.manifest.clone();
    if let Some(defaults) = &mut manifest.request {
        sanitize_defaults(defaults, "coleção", &mut report);
    }
    let mut root = serde_yaml::to_value(&manifest)?;
    root["bundled"] = serde_yaml::Value::Bool(true);
    root["items"] = serde_yaml::Value::Sequence(oc_items(store, None, &mut report));
    // Remove o que só faz sentido no layout de diretório.
    if let Some(mapping) = root.as_mapping_mut() {
        mapping.remove(&serde_yaml::Value::String("config".into()));
    }
    report.content = Some(format!(
        "# Reqlivo — exportação OpenCollection 1.0.0 (bundled)\n{}",
        serde_yaml::to_string(&root)?
    ));
    Ok(report)
}

fn oc_items(
    store: &CollectionStore,
    parent: Option<&str>,
    report: &mut ExportReport,
) -> Vec<serde_yaml::Value> {
    let nodes = if let Some(parent) = parent {
        find_children(store, parent)
    } else {
        store.tree()
    };
    let mut items = Vec::new();
    for node in nodes {
        match node {
            crate::store::TreeNode::Folder { id, .. } => {
                let Ok(folder) = store.folder(&id) else {
                    continue;
                };
                let mut file = folder.file.clone();
                if let Some(defaults) = &mut file.request {
                    sanitize_defaults(defaults, &format!("pasta '{}'", folder.name), report);
                }
                let mut value = serde_yaml::to_value(&file).unwrap_or_default();
                value["items"] = serde_yaml::Value::Sequence(oc_items(store, Some(&id), report));
                // `info.type` e `items` são canônicos; `request` vazio é omitido.
                items.push(value);
            }
            crate::store::TreeNode::Request { id, .. } => {
                let Ok(entry) = store.request(&id) else {
                    continue;
                };
                let mut file = entry.file.clone();
                sanitize_request_for_export(&mut file, &entry.name, report);
                let value = serde_yaml::to_value(&file).unwrap_or_default();
                items.push(value);
            }
        }
    }
    items
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        "''".to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

/// Gera um projeto mínimo de demonstração em um diretório existente.
pub fn seed_demo(store: &mut CollectionStore) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    let echo = store.add_request(
        None,
        RequestFile {
            info: RequestInfo {
                name: "Echo (200)".into(),
                kind: "http".into(),
                seq: 1,
                tags: vec!["demo".into(), "http".into()],
                description: None,
                extra: Extra::new(),
            },
            http: HttpDetails {
                method: "GET".into(),
                url: "{{baseUrl}}/echo?cliente=reqlivo".into(),
                headers: vec![Header {
                    name: "X-Synthetic".into(),
                    value: "yes".into(),
                    description: None,
                    disabled: false,
                    extra: Extra::new(),
                }],
                params: Vec::new(),
                body: None,
                auth: Some(AuthConfig::inherit()),
            },
            runtime: RequestRuntime {
                assertions: vec![Assertion {
                    expression: "res.status".into(),
                    operator: "eq".into(),
                    value: Some(serde_yaml::Value::String("200".into())),
                    disabled: false,
                    description: Some(serde_yaml::Value::String(
                        "o servidor sintético responde 200".into(),
                    )),
                    extra: Extra::new(),
                }],
                ..RequestRuntime::default()
            },
            settings: HttpSettings::default(),
            docs: Some("Demonstração de GET com asserção declarativa.".into()),
            extensions: Extensions::default(),
            extra: Extra::new(),
        },
    )?;
    ids.push(echo.id);
    let not_found = store.add_request(
        None,
        RequestFile {
            info: RequestInfo {
                name: "Status 404".into(),
                kind: "http".into(),
                seq: 2,
                tags: vec!["demo".into(), "erro".into()],
                description: None,
                extra: Extra::new(),
            },
            http: HttpDetails {
                method: "GET".into(),
                url: "{{baseUrl}}/status/404".into(),
                headers: Vec::new(),
                params: Vec::new(),
                body: None,
                auth: Some(AuthConfig::inherit()),
            },
            runtime: RequestRuntime::default(),
            settings: HttpSettings::default(),
            docs: Some("Sem testes: o runner deve marcar **executed**, não **passed**.".into()),
            extensions: Extensions::default(),
            extra: Extra::new(),
        },
    )?;
    ids.push(not_found.id);
    let _ = new_id;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::init_collection;

    #[test]
    fn curl_parses_method_headers_body_and_basic_auth() {
        let mut report = ImportReport::new(ImportFormat::Curl, "teste");
        let request = parse_curl(
            "curl -X POST http://127.0.0.1:18080/echo -H 'Content-Type: application/json' -u user:pass -d '{\"a\":1}'",
            &mut report,
        )
        .unwrap();
        assert_eq!(request.http.method, "POST");
        assert!(matches!(request.http.body, Some(Body::Json { .. })));
        assert_eq!(request.http.auth.as_ref().unwrap().username, "user");
        assert_eq!(request.http.headers.len(), 1);
    }

    #[test]
    fn postman_roundtrip_imports_signature_request() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Teste").unwrap();
        let postman = r##"{
          "info": {"name": "Demo", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
          "item": [
            {"name": "Folder", "item": [
              {"name": "Echo", "request": {"method": "GET", "header": [{"key": "X-A", "value": "1"}], "url": {"raw": "http://127.0.0.1:18080/echo"}}}
            ]}
          ]
        }"##;
        let report = import_content(
            &mut store,
            None,
            ImportFormat::Postman,
            postman,
            Some("demo.json"),
        )
        .unwrap();
        assert_eq!(report.requests_imported, 1);
        assert_eq!(report.folders_imported, 1);
        let request = store.requests.values().next().unwrap();
        assert_eq!(request.file.http.headers[0].name, "X-A");
    }

    #[test]
    fn opencollection_export_is_valid_yaml_with_items() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Teste").unwrap();
        store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        let report = export(&store, ExportFormat::OpenCollection, None).unwrap();
        let content = report.content.unwrap();
        let value: serde_yaml::Value = serde_yaml::from_str(&content).unwrap();
        assert_eq!(value["bundled"], serde_yaml::Value::Bool(true));
        assert!(value["items"].as_sequence().unwrap().len() >= 1);
    }

    #[test]
    fn curl_export_omits_literal_sensitive_header() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Teste").unwrap();
        let payload = store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        let mut request = payload.request.clone();
        request.http.headers.push(Header {
            name: "Authorization".into(),
            value: "Bearer literal-secret".into(),
            description: None,
            disabled: false,
            extra: Extra::new(),
        });
        request.http.method = "POST".into();
        request.http.body = Some(Body::Json {
            data: "{\"a\":1}".into(),
        });
        store.save_request(&payload.id, request, None).unwrap();
        let report = export(&store, ExportFormat::Curl, Some(&payload.id)).unwrap();
        let content = report.content.unwrap();
        assert!(!content.contains("literal-secret"));
        assert!(content.contains("Content-Type: application/json"));
    }
}
