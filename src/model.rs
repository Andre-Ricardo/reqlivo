use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_yaml::Value;
use std::collections::BTreeMap;

pub const OPENCOLLECTION_VERSION: &str = "1.0.0";
pub const SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;
pub const DEFAULT_MAX_REDIRECTS: u32 = 5;
pub const DEFAULT_MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024;
pub const DEFAULT_SCRIPT_TIMEOUT_MS: u64 = 2_000;

pub type Extra = BTreeMap<String, Value>;

fn default_oc_version() -> String {
    OPENCOLLECTION_VERSION.to_string()
}

fn is_false(value: &bool) -> bool {
    !*value
}

// ---------------------------------------------------------------------------
// Manifesto da coleção (opencollection.yml)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionManifest {
    #[serde(default = "default_oc_version")]
    pub opencollection: String,
    pub info: CollectionInfo,
    #[serde(default)]
    pub config: CollectionConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<RequestDefaults>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bundled: bool,
    #[serde(default)]
    pub extensions: Extensions,
    #[serde(flatten, default)]
    pub extra: Extra,
}

impl CollectionManifest {
    pub fn reqlivo(&self) -> Option<&ReqlivoExtension> {
        self.extensions.reqlivo.as_ref()
    }

    pub fn settings(&self) -> CollectionSettings {
        self.reqlivo()
            .and_then(|ext| ext.settings.clone())
            .unwrap_or_default()
    }

    pub fn collection_id(&self) -> Option<&str> {
        self.reqlivo().and_then(|ext| ext.id.as_deref())
    }

    pub fn secret_refs(&self) -> &[SecretRef] {
        self.reqlivo()
            .map(|ext| ext.secret_refs.as_slice())
            .unwrap_or(&[])
    }

    pub fn environments(&self) -> &[Environment] {
        &self.config.environments
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authors: Option<Value>,
    #[serde(flatten, default)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CollectionConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub environments: Vec<Environment>,
    #[serde(flatten, default)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Extensions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reqlivo: Option<ReqlivoExtension>,
    #[serde(flatten, default)]
    pub extra: Extra,
}

impl Default for Extensions {
    fn default() -> Self {
        Self {
            reqlivo: None,
            extra: Extra::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReqlivoExtension {
    #[serde(default = "default_schema_version", rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secret_refs: Vec<SecretRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<CollectionSettings>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub environment_ids: BTreeMap<String, String>,
    #[serde(flatten, default)]
    pub extra: Extra,
}

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_environment: Option<String>,
    #[serde(default = "default_timeout")]
    pub default_timeout_ms: u64,
    #[serde(default = "default_true")]
    pub follow_redirects: bool,
    #[serde(default = "default_redirects")]
    pub max_redirects: u32,
    /// Quando verdadeiro, o transporte só permite loopback, link-local e hosts
    /// privados. Nenhuma chamada externa é feita.
    #[serde(default)]
    pub offline_only: bool,
    /// Permite que `{{VAR}}` seja resolvida a partir do ambiente do processo.
    /// Desligado por padrão; segredos usam sempre referência explícita.
    #[serde(default)]
    pub allow_process_env: bool,
    #[serde(default = "default_max_response")]
    pub max_response_bytes: usize,
    #[serde(default = "default_script_timeout")]
    pub script_timeout_ms: u64,
    #[serde(flatten, default)]
    pub extra: Extra,
}

impl Default for CollectionSettings {
    fn default() -> Self {
        Self {
            default_environment: None,
            default_timeout_ms: DEFAULT_TIMEOUT_MS,
            follow_redirects: true,
            max_redirects: DEFAULT_MAX_REDIRECTS,
            offline_only: false,
            allow_process_env: false,
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            script_timeout_ms: DEFAULT_SCRIPT_TIMEOUT_MS,
            extra: Extra::new(),
        }
    }
}

fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_MS
}
fn default_redirects() -> u32 {
    DEFAULT_MAX_REDIRECTS
}
fn default_true() -> bool {
    true
}
fn default_max_response() -> usize {
    DEFAULT_MAX_RESPONSE_BYTES
}
fn default_script_timeout() -> u64 {
    DEFAULT_SCRIPT_TIMEOUT_MS
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretRef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default)]
    pub provider: SecretProvider,
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(flatten, default)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SecretProvider {
    #[default]
    Env,
    System,
    File,
}

// ---------------------------------------------------------------------------
// Ambientes e variáveis
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<Variable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "dotEnvFilePath"
    )]
    pub dot_env_file_path: Option<String>,
    #[serde(flatten, default)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<VarValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Value>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
    /// Variável secreta no padrão OpenCollection: sem valor no arquivo.
    /// O valor vem de `extensions.reqlivo.secretRefs`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub secret: bool,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    pub kind: Option<String>,
    #[serde(flatten, default)]
    pub extra: Extra,
}

