use crate::auth::{AuthExplanation, apply_auth, effective_auth};
use crate::error::{Error, Result};
use crate::model::{ApiKeyPlacement, AuthConfig, Body, Header, RequestFile, ScriptPhase};
use crate::redact::{redact_headers, redact_url};
use crate::store::CollectionStore;
use crate::variables::{EffectiveVariable, Resolver};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub enum PreparedBody {
    None,
    Bytes {
        bytes: Vec<u8>,
        content_type: &'static str,
    },
    Multipart(Vec<MultipartField>),
}

#[derive(Debug, Clone)]
pub struct MultipartField {
    pub name: String,
    pub value: Vec<u8>,
    pub file_name: Option<String>,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreparedRequest {
    pub method: String,
    pub url: String,
    pub display_url: String,
    pub headers: Vec<(String, String)>,
    pub display_headers: Vec<(String, String)>,
    pub auth: Option<AuthExplanation>,
    pub body_kind: String,
    pub body_preview: Option<String>,
    pub body_size: usize,
    pub effective_variables: Vec<EffectiveVariable>,
    #[serde(skip)]
    pub body: PreparedBody,
    #[serde(skip)]
    pub secret_values: Vec<String>,
}

impl PreparedRequest {
    pub fn new(
        store: &CollectionStore,
        request_id: &str,
        request: &RequestFile,
        environment: Option<&str>,
        runtime: &BTreeMap<String, String>,
    ) -> Result<Self> {
        let scope = store.scope_input_with(request_id, request, environment, runtime)?;
        let resolver = Resolver::build(scope);
        let settings = store.effective_settings_with(request)?;
        let mut secret_values = resolver.secret_values();

        let mut url = resolver.resolve(&request.http.url)?;
        if url.trim().is_empty() {
            return Err(Error::Validation("a URL da requisição está vazia".into()));
        }
        if !url.contains("://") {
            return Err(Error::Validation(format!(
                "URL inválida: '{url}'. Inclua o esquema, por exemplo http://127.0.0.1:8080."
            )));
        }

        // Path params: substitui :nome antes da query.
        for param in request
            .http
            .params
            .iter()
            .filter(|param| !param.disabled && param.kind == crate::model::ParamKind::Path)
        {
            let name = resolver.resolve(&param.name)?;
            let value = resolver.resolve(&param.value)?;
            let encoded = encode_component(&value, settings.encode_url);
            url = url.replace(&format!(":{name}"), &encoded);
        }

        if url.contains("{{") {
            return Err(Error::Variable(format!(
                "URL ainda contém variáveis não resolvidas: {url}"
            )));
        }

        // Remove fragmento (não é enviado ao servidor) e valida o esquema.
        if let Some(index) = url.find('#') {
            url.truncate(index);
        }
        let parsed = url::Url::parse(&url)
            .map_err(|error| Error::Validation(format!("URL inválida '{url}': {error}")))?;
        if parsed.host_str().is_none() {
            return Err(Error::Validation(format!("URL sem host: '{url}'")));
        }

        // Query params habilitados, preservando a query existente e duplicatas.
        let mut query = String::new();
        for param in request
            .http
            .params
            .iter()
            .filter(|param| !param.disabled && param.kind == crate::model::ParamKind::Query)
        {
            let name = resolver.resolve(&param.name)?;
            let value = resolver.resolve(&param.value)?;
            if name.is_empty() {
                continue;
            }
            if !query.is_empty() {
                query.push('&');
            }
            query.push_str(&encode_component(&name, settings.encode_url));
            query.push('=');
            query.push_str(&encode_component(&value, settings.encode_url));
        }

        // Cabeçalhos herdados da coleção/pastas, sobrescritos pela requisição.
        let mut headers: Vec<(String, String)> = Vec::new();
        for header in store.effective_headers_with(request_id, request)? {
            if header.disabled || header.name.trim().is_empty() {
                continue;
            }
            let name = header.name.trim().to_string();
            let value = resolver.resolve(&header.value)?;
            set_header(&mut headers, &name, &value);
        }

        // Autenticação efetiva.
        let (request_auth, folders, collection_auth) =
            store.auth_inputs_with(request_id, request)?;
        let source = effective_auth(request_auth.as_ref(), &folders, collection_auth.as_ref());
        let applied = apply_auth(source.as_ref(), &resolver)?;
        if let Some(applied) = &applied {
            for (name, value) in &applied.headers {
                set_header(&mut headers, name, value);
            }
            if !applied.query.is_empty() {
                for (name, value) in &applied.query {
                    if !query.is_empty() {
                        query.push('&');
                    }
                    query.push_str(&encode_component(name, settings.encode_url));
                    query.push('=');
                    query.push_str(&encode_component(value, settings.encode_url));
                }
            }
            secret_values.extend(applied.secret_values.clone());
        }

        let body = prepare_body(&resolver, request.http.body.as_ref(), settings.encode_url)?;
        let (default_content_type, body_kind, body_preview, body_size) = match &body {
            PreparedBody::None => (None, "none".to_string(), None, 0),
            PreparedBody::Bytes {
                bytes,
                content_type,
            } => (
                Some(*content_type),
                request
                    .http
                    .body
                    .as_ref()
                    .map(Body::kind)
                    .unwrap_or("none")
                    .to_string(),
                preview_bytes(bytes),
                bytes.len(),
            ),
            PreparedBody::Multipart(fields) => {
                let size = fields
                    .iter()
                    .map(|field| field.value.len() + field.name.len())
                    .sum();
                let preview = fields
                    .iter()
                    .map(|field| {
                        if field.file_name.is_some() {
                            format!("{}: <arquivo {}>", field.name, field.value.len())
                        } else {
                            format!("{}: {}", field.name, String::from_utf8_lossy(&field.value))
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                (
                    Some("multipart/form-data"),
                    "multipart-form".to_string(),
                    Some(truncate_preview(&preview)),
                    size,
                )
            }
        };

        if let Some(content_type) = default_content_type {
            if !has_header(&headers, "content-type") {
                set_header(&mut headers, "Content-Type", content_type);
            }
        }
        if !has_header(&headers, "user-agent") {
            set_header(&mut headers, "User-Agent", crate::USER_AGENT);
        }
        if !has_header(&headers, "accept") {
            set_header(&mut headers, "Accept", "*/*");
        }

        let display_url = redact_url(&format_with_query(&url, &query), &secret_values);
        let display_headers = redact_headers(&headers, &secret_values);

        let mut full_url = parsed;
        if !query.is_empty() {
            let base = format_with_query(&url, &query);
            full_url = url::Url::parse(&base).map_err(|error| {
                Error::Validation(format!("URL final inválida '{base}': {error}"))
            })?;
        }
        full_url.set_fragment(None);

        Ok(Self {
            method: request.http.method.to_uppercase(),
            url: full_url.to_string(),
            display_url,
            headers,
            display_headers,
            auth: applied.map(|applied| applied.explanation),
            body_kind,
            body_preview,
            body_size,
            effective_variables: resolver.effective(),
            body,
            secret_values,
        })
    }

    pub fn has_body(&self) -> bool {
        !matches!(self.body, PreparedBody::None)
    }
}

fn prepare_body(
    resolver: &Resolver,
    body: Option<&Body>,
    encode_url: bool,
) -> Result<PreparedBody> {
    let Some(body) = body else {
        return Ok(PreparedBody::None);
    };
    match body {
        Body::Json { data } => {
            let data = resolver.resolve(data)?;
            if !data.trim().is_empty() {
                serde_json::from_str::<serde_json::Value>(&data).map_err(|error| {
                    Error::Body(format!("corpo JSON inválido: {error}"))
                })?;
            }
            Ok(PreparedBody::Bytes {
                bytes: data.into_bytes(),
                content_type: "application/json",
            })
        }
        Body::Text { data } => Ok(PreparedBody::Bytes {
            bytes: resolver.resolve(data)?.into_bytes(),
            content_type: "text/plain; charset=utf-8",
        }),
        Body::Xml { data } => {
            let data = resolver.resolve(data)?;
            if !data.trim().is_empty() {
                roxmltree::Document::parse(&data)
                    .map_err(|error| Error::Body(format!("corpo XML inválido: {error}")))?;
            }
            Ok(PreparedBody::Bytes {
                bytes: data.into_bytes(),
                content_type: "application/xml",
            })
        }
        Body::Sparql { data } => Ok(PreparedBody::Bytes {
            bytes: resolver.resolve(data)?.into_bytes(),
            content_type: "application/sparql-query",
        }),
        Body::FormUrlEncoded { data } => {
            let mut encoded = String::new();
            for field in data.iter().filter(|field| !field.disabled) {
                let name = resolver.resolve(&field.name)?;
                let value = resolver.resolve(&field.value)?;
                if !encoded.is_empty() {
                    encoded.push('&');
                }
                encoded.push_str(&encode_component(&name, encode_url));
                encoded.push('=');
                encoded.push_str(&encode_component(&value, encode_url));
            }
            Ok(PreparedBody::Bytes {
                bytes: encoded.into_bytes(),
                content_type: "application/x-www-form-urlencoded",
            })
        }
        Body::Multipart { data } => {
            let mut fields = Vec::new();
            for field in data.iter().filter(|field| !field.disabled) {
                let name = resolver.resolve(&field.name)?;
                let value = resolver.resolve(&field.value)?;
                let is_file = field
                    .kind
                    .as_deref()
                    .map(|kind| kind.eq_ignore_ascii_case("file"))
                    .unwrap_or(false);
                if is_file {
                    let path = std::path::Path::new(&value);
                    let bytes = std::fs::read(path).map_err(|error| {
                        Error::Body(format!(
                            "não foi possível ler o arquivo multipart '{}': {error}",
                            path.display()
                        ))
                    })?;
                    let file_name = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("file")
                        .to_string();
                    let content_type = mime_guess::from_path(path)
                        .first()
                        .map(|mime| mime.to_string());
                    fields.push(MultipartField {
                        name,
                        value: bytes,
                        file_name: Some(file_name),
                        content_type,
                    });
                } else {
                    fields.push(MultipartField {
                        name,
                        value: value.into_bytes(),
                        file_name: None,
                        content_type: None,
                    });
                }
            }
            Ok(PreparedBody::Multipart(fields))
        }
        Body::Unsupported => Err(Error::Body(
            "tipo de corpo não suportado no MVP; use json, text, xml, sparql, form-urlencoded ou multipart-form"
                .into(),
        )),
    }
}

fn format_with_query(url: &str, query: &str) -> String {
    if query.is_empty() {
        url.to_string()
    } else if url.contains('?') {
        format!("{url}&{query}")
    } else {
        format!("{url}?{query}")
    }
}

fn set_header(headers: &mut Vec<(String, String)>, name: &str, value: &str) {
    if let Some(existing) = headers
        .iter_mut()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
    {
        existing.1 = value.to_string();
    } else {
        headers.push((name.to_string(), value.to_string()));
    }
}

fn has_header(headers: &[(String, String)], name: &str) -> bool {
    headers
        .iter()
        .any(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
}

pub fn encode_component(value: &str, encode: bool) -> String {
    if encode {
        url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
    } else {
        value.to_string()
    }
}

fn preview_bytes(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(bytes);
    Some(truncate_preview(&text))
}

fn truncate_preview(text: &str) -> String {
    const LIMIT: usize = 4096;
    if text.len() <= LIMIT {
        text.to_string()
    } else {
        let mut end = LIMIT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}\n… (prévia truncada)", &text[..end])
    }
}

/// Mantém a API de autenticação acessível a partir do transporte sem expor
/// valores secretos em estruturas serializáveis.
pub fn auth_placement_is_query(auth: &AuthConfig) -> bool {
    auth.mode == crate::model::AuthMode::ApiKey && auth.placement == ApiKeyPlacement::Query
}

pub fn has_script_phase(request: &RequestFile, phase: ScriptPhase) -> bool {
    request
        .runtime
        .scripts
        .iter()
        .any(|script| script.phase == phase)
}

pub fn effective_header_list(headers: &[Header]) -> Vec<(String, String)> {
    headers
        .iter()
        .filter(|header| !header.disabled && !header.name.trim().is_empty())
        .map(|header| (header.name.clone(), header.value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::init_collection;

    #[test]
    fn resolves_url_headers_and_query() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Teste").unwrap();
        store
            .upsert_environment(
                "dev",
                vec![
                    crate::model::Variable::plain("baseUrl", "http://127.0.0.1:18080"),
                    crate::model::Variable::plain("token", "abc"),
                ],
                vec![],
                None,
            )
            .unwrap();
        let payload = store
            .create_request(None, "Echo", "GET", "{{baseUrl}}/echo?existing=1")
            .unwrap();
        let mut request = payload.request.clone();
        request.http.headers.push(Header {
            name: "X-Test".into(),
            value: "{{token}}".into(),
            description: None,
            disabled: false,
            extra: Default::default(),
        });
        request.http.params.push(crate::model::Param {
            name: "q".into(),
            value: "a b".into(),
            kind: crate::model::ParamKind::Query,
            description: None,
            disabled: false,
            extra: Default::default(),
        });
        request.http.auth = Some(AuthConfig::bearer("{{token}}"));
        let prepared =
            PreparedRequest::new(&store, &payload.id, &request, Some("dev"), &BTreeMap::new())
                .unwrap();
        assert_eq!(prepared.url, "http://127.0.0.1:18080/echo?existing=1&q=a+b");
        assert_eq!(prepared.method, "GET");
        assert!(
            prepared
                .headers
                .iter()
                .any(|(name, value)| name == "Authorization" && value == "Bearer abc")
        );
        assert!(
            prepared
                .headers
                .iter()
                .any(|(name, value)| name == "X-Test" && value == "abc")
        );
    }

    #[test]
    fn rejects_invalid_json_body() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Teste").unwrap();
        let payload = store
            .create_request(None, "Broken", "POST", "http://127.0.0.1:18080/echo")
            .unwrap();
        let mut request = payload.request.clone();
        request.http.body = Some(Body::Json { data: "{".into() });
        let error =
            PreparedRequest::new(&store, &payload.id, &request, Some("dev"), &BTreeMap::new())
                .unwrap_err()
                .to_string();
        assert!(error.contains("JSON inválido"));
    }
}
