//! Reqlivo — núcleo local-first de um cliente de APIs.
//!
//! O projeto é a fonte de verdade (OpenCollection 1.0.0 + extensões
//! `reqlivo`), o cache é descartável e segredos são referências ao ambiente.
//! Este crate é compartilhado pela CLI e pela aplicativo desktop Tauri.

pub mod assertions;
pub mod auth;
pub mod body;
pub mod desktop;
pub mod error;
pub mod import_export;
pub mod log;
pub mod model;
pub mod redact;
pub mod runner;
pub mod scripts;
pub mod server;
pub mod store;
pub mod transport;
pub mod variables;

pub use error::{Error, ErrorCategory, ErrorInfo, Result};
pub use model::*;
pub use store::{
    CollectionSnapshot, CollectionStore, EffectiveSettings, FolderEntry, RequestEntry,
    RequestPayload, TreeNode,
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const USER_AGENT: &str = concat!("reqlivo/", env!("CARGO_PKG_VERSION"));
