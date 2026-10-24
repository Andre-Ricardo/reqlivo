use crate::body::{PreparedBody, PreparedRequest};
use crate::error::{Error, ErrorCategory, Result};
use crate::model::CollectionSettings;
use crate::store::EffectiveSettings;
use base64::Engine;
use serde::Serialize;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

pub const MAX_BINARY_PREVIEW: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct HttpResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body_text: Option<String>,
    pub body_base64: Option<String>,
    pub is_binary: bool,
    pub truncated: bool,
    pub declared_size: Option<u64>,
    pub size_bytes: usize,
    pub time_ms: u128,
    pub final_url: String,
    pub requested_url: String,
    pub content_type: Option<String>,
    pub redirected: bool,
    #[serde(skip)]
    pub body_bytes: Vec<u8>,
}

pub struct TransportOptions {
    pub settings: EffectiveSettings,
    pub cookie_jar: Arc<reqwest::cookie::Jar>,
    pub cancellation: Option<CancellationToken>,
}

pub async fn send(prepared: PreparedRequest, options: TransportOptions) -> Result<HttpResponse> {
    if options.settings.offline_only {
        let parsed = url::Url::parse(&prepared.url).map_err(|error| {
            Error::Validation(format!("URL inválida '{}': {error}", prepared.url))
        })?;
        if !is_local_destination(&parsed) {
            return Err(Error::Offline(prepared.display_url.clone()));
        }
    }

    let redirect_policy = if !options.settings.follow_redirects {
        reqwest::redirect::Policy::none()
    } else if options.settings.offline_only {
        // Redirecionamentos também respeitam o modo offline: um salto para
        // fora da rede local é interrompido e a resposta 3xx é devolvida.
        let max_redirects = options.settings.max_redirects as usize;
        reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= max_redirects || !is_local_destination(attempt.url()) {
                attempt.stop()
            } else {
                attempt.follow()
            }
        })
    } else {
        reqwest::redirect::Policy::limited(options.settings.max_redirects as usize)
    };
    let mut builder = reqwest::Client::builder()
        .cookie_provider(options.cookie_jar.clone())
        .redirect(redirect_policy)
        .pool_max_idle_per_host(2);
    if options.settings.timeout_ms > 0 {
        builder = builder.timeout(Duration::from_millis(options.settings.timeout_ms));
    }
    let client = builder.build().map_err(|error| {
        Error::Internal(format!("não foi possível criar o cliente HTTP: {error}"))
    })?;

    let method = reqwest::Method::from_bytes(prepared.method.as_bytes()).map_err(|error| {
        Error::Validation(format!(
            "método HTTP inválido '{}': {error}",
            prepared.method
        ))
    })?;
    let mut request = client.request(method, &prepared.url);
    for (name, value) in &prepared.headers {
        let header_name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| Error::Validation(format!("cabeçalho inválido '{name}': {error}")))?;
        let header_value = reqwest::header::HeaderValue::from_str(value).map_err(|error| {
            Error::Validation(format!("valor inválido para o cabeçalho '{name}': {error}"))
        })?;
        request = request.header(header_name, header_value);
    }

    match &prepared.body {
        PreparedBody::None => {}
        PreparedBody::Bytes { bytes, .. } => {
            request = request.body(bytes.clone());
        }
        PreparedBody::Multipart(fields) => {
            let mut form = reqwest::multipart::Form::new();
            for field in fields {
                if let Some(file_name) = &field.file_name {
                    let mut part = reqwest::multipart::Part::bytes(field.value.clone())
                        .file_name(file_name.clone());
                    if let Some(content_type) = &field.content_type {
                        part = part.mime_str(content_type).map_err(|error| {
                            Error::Body(format!("content-type inválido no multipart: {error}"))
                        })?;
                    }
                    form = form.part(field.name.clone(), part);
                } else {
                    form = form.text(
                        field.name.clone(),
                        String::from_utf8_lossy(&field.value).to_string(),
                    );
                }
            }
            request = request.multipart(form);
        }
    }

    let started = Instant::now();
    let response = if let Some(token) = &options.cancellation {
        tokio::select! {
            _ = token.cancelled() => return Err(Error::Cancelled),
            result = request.send() => result,
        }
    } else {
        request.send().await
    }
    .map_err(classify_reqwest_error)?;

    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or("").to_string();
    let raw_final_url = response.url().to_string();
    let redirected = raw_final_url != prepared.url;
    let final_url = crate::redact::redact_url(&raw_final_url, &prepared.secret_values);
    let headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                String::from_utf8_lossy(value.as_bytes()).to_string(),
            )
        })
        .collect();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let declared_size = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());

    let limit = options.settings.max_response_bytes.max(1024);
    let mut body_bytes: Vec<u8> = Vec::new();
    let mut truncated = false;
    let mut response = response;
    loop {
        let chunk = if let Some(token) = &options.cancellation {
            tokio::select! {
                _ = token.cancelled() => return Err(Error::Cancelled),
                chunk = response.chunk() => chunk,
            }
        } else {
            response.chunk().await
        }
        .map_err(classify_reqwest_error)?;
        let Some(chunk) = chunk else {
            break;
        };
        if body_bytes.len() + chunk.len() > limit {
            let remaining = limit.saturating_sub(body_bytes.len());
            body_bytes.extend_from_slice(&chunk[..remaining]);
            truncated = true;
            break;
        }
        body_bytes.extend_from_slice(&chunk);
    }

    let elapsed = started.elapsed();
    let is_text = content_type
        .as_deref()
        .map(is_textual_content_type)
        .unwrap_or_else(|| std::str::from_utf8(&body_bytes).is_ok());
    let (body_text, body_base64) = if is_text {
        (
            Some(
                String::from_utf8_lossy(&body_bytes)
                    .trim_start_matches('\u{feff}')
                    .to_string(),
            ),
            None,
        )
    } else {
        let preview = &body_bytes[..body_bytes.len().min(MAX_BINARY_PREVIEW)];
        (
            None,
            Some(base64::engine::general_purpose::STANDARD.encode(preview)),
        )
    };

    Ok(HttpResponse {
        status: status.as_u16(),
        status_text,
        headers,
        body_text,
        body_base64,
        is_binary: !is_text,
        truncated,
        declared_size,
        size_bytes: body_bytes.len(),
        time_ms: elapsed.as_millis(),
        final_url,
        requested_url: prepared.display_url,
        content_type,
        redirected,
        body_bytes,
    })
}

