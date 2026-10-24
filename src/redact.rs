use serde_json::Value;

pub const MASK: &str = "••••••";

pub fn is_sensitive_name(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    matches!(
        name.as_str(),
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "set-cookie"
            | "x-api-key"
            | "api-key"
            | "apikey"
            | "x-auth-token"
            | "x-access-token"
            | "private-key"
            | "client-secret"
    ) || name.ends_with("-token")
        || name.ends_with("_token")
        || name.ends_with("-secret")
        || name.ends_with("_secret")
}

pub fn redact_text(text: &str, secrets: &[String]) -> String {
    let mut output = text.to_string();
    for secret in secrets {
        if secret.len() >= 4 {
            output = output.replace(secret, MASK);
        }
    }
    output
}

pub fn redact_headers(headers: &[(String, String)], secrets: &[String]) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(name, value)| {
            if is_sensitive_name(name) {
                (name.clone(), MASK.to_string())
            } else {
                (name.clone(), redact_text(value, secrets))
            }
        })
        .collect()
}

/// Mascara valores sensíveis em parâmetros de query (`token`, `api_key`,
/// `password`, etc.) e credenciais embutidas na URL.
pub fn redact_url(input: &str, secrets: &[String]) -> String {
    let Ok(mut url) = url::Url::parse(input) else {
        return redact_text(input, secrets);
    };
    if url.password().is_some() {
        let user = url.username().to_string();
        let _ = url.set_password(Some(MASK));
        let _ = url.set_username(&user);
    }
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(key, value)| {
            if is_sensitive_query(&key) {
                (key.to_string(), MASK.to_string())
            } else {
                (key.to_string(), redact_text(&value, secrets))
            }
        })
        .collect();
    if !pairs.is_empty() {
        url.query_pairs_mut().clear().extend_pairs(pairs);
    }
    redact_text(url.as_str(), secrets)
}

pub fn is_sensitive_query(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "token"
            | "access_token"
            | "refresh_token"
            | "id_token"
            | "api_key"
            | "apikey"
            | "key"
            | "secret"
            | "client_secret"
            | "password"
            | "senha"
            | "auth"
    )
}

pub fn redact_json(value: &mut Value, secrets: &[String]) {
    match value {
        Value::Object(map) => {
            for (key, item) in map.iter_mut() {
                if is_sensitive_name(key) || is_sensitive_query(key) {
                    *item = Value::String(MASK.to_string());
                } else {
                    redact_json(item, secrets);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_json(item, secrets);
            }
        }
        Value::String(text) => {
            *text = redact_text(text, secrets);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_authorization_and_known_secret() {
        let headers = vec![
            ("Authorization".into(), "Bearer abc123".into()),
            ("X-Custom".into(), "value abc123".into()),
        ];
        let redacted = redact_headers(&headers, &["abc123".to_string()]);
        assert_eq!(redacted[0].1, MASK);
        assert!(!redacted[1].1.contains("abc123"));
    }

    #[test]
    fn redacts_sensitive_query_and_userinfo() {
        let url = redact_url("https://user:pass@example.test/a?token=abc&x=1", &[]);
        assert!(!url.contains("pass"));
        assert!(!url.contains("token=abc"));
        assert!(url.contains("x=1"));
    }
}
