//! Lixeira recuperável com índice versionado e suporte a entradas legadas.

use crate::error::{Error, Result};
use crate::model::{FolderFile, RequestFile};
use crate::persist;
use crate::store::{CollectionStore, FOLDER_FILE};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const TRASH_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashEntry {
    pub version: u32,
    pub id: String,
    pub item_id: Option<String>,
    pub kind: String,
    pub origin: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub deleted_at: String,
    pub content_hash: String,
    pub state: String,
    pub payload: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashEntryView {
    pub id: String,
    pub item_id: Option<String>,
    pub kind: String,
    pub name: String,
    pub origin: Option<String>,
    pub parent_id: Option<String>,
    pub deleted_at: Option<String>,
    pub state: String,
    pub legacy: bool,
    pub revision: Option<String>,
    /// Revisão do conteúdo guardado; precondição de restauração.
    pub entry_revision: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOutcome {
    pub entry_id: String,
    pub item_id: String,
    pub kind: String,
    pub path: String,
    pub already_restored: bool,
}

fn trash_dir(root: &Path) -> Result<PathBuf> {
    persist::ensure_internal_dir(root, "trash")
}

fn entry_dir(root: &Path, entry_id: &str) -> Result<PathBuf> {
    if !persist::is_safe_id(entry_id) {
        return Err(Error::Validation(
            "identificador de lixeira inválido".into(),
        ));
    }
    Ok(trash_dir(root)?.join(entry_id))
}

/// Resolve uma entrada por nome. Aceita nomes legados (v0.2.0) que não são IDs
/// opacos, mas apenas um único componente dentro de `.reqlivo/trash/`.
fn entry_path(root: &Path, entry_id: &str) -> Result<PathBuf> {
    if entry_id.is_empty()
        || entry_id == "."
        || entry_id == ".."
        || entry_id.contains('/')
        || entry_id.contains('\\')
        || Path::new(entry_id).components().count() != 1
    {
        return Err(Error::Validation(
            "identificador de lixeira inválido".into(),
        ));
    }
    Ok(trash_dir(root)?.join(entry_id))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RestoreJournal {
    version: u32,
    destination: String,
}

fn hash_tree(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(Error::Validation("symlink não é aceito na lixeira".into()));
    }
    if metadata.is_file() {
        return persist::hash_file(path);
    }
    let mut files = BTreeMap::new();
    collect_tree(path, path, &mut files)?;
    Ok(persist::fingerprint(&files))
}

fn collect_tree(root: &Path, dir: &Path, files: &mut BTreeMap<String, String>) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(Error::Validation(
                "symlink não é aceito no conteúdo da lixeira".into(),
            ));
        }
        if metadata.is_dir() {
            collect_tree(root, &path, files)?;
        } else {
            files.insert(persist::relative(root, &path), persist::hash_file(&path)?);
        }
    }
    Ok(())
}

fn read_entry(path: &Path) -> std::result::Result<TrashEntry, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let entry: TrashEntry =
        serde_json::from_slice(&bytes).map_err(|error| format!("entrada ilegível: {error}"))?;
    if entry.version > TRASH_VERSION {
        return Err(format!(
            "versão da lixeira {} é mais nova que a suportada ({TRASH_VERSION})",
            entry.version
        ));
    }
    if entry.version != TRASH_VERSION {
        return Err(format!("versão da lixeira desconhecida: {}", entry.version));
    }
    if !persist::is_safe_id(&entry.id) {
        return Err("entrada com identificador inválido".into());
    }
    Ok(entry)
}

