use crate::error::{Error, Result};
use crate::model::{SecretProvider, SecretRef, VarValue, Variable};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

pub const MAX_DEPTH: usize = 10;
const SECRET_MASK: &str = "••••••";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VarScope {
    Collection,
    Environment,
    Folder,
    Request,
    Runtime,
    Process,
}

impl VarScope {
    pub fn label(&self) -> &'static str {
        match self {
            VarScope::Collection => "coleção",
            VarScope::Environment => "ambiente",
            VarScope::Folder => "pasta",
            VarScope::Request => "requisição",
            VarScope::Runtime => "runtime",
            VarScope::Process => "processo",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EffectiveVariable {
    pub name: String,
    pub value: Option<String>,
    pub display_value: Option<String>,
    pub scope: VarScope,
    pub origin: String,
    pub secret: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedVar {
    pub value: Option<String>,
    pub scope: VarScope,
    pub origin: String,
    pub secret: bool,
    pub error: Option<String>,
}

/// Entradas para montar o resolvedor. As listas já vêm na ordem
/// coleção → ambiente (pai→filho) → pasta (raiz→folha) → requisição.
#[derive(Debug, Clone, Default)]
pub struct ScopeInput {
    pub collection: Vec<Variable>,
    pub environments: Vec<Variable>,
    pub folders: Vec<Variable>,
    pub request: Vec<Variable>,
    pub runtime: BTreeMap<String, String>,
    pub secret_refs: Vec<SecretRef>,
    pub environment_name: Option<String>,
    pub allow_process_env: bool,
}

#[derive(Debug, Clone)]
pub struct Resolver {
    values: BTreeMap<String, ResolvedVar>,
    allow_process_env: bool,
}

impl Resolver {
    pub fn build(input: ScopeInput) -> Self {
        let mut values: BTreeMap<String, ResolvedVar> = BTreeMap::new();

        for variable in input.collection.iter().filter(|var| !var.disabled) {
            insert_variable(
                &mut values,
                variable,
                VarScope::Collection,
                "coleção",
                &input,
            );
        }
        for variable in input.environments.iter().filter(|var| !var.disabled) {
            let origin = match &input.environment_name {
                Some(name) => format!("ambiente '{name}'"),
                None => "ambiente".to_string(),
            };
            insert_variable(
                &mut values,
                variable,
                VarScope::Environment,
                &origin,
                &input,
            );
        }
        for variable in input.folders.iter().filter(|var| !var.disabled) {
            insert_variable(&mut values, variable, VarScope::Folder, "pasta", &input);
        }
        for variable in input.request.iter().filter(|var| !var.disabled) {
            insert_variable(
                &mut values,
                variable,
                VarScope::Request,
                "requisição",
                &input,
            );
        }
        for (name, value) in &input.runtime {
            values.insert(
                name.clone(),
                ResolvedVar {
                    value: Some(value.clone()),
                    scope: VarScope::Runtime,
                    origin: "runtime (script)".to_string(),
                    secret: false,
                    error: None,
                },
            );
        }

        Self {
            values,
            allow_process_env: input.allow_process_env,
        }
    }

    pub fn get(&self, name: &str) -> Option<&ResolvedVar> {
        self.values.get(name)
    }

    pub fn values(&self) -> &BTreeMap<String, ResolvedVar> {
        &self.values
    }

    /// Resolve `{{nome}}` em um texto. Erros são explícitos; nunca há
    /// substituição silenciosa por string vazia.
    pub fn resolve(&self, input: &str) -> Result<String> {
        let mut stack = Vec::new();
        self.resolve_inner(input, 0, &mut stack)
    }

    fn resolve_inner(&self, input: &str, depth: usize, stack: &mut Vec<String>) -> Result<String> {
        if depth > MAX_DEPTH {
            return Err(Error::Variable(format!(
                "limite de recursão de variáveis excedido ({MAX_DEPTH})"
            )));
        }

        let mut output = String::with_capacity(input.len());
        let chars: Vec<char> = input.chars().collect();
        let mut index = 0;
        while index < chars.len() {
            if chars[index] == '\\'
                && index + 2 < chars.len()
                && chars[index + 1] == '{'
                && chars[index + 2] == '{'
            {
                output.push_str("{{");
                index += 3;
                continue;
            }
            if chars[index] == '{' && index + 1 < chars.len() && chars[index + 1] == '{' {
                let mut end = index + 2;
                while end + 1 < chars.len() && !(chars[end] == '}' && chars[end + 1] == '}') {
                    end += 1;
                }
                if end + 1 >= chars.len() {
                    return Err(Error::Variable(
                        "expressão de variável sem fechamento '}}'".into(),
                    ));
                }
                let expression: String = chars[index + 2..end].iter().collect();
                let name = expression.trim();
                if name.is_empty() {
                    return Err(Error::Variable("expressão de variável vazia".into()));
                }

                let (value, source_name) = self.lookup(name)?;
                if stack.iter().any(|item| item == &source_name) {
                    let mut chain = stack.clone();
                    chain.push(source_name.clone());
                    return Err(Error::Variable(format!(
                        "variável circular: {}",
                        chain.join(" → ")
                    )));
                }
                stack.push(source_name);
                let resolved = self.resolve_inner(&value, depth + 1, stack)?;
                stack.pop();
                output.push_str(&resolved);
                index = end + 2;
                continue;
            }
            output.push(chars[index]);
            index += 1;
        }
        Ok(output)
    }

    fn lookup(&self, name: &str) -> Result<(String, String)> {
        if let Some(variable) = self.values.get(name) {
            if let Some(error) = &variable.error {
                return Err(Error::Variable(error.clone()));
            }
            let value = variable.value.clone().ok_or_else(|| {
                Error::Variable(format!("variável '{name}' não possui valor resolvido"))
            })?;
            return Ok((value, name.to_string()));
        }

        // Acesso aninhado: {{objeto.campo}} ou {{lista.0}}.
        if let Some((root, path)) = name.split_once('.') {
            if let Some(variable) = self.values.get(root) {
                if let Some(error) = &variable.error {
                    return Err(Error::Variable(error.clone()));
                }
                if let Some(raw) = &variable.value {
                    let parsed: serde_json::Value = serde_json::from_str(raw).map_err(|_| {
                        Error::Variable(format!(
                            "variável '{root}' não é JSON; não é possível acessar '{path}'"
                        ))
                    })?;
                    if let Some(found) = json_path(&parsed, path) {
                        return Ok((json_to_text(found), name.to_string()));
                    }
                    return Err(Error::Variable(format!(
                        "caminho '{path}' não encontrado na variável '{root}'"
                    )));
                }
            }
        }

        if self.allow_process_env {
            if let Ok(value) = std::env::var(name) {
                return Ok((value, name.to_string()));
            }
        }

        Err(Error::Variable(format!(
            "variável não definida: '{name}'. Defina-a na coleção, no ambiente selecionado, na pasta ou na requisição."
        )))
    }

    /// Lista o valor efetivo e a origem. Segredos nunca expõem o valor.
    pub fn effective(&self) -> Vec<EffectiveVariable> {
        self.values
            .iter()
            .map(|(name, variable)| EffectiveVariable {
                name: name.clone(),
                value: if variable.secret {
                    None
                } else {
                    variable.value.clone()
                },
                display_value: if variable.secret {
                    variable.value.as_ref().map(|_| SECRET_MASK.to_string())
                } else {
                    variable.value.clone()
                },
                scope: variable.scope,
                origin: variable.origin.clone(),
                secret: variable.secret,
                error: variable.error.clone(),
            })
            .collect()
    }

    pub fn origin_of(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(|var| var.origin.as_str())
    }

    pub fn is_secret(&self, name: &str) -> bool {
        self.values
            .get(name)
            .map(|variable| variable.secret)
            .unwrap_or(false)
    }

    /// Valores reais de segredos resolvidos. Usado apenas para redaction;
    /// nunca deve ser serializado para a UI.
    pub fn secret_values(&self) -> Vec<String> {
        self.values
            .values()
            .filter(|variable| variable.secret)
            .filter_map(|variable| variable.value.clone())
            .filter(|value| value.len() >= 4)
            .collect()
    }
}

fn insert_variable(
    values: &mut BTreeMap<String, ResolvedVar>,
    variable: &Variable,
    scope: VarScope,
    origin: &str,
    input: &ScopeInput,
) {
    if variable.secret {
        let reference =
            find_secret_ref(&input.secret_refs, &variable.name, &input.environment_name);
        match reference {
            Some(reference) => {
                let (value, error) = resolve_secret(reference);
                values.insert(
                    variable.name.clone(),
                    ResolvedVar {
                        value,
                        scope,
                        origin: format!(
                            "{origin} · segredo {}:{}",
                            provider_name(reference.provider),
                            reference.key
                        ),
                        secret: true,
                        error,
                    },
                );
            }
            None => {
                values.insert(
                    variable.name.clone(),
                    ResolvedVar {
                        value: None,
                        scope,
                        origin: format!("{origin} · segredo sem referência"),
                        secret: true,
                        error: Some(format!(
                            "segredo '{}' sem referência declarada em extensions.reqlivo.secretRefs",
                            variable.name
                        )),
                    },
                );
            }
        }
        return;
    }

    let value = variable.value.as_ref().map(VarValue::as_text);
    values.insert(
        variable.name.clone(),
        ResolvedVar {
            value,
            scope,
            origin: origin.to_string(),
            secret: false,
            error: None,
        },
    );
}

fn find_secret_ref<'a>(
    refs: &'a [SecretRef],
    name: &str,
    environment: &Option<String>,
) -> Option<&'a SecretRef> {
    let mut generic = None;
    for reference in refs.iter().filter(|reference| reference.name == name) {
        match (&reference.environment, environment) {
            (Some(expected), Some(current)) if expected == current => return Some(reference),
            (None, _) => generic = Some(reference),
            _ => {}
        }
    }
    generic
}

fn resolve_secret(reference: &SecretRef) -> (Option<String>, Option<String>) {
    match reference.provider {
        SecretProvider::Env => match std::env::var(&reference.key) {
            Ok(value) => (Some(value), None),
            Err(_) => (
                None,
                Some(format!(
                    "segredo '{}' não encontrado na variável de ambiente '{}'",
                    reference.name, reference.key
                )),
            ),
        },
        SecretProvider::System => (
            None,
            Some(format!(
                "provedor 'system' ainda não implementado para o segredo '{}'; use provider: env",
                reference.name
            )),
        ),
        SecretProvider::File => (
            None,
            Some(format!(
                "provedor 'file' ainda não implementado para o segredo '{}'; use provider: env",
                reference.name
            )),
        ),
    }
}

fn provider_name(provider: SecretProvider) -> &'static str {
    match provider {
        SecretProvider::Env => "env",
        SecretProvider::System => "system",
        SecretProvider::File => "file",
    }
}

