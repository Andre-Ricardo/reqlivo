use crate::error::Result;
use crate::model::Assertion;
use crate::transport::HttpResponse;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub message: String,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub source: String,
}

impl TestResult {
    pub fn new(
        name: impl Into<String>,
        passed: bool,
        message: impl Into<String>,
        expected: Option<String>,
        actual: Option<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            passed,
            message: message.into(),
            expected,
            actual,
            source: source.into(),
        }
    }
}

/// Constrói o objeto `res` disponível para asserções e scripts.
pub fn response_value(response: &HttpResponse) -> Value {
    let mut headers = serde_json::Map::new();
    for (name, value) in &response.headers {
        headers.insert(name.to_ascii_lowercase(), Value::String(value.clone()));
    }
    let body = match &response.body_text {
        Some(text) => serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.clone())),
        None => serde_json::json!({
            "base64": response.body_base64.clone().unwrap_or_default(),
            "binary": true
        }),
    };
    serde_json::json!({
        "status": response.status,
        "statusText": response.status_text,
        "headers": headers,
        "body": body,
        "timeMs": response.time_ms,
        "sizeBytes": response.size_bytes,
        "url": response.final_url,
        "truncated": response.truncated,
        "binary": response.is_binary,
    })
}

pub fn evaluate_assertions(
    assertions: &[Assertion],
    response: &HttpResponse,
) -> Result<Vec<TestResult>> {
    let root = response_value(response);
    let mut results = Vec::new();
    for (index, assertion) in assertions.iter().enumerate() {
        if assertion.disabled {
            continue;
        }
        let expression = assertion.expression.trim();
        let actual = evaluate_path(&root, expression);
        let expected_text = assertion.expected_text();
        let name = if let Some(description) = &assertion.description {
            plain_text(description)
        } else {
            format!(
                "{} {} {}",
                expression,
                assertion.operator,
                expected_text.clone().unwrap_or_default()
            )
            .trim()
            .to_string()
        };
        let result = evaluate_operator(
            &assertion.operator,
            actual.as_ref(),
            expected_text.as_deref(),
        );
        let (passed, message) = match result {
            Ok(()) => (true, "asserção atendida".to_string()),
            Err(message) => (false, message),
        };
        results.push(TestResult {
            name: if name.is_empty() {
                format!("asserção #{}", index + 1)
            } else {
                name
            },
            passed,
            message,
            expected: expected_text,
            actual: actual.as_ref().map(stringify),
            source: "assertion".to_string(),
        });
    }
    Ok(results)
}

pub fn evaluate_path(root: &Value, expression: &str) -> Option<Value> {
    let expression = expression.trim();
    let expression = expression
        .strip_prefix("response.")
        .or_else(|| expression.strip_prefix("res."))
        .unwrap_or(expression);
    if expression.is_empty() || expression == "res" || expression == "response" {
        return Some(root.clone());
    }
    let segments = parse_path(expression)?;
    let mut current = root;
    for segment in segments {
        current = match (current, segment) {
            (Value::Object(map), PathSegment::Key(key)) => map.get(&key)?,
            (Value::Array(items), PathSegment::Key(key)) => {
                items.get(key.parse::<usize>().ok()?)?
            }
            (Value::Array(items), PathSegment::Index(index)) => items.get(index)?,
            _ => return None,
        };
    }
    Some(current.clone())
}

#[derive(Debug)]
enum PathSegment {
    Key(String),
    Index(usize),
}

fn parse_path(input: &str) -> Option<Vec<PathSegment>> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '.' => {
                if !current.is_empty() {
                    segments.push(PathSegment::Key(std::mem::take(&mut current)));
                }
            }
            '[' => {
                let mut bracket = String::new();
                for next in chars.by_ref() {
                    if next == ']' {
                        break;
                    }
                    bracket.push(next);
                }
                if !current.is_empty() {
                    segments.push(PathSegment::Key(std::mem::take(&mut current)));
                }
                let bracket = bracket.trim().trim_matches(['"', '\'']).to_string();
                if let Ok(index) = bracket.parse::<usize>() {
                    segments.push(PathSegment::Index(index));
                } else {
                    segments.push(PathSegment::Key(bracket));
                }
            }
            other => current.push(other),
        }
    }
    if !current.is_empty() {
        segments.push(PathSegment::Key(current));
    }
    if segments.is_empty() {
        None
    } else {
        Some(segments)
    }
}