/// Reconcilia exclusões interrompidas sem perder conteúdo.
pub fn reconcile(root: &Path) -> Result<()> {
    let directory = trash_dir(root)?;
    for entry in fs::read_dir(&directory)?.collect::<std::result::Result<Vec<_>, _>>()? {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let metadata_file = path.join("entry.json");
        if !metadata_file.exists() {
            continue;
        }
        let Ok(mut record) = read_entry(&metadata_file) else {
            continue;
        };
        let journal_file = path.join("restore.json");
        if record.state == "restoring" {
            let destination = fs::read(&journal_file)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<RestoreJournal>(&bytes).ok())
                .map(|journal| journal.destination);
            let payload = path.join(&record.payload);
            if payload.exists() {
                // A cópia para o projeto ainda não começou.
                record.state = "trashed".into();
                let _ = fs::remove_file(&journal_file);
                persist::atomic_write_json(&metadata_file, &record)?;
                continue;
            }
            if let Some(destination) = destination {
                let destination =
                    persist::safe_path(root, &destination).map_err(|_| Error::Conflict {
                        code: "backup_invalid",
                        message: "diário de restauração aponta para fora da coleção".into(),
                    })?;
                if destination.exists()
                    && hash_tree(&destination).ok().as_deref() == Some(record.content_hash.as_str())
                {
                    record.state = "restored".into();
                    let _ = fs::remove_file(&journal_file);
                    persist::atomic_write_json(&metadata_file, &record)?;
                    continue;
                }
            }
            // Sem conteúdo nem destino verificável: mantém para reparação manual.
            continue;
        }
        if record.state != "preparing" {
            continue;
        }
        let payload = path.join(&record.payload);
        if payload.exists() {
            record.content_hash = hash_tree(&payload)?;
            record.state = "trashed".into();
            persist::atomic_write_json(&metadata_file, &record)?;
        } else {
            // O movimento não chegou a acontecer: o item continua no projeto.
            let _ = fs::remove_dir_all(&path);
        }
    }
    Ok(())
}

pub fn list(root: &Path, store: &CollectionStore) -> Result<Vec<TrashEntryView>> {
    reconcile(root)?;
    let directory = trash_dir(root)?;
    let mut out = Vec::new();
    let mut entries: Vec<_> =
        fs::read_dir(&directory)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata_file = path.join("entry.json");
        if path.is_dir() && metadata_file.exists() {
            match read_entry(&metadata_file) {
                Ok(record) => {
                    if record.state == "restored" {
                        continue;
                    }
                    let revision = revision_of(store, &record.item_id);
                    let restoring = record.state == "restoring";
                    out.push(TrashEntryView {
                        id: record.id,
                        item_id: record.item_id,
                        kind: record.kind,
                        name: record.name,
                        origin: Some(record.origin),
                        parent_id: record.parent_id,
                        deleted_at: Some(record.deleted_at),
                        state: record.state,
                        legacy: false,
                        revision,
                        entry_revision: Some(record.content_hash),
                        error: restoring.then(|| {
                            "restauração interrompida; conteúdo e destino serão conferidos novamente"
                                .into()
                        }),
                    });
                }
                Err(message) => out.push(TrashEntryView {
                    id: path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("desconhecido")
                        .to_string(),
                    item_id: None,
                    kind: "unknown".into(),
                    name: "entrada ilegível".into(),
                    origin: None,
                    parent_id: None,
                    deleted_at: None,
                    state: "corrupt".into(),
                    legacy: false,
                    revision: None,
                    entry_revision: None,
                    error: Some(message),
                }),
            }
            continue;
        }
        // Entradas da v0.2.0: arquivo ou diretório direto em .reqlivo/trash/.
        let legacy = read_legacy(root, &path);
        out.push(legacy);
    }
    Ok(out)
}

fn read_legacy(root: &Path, path: &Path) -> TrashEntryView {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("entrada")
        .to_string();
    let kind = if path.is_dir() { "folder" } else { "request" };
    let (item_id, name, origin, error) = match kind {
        "request" => match fs::read(path)
            .ok()
            .and_then(|bytes| serde_yaml::from_slice::<RequestFile>(&bytes).ok())
        {
            Some(request) => (
                request.reqlivo_id().map(str::to_string),
                request.info.name,
                None,
                None,
            ),
            None => (
                None,
                name.clone(),
                None,
                Some("conteúdo legado ilegível; recuperação manual necessária".into()),
            ),
        },
        _ => {
            let folder_file = [path.join(FOLDER_FILE), path.join("folder.yaml")]
                .into_iter()
                .find(|candidate| candidate.exists());
            let folder = folder_file
                .and_then(|file| fs::read(file).ok())
                .and_then(|bytes| serde_yaml::from_slice::<FolderFile>(&bytes).ok());
            match folder {
                Some(folder) => (
                    folder.reqlivo_id().map(str::to_string),
                    folder.info.name,
                    None,
                    None,
                ),
                None => (
                    None,
                    name.clone(),
                    None,
                    Some("pasta legada ilegível; recuperação manual necessária".into()),
                ),
            }
        }
    };
    let revision = persist::hash_file(path)
        .ok()
        .or_else(|| hash_tree(path).ok());
    let _ = root;
    TrashEntryView {
        id: name.clone(),
        item_id,
        kind: kind.into(),
        name,
        origin,
        parent_id: None,
        deleted_at: None,
        state: "legacy".into(),
        legacy: true,
        revision: revision.clone(),
        entry_revision: revision,
        error,
    }
}