fn json_path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for segment in path.split('.') {
        current = match current {
            serde_json::Value::Object(map) => map.get(segment)?,
            serde_json::Value::Array(items) => items.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

fn json_to_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

/// Substitui valores conhecidos de segredos por máscara. Usado em logs,
/// relatórios e mensagens de erro.
pub fn redact_known(input: &str, secrets: &[String]) -> String {
    let mut output = input.to_string();
    let mut unique: HashSet<&str> = HashSet::new();
    for secret in secrets {
        if secret.len() >= 4 && unique.insert(secret.as_str()) {
            output = output.replace(secret, SECRET_MASK);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SecretProvider;

    fn variable(name: &str, value: &str) -> Variable {
        Variable::plain(name, value)
    }

    #[test]
    fn precedence_last_scope_wins() {
        let resolver = Resolver::build(ScopeInput {
            collection: vec![variable("host", "global")],
            environments: vec![variable("host", "ambiente")],
            folders: vec![variable("host", "pasta")],
            request: vec![variable("host", "requisição")],
            runtime: BTreeMap::from([("host".to_string(), "runtime".to_string())]),
            ..ScopeInput::default()
        });
        assert_eq!(resolver.resolve("{{host}}").unwrap(), "runtime");
        assert_eq!(resolver.get("host").unwrap().scope, VarScope::Runtime);
    }

    #[test]
    fn environment_beats_collection() {
        let resolver = Resolver::build(ScopeInput {
            collection: vec![variable("token", "global")],
            environments: vec![variable("token", "ambiente")],
            ..ScopeInput::default()
        });
        assert_eq!(resolver.resolve("{{token}}").unwrap(), "ambiente");
    }

    #[test]
    fn nested_json_lookup() {
        let resolver = Resolver::build(ScopeInput {
            collection: vec![variable("user", r#"{"name":"Ada","roles":["admin"]}"#)],
            ..ScopeInput::default()
        });
        assert_eq!(resolver.resolve("{{user.name}}").unwrap(), "Ada");
        assert_eq!(resolver.resolve("{{user.roles.0}}").unwrap(), "admin");
    }

    #[test]
    fn missing_variable_is_an_error() {
        let resolver = Resolver::build(ScopeInput::default());
        let error = resolver.resolve("{{host}}").unwrap_err().to_string();
        assert!(error.contains("variável não definida"));
    }

    #[test]
    fn circular_variable_is_an_error() {
        let resolver = Resolver::build(ScopeInput {
            collection: vec![variable("a", "{{b}}"), variable("b", "{{a}}")],
            ..ScopeInput::default()
        });
        let error = resolver.resolve("{{a}}").unwrap_err().to_string();
        assert!(error.contains("circular"));
    }

    #[test]
    fn escaped_template_is_literal() {
        let resolver = Resolver::build(ScopeInput::default());
        assert_eq!(resolver.resolve(r"\{{host}}").unwrap(), "{{host}}");
    }

    #[test]
    fn secret_is_masked_in_effective_list() {
        let reference = SecretRef {
            name: "token".into(),
            environment: None,
            provider: SecretProvider::Env,
            key: "REQLIVO_TEST_SECRET_XYZ".into(),
            description: None,
            extra: Default::default(),
        };
        unsafe { std::env::set_var("REQLIVO_TEST_SECRET_XYZ", "super-secret-value") };
        let resolver = Resolver::build(ScopeInput {
            environments: vec![Variable::secret("token")],
            secret_refs: vec![reference],
            ..ScopeInput::default()
        });
        assert_eq!(resolver.resolve("{{token}}").unwrap(), "super-secret-value");
        let effective = resolver.effective();
        let token = effective.iter().find(|item| item.name == "token").unwrap();
        assert_eq!(token.value, None);
        assert_eq!(token.display_value.as_deref(), Some(SECRET_MASK));
    }
}
