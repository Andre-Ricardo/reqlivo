use serde::Serialize;
use std::fmt;

/// Categoria estável de erro, usada pela CLI, pela API e pela interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    Config,
    Storage,
    Validation,
    Variable,
    Auth,
    Dns,
    Connect,
    Timeout,
    Tls,
    Proxy,
    Redirect,
    Body,
    Script,
    Test,
    Cancelled,
    Offline,
    Import,
    Export,
    Internal,
}

/// Representação pública de um erro. Nunca contém segredos.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorInfo {
    pub category: ErrorCategory,
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    Storage(String),
    #[error("{0}")]
    Validation(String),
    #[error("{0}")]
    Variable(String),
    #[error("{0}")]
    Auth(String),
    #[error("{0}")]
    Body(String),
    #[error("{0}")]
    Script(String),
    #[error("{0}")]
    Import(String),
    #[error("{0}")]
    Export(String),
    #[error("requisição cancelada")]
    Cancelled,
    #[error("modo offline: destino externo bloqueado: {0}")]
    Offline(String),
    #[error("erro de rede ({category}): {message}")]
    Network {
        category: ErrorCategory,
        message: String,
    },
    #[error("{0}")]
    Internal(String),
}

impl Error {
    pub fn info(&self) -> ErrorInfo {
        let (category, code, retryable) = match self {
            Error::Config(_) => (ErrorCategory::Config, "config", false),
            Error::Storage(_) => (ErrorCategory::Storage, "storage", false),
            Error::Validation(_) => (ErrorCategory::Validation, "validation", false),
            Error::Variable(_) => (ErrorCategory::Variable, "variable", false),
            Error::Auth(_) => (ErrorCategory::Auth, "auth", false),
            Error::Body(_) => (ErrorCategory::Body, "body", false),
            Error::Script(_) => (ErrorCategory::Script, "script", false),
            Error::Import(_) => (ErrorCategory::Import, "import", false),
            Error::Export(_) => (ErrorCategory::Export, "export", false),
            Error::Cancelled => (ErrorCategory::Cancelled, "cancelled", false),
            Error::Offline(_) => (ErrorCategory::Offline, "offline", false),
            Error::Network { category, .. } => {
                let code = match category {
                    ErrorCategory::Dns => "dns",
                    ErrorCategory::Connect => "connect",
                    ErrorCategory::Timeout => "timeout",
                    ErrorCategory::Tls => "tls",
                    ErrorCategory::Proxy => "proxy",
                    ErrorCategory::Redirect => "redirect",
                    _ => "network",
                };
                let retryable = matches!(
                    category,
                    ErrorCategory::Dns
                        | ErrorCategory::Connect
                        | ErrorCategory::Timeout
                        | ErrorCategory::Proxy
                );
                (*category, code, retryable)
            }
            Error::Internal(_) => (ErrorCategory::Internal, "internal", false),
        };
        ErrorInfo {
            category,
            code,
            message: self.to_string(),
            retryable,
        }
    }
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Error::Storage(value.to_string())
    }
}

impl From<serde_yaml::Error> for Error {
    fn from(value: serde_yaml::Error) -> Self {
        Error::Validation(format!("YAML inválido: {value}"))
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Error::Validation(format!("JSON inválido: {value}"))
    }
}
