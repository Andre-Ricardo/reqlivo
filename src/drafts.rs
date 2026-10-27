//! Rascunhos persistentes do editor de requisição.
//!
//! Um rascunho nunca altera o YAML canônico e nunca contém resposta, cookies,
//! variáveis de runtime ou valores resolvidos de segredos. O conteúdo editável
//! pode conter credenciais literais digitadas pelo usuário; por isso o arquivo
//! é local, sem criptografia, com permissões restritas quando suportadas.

use crate::error::{Error, Result};
use crate::model::RequestFile;
use crate::persist;
use crate::store::{CollectionStore, RequestPayload};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const DRAFT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftRecord {
    pub version: u32,
    pub collection_id: Option<String>,
    pub request_id: String,
    pub base_revision: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub generation: u64,
    pub request: RequestFile,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftSummary {
    pub request_id: String,
    pub generation: u64,
    pub base_revision: Option<String>,
    pub updated_at: Option<String>,
    pub state: String,
    pub file_exists: bool,
    pub file_revision: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftDetail {
    pub summary: DraftSummary,
    pub record: Option<DraftRecord>,
    pub error: Option<String>,
}

fn drafts_dir(root: &Path) -> Result<PathBuf> {
    persist::ensure_internal_dir(root, "drafts")
}

fn draft_path(root: &Path, request_id: &str) -> Result<PathBuf> {
    if !persist::is_safe_id(request_id) {
        return Err(Error::Validation(
            "identificador de requisição inválido para rascunho".into(),
        ));
    }
    Ok(drafts_dir(root)?.join(format!("{request_id}.json")))
}

fn read_record(path: &Path) -> std::result::Result<DraftRecord, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let record: DraftRecord =
        serde_json::from_slice(&bytes).map_err(|error| format!("registro ilegível: {error}"))?;
    if record.version > DRAFT_VERSION {
        return Err(format!(
            "versão de rascunho {} é mais nova que a suportada ({DRAFT_VERSION})",
            record.version
        ));
    }
    if record.version != DRAFT_VERSION {
        return Err(format!(
            "versão de rascunho desconhecida: {}",
            record.version
        ));
    }
    if !persist::is_safe_id(&record.request_id) {
        return Err("registro com identificador inválido".into());
    }
    Ok(record)
}

fn load(root: &Path, request_id: &str) -> Result<Option<DraftRecord>> {
    let path = draft_path(root, request_id)?;
    if !path.exists() {
        return Ok(None);
    }
    read_record(&path)
        .map(Some)
        .map_err(|message| Error::Conflict {
            code: "draft_corrupt",
            message: format!("rascunho de '{request_id}' não pôde ser lido: {message}"),
        })
}

pub fn get(root: &Path, store: &CollectionStore, request_id: &str) -> Result<DraftDetail> {
    let summary = summarize(root, store, request_id)?;
    if summary.state == "corrupt" || summary.state == "future" {
        return Ok(DraftDetail {
            error: Some(
                "o registro existe, mas não pôde ser interpretado; o original foi preservado"
                    .into(),
            ),
            summary,
            record: None,
        });
    }
    Ok(DraftDetail {
        record: load(root, request_id)?,
        summary,
        error: None,
    })
}

pub fn list(root: &Path, store: &CollectionStore) -> Result<Vec<DraftDetail>> {
    let directory = drafts_dir(root)?;
    let mut out = Vec::new();
    let mut entries: Vec<_> =
        fs::read_dir(&directory)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if !persist::is_safe_id(id) {
            continue;
        }
        let summary = summarize(root, store, id)?;
        let same_collection = read_record(&path)
            .ok()
            .map(|record| {
                record.collection_id == store.manifest.collection_id().map(str::to_string)
                    || record.collection_id.is_none()
            })
            .unwrap_or(true);
        if !same_collection {
            continue;
        }
        if summary.state == "corrupt" || summary.state == "future" {
            out.push(DraftDetail {
                summary,
                record: None,
                error: Some(
                    "registro ilegível ou de versão futura; o original foi preservado".into(),
                ),
            });
        } else {
            out.push(DraftDetail {
                record: load(root, id)?,
                summary,
                error: None,
            });
        }
    }
    Ok(out)
}

