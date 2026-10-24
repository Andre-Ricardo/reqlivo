use crate::error::{Error, Result};
use crate::model::{ApiKeyPlacement, AuthConfig, AuthMode};
use crate::variables::Resolver;
use base64::Engine;
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct AuthSource {
    pub auth: AuthConfig,
    pub origin: String,
}

/// Resolve a autenticação efetiva, aplicando a regra de herança:
/// requisição (se não for `inherit`) → pasta folha→raiz → coleção → nenhuma.
pub fn effective_auth(
    request: Option<&AuthConfig>,
    folders_leaf_to_root: &[(String, Option<AuthConfig>)],
    collection: Option<&AuthConfig>,
) -> Option<AuthSource> {
    let request = request?;
    if request.mode != AuthMode::Inherit {
        return Some(AuthSource {
            auth: request.clone(),
            origin: "requisição".to_string(),
        });
    }
    for (name, auth) in folders_leaf_to_root {
        if let Some(auth) = auth {
            if auth.mode != AuthMode::Inherit {
                return Some(AuthSource {
                    auth: auth.clone(),
                    origin: format!("pasta '{name}'"),
                });
            }
        }
    }
    if let Some(auth) = collection {
        if auth.mode != AuthMode::Inherit {
            return Some(AuthSource {
                auth: auth.clone(),
                origin: "coleção".to_string(),
            });
        }
    }
    None
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthExplanation {
    pub mode: String,
    pub origin: String,
    pub description: String,
    pub secret_used: bool,
}

#[derive(Debug, Clone)]
pub struct AppliedAuth {
    pub explanation: AuthExplanation,
    pub headers: Vec<(String, String)>,
    pub query: Vec<(String, String)>,
    pub secret_values: Vec<String>,
}

pub fn apply_auth(source: Option<&AuthSource>, resolver: &Resolver) -> Result<Option<AppliedAuth>> {
    let Some(source) = source else {
        return Ok(None);
    };
    let auth = &source.auth;
    let secret_values = resolver.secret_values();
    let mut applied = AppliedAuth {
        explanation: AuthExplanation {
            mode: mode_name(auth.mode).to_string(),
            origin: source.origin.clone(),
            description: String::new(),
            secret_used: false,
        },
        headers: Vec::new(),
        query: Vec::new(),
        secret_values,
    };

    match auth.mode {
        AuthMode::Inherit => {
            applied.explanation.mode = "inherit".into();
            applied.explanation.description = "herança não resolvida".into();
        }
        AuthMode::Basic => {
            let username = resolver.resolve(&auth.username)?;
            let password = resolver.resolve(&auth.password)?;
            let encoded =
                base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
            applied
                .headers
                .push(("Authorization".into(), format!("Basic {encoded}")));
            applied.explanation.description =
                format!("Authorization: Basic (usuário '{username}')");
        }
        AuthMode::Bearer => {
            let token = resolver.resolve(&auth.token)?;
            if token.trim().is_empty() {
                return Err(Error::Auth(
                    "token Bearer vazio; informe um valor ou uma variável".into(),
                ));
            }
            applied
                .headers
                .push(("Authorization".into(), format!("Bearer {token}")));
            applied.explanation.description = "Authorization: Bearer".into();
        }
        AuthMode::ApiKey => {
            let key = resolver.resolve(&auth.key)?;
            let value = resolver.resolve(&auth.value)?;
            if key.trim().is_empty() {
                return Err(Error::Auth("nome da API key vazio".into()));
            }
            match auth.placement {
                ApiKeyPlacement::Header => {
                    applied.headers.push((key.clone(), value));
                    applied.explanation.description = format!("header {key}");
                }
                ApiKeyPlacement::Query => {
                    applied.query.push((key.clone(), value));
                    applied.explanation.description = format!("query {key}");
                }
            }
        }
    }

    applied.explanation.secret_used = match auth.mode {
        AuthMode::Basic => {
            uses_secret(&auth.username, resolver) || uses_secret(&auth.password, resolver)
        }
        AuthMode::Bearer => uses_secret(&auth.token, resolver),
        AuthMode::ApiKey => uses_secret(&auth.key, resolver) || uses_secret(&auth.value, resolver),
        AuthMode::Inherit => false,
    };
    Ok(Some(applied))
}

fn uses_secret(template: &str, resolver: &Resolver) -> bool {
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            break;
        };
        let name = after[..end].trim();
        if resolver.is_secret(name) {
            return true;
        }
        rest = &after[end + 2..];
    }
    false
}

fn mode_name(mode: AuthMode) -> &'static str {
    match mode {
        AuthMode::Inherit => "inherit",
        AuthMode::Basic => "basic",
        AuthMode::Bearer => "bearer",
        AuthMode::ApiKey => "apikey",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::variables::ScopeInput;

    #[test]
    fn inherit_uses_nearest_folder_then_collection() {
        let request = AuthConfig::inherit();
        let folders = vec![
            ("raiz".to_string(), Some(AuthConfig::bearer("root"))),
            ("filha".to_string(), None),
        ];
        let collection = Some(AuthConfig::bearer("collection"));
        let source = effective_auth(Some(&request), &folders, collection.as_ref()).unwrap();
        assert_eq!(source.origin, "pasta 'raiz'");
        assert_eq!(source.auth.token, "root");
    }

    #[test]
    fn bearer_is_applied_once() {
        let source = AuthSource {
            auth: AuthConfig::bearer("{{token}}"),
            origin: "coleção".into(),
        };
        let resolver = Resolver::build(ScopeInput {
            collection: vec![crate::model::Variable::plain("token", "abc")],
            ..ScopeInput::default()
        });
        let applied = apply_auth(Some(&source), &resolver).unwrap().unwrap();
        assert_eq!(applied.headers.len(), 1);
        assert_eq!(applied.headers[0].1, "Bearer abc");
        assert_eq!(applied.explanation.origin, "coleção");
    }

    #[test]
    fn api_key_goes_to_query_when_configured() {
        let source = AuthSource {
            auth: AuthConfig::api_key("key", "value", ApiKeyPlacement::Query),
            origin: "requisição".into(),
        };
        let resolver = Resolver::build(ScopeInput::default());
        let applied = apply_auth(Some(&source), &resolver).unwrap().unwrap();
        assert!(applied.headers.is_empty());
        assert_eq!(applied.query[0], ("key".into(), "value".into()));
    }
}