fn revision_of(store: &CollectionStore, item_id: &Option<String>) -> Option<String> {
    let id = item_id.as_deref()?;
    store
        .requests
        .get(id)
        .map(|entry| entry.revision.clone())
        .or_else(|| store.folders.get(id).map(|entry| entry.revision.clone()))
}

pub fn delete_request(root: &Path, store: &CollectionStore, id: &str) -> Result<String> {
    delete_request_with(root, store, id, None)
}

pub fn delete_request_with(
    root: &Path,
    store: &CollectionStore,
    id: &str,
    expected_revision: Option<&str>,
) -> Result<String> {
    let entry = store.request(id)?.clone();
    move_to_trash(
        root,
        "request",
        &entry.path,
        entry.id.clone(),
        entry.parent.clone(),
        entry.name.clone(),
        &entry.rel_path,
        expected_revision,
    )
}

pub fn delete_folder(root: &Path, store: &CollectionStore, id: &str) -> Result<String> {
    delete_folder_with(root, store, id, None)
}

pub fn delete_folder_with(
    root: &Path,
    store: &CollectionStore,
    id: &str,
    expected_revision: Option<&str>,
) -> Result<String> {
    let entry = store.folder(id)?.clone();
    if store
        .folders
        .values()
        .any(|folder| folder.parent.as_deref() == Some(id))
        || store
            .requests
            .values()
            .any(|request| request.parent.as_deref() == Some(id))
    {
        return Err(Error::Validation(
            "a pasta não está vazia; mova ou apague os itens primeiro".into(),
        ));
    }
    move_to_trash(
        root,
        "folder",
        &entry.dir,
        entry.id.clone(),
        entry.parent.clone(),
        entry.name.clone(),
        &entry.rel_dir,
        expected_revision,
    )
}

fn move_to_trash(
    root: &Path,
    kind: &str,
    source: &Path,
    item_id: String,
    parent_id: Option<String>,
    name: String,
    origin: &str,
    expected_revision: Option<&str>,
) -> Result<String> {
    let _lock = persist::CollectionLock::acquire(root)?;
    reconcile(root)?;
    persist::ensure_revision(source, expected_revision)?;
    let entry_id = format!("trash_{}", crate::store::short_uuid());
    let directory = entry_dir(root, &entry_id)?;
    fs::create_dir_all(&directory)?;
    let mut record = TrashEntry {
        version: TRASH_VERSION,
        id: entry_id.clone(),
        item_id: Some(item_id),
        kind: kind.to_string(),
        origin: origin.to_string(),
        parent_id,
        name,
        deleted_at: chrono::Utc::now().to_rfc3339(),
        content_hash: String::new(),
        state: "preparing".into(),
        payload: "payload".into(),
    };
    persist::atomic_write_json(&directory.join("entry.json"), &record)?;
    let payload = directory.join("payload");
    if let Err(error) = fs::rename(source, &payload) {
        let _ = fs::remove_dir_all(&directory);
        return Err(error.into());
    }
    record.content_hash = hash_tree(&payload)?;
    record.state = "trashed".into();
    persist::atomic_write_json(&directory.join("entry.json"), &record)?;
    Ok(entry_id)
}

#[derive(Debug, Default)]
pub struct RestoreOptions {
    pub destination_parent_id: Option<String>,
    pub name: Option<String>,
    pub expected_revision: Option<String>,
}

fn ensure_entry_revision(expected: Option<&str>, actual: Option<&str>) -> Result<()> {
    if let Some(expected) = expected {
        if actual != Some(expected) {
            return Err(Error::Conflict {
                code: "revision_conflict",
                message:
                    "a entrada da lixeira mudou desde a listagem; recarregue antes de restaurar"
                        .into(),
            });
        }
    }
    Ok(())
}