fn summarize(root: &Path, store: &CollectionStore, request_id: &str) -> Result<DraftSummary> {
    let path = draft_path(root, request_id)?;
    if !path.exists() {
        return Ok(DraftSummary {
            request_id: request_id.to_string(),
            generation: 0,
            base_revision: None,
            updated_at: None,
            state: "absent".into(),
            file_exists: store.requests.contains_key(request_id),
            file_revision: store
                .requests
                .get(request_id)
                .map(|entry| entry.revision.clone()),
        });
    }
    let record = match read_record(&path) {
        Ok(record) => record,
        Err(message) => {
            let future = message.contains("mais nova");
            return Ok(DraftSummary {
                request_id: request_id.to_string(),
                generation: 0,
                base_revision: None,
                updated_at: None,
                state: if future { "future" } else { "corrupt" }.into(),
                file_exists: store.requests.contains_key(request_id),
                file_revision: store
                    .requests
                    .get(request_id)
                    .map(|entry| entry.revision.clone()),
            });
        }
    };
    let file_revision = store
        .requests
        .get(request_id)
        .map(|entry| entry.revision.clone());
    let state = match (&file_revision, &record.base_revision) {
        (None, _) => "orphan",
        (Some(current), Some(base)) if current == base => "current",
        (Some(_), Some(_)) => "diverged",
        (Some(_), None) => "current",
    };
    Ok(DraftSummary {
        request_id: record.request_id,
        generation: record.generation,
        base_revision: record.base_revision,
        updated_at: Some(record.updated_at),
        state: state.into(),
        file_exists: file_revision.is_some(),
        file_revision,
    })
}

pub fn save(
    root: &Path,
    collection_id: Option<&str>,
    request_id: &str,
    base_revision: Option<String>,
    expected_generation: Option<u64>,
    request: RequestFile,
) -> Result<DraftRecord> {
    let path = draft_path(root, request_id)?;
    let _lock = persist::CollectionLock::acquire(root)?;
    let existing = if path.exists() {
        Some(read_record(&path).map_err(|message| Error::Conflict {
            code: "draft_corrupt",
            message: format!("rascunho existente não pôde ser lido: {message}"),
        })?)
    } else {
        None
    };
    if let Some(expected) = expected_generation {
        let current = existing
            .as_ref()
            .map(|record| record.generation)
            .unwrap_or(0);
        if expected != current {
            return Err(Error::Conflict {
                code: "draft_generation",
                message: format!(
                    "o rascunho está na geração {current}; recarregue antes de gravar"
                ),
            });
        }
    }
    let now = chrono::Utc::now().to_rfc3339();
    let record = DraftRecord {
        version: DRAFT_VERSION,
        collection_id: collection_id.map(str::to_string),
        request_id: request_id.to_string(),
        base_revision,
        created_at: existing
            .as_ref()
            .map(|record| record.created_at.clone())
            .unwrap_or_else(|| now.clone()),
        updated_at: now,
        generation: existing.map(|record| record.generation).unwrap_or(0) + 1,
        request,
    };
    persist::atomic_write_json(&path, &record)?;
    Ok(record)
}

/// Remove exatamente a geração indicada. Uma gravação atrasada nunca apaga
/// uma geração posterior.
pub fn delete(root: &Path, request_id: &str, expected_generation: Option<u64>) -> Result<bool> {
    let path = draft_path(root, request_id)?;
    let _lock = persist::CollectionLock::acquire(root)?;
    if !path.exists() {
        return Ok(false);
    }
    let record = read_record(&path).map_err(|message| Error::Conflict {
        code: "draft_corrupt",
        message: format!("rascunho não pôde ser lido: {message}"),
    })?;
    if let Some(expected) = expected_generation {
        if expected != record.generation {
            return Err(Error::Conflict {
                code: "draft_generation",
                message: format!(
                    "o rascunho avançou para a geração {}; nada foi descartado",
                    record.generation
                ),
            });
        }
    }
    fs::remove_file(&path)?;
    Ok(true)
}

