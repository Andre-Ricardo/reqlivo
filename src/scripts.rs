use crate::assertions::TestResult;
use crate::error::Error;
use crate::model::{Body, RequestFile, ScriptPhase};
use crate::redact::redact_text;
use crate::transport::HttpResponse;
use rquickjs::{CatchResultExt, Context, Function, Runtime};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
const STACK_LIMIT: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct ScriptLog {
    pub phase: String,
    pub origin: String,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ScriptOutcome {
    pub error: Option<String>,
    pub timed_out: bool,
    pub tests: Vec<TestResult>,
    pub logs: Vec<ScriptLog>,
    pub request_patch: Option<Value>,
}

/// Estado de ambiente compartilhado entre os scripts de uma execução.
/// `base` contém os valores efetivos (inclusive segredos, apenas em memória);
/// `runtime` contém o que scripts escreveram e tem precedência.
#[derive(Debug, Clone, Default)]
pub struct ScriptRuntime {
    pub base: BTreeMap<String, String>,
    pub runtime: BTreeMap<String, String>,
    pub secret_values: Vec<String>,
}

impl ScriptRuntime {
    pub fn new(base: BTreeMap<String, String>, runtime: BTreeMap<String, String>) -> Self {
        Self {
            base,
            runtime,
            secret_values: Vec::new(),
        }
    }

    pub fn with_secrets(mut self, secrets: Vec<String>) -> Self {
        self.secret_values = secrets;
        self
    }

    pub fn run(
        &mut self,
        code: &str,
        phase: ScriptPhase,
        origin: &str,
        request: &RequestFile,
        response: Option<&HttpResponse>,
        timeout_ms: u64,
    ) -> ScriptOutcome {
        let mut outcome = ScriptOutcome::default();
        let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(50));

        let runtime_handle = match Runtime::new() {
            Ok(runtime) => runtime,
            Err(error) => {
                outcome.error = Some(format!("não foi possível iniciar o sandbox: {error}"));
                return outcome;
            }
        };
        runtime_handle.set_memory_limit(MEMORY_LIMIT);
        runtime_handle.set_max_stack_size(STACK_LIMIT);
        runtime_handle.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));

        let context = match Context::full(&runtime_handle) {
            Ok(context) => context,
            Err(error) => {
                outcome.error = Some(format!("não foi possível criar o contexto JS: {error}"));
                return outcome;
            }
        };

        let runtime_vars = Rc::new(RefCell::new(self.runtime.clone()));
        let base_vars = Rc::new(self.base.clone());
        let request_json = Rc::new(
            serde_json::to_string(&request_for_script(request)).unwrap_or_else(|_| "{}".into()),
        );
        let response_json = Rc::new(
            response
                .map(|response| crate::assertions::response_value(response).to_string())
                .unwrap_or_else(|| "null".to_string()),
        );
        let logs: Rc<RefCell<Vec<(String, String)>>> = Rc::new(RefCell::new(Vec::new()));
        let tests: Rc<RefCell<Vec<(String, bool, String)>>> = Rc::new(RefCell::new(Vec::new()));

        let result = context.with(|ctx| -> Result<(), String> {
            let globals = ctx.globals();

            let runtime_for_get = runtime_vars.clone();
            let base_for_get = base_vars.clone();
            let env_get = Function::new(ctx.clone(), move |name: String| -> Option<String> {
                if let Some(value) = runtime_for_get.borrow().get(&name) {
                    return Some(value.clone());
                }
                base_for_get.get(&name).cloned()
            })
            .map_err(|error| format!("falha ao criar rq.env.get: {error}"))?;

            let runtime_for_set = runtime_vars.clone();
            let env_set = Function::new(ctx.clone(), move |name: String, value: String| {
                runtime_for_set.borrow_mut().insert(name, value);
            })
            .map_err(|error| format!("falha ao criar rq.env.set: {error}"))?;

            let runtime_for_has = runtime_vars.clone();
            let base_for_has = base_vars.clone();
            let env_has = Function::new(ctx.clone(), move |name: String| -> bool {
                runtime_for_has.borrow().contains_key(&name) || base_for_has.contains_key(&name)
            })
            .map_err(|error| format!("falha ao criar rq.env.has: {error}"))?;

            let runtime_for_delete = runtime_vars.clone();
            let env_delete = Function::new(ctx.clone(), move |name: String| {
                runtime_for_delete.borrow_mut().remove(&name);
            })
            .map_err(|error| format!("falha ao criar rq.env.delete: {error}"))?;

            let all_runtime = runtime_vars.clone();
            let all_base = base_vars.clone();
            let env_all = Function::new(ctx.clone(), move || -> String {
                let mut merged = all_base.as_ref().clone();
                for (key, value) in all_runtime.borrow().iter() {
                    merged.insert(key.clone(), value.clone());
                }
                serde_json::to_string(&merged).unwrap_or_else(|_| "{}".into())
            })
            .map_err(|error| format!("falha ao criar rq.env.all: {error}"))?;

            let report_tests = tests.clone();
            let report = Function::new(
                ctx.clone(),
                move |name: String, passed: bool, message: String| {
                    report_tests
                        .borrow_mut()
                        .push((name, passed, message));
                },
            )
            .map_err(|error| format!("falha ao criar __rq_report: {error}"))?;

            let log_sink = logs.clone();
            let log = Function::new(ctx.clone(), move |level: String, message: String| {
                log_sink.borrow_mut().push((level, message));
            })
            .map_err(|error| format!("falha ao criar __rq_log: {error}"))?;

            let request_provider = request_json.clone();
            let get_request = Function::new(ctx.clone(), move || -> String {
                request_provider.as_ref().clone()
            })
            .map_err(|error| format!("falha ao criar __rq_request_json: {error}"))?;

            let response_provider = response_json.clone();
            let get_response = Function::new(ctx.clone(), move || -> String {
                response_provider.as_ref().clone()
            })
            .map_err(|error| format!("falha ao criar __rq_response_json: {error}"))?;

            let runtime_export = runtime_vars.clone();
            let export_runtime = Function::new(ctx.clone(), move || -> String {
                serde_json::to_string(&*runtime_export.borrow()).unwrap_or_else(|_| "{}".into())
            })
            .map_err(|error| format!("falha ao criar __rq_runtime_json: {error}"))?;

            globals
                .set("__rq_env_get", env_get)
                .map_err(|error| format!("falha ao registrar rq.env.get: {error}"))?;
            globals
                .set("__rq_env_set", env_set)
                .map_err(|error| format!("falha ao registrar rq.env.set: {error}"))?;
            globals
                .set("__rq_env_has", env_has)
                .map_err(|error| format!("falha ao registrar rq.env.has: {error}"))?;
            globals
                .set("__rq_env_delete", env_delete)
                .map_err(|error| format!("falha ao registrar rq.env.delete: {error}"))?;
            globals
                .set("__rq_env_all", env_all)
                .map_err(|error| format!("falha ao registrar rq.env.all: {error}"))?;
            globals
                .set("__rq_report", report)
                .map_err(|error| format!("falha ao registrar __rq_report: {error}"))?;
            globals
                .set("__rq_log", log)
                .map_err(|error| format!("falha ao registrar __rq_log: {error}"))?;
            globals
                .set("__rq_request_json", get_request)
                .map_err(|error| format!("falha ao registrar __rq_request_json: {error}"))?;
            globals
                .set("__rq_response_json", get_response)
                .map_err(|error| format!("falha ao registrar __rq_response_json: {error}"))?;
            globals
                .set("__rq_runtime_json", export_runtime)
                .map_err(|error| format!("falha ao registrar __rq_runtime_json: {error}"))?;

            ctx.eval::<(), _>(PRELUDE)
                .map_err(|error| format!("falha ao inicializar o sandbox: {error}"))?;

            // O código do usuário roda dentro de try/catch para que um throw
            // vire resultado inspecionável, não pânico do host.
            let wrapped = format!(
                "(function(){{try {{\n{code}\n;return JSON.stringify({{ok:true}});}} catch (e) {{ return JSON.stringify({{ok:false,error:String((e && e.stack) || e)}}); }}}})()"
            );
            let raw: String = ctx
                .eval(wrapped)
                .catch(&ctx)
                .map_err(|error| format!("{error}"))?;
            let parsed: Value = serde_json::from_str(&raw)
                .map_err(|error| format!("resposta inválida do sandbox: {error}"))?;
            if parsed.get("ok").and_then(Value::as_bool) != Some(true) {
                let message = parsed
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("erro desconhecido no script")
                    .to_string();
                return Err(message);
            }

            // Sincroniza mutações de rq.request.
            let patch: String = ctx
                .eval("JSON.stringify(rq.request)")
                .catch(&ctx)
                .map_err(|error| format!("falha ao ler rq.request: {error}"))?;
            let patch: Value = serde_json::from_str(&patch).unwrap_or(Value::Null);
            outcome.request_patch = Some(patch);

            // Exporta variáveis de runtime definidas por scripts.
            let exported: String = ctx
                .eval("__rq_runtime_json()")
                .catch(&ctx)
                .map_err(|error| format!("falha ao ler variáveis de runtime: {error}"))?;
            if let Ok(values) = serde_json::from_str::<BTreeMap<String, String>>(&exported) {
                *runtime_vars.borrow_mut() = values;
            }

            // O MVP executa apenas código síncrono; promessas não são drenadas
            // porque `Context::with` já mantém o runtime emprestado.
            Ok(())
        });

        if Instant::now() >= deadline {
            outcome.timed_out = true;
            outcome.error = Some(format!(
                "script interrompido após {} ms (limite de execução)",
                timeout_ms
            ));
        } else if let Err(error) = result {
            outcome.error = Some(redact_text(&error, &self.secret_values));
        }

        // Mesmo com erro, preserva o que foi registrado antes dele.
        let captured_tests = tests.borrow().clone();
        for (name, passed, message) in captured_tests {
            outcome.tests.push(TestResult::new(
                name,
                passed,
                redact_text(&message, &self.secret_values),
                None,
                None,
                format!("script:{origin}"),
            ));
        }
        for (level, message) in logs.borrow().iter() {
            outcome.logs.push(ScriptLog {
                phase: phase.as_str().to_string(),
                origin: origin.to_string(),
                level: level.clone(),
                message: redact_text(message, &self.secret_values),
            });
        }

        self.runtime = runtime_vars.borrow().clone();
        outcome
    }
}