fn evaluate_operator(
    operator: &str,
    actual: Option<&Value>,
    expected: Option<&str>,
) -> std::result::Result<(), String> {
    let op = operator.trim().to_ascii_lowercase().replace(['_', '-'], "");
    let expected_value = expected.map(parse_expected);
    let actual_text = actual.map(stringify).unwrap_or_else(|| "<ausente>".into());
    let expected_display = expected.unwrap_or("<não informado>").to_string();

    let fail = |message: String| Err(message);

    match op.as_str() {
        "exists" => {
            if actual.is_some_and(|value| !value.is_null()) {
                Ok(())
            } else {
                fail("valor esperado existente, mas o caminho não foi encontrado".into())
            }
        }
        "notexists" => {
            if actual.is_none() || actual.is_some_and(Value::is_null) {
                Ok(())
            } else {
                fail(format!(
                    "valor esperado ausente, mas foi encontrado {actual_text}"
                ))
            }
        }
        "isstring" | "isnumber" | "isboolean" | "isarray" | "isobject" | "isnull" => {
            let ok = match (op.as_str(), actual) {
                ("isstring", Some(Value::String(_))) => true,
                ("isnumber", Some(Value::Number(_))) => true,
                ("isboolean", Some(Value::Bool(_))) => true,
                ("isarray", Some(Value::Array(_))) => true,
                ("isobject", Some(Value::Object(_))) => true,
                ("isnull", Some(Value::Null)) | ("isnull", None) => true,
                _ => false,
            };
            if ok {
                Ok(())
            } else {
                fail(format!("esperado {op}, obtido {actual_text}"))
            }
        }
        "eq" | "equals" | "equal" | "==" => {
            let Some(actual) = actual else {
                return fail(format!(
                    "esperado {expected_display}, caminho não encontrado"
                ));
            };
            if values_equal(actual, expected_value.as_ref()) {
                Ok(())
            } else {
                fail(format!("esperado {expected_display}, obtido {actual_text}"))
            }
        }
        "neq" | "notequals" | "!=" => {
            let equal = actual.is_some_and(|actual| values_equal(actual, expected_value.as_ref()));
            if equal {
                fail(format!("esperado valor diferente de {expected_display}"))
            } else {
                Ok(())
            }
        }
        "gt" | "gte" | "lt" | "lte" | ">" | ">=" | "<" | "<=" => {
            let Some(actual) = actual else {
                return fail("caminho não encontrado para comparação".into());
            };
            let (Some(left), Some(right)) = (
                as_number(actual),
                expected_value.as_ref().and_then(as_number),
            ) else {
                return fail(format!(
                    "comparação numérica requer números; obtido {actual_text}, esperado {expected_display}"
                ));
            };
            let ok = match op.as_str() {
                "gt" | ">" => left > right,
                "gte" | ">=" => left >= right,
                "lt" | "<" => left < right,
                "lte" | "<=" => left <= right,
                _ => false,
            };
            if ok {
                Ok(())
            } else {
                fail(format!(
                    "esperado {operator} {expected_display}, obtido {actual_text}"
                ))
            }
        }
        "contains" | "notcontains" => {
            let contains = match actual {
                Some(Value::String(text)) => text.contains(expected.unwrap_or_default()),
                Some(Value::Array(items)) => items
                    .iter()
                    .any(|item| values_equal(item, expected_value.as_ref())),
                Some(Value::Object(map)) => map.contains_key(expected.unwrap_or_default()),
                _ => false,
            };
            if (op == "contains" && contains) || (op == "notcontains" && !contains) {
                Ok(())
            } else if op == "contains" {
                fail(format!(
                    "esperado conter {expected_display}, obtido {actual_text}"
                ))
            } else {
                fail(format!(
                    "esperado não conter {expected_display}, obtido {actual_text}"
                ))
            }
        }
        "startswith" | "endswith" => {
            let text = match actual {
                Some(Value::String(text)) => text.clone(),
                _ => stringify_opt(actual),
            };
            let expected = expected.unwrap_or_default();
            let ok = if op == "startswith" {
                text.starts_with(expected)
            } else {
                text.ends_with(expected)
            };
            if ok {
                Ok(())
            } else {
                fail(format!("esperado {op} '{expected}', obtido '{text}'"))
            }
        }
        "matches" => {
            let text = stringify_opt(actual);
            let pattern = expected.unwrap_or_default();
            let regex = regex::Regex::new(pattern)
                .map_err(|error| format!("expressão regular inválida '{pattern}': {error}"))?;
            if regex.is_match(&text) {
                Ok(())
            } else {
                fail(format!("'{text}' não corresponde a /{pattern}/"))
            }
        }
        "in" | "notin" => {
            let Some(actual) = actual else {
                return fail("caminho não encontrado".into());
            };
            let list = match expected_value.as_ref() {
                Some(Value::Array(items)) => items.clone(),
                Some(other) => vec![other.clone()],
                None => Vec::new(),
            };
            let found = list.iter().any(|item| values_equal(actual, Some(item)));
            if (op == "in" && found) || (op == "notin" && !found) {
                Ok(())
            } else {
                fail(format!(
                    "esperado {} {expected_display}, obtido {actual_text}",
                    if op == "in" { "em" } else { "fora de" }
                ))
            }
        }
        "lengtheq" | "length" => {
            let length = match actual {
                Some(Value::String(text)) => text.chars().count(),
                Some(Value::Array(items)) => items.len(),
                Some(Value::Object(map)) => map.len(),
                _ => {
                    return fail(format!("valor sem comprimento: {actual_text}"));
                }
            };
            let expected_length = expected_value.as_ref().and_then(as_number).unwrap_or(-1.0);
            if (length as f64 - expected_length).abs() < f64::EPSILON {
                Ok(())
            } else {
                fail(format!(
                    "esperado comprimento {expected_display}, obtido {length}"
                ))
            }
        }
        other => fail(format!(
            "operador de asserção desconhecido: '{other}' (suportados: eq, neq, gt, gte, lt, lte, contains, startsWith, endsWith, matches, in, isString, isNumber, isBoolean, isArray, isObject, isNull, exists, notExists, lengthEq)"
        )),
    }
}