/// Cria uma nova requisição canônica a partir de um rascunho órfão. O novo
/// ID caminho são próprios; o rascunho só é removido após o sucesso.
pub fn adopt(
    root: &Path,
    store: &mut CollectionStore,
    request_id: &str,
    parent: Option<&str>,
    name: Option<String>,
) -> Result<RequestPayload> {
    let record = load(root, request_id)?
        .ok_or_else(|| Error::NotFound(format!("rascunho de '{request_id}' não encontrado")))?;
    let mut request = record.request.clone();
    if let Some(name) = name {
        if !name.trim().is_empty() {
            request.info.name = name.trim().to_string();
        }
    }
    let method = request.http.method.clone();
    let url = request.http.url.clone();
    let payload = store.create_request(parent, request.info.name.clone(), method, url)?;
    let mut saved = request;
    saved.info.seq = 0;
    let payload = store.save_request(&payload.id, saved, Some(&payload.revision))?;
    delete(root, request_id, Some(record.generation))?;
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::init_collection;

    #[test]
    fn draft_does_not_touch_canonical_file_and_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Rascunhos").unwrap();
        let payload = store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        let path = store.request(&payload.id).unwrap().path.clone();
        let before = fs::read(&path).unwrap();
        let mut request = payload.request.clone();
        request.http.url = "http://127.0.0.1/alterado".into();
        let record = save(
            dir.path(),
            store.manifest.collection_id(),
            &payload.id,
            Some(payload.revision.clone()),
            Some(0),
            request,
        )
        .unwrap();
        assert_eq!(record.generation, 1);
        assert_eq!(fs::read(&path).unwrap(), before);
        let detail = get(dir.path(), &store, &payload.id).unwrap();
        assert_eq!(
            detail.record.unwrap().request.http.url,
            "http://127.0.0.1/alterado"
        );
        assert_eq!(detail.summary.state, "current");
    }

    #[test]
    fn stale_generation_is_rejected_and_saved_generation_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Rascunhos").unwrap();
        let payload = store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        let first = save(
            dir.path(),
            store.manifest.collection_id(),
            &payload.id,
            Some(payload.revision.clone()),
            Some(0),
            payload.request.clone(),
        )
        .unwrap();
        let second = save(
            dir.path(),
            store.manifest.collection_id(),
            &payload.id,
            Some(payload.revision.clone()),
            Some(first.generation),
            payload.request.clone(),
        )
        .unwrap();
        let error = delete(dir.path(), &payload.id, Some(first.generation)).unwrap_err();
        assert_eq!(error.info().code, "draft_generation");
        assert!(delete(dir.path(), &payload.id, Some(second.generation)).unwrap());
        assert!(
            get(dir.path(), &store, &payload.id)
                .unwrap()
                .record
                .is_none()
        );
    }

    #[test]
    fn drafts_are_isolated_by_collection_and_orphans_remain() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let mut store_a = init_collection(first.path(), "A").unwrap();
        let store_b = init_collection(second.path(), "B").unwrap();
        let payload = store_a
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        save(
            first.path(),
            store_a.manifest.collection_id(),
            &payload.id,
            Some(payload.revision.clone()),
            Some(0),
            payload.request.clone(),
        )
        .unwrap();
        assert!(
            list(first.path(), &store_a)
                .unwrap()
                .iter()
                .any(|draft| draft.summary.request_id == payload.id)
        );
        assert!(list(second.path(), &store_b).unwrap().is_empty());
        store_a.delete_request(&payload.id).unwrap();
        let detail = get(first.path(), &store_a, &payload.id).unwrap();
        assert_eq!(detail.summary.state, "orphan");
        assert!(detail.record.is_some());
    }

    #[test]
    fn external_change_marks_draft_diverged_without_losing_content() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Rascunhos").unwrap();
        let payload = store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        let path = store.request(&payload.id).unwrap().path.clone();
        let mut request = payload.request.clone();
        request.http.url = "http://127.0.0.1/rascunho".into();
        save(
            dir.path(),
            store.manifest.collection_id(),
            &payload.id,
            Some(payload.revision.clone()),
            Some(0),
            request,
        )
        .unwrap();
        let mut external: RequestFile = serde_yaml::from_slice(&fs::read(&path).unwrap()).unwrap();
        external.http.url = "http://127.0.0.1/externo".into();
        fs::write(&path, serde_yaml::to_string(&external).unwrap()).unwrap();
        store = crate::store::CollectionStore::open(dir.path()).unwrap();
        let detail = get(dir.path(), &store, &payload.id).unwrap();
        assert_eq!(detail.summary.state, "diverged");
        assert_eq!(
            detail.record.unwrap().request.http.url,
            "http://127.0.0.1/rascunho"
        );
    }

    #[test]
    fn corrupt_and_future_records_are_isolated_without_deleting_originals() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Rascunhos").unwrap();
        let payload = store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        let directory = drafts_dir(dir.path()).unwrap();
        let path = directory.join(format!("{}.json", payload.id));
        fs::write(&path, b"{ nao e json").unwrap();
        let corrupt = get(dir.path(), &store, &payload.id).unwrap();
        assert_eq!(corrupt.summary.state, "corrupt");
        assert!(corrupt.record.is_none());
        assert!(path.exists());

        let future = serde_json::json!({
            "version": DRAFT_VERSION + 1,
            "collection_id": store.manifest.collection_id(),
            "request_id": payload.id,
            "base_revision": payload.revision,
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "generation": 1,
            "request": payload.request,
        });
        fs::write(&path, serde_json::to_vec(&future).unwrap()).unwrap();
        let future = get(dir.path(), &store, &payload.id).unwrap();
        assert_eq!(future.summary.state, "future");
        assert!(future.record.is_none());
        assert!(path.exists());
        assert_eq!(list(dir.path(), &store).unwrap().len(), 1);
    }

    #[test]
    fn orphan_is_adopted_with_new_id_and_old_draft_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Rascunhos").unwrap();
        let payload = store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        let mut request = payload.request.clone();
        request.http.method = "POST".into();
        request.http.body = Some(crate::model::Body::Json {
            data: "{\"ok\":true}".into(),
        });
        save(
            dir.path(),
            store.manifest.collection_id(),
            &payload.id,
            Some(payload.revision.clone()),
            Some(0),
            request,
        )
        .unwrap();
        store.delete_request(&payload.id).unwrap();
        let adopted = adopt(
            dir.path(),
            &mut store,
            &payload.id,
            None,
            Some("Recuperada".into()),
        )
        .unwrap();
        assert_ne!(adopted.id, payload.id);
        assert_eq!(adopted.request.info.name, "Recuperada");
        assert_eq!(adopted.request.http.method, "POST");
        assert!(
            get(dir.path(), &store, &payload.id)
                .unwrap()
                .record
                .is_none()
        );
        assert!(store.request(&adopted.id).is_ok());
    }

    #[test]
    fn discarded_generation_is_not_resurrected_by_a_late_write() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Rascunhos").unwrap();
        let payload = store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        let first = save(
            dir.path(),
            store.manifest.collection_id(),
            &payload.id,
            Some(payload.revision.clone()),
            Some(0),
            payload.request.clone(),
        )
        .unwrap();
        assert!(delete(dir.path(), &payload.id, Some(first.generation)).unwrap());
        let error = save(
            dir.path(),
            store.manifest.collection_id(),
            &payload.id,
            Some(payload.revision.clone()),
            Some(first.generation),
            payload.request.clone(),
        )
        .unwrap_err();
        assert_eq!(error.info().code, "draft_generation");
        assert!(
            get(dir.path(), &store, &payload.id)
                .unwrap()
                .record
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn draft_files_are_private_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Rascunhos").unwrap();
        let payload = store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        save(
            dir.path(),
            store.manifest.collection_id(),
            &payload.id,
            Some(payload.revision.clone()),
            Some(0),
            payload.request.clone(),
        )
        .unwrap();
        let path = drafts_dir(dir.path())
            .unwrap()
            .join(format!("{}.json", payload.id));
        let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn draft_never_contains_a_resolved_provider_secret() {
        use crate::model::{Header, SecretProvider, SecretRef, Variable};

        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Rascunhos").unwrap();
        let key = format!("REQLIVO_DRAFT_SECRET_{}", std::process::id());
        let value = "segredo-sintetico-nao-persistir";
        unsafe { std::env::set_var(&key, value) };
        store
            .upsert_environment(
                "dev",
                vec![
                    Variable::plain("baseUrl", "http://127.0.0.1"),
                    Variable::secret("token"),
                ],
                vec![SecretRef {
                    name: "token".into(),
                    environment: Some("dev".into()),
                    provider: SecretProvider::Env,
                    key: key.clone(),
                    description: None,
                    extra: Default::default(),
                }],
                None,
            )
            .unwrap();
        let payload = store
            .create_request(None, "Segredo", "GET", "{{baseUrl}}/echo")
            .unwrap();
        let mut request = payload.request.clone();
        request.http.headers.push(Header {
            name: "Authorization".into(),
            value: "Bearer {{token}}".into(),
            description: None,
            disabled: false,
            extra: Default::default(),
        });
        save(
            dir.path(),
            store.manifest.collection_id(),
            &payload.id,
            Some(payload.revision.clone()),
            Some(0),
            request,
        )
        .unwrap();
        let path = drafts_dir(dir.path())
            .unwrap()
            .join(format!("{}.json", payload.id));
        let bytes = fs::read_to_string(path).unwrap();
        assert!(!bytes.contains(value));
        assert!(bytes.contains("{{token}}"));
        unsafe { std::env::remove_var(&key) };
    }
}