pub fn restore(
    root: &Path,
    store: &mut CollectionStore,
    entry_id: &str,
    options: RestoreOptions,
) -> Result<RestoreOutcome> {
    let _lock = persist::CollectionLock::acquire(root)?;
    reconcile(root)?;
    let directory = entry_path(root, entry_id)?;
    let metadata_file = directory.join("entry.json");
    if !metadata_file.exists() {
        let entry = read_legacy(root, &directory);
        ensure_entry_revision(
            options.expected_revision.as_deref(),
            entry.entry_revision.as_deref(),
        )?;
        return restore_legacy(root, store, entry_id, &directory, options);
    }
    let mut record = read_entry(&metadata_file).map_err(|message| Error::Conflict {
        code: "backup_invalid",
        message: format!("entrada da lixeira inválida: {message}"),
    })?;
    ensure_entry_revision(
        options.expected_revision.as_deref(),
        Some(record.content_hash.as_str()),
    )?;
    if record.state == "restored" {
        return Err(Error::Conflict {
            code: "restore_conflict",
            message: "a entrada já foi restaurada; recarregue a lixeira".into(),
        });
    }
    if record.state == "restoring" {
        return Err(Error::Conflict {
            code: "restore_conflict",
            message: "há uma restauração interrompida; reabra a coleção para reconciliar ou recupere manualmente"
                .into(),
        });
    }
    if record.state != "trashed" {
        return Err(Error::Conflict {
            code: "restore_conflict",
            message: format!(
                "entrada em estado '{}' não pode ser restaurada",
                record.state
            ),
        });
    }
    let payload = directory.join(&record.payload);
    let payload_metadata = fs::symlink_metadata(&payload).map_err(|error| Error::Conflict {
        code: "backup_invalid",
        message: format!("conteúdo da lixeira ausente: {error}"),
    })?;
    if payload_metadata.file_type().is_symlink() {
        return Err(Error::Conflict {
            code: "backup_invalid",
            message: "o conteúdo da lixeira é um symlink; restauração recusada".into(),
        });
    }
    let actual = hash_tree(&payload)?;
    if actual != record.content_hash {
        return Err(Error::Conflict {
            code: "backup_invalid",
            message: "o hash do conteúdo da lixeira não confere; nada foi restaurado".into(),
        });
    }
    let item_id = record.item_id.clone().ok_or_else(|| Error::Conflict {
        code: "backup_invalid",
        message: "entrada sem identidade lógica".into(),
    })?;
    if store.all_ids().contains(&item_id) {
        return Err(Error::Conflict {
            code: "duplicate_id",
            message: format!("o ID '{item_id}' já está ativo na coleção"),
        });
    }
    let parent = resolve_parent(
        store,
        record.parent_id.as_deref(),
        options.destination_parent_id.as_deref(),
    )?;
    let destination = destination_path(
        root,
        store,
        &record.kind,
        parent.as_deref(),
        &record.name,
        &record.origin,
        options.name.as_deref(),
    )?;
    if destination.exists() {
        return Err(Error::Conflict {
            code: "destination_exists",
            message: format!(
                "o destino '{}' já existe; escolha outro nome ou pasta",
                persist::relative(root, &destination)
            ),
        });
    }
    // Diário antes do movimento: uma interrupção entre rename e atualização do
    // índice pode ser reconciliada na próxima abertura.
    let journal = RestoreJournal {
        version: TRASH_VERSION,
        destination: persist::relative(root, &destination),
    };
    persist::atomic_write_json(&directory.join("restore.json"), &journal)?;
    record.state = "restoring".into();
    persist::atomic_write_json(&metadata_file, &record)?;
    if let Err(error) = fs::rename(&payload, &destination) {
        let _ = fs::remove_file(directory.join("restore.json"));
        record.state = "trashed".into();
        persist::atomic_write_json(&metadata_file, &record)?;
        return Err(error.into());
    }
    record.state = "restored".into();
    persist::atomic_write_json(&metadata_file, &record)?;
    let _ = fs::remove_file(directory.join("restore.json"));
    *store = CollectionStore::open(root)?;
    let path = persist::relative(root, &destination);
    Ok(RestoreOutcome {
        entry_id: entry_id.to_string(),
        item_id,
        kind: record.kind,
        path,
        already_restored: false,
    })
}