fn parse_expected(text: &str) -> Value {
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return value;
    }
    Value::String(text.to_string())
}

fn values_equal(actual: &Value, expected: Option<&Value>) -> bool {
    let Some(expected) = expected else {
        return actual.is_null();
    };
    if actual == expected {
        return true;
    }
    match (actual, expected) {
        (Value::Number(left), Value::String(right)) => left.to_string() == *right,
        (Value::String(left), Value::Number(right)) => *left == right.to_string(),
        (Value::Bool(left), Value::String(right)) => left.to_string() == *right,
        (Value::String(left), Value::Bool(right)) => *left == right.to_string(),
        _ => false,
    }
}

fn as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.parse::<f64>().ok(),
        _ => None,
    }
}

fn stringify(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "null".to_string(),
        other => serde_json::to_string(other).unwrap_or_else(|_| other.to_string()),
    }
}

fn stringify_opt(value: Option<&Value>) -> String {
    value.map(stringify).unwrap_or_else(|| "<ausente>".into())
}

fn plain_text(value: &serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::String(text) => text.clone(),
        other => serde_yaml::to_string(other)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::HttpResponse;

    fn response(status: u16, body: &str) -> HttpResponse {
        HttpResponse {
            status,
            status_text: "X".into(),
            headers: vec![("content-type".into(), "application/json".into())],
            body_text: Some(body.into()),
            body_base64: None,
            is_binary: false,
            truncated: false,
            declared_size: None,
            size_bytes: body.len(),
            time_ms: 1,
            final_url: "http://127.0.0.1/x".into(),
            requested_url: "http://127.0.0.1/x".into(),
            content_type: Some("application/json".into()),
            redirected: false,
            body_bytes: body.as_bytes().to_vec(),
        }
    }

    fn assertion(expression: &str, operator: &str, value: Option<&str>) -> Assertion {
        Assertion {
            expression: expression.into(),
            operator: operator.into(),
            value: value.map(|value| serde_yaml::Value::String(value.to_string())),
            disabled: false,
            description: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn status_assertion_passes_and_fails_with_expected_and_actual() {
        let response = response(404, r#"{"error":true}"#);
        let results =
            evaluate_assertions(&[assertion("res.status", "eq", Some("200"))], &response).unwrap();
        assert!(!results[0].passed);
        assert_eq!(results[0].expected.as_deref(), Some("200"));
        assert_eq!(results[0].actual.as_deref(), Some("404"));
    }

    #[test]
    fn nested_body_and_header_paths() {
        let response = response(200, r#"{"user":{"roles":["admin"]}}"#);
        let results = evaluate_assertions(
            &[
                assertion("res.body.user.roles[0]", "eq", Some("admin")),
                assertion("res.headers[\"content-type\"]", "contains", Some("json")),
            ],
            &response,
        )
        .unwrap();
        assert!(results.iter().all(|result| result.passed));
    }

    #[test]
    fn type_operators() {
        let response = response(200, r#"{"ok":true,"n":3,"list":[]}"#);
        let results = evaluate_assertions(
            &[
                assertion("res.body.ok", "isBoolean", None),
                assertion("res.body.n", "isNumber", None),
                assertion("res.body.list", "isArray", None),
                assertion("res.body.missing", "notExists", None),
            ],
            &response,
        )
        .unwrap();
        assert!(results.iter().all(|result| result.passed));
    }
}