fn classify_reqwest_error(error: reqwest::Error) -> Error {
    let mut sources = Vec::new();
    let mut source: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(&error);
    while let Some(cause) = source {
        sources.push(cause.to_string().to_lowercase());
        source = cause.source();
    }
    let joined = format!("{error} {}", sources.join(" ")).to_lowercase();

    if error.is_timeout() {
        return Error::Network {
            category: ErrorCategory::Timeout,
            message: "tempo limite esgotado ao conectar ou aguardar a resposta".into(),
        };
    }
    if joined.contains("dns")
        || joined.contains("failed to lookup")
        || joined.contains("name or service not known")
        || joined.contains("nodename nor servname")
    {
        return Error::Network {
            category: ErrorCategory::Dns,
            message: format!("não foi possível resolver o host: {error}"),
        };
    }
    if joined.contains("certificate")
        || joined.contains("tls")
        || joined.contains("ssl")
        || joined.contains("handshake")
    {
        return Error::Network {
            category: ErrorCategory::Tls,
            message: format!("falha de TLS: {error}"),
        };
    }
    if joined.contains("proxy") {
        return Error::Network {
            category: ErrorCategory::Proxy,
            message: format!("falha de proxy: {error}"),
        };
    }
    if error.is_redirect() {
        return Error::Network {
            category: ErrorCategory::Redirect,
            message: format!("redirecionamento excessivo ou inválido: {error}"),
        };
    }
    if error.is_body() || error.is_decode() {
        return Error::Network {
            category: ErrorCategory::Body,
            message: format!("falha ao ler o corpo da resposta: {error}"),
        };
    }
    if error.is_builder() || error.is_request() {
        return Error::Network {
            category: ErrorCategory::Connect,
            message: format!("falha ao enviar a requisição: {error}"),
        };
    }
    Error::Network {
        category: ErrorCategory::Connect,
        message: format!("falha de conexão: {error}"),
    }
}

fn is_textual_content_type(content_type: &str) -> bool {
    let content_type = content_type.to_ascii_lowercase();
    content_type.starts_with("text/")
        || content_type.contains("json")
        || content_type.contains("xml")
        || content_type.contains("javascript")
        || content_type.contains("x-www-form-urlencoded")
        || content_type.contains("graphql")
        || content_type.contains("yaml")
        || content_type.contains("csv")
        || content_type.contains("event-stream")
}

pub fn is_local_destination(url: &url::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(ip) => {
                ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
            }
            IpAddr::V6(ip) => {
                ip.is_loopback()
                    || ip.is_unique_local()
                    || ip.is_unicast_link_local()
                    || ip.is_unspecified()
            }
        };
    }
    false
}

pub fn settings_from_collection(settings: &CollectionSettings) -> EffectiveSettings {
    EffectiveSettings {
        timeout_ms: settings.default_timeout_ms,
        follow_redirects: settings.follow_redirects,
        max_redirects: settings.max_redirects,
        encode_url: true,
        offline_only: settings.offline_only,
        max_response_bytes: settings.max_response_bytes,
        script_timeout_ms: settings.script_timeout_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_mode_only_allows_local_hosts() {
        assert!(is_local_destination(
            &url::Url::parse("http://127.0.0.1:18080/echo").unwrap()
        ));
        assert!(is_local_destination(
            &url::Url::parse("http://localhost:18080/echo").unwrap()
        ));
        assert!(is_local_destination(
            &url::Url::parse("http://192.168.0.10/echo").unwrap()
        ));
        assert!(!is_local_destination(
            &url::Url::parse("https://example.com/echo").unwrap()
        ));
    }

    #[test]
    fn textual_content_types() {
        assert!(is_textual_content_type("application/json"));
        assert!(is_textual_content_type("text/plain; charset=utf-8"));
        assert!(!is_textual_content_type("image/png"));
    }
}