fn request_for_script(request: &RequestFile) -> Value {
    let headers: Vec<Value> = request
        .http
        .headers
        .iter()
        .map(|header| {
            serde_json::json!({
                "name": header.name,
                "value": header.value,
                "disabled": header.disabled,
            })
        })
        .collect();
    let body = match &request.http.body {
        Some(Body::Json { data })
        | Some(Body::Text { data })
        | Some(Body::Xml { data })
        | Some(Body::Sparql { data }) => data.clone(),
        Some(Body::FormUrlEncoded { data }) => serde_json::to_string(data).unwrap_or_default(),
        Some(Body::Multipart { data }) => serde_json::to_string(data).unwrap_or_default(),
        Some(Body::Unsupported) | None => String::new(),
    };
    serde_json::json!({
        "method": request.http.method,
        "url": request.http.url,
        "headers": headers,
        "body": body,
        "bodyKind": request.http.body.as_ref().map(Body::kind).unwrap_or("none"),
    })
}

/// Aplica mutações feitas em `rq.request` antes do transporte.
pub fn apply_request_patch(request: &mut RequestFile, patch: &Value) -> Result<(), Error> {
    let Some(object) = patch.as_object() else {
        return Ok(());
    };
    if let Some(method) = object.get("method").and_then(Value::as_str) {
        if !method.trim().is_empty() {
            request.http.method = method.to_string();
        }
    }
    if let Some(url) = object.get("url").and_then(Value::as_str) {
        if !url.trim().is_empty() {
            request.http.url = url.to_string();
        }
    }
    if let Some(headers) = object.get("headers").and_then(Value::as_array) {
        let mut updated = Vec::new();
        for header in headers {
            let Some(name) = header.get("name").and_then(Value::as_str) else {
                continue;
            };
            let value = header.get("value").and_then(Value::as_str).unwrap_or("");
            let disabled = header
                .get("disabled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            updated.push(crate::model::Header {
                name: name.to_string(),
                value: value.to_string(),
                description: None,
                disabled,
                extra: Default::default(),
            });
        }
        request.http.headers = updated;
    }
    if let Some(body) = object.get("body").and_then(Value::as_str) {
        match request.http.body.as_mut() {
            Some(Body::Json { data })
            | Some(Body::Text { data })
            | Some(Body::Xml { data })
            | Some(Body::Sparql { data }) => {
                if *data != body {
                    *data = body.to_string();
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ScriptTestPayload {
    name: String,
    passed: bool,
    message: String,
}

const PRELUDE: &str = r#"
const rq = globalThis.rq || (globalThis.rq = {});
rq.env = {
  get: function(name) { return __rq_env_get(String(name)); },
  set: function(name, value) { __rq_env_set(String(name), String(value)); },
  has: function(name) { return __rq_env_has(String(name)); },
  delete: function(name) { __rq_env_delete(String(name)); },
  all: function() { return JSON.parse(__rq_env_all()); }
};
rq.request = JSON.parse(__rq_request_json());
rq.request.setHeader = function(name, value) {
  const lower = String(name).toLowerCase();
  this.headers = (this.headers || []).filter(function(h) { return String(h.name).toLowerCase() !== lower; });
  this.headers.push({ name: String(name), value: String(value), disabled: false });
};
rq.request.header = function(name) {
  const lower = String(name).toLowerCase();
  const found = (this.headers || []).find(function(h) { return String(h.name).toLowerCase() === lower && !h.disabled; });
  return found ? found.value : undefined;
};
rq.request.removeHeader = function(name) {
  const lower = String(name).toLowerCase();
  this.headers = (this.headers || []).filter(function(h) { return String(h.name).toLowerCase() !== lower; });
};
const __rq_response = JSON.parse(__rq_response_json());
if (__rq_response) {
  rq.response = __rq_response;
  rq.response.json = function() {
    if (typeof this.body === 'string') { try { return JSON.parse(this.body); } catch (e) { return null; } }
    return this.body;
  };
  rq.response.header = function(name) {
    return (this.headers || {})[String(name).toLowerCase()];
  };
} else {
  rq.response = undefined;
}
function __rq_stringify(value) {
  if (typeof value === 'string') return value;
  try { return JSON.stringify(value); } catch (e) { return String(value); }
}
function __rq_fail(message, negate) {
  throw new Error((negate ? 'não ' : '') + message);
}
function __rq_matcher(actual, negate) {
  function check(condition, message) {
    if (negate ? condition : !condition) __rq_fail(message, negate);
  }
  return {
    toBe: function(expected) { check(actual === expected, 'esperado ' + __rq_stringify(expected) + ', obtido ' + __rq_stringify(actual)); },
    toEqual: function(expected) { check(JSON.stringify(actual) === JSON.stringify(expected), 'esperado ' + __rq_stringify(expected) + ', obtido ' + __rq_stringify(actual)); },
    toContain: function(expected) {
      const ok = typeof actual === 'string' ? actual.indexOf(expected) >= 0 : Array.isArray(actual) ? actual.some(function(v) { return JSON.stringify(v) === JSON.stringify(expected); }) : false;
      check(ok, 'esperado conter ' + __rq_stringify(expected) + ', obtido ' + __rq_stringify(actual));
    },
    toMatch: function(pattern) {
      const regex = pattern instanceof RegExp ? pattern : new RegExp(String(pattern));
      check(regex.test(String(actual)), 'esperado corresponder a ' + String(pattern) + ', obtido ' + __rq_stringify(actual));
    },
    toBeTruthy: function() { check(!!actual, 'esperado valor verdadeiro, obtido ' + __rq_stringify(actual)); },
    toBeFalsy: function() { check(!actual, 'esperado valor falso, obtido ' + __rq_stringify(actual)); },
    toBeNull: function() { check(actual === null, 'esperado null, obtido ' + __rq_stringify(actual)); },
    toBeDefined: function() { check(actual !== undefined, 'esperado valor definido, obtido undefined'); },
    toBeGreaterThan: function(expected) { check(Number(actual) > Number(expected), 'esperado > ' + expected + ', obtido ' + __rq_stringify(actual)); },
    toBeLessThan: function(expected) { check(Number(actual) < Number(expected), 'esperado < ' + expected + ', obtido ' + __rq_stringify(actual)); },
    toHaveLength: function(expected) { check(actual != null && Number(actual.length) === Number(expected), 'esperado comprimento ' + expected + ', obtido ' + (actual == null ? 'null' : actual.length)); },
    get not() { return __rq_matcher(actual, !negate); }
  };
}
function expect(actual) { return __rq_matcher(actual, false); }
function test(name, fn) {
  try { fn(); __rq_report(String(name), true, ''); }
  catch (e) { __rq_report(String(name), false, String((e && e.message) || e)); }
}
rq.test = test;
rq.expect = expect;
rq.assert = function(condition, message) { if (!condition) throw new Error(String(message || 'asserção falhou')); };
const console = {
  log: function() { __rq_log('log', Array.prototype.slice.call(arguments).map(__rq_stringify).join(' ')); },
  info: function() { __rq_log('info', Array.prototype.slice.call(arguments).map(__rq_stringify).join(' ')); },
  warn: function() { __rq_log('warn', Array.prototype.slice.call(arguments).map(__rq_stringify).join(' ')); },
  error: function() { __rq_log('error', Array.prototype.slice.call(arguments).map(__rq_stringify).join(' ')); }
};
globalThis.console = console;
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::init_collection;

    fn runtime() -> ScriptRuntime {
        ScriptRuntime::new(BTreeMap::new(), BTreeMap::new())
    }

    fn request(dir: &std::path::Path) -> (crate::store::CollectionStore, RequestFile) {
        let mut store = init_collection(dir, "Teste").unwrap();
        let payload = store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        (store, payload.request)
    }

    #[test]
    fn env_set_persists_runtime_and_mutates_request() {
        let dir = tempfile::tempdir().unwrap();
        let (_store, request) = request(dir.path());
        let mut runtime = runtime();
        let outcome = runtime.run(
            "rq.env.set('token', 'abc'); rq.request.setHeader('X-Test', '1');",
            ScriptPhase::BeforeRequest,
            "requisição",
            &request,
            None,
            1000,
        );
        assert!(outcome.error.is_none(), "{:?}", outcome.error);
        assert_eq!(
            runtime.runtime.get("token").map(String::as_str),
            Some("abc")
        );
        let patch = outcome.request_patch.unwrap();
        assert!(
            patch["headers"]
                .as_array()
                .unwrap()
                .iter()
                .any(|header| header["name"] == "X-Test")
        );
    }

    #[test]
    fn test_failure_is_reported_with_message() {
        let dir = tempfile::tempdir().unwrap();
        let (_store, request) = request(dir.path());
        let mut runtime = runtime();
        let outcome = runtime.run(
            "test('status é 200', function(){ expect(404).toBe(200); });",
            ScriptPhase::Tests,
            "requisição",
            &request,
            None,
            1000,
        );
        assert_eq!(outcome.tests.len(), 1);
        assert!(!outcome.tests[0].passed);
        assert!(outcome.tests[0].message.contains("404"));
    }

    #[test]
    fn infinite_loop_is_interrupted() {
        let dir = tempfile::tempdir().unwrap();
        let (_store, request) = request(dir.path());
        let mut runtime = runtime();
        let started = Instant::now();
        let outcome = runtime.run(
            "while (true) {}",
            ScriptPhase::BeforeRequest,
            "requisição",
            &request,
            None,
            200,
        );
        assert!(
            outcome.timed_out,
            "esperava timeout, veio {:?}",
            outcome.error
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn console_log_is_captured_and_redacted() {
        let dir = tempfile::tempdir().unwrap();
        let (_store, request) = request(dir.path());
        let mut runtime = ScriptRuntime::new(BTreeMap::new(), BTreeMap::new())
            .with_secrets(vec!["super-secret".into()]);
        let outcome = runtime.run(
            "console.log('valor', 'super-secret');",
            ScriptPhase::Tests,
            "requisição",
            &request,
            None,
            1000,
        );
        assert_eq!(outcome.logs.len(), 1);
        assert!(!outcome.logs[0].message.contains("super-secret"));
    }
}