fn restore_legacy(
    root: &Path,
    store: &mut CollectionStore,
    entry_id: &str,
    path: &Path,
    options: RestoreOptions,
) -> Result<RestoreOutcome> {
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| Error::NotFound(format!("entrada de lixeira '{entry_id}' não encontrada")))?;
    if metadata.file_type().is_symlink() {
        return Err(Error::Conflict {
            code: "backup_invalid",
            message: "entrada legada é um symlink; restauração recusada".into(),
        });
    }
    let kind = if metadata.is_dir() {
        "folder"
    } else {
        "request"
    };
    let (item_id, name) = if kind == "request" {
        let request: RequestFile = serde_yaml::from_slice(&fs::read(&path)?)?;
        let id = request
            .reqlivo_id()
            .map(str::to_string)
            .ok_or_else(|| Error::Conflict {
                code: "backup_invalid",
                message: "entrada legada sem ID; recuperação manual necessária".into(),
            })?;
        (id, request.info.name)
    } else {
        hash_tree(&path)?;
        let folder_file = [path.join(FOLDER_FILE), path.join("folder.yaml")]
            .into_iter()
            .find(|candidate| candidate.exists())
            .ok_or_else(|| Error::Conflict {
                code: "backup_invalid",
                message: "pasta legada sem folder.yml; recuperação manual necessária".into(),
            })?;
        let folder: FolderFile = serde_yaml::from_slice(&fs::read(folder_file)?)?;
        let id = folder
            .reqlivo_id()
            .map(str::to_string)
            .ok_or_else(|| Error::Conflict {
                code: "backup_invalid",
                message: "pasta legada sem ID; recuperação manual necessária".into(),
            })?;
        (id, folder.info.name)
    };
    if store.all_ids().contains(&item_id) {
        return Err(Error::Conflict {
            code: "duplicate_id",
            message: format!("o ID '{item_id}' já está ativo na coleção"),
        });
    }
    let parent = resolve_parent(store, None, options.destination_parent_id.as_deref())?;
    let name = options.name.clone().unwrap_or(name);
    let destination =
        destination_path(root, store, kind, parent.as_deref(), &name, "", Some(&name))?;
    if destination.exists() {
        return Err(Error::Conflict {
            code: "destination_exists",
            message: "o destino já existe; escolha outro nome ou pasta".into(),
        });
    }
    fs::rename(&path, &destination)?;
    *store = CollectionStore::open(root)?;
    Ok(RestoreOutcome {
        entry_id: entry_id.to_string(),
        item_id,
        kind: kind.into(),
        path: persist::relative(root, &destination),
        already_restored: false,
    })
}

fn resolve_parent(
    store: &CollectionStore,
    original: Option<&str>,
    requested: Option<&str>,
) -> Result<Option<String>> {
    if let Some(requested) = requested {
        store.folder(requested).map_err(|_| Error::Conflict {
            code: "destination_not_found",
            message: format!("a pasta de destino '{requested}' não existe"),
        })?;
        return Ok(Some(requested.to_string()));
    }
    if let Some(original) = original {
        if store.folders.contains_key(original) {
            return Ok(Some(original.to_string()));
        }
        return Err(Error::Conflict {
            code: "parent_missing",
            message: "a pasta original não existe mais; escolha uma pasta de destino".into(),
        });
    }
    Ok(None)
}