impl Variable {
    pub fn plain(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: Some(VarValue::String(value.into())),
            description: None,
            disabled: false,
            secret: false,
            kind: None,
            extra: Extra::new(),
        }
    }

    pub fn secret(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: None,
            description: None,
            disabled: false,
            secret: true,
            kind: None,
            extra: Extra::new(),
        }
    }

    pub fn text_value(&self) -> Option<String> {
        self.value.as_ref().map(VarValue::as_text)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VarValue {
    String(String),
    Typed {
        #[serde(rename = "type")]
        kind: String,
        data: String,
    },
}

impl VarValue {
    pub fn as_text(&self) -> String {
        match self {
            VarValue::String(value) => value.clone(),
            VarValue::Typed { kind, data } => match kind.as_str() {
                "null" => "null".to_string(),
                "string" => data.clone(),
                _ => data.clone(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Requisição
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestFile {
    pub info: RequestInfo,
    pub http: HttpDetails,
    #[serde(default)]
    pub runtime: RequestRuntime,
    #[serde(default)]
    pub settings: HttpSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    #[serde(default)]
    pub extensions: Extensions,
    #[serde(flatten, default)]
    pub extra: Extra,
}

impl RequestFile {
    pub fn validate(&self) -> crate::error::Result<()> {
        if self.info.name.trim().is_empty() {
            return Err(crate::error::Error::Validation(
                "o nome da requisição é obrigatório".into(),
            ));
        }
        if !self.info.kind.eq_ignore_ascii_case("http") {
            return Err(crate::error::Error::Validation(format!(
                "tipo de requisição não suportado no MVP: {}",
                self.info.kind
            )));
        }
        if self.http.method.trim().is_empty() {
            return Err(crate::error::Error::Validation(
                "o método HTTP é obrigatório".into(),
            ));
        }
        if self.http.url.trim().is_empty() {
            return Err(crate::error::Error::Validation(
                "a URL da requisição é obrigatória".into(),
            ));
        }
        Ok(())
    }

    pub fn reqlivo_id(&self) -> Option<&str> {
        self.extensions
            .reqlivo
            .as_ref()
            .and_then(|ext| ext.id.as_deref())
    }

    pub fn secret_refs(&self) -> &[SecretRef] {
        self.extensions
            .reqlivo
            .as_ref()
            .map(|ext| ext.secret_refs.as_slice())
            .unwrap_or(&[])
    }

    pub fn scripts(&self, phase: ScriptPhase) -> Vec<&Script> {
        self.runtime
            .scripts
            .iter()
            .filter(|script| script.phase == phase)
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestInfo {
    pub name: String,
    #[serde(default = "default_http_type", rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub seq: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Value>,
    #[serde(flatten, default)]
    pub extra: Extra,
}

fn default_http_type() -> String {
    "http".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpDetails {
    pub method: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<Header>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<Param>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Body>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
    #[serde(default)]
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Value>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
    #[serde(flatten, default)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    #[serde(default)]
    pub value: String,
    #[serde(default, rename = "type")]
    pub kind: ParamKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Value>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
    #[serde(flatten, default)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamKind {
    #[default]
    Query,
    Path,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Body {
    #[serde(rename = "json")]
    Json {
        #[serde(default)]
        data: String,
    },
    #[serde(rename = "text")]
    Text {
        #[serde(default)]
        data: String,
    },
    #[serde(rename = "xml")]
    Xml {
        #[serde(default)]
        data: String,
    },
    #[serde(rename = "sparql")]
    Sparql {
        #[serde(default)]
        data: String,
    },
    #[serde(rename = "form-urlencoded")]
    FormUrlEncoded {
        #[serde(default)]
        data: Vec<FormField>,
    },
    #[serde(rename = "multipart-form")]
    Multipart {
        #[serde(default)]
        data: Vec<FormField>,
    },
    #[serde(other)]
    Unsupported,
}

impl Body {
    pub fn kind(&self) -> &'static str {
        match self {
            Body::Json { .. } => "json",
            Body::Text { .. } => "text",
            Body::Xml { .. } => "xml",
            Body::Sparql { .. } => "sparql",
            Body::FormUrlEncoded { .. } => "form-urlencoded",
            Body::Multipart { .. } => "multipart-form",
            Body::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormField {
    pub name: String,
    #[serde(default)]
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Value>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
    #[serde(flatten, default)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RequestRuntime {
    pub variables: Vec<Variable>,
    pub scripts: Vec<Script>,
    pub assertions: Vec<Assertion>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for RequestRuntime {
    fn default() -> Self {
        Self {
            variables: Vec::new(),
            scripts: Vec::new(),
            assertions: Vec::new(),
            extra: Extra::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Script {
    #[serde(rename = "type")]
    pub phase: ScriptPhase,
    pub code: String,
    #[serde(flatten, default)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScriptPhase {
    #[serde(rename = "before-request")]
    BeforeRequest,
    #[serde(rename = "after-response")]
    AfterResponse,
    #[serde(rename = "tests")]
    Tests,
    #[serde(rename = "hooks")]
    Hooks,
}

impl ScriptPhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            ScriptPhase::BeforeRequest => "before-request",
            ScriptPhase::AfterResponse => "after-response",
            ScriptPhase::Tests => "tests",
            ScriptPhase::Hooks => "hooks",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assertion {
    pub expression: String,
    pub operator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Value>,
    #[serde(flatten, default)]
    pub extra: Extra,
}

impl Assertion {
    pub fn expected_text(&self) -> Option<String> {
        self.value.as_ref().map(|value| match value {
            Value::String(text) => text.clone(),
            other => serde_yaml::to_string(other)
                .unwrap_or_default()
                .trim()
                .to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Autenticação
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    Inherit,
    Basic,
    Bearer,
    ApiKey,
}

#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub mode: AuthMode,
    pub username: String,
    pub password: String,
    pub token: String,
    pub key: String,
    pub value: String,
    pub placement: ApiKeyPlacement,
}

impl AuthConfig {
    pub fn inherit() -> Self {
        Self::default_with(AuthMode::Inherit)
    }

    pub fn basic(username: impl Into<String>, password: impl Into<String>) -> Self {
        let mut auth = Self::default_with(AuthMode::Basic);
        auth.username = username.into();
        auth.password = password.into();
        auth
    }

    pub fn bearer(token: impl Into<String>) -> Self {
        let mut auth = Self::default_with(AuthMode::Bearer);
        auth.token = token.into();
        auth
    }

    pub fn api_key(
        key: impl Into<String>,
        value: impl Into<String>,
        placement: ApiKeyPlacement,
    ) -> Self {
        let mut auth = Self::default_with(AuthMode::ApiKey);
        auth.key = key.into();
        auth.value = value.into();
        auth.placement = placement;
        auth
    }

    fn default_with(mode: AuthMode) -> Self {
        Self {
            mode,
            username: String::new(),
            password: String::new(),
            token: String::new(),
            key: String::new(),
            value: String::new(),
            placement: ApiKeyPlacement::Header,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ApiKeyPlacement {
    #[default]
    Header,
    Query,
}

impl Serialize for AuthConfig {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self.mode {
            AuthMode::Inherit => serializer.serialize_str("inherit"),
            AuthMode::Basic => {
                let mut map = serializer.serialize_map(Some(3))?;
                map.serialize_entry("type", "basic")?;
                map.serialize_entry("username", &self.username)?;
                map.serialize_entry("password", &self.password)?;
                map.end()
            }
            AuthMode::Bearer => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("type", "bearer")?;
                map.serialize_entry("token", &self.token)?;
                map.end()
            }
            AuthMode::ApiKey => {
                let mut map = serializer.serialize_map(Some(5))?;
                map.serialize_entry("type", "apikey")?;
                map.serialize_entry("key", &self.key)?;
                map.serialize_entry("value", &self.value)?;
                map.serialize_entry(
                    "placement",
                    match self.placement {
                        ApiKeyPlacement::Header => "header",
                        ApiKeyPlacement::Query => "query",
                    },
                )?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for AuthConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            Text(String),
            Map(MapHelper),
        }

        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "lowercase")]
        enum MapHelper {
            Basic {
                #[serde(default)]
                username: String,
                #[serde(default)]
                password: String,
            },
            Bearer {
                #[serde(default)]
                token: String,
            },
            Apikey {
                #[serde(default)]
                key: String,
                #[serde(default)]
                value: String,
                #[serde(default)]
                placement: PlacementHelper,
            },
        }

        #[derive(Deserialize, Default)]
        #[serde(rename_all = "lowercase")]
        enum PlacementHelper {
            #[default]
            Header,
            Query,
        }

        match Helper::deserialize(deserializer)? {
            Helper::Text(text) if text.eq_ignore_ascii_case("inherit") => Ok(AuthConfig::inherit()),
            Helper::Text(other) => Err(serde::de::Error::custom(format!(
                "autenticação desconhecida: {other}"
            ))),
            Helper::Map(MapHelper::Basic { username, password }) => {
                Ok(AuthConfig::basic(username, password))
            }
            Helper::Map(MapHelper::Bearer { token }) => Ok(AuthConfig::bearer(token)),
            Helper::Map(MapHelper::Apikey {
                key,
                value,
                placement,
            }) => Ok(AuthConfig::api_key(
                key,
                value,
                match placement {
                    PlacementHelper::Header => ApiKeyPlacement::Header,
                    PlacementHelper::Query => ApiKeyPlacement::Query,
                },
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Defaults herdáveis de coleção/pasta
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequestDefaults {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<Header>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<Variable>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<Script>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Value>,
    #[serde(flatten, default)]
    pub extra: Extra,
}

// ---------------------------------------------------------------------------
// Pastas
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderFile {
    pub info: FolderInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<RequestDefaults>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    #[serde(default)]
    pub extensions: Extensions,
    #[serde(flatten, default)]
    pub extra: Extra,
}

impl FolderFile {
    pub fn reqlivo_id(&self) -> Option<&str> {
        self.extensions
            .reqlivo
            .as_ref()
            .and_then(|ext| ext.id.as_deref())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderInfo {
    pub name: String,
    #[serde(default = "default_folder_type", rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub seq: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Value>,
    #[serde(flatten, default)]
    pub extra: Extra,
}

fn default_folder_type() -> String {
    "folder".to_string()
}

// ---------------------------------------------------------------------------
// Configurações de execução HTTP
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HttpSettings {
    #[serde(
        default,
        rename = "encodeUrl",
        deserialize_with = "de_opt_bool",
        skip_serializing_if = "Option::is_none"
    )]
    pub encode_url: Option<bool>,
    #[serde(
        default,
        deserialize_with = "de_opt_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub timeout: Option<u64>,
    #[serde(
        default,
        rename = "followRedirects",
        deserialize_with = "de_opt_bool",
        skip_serializing_if = "Option::is_none"
    )]
    pub follow_redirects: Option<bool>,
    #[serde(
        default,
        rename = "maxRedirects",
        deserialize_with = "de_opt_u32",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_redirects: Option<u32>,
    #[serde(flatten, default)]
    pub extra: Extra,
}

fn de_opt_bool<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<bool>, D::Error> {
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(value)),
        Some(Value::String(text)) if text.eq_ignore_ascii_case("inherit") => Ok(None),
        Some(other) => Err(serde::de::Error::custom(format!(
            "esperado booleano ou 'inherit', recebido {other:?}"
        ))),
    }
}

fn de_opt_u64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u64>, D::Error> {
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom("número inválido")),
        Some(Value::String(text)) if text.eq_ignore_ascii_case("inherit") => Ok(None),
        Some(other) => Err(serde::de::Error::custom(format!(
            "esperado número ou 'inherit', recebido {other:?}"
        ))),
    }
}

fn de_opt_u32<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u32>, D::Error> {
    de_opt_u64(deserializer).map(|value| value.map(|number| number.min(u32::MAX as u64) as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_roundtrip_inherit_and_bearer() {
        let inherit: AuthConfig = serde_yaml::from_str("inherit").unwrap();
        assert_eq!(inherit.mode, AuthMode::Inherit);
        assert_eq!(serde_yaml::to_string(&inherit).unwrap().trim(), "inherit");

        let bearer: AuthConfig =
            serde_yaml::from_str("type: bearer\ntoken: '{{token}}'\n").unwrap();
        assert_eq!(bearer.mode, AuthMode::Bearer);
        let yaml = serde_yaml::to_string(&bearer).unwrap();
        assert!(yaml.contains("type: bearer"));
        assert!(yaml.contains("token: '{{token}}'"));
    }

    #[test]
    fn settings_accept_inherit() {
        let settings: HttpSettings =
            serde_yaml::from_str("encodeUrl: inherit\ntimeout: 1000\n").unwrap();
        assert_eq!(settings.encode_url, None);
        assert_eq!(settings.timeout, Some(1000));
    }

    #[test]
    fn secret_variable_has_no_value() {
        let variable = Variable::secret("token");
        let yaml = serde_yaml::to_string(&variable).unwrap();
        assert!(yaml.contains("secret: true"));
        assert!(!yaml.contains("value:"));
    }
}