fn destination_path(
    root: &Path,
    store: &CollectionStore,
    kind: &str,
    parent: Option<&str>,
    fallback_name: &str,
    origin: &str,
    requested_name: Option<&str>,
) -> Result<PathBuf> {
    let parent_dir = parent
        .and_then(|id| store.folders.get(id))
        .map(|folder| folder.dir.clone())
        .unwrap_or_else(|| root.to_path_buf());
    if requested_name.is_none() && !origin.is_empty() {
        let original = persist::safe_path(root, origin)?;
        let parent_matches = original
            .parent()
            .map(|value| value == parent_dir)
            .unwrap_or(false);
        if parent_matches && !original.exists() {
            return Ok(original);
        }
    }
    let name = requested_name.unwrap_or(fallback_name);
    if name.trim().is_empty() {
        return Err(Error::Validation("informe um nome para restaurar".into()));
    }
    let candidate = if kind == "request" {
        let base = name
            .strip_suffix(".yml")
            .or_else(|| name.strip_suffix(".yaml"))
            .unwrap_or(name);
        parent_dir.join(format!("{}.yml", crate::store::slugify(base)))
    } else {
        parent_dir.join(crate::store::slugify(name))
    };
    persist::safe_path(root, &persist::relative(root, &candidate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::init_collection;

    #[test]
    fn request_is_restored_with_same_bytes_and_id() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Lixeira").unwrap();
        let payload = store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        let original = fs::read(store.request(&payload.id).unwrap().path.clone()).unwrap();
        delete_request(dir.path(), &store, &payload.id).unwrap();
        store = CollectionStore::open(dir.path()).unwrap();
        assert!(store.request(&payload.id).is_err());
        let entries = list(dir.path(), &store).unwrap();
        assert_eq!(entries.len(), 1);
        let outcome = restore(
            dir.path(),
            &mut store,
            &entries[0].id,
            RestoreOptions::default(),
        )
        .unwrap();
        assert_eq!(outcome.item_id, payload.id);
        let restored = store.request(&payload.id).unwrap();
        assert_eq!(fs::read(&restored.path).unwrap(), original);
    }

    #[test]
    fn empty_folder_is_restored_and_non_empty_folder_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Lixeira").unwrap();
        let folder = store.create_folder(None, "Vazia").unwrap();
        store
            .create_request(Some(&folder), "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        let error = delete_folder(dir.path(), &store, &folder).unwrap_err();
        assert!(error.to_string().contains("não está vazia"));
    }

    #[test]
    fn occupied_destination_does_not_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Lixeira").unwrap();
        let payload = store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        delete_request(dir.path(), &store, &payload.id).unwrap();
        store = CollectionStore::open(dir.path()).unwrap();
        let other = store
            .create_request(None, "Outro", "GET", "http://127.0.0.1/outro")
            .unwrap();
        fs::rename(dir.path().join(&other.path), dir.path().join("echo.yml")).unwrap();
        store = CollectionStore::open(dir.path()).unwrap();
        let entries = list(dir.path(), &store).unwrap();
        let error = restore(
            dir.path(),
            &mut store,
            &entries[0].id,
            RestoreOptions {
                destination_parent_id: None,
                name: Some("Echo".into()),
                expected_revision: None,
            },
        )
        .unwrap_err();
        assert_eq!(error.info().code, "destination_exists");
    }

    #[test]
    fn empty_folder_roundtrips_with_same_id_and_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Lixeira").unwrap();
        let folder_id = store.create_folder(None, "Vazia").unwrap();
        let before = {
            let folder = store.folder(&folder_id).unwrap();
            fs::read(folder.dir.join(FOLDER_FILE)).unwrap()
        };
        delete_folder(dir.path(), &store, &folder_id).unwrap();
        store = CollectionStore::open(dir.path()).unwrap();
        assert!(store.folder(&folder_id).is_err());
        let entries = list(dir.path(), &store).unwrap();
        assert_eq!(entries.len(), 1);
        let outcome = restore(
            dir.path(),
            &mut store,
            &entries[0].id,
            RestoreOptions::default(),
        )
        .unwrap();
        assert_eq!(outcome.item_id, folder_id);
        let restored = store.folder(&folder_id).unwrap();
        assert_eq!(fs::read(restored.dir.join(FOLDER_FILE)).unwrap(), before);
    }

    #[test]
    fn tampered_and_symlinked_payloads_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Lixeira").unwrap();
        let payload = store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        delete_request(dir.path(), &store, &payload.id).unwrap();
        store = CollectionStore::open(dir.path()).unwrap();
        let entries = list(dir.path(), &store).unwrap();
        let entry = &entries[0];
        let directory = entry_dir(dir.path(), &entry.id).unwrap();
        let stored = directory.join("payload");
        let mut bytes = fs::read(&stored).unwrap();
        bytes.push(b'\n');
        fs::write(&stored, bytes).unwrap();
        let error =
            restore(dir.path(), &mut store, &entry.id, RestoreOptions::default()).unwrap_err();
        assert_eq!(error.info().code, "backup_invalid");

        let outside = tempfile::tempdir().unwrap();
        let external = outside.path().join("fora.yml");
        fs::write(&external, b"id: fora").unwrap();
        fs::remove_file(&stored).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&external, &stored).unwrap();
            let error =
                restore(dir.path(), &mut store, &entry.id, RestoreOptions::default()).unwrap_err();
            let code = error.info().code;
            assert!(code == "backup_invalid" || code == "validation");
            assert_eq!(fs::read(&external).unwrap(), b"id: fora");
        }
    }

    #[test]
    fn legacy_entry_can_be_restored_with_a_free_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Lixeira").unwrap();
        let payload = store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        let path = store.request(&payload.id).unwrap().path.clone();
        let bytes = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        store = CollectionStore::open(dir.path()).unwrap();
        let legacy = trash_dir(dir.path()).unwrap().join("echo-legado.yml");
        fs::write(&legacy, &bytes).unwrap();
        let outcome = restore(
            dir.path(),
            &mut store,
            "echo-legado.yml",
            RestoreOptions {
                destination_parent_id: None,
                name: Some("Recuperada".into()),
                expected_revision: None,
            },
        )
        .unwrap();
        assert_eq!(outcome.item_id, payload.id);
        assert!(!legacy.exists());
        assert_eq!(fs::read(dir.path().join(&outcome.path)).unwrap(), bytes);
        assert!(store.request(&payload.id).is_ok());
    }

    #[test]
    fn duplicate_active_id_blocks_restore_and_keeps_entry() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Lixeira").unwrap();
        let payload = store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        let original = fs::read(store.request(&payload.id).unwrap().path.clone()).unwrap();
        delete_request(dir.path(), &store, &payload.id).unwrap();
        fs::write(dir.path().join("duplicada.yml"), &original).unwrap();
        store = CollectionStore::open(dir.path()).unwrap();
        let entries = list(dir.path(), &store).unwrap();
        let error = restore(
            dir.path(),
            &mut store,
            &entries[0].id,
            RestoreOptions::default(),
        )
        .unwrap_err();
        assert_eq!(error.info().code, "duplicate_id");
        assert_eq!(list(dir.path(), &store).unwrap().len(), 1);
    }

    #[test]
    fn interrupted_restore_is_reconciled_without_losing_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Lixeira").unwrap();
        let payload = store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        let original = fs::read(store.request(&payload.id).unwrap().path.clone()).unwrap();
        delete_request(dir.path(), &store, &payload.id).unwrap();
        let entries = list(dir.path(), &store).unwrap();
        let entry = &entries[0];
        let directory = entry_dir(dir.path(), &entry.id).unwrap();
        let destination = dir.path().join("recuperada.yml");
        let journal = RestoreJournal {
            version: TRASH_VERSION,
            destination: persist::relative(dir.path(), &destination),
        };
        persist::atomic_write_json(&directory.join("restore.json"), &journal).unwrap();
        let mut record = read_entry(&directory.join("entry.json")).unwrap();
        record.state = "restoring".into();
        persist::atomic_write_json(&directory.join("entry.json"), &record).unwrap();
        fs::rename(directory.join("payload"), &destination).unwrap();

        reconcile(dir.path()).unwrap();
        let record = read_entry(&directory.join("entry.json")).unwrap();
        assert_eq!(record.state, "restored");
        assert!(!directory.join("restore.json").exists());
        assert_eq!(fs::read(&destination).unwrap(), original);
    }

    #[test]
    fn second_restore_of_same_entry_reports_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Lixeira").unwrap();
        let payload = store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        delete_request(dir.path(), &store, &payload.id).unwrap();
        store = CollectionStore::open(dir.path()).unwrap();
        let entries = list(dir.path(), &store).unwrap();
        restore(
            dir.path(),
            &mut store,
            &entries[0].id,
            RestoreOptions::default(),
        )
        .unwrap();
        let error = restore(
            dir.path(),
            &mut store,
            &entries[0].id,
            RestoreOptions::default(),
        )
        .unwrap_err();
        assert_eq!(error.info().code, "restore_conflict");
        assert!(store.request(&payload.id).is_ok());
    }
}
