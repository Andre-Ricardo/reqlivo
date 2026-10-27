//! Persistência interna compartilhada: hashes, varredura, escrita atômica,
//! confinamento de caminhos e coordenação de escritores.
//!
//! Nada aqui abre rede ou executa HTTP/scripts. Os formatos internos
//! (`.reqlivo/`) têm versões próprias e nunca substituem a fonte de verdade.

use crate::error::{Error, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const INTERNAL_DIR: &str = ".reqlivo";

pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn hash_file(path: &Path) -> Result<String> {
    Ok(hash_bytes(&fs::read(path)?))
}

/// Relativo à raiz, sempre com `/`, sem prefixo acidental.
pub fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Lista os arquivos que participam do modelo (manifesto, pastas e
/// requisições), ignorando estado interno, Git e temporários.
pub fn scan_tree(root: &Path) -> Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    scan_directory(root, root, &mut files)?;
    Ok(files)
}

fn scan_directory(root: &Path, dir: &Path, files: &mut BTreeMap<String, String>) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "target" || name.ends_with('~') {
            continue;
        }
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            // Um symlink para fora da coleção nunca entra na varredura.
            if fs::canonicalize(&path)
                .ok()
                .and_then(|target| {
                    target
                        .starts_with(fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()))
                        .then_some(())
                })
                .is_none()
            {
                continue;
            }
        }
        if file_type.is_dir() {
            scan_directory(root, &path, files)?;
        } else if is_collection_yaml(&path) {
            files.insert(relative(root, &path), hash_file(&path)?);
        }
    }
    Ok(())
}

fn is_collection_yaml(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("yml") | Some("yaml")
    )
}

pub fn fingerprint(files: &BTreeMap<String, String>) -> String {
    let mut hasher = Sha256::new();
    for (path, revision) in files {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(revision.as_bytes());
        hasher.update([b'\n']);
    }
    hex::encode(hasher.finalize())
}

pub fn changed_paths(
    previous: &BTreeMap<String, String>,
    current: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut paths: Vec<String> = previous
        .keys()
        .chain(current.keys())
        .filter(|path| previous.get(*path) != current.get(*path))
        .cloned()
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

/// Aceita apenas identificadores opacos, nunca caminhos.
pub fn is_safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

/// Resolve um caminho relativo dentro da coleção e recusa travessias,
/// caminhos absolutos, `.reqlivo/` e `.git/`, inclusive por symlink.
pub fn safe_path(root: &Path, relative_path: &str) -> Result<PathBuf> {
    if relative_path.trim().is_empty() {
        return Err(Error::Validation("caminho relativo vazio".into()));
    }
    let candidate = Path::new(relative_path);
    if candidate.is_absolute() {
        return Err(Error::Validation("caminho absoluto não é permitido".into()));
    }
    let mut first = None;
    for component in candidate.components() {
        match component {
            Component::Normal(name) => {
                if first.is_none() {
                    first = Some(name.to_os_string());
                }
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(Error::Validation(
                    "o caminho não pode sair da coleção".into(),
                ));
            }
        }
    }
    if let Some(first) = first {
        let first = first.to_string_lossy();
        if first == INTERNAL_DIR || first == ".git" {
            return Err(Error::Validation(
                "caminhos internos não podem ser usados como destino".into(),
            ));
        }
    }
    let root_canonical = fs::canonicalize(root)?;
    let joined = root.join(candidate);
    let existing = if joined.exists() {
        Some(fs::canonicalize(&joined)?)
    } else {
        joined
            .parent()
            .and_then(|parent| fs::canonicalize(parent).ok())
    };
    if let Some(existing) = existing {
        if !existing.starts_with(&root_canonical) {
            return Err(Error::Validation(
                "o destino resolve para fora da coleção".into(),
            ));
        }
    }
    Ok(joined)
}

/// Escrita atômica com sincronização. No Windows, quando o destino existe,
/// usa um backup temporário para concluir a substituição.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        restrict_directory(parent)?;
    }
    let temp = path.with_file_name(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("reqlivo"),
        crate::store::short_uuid()
    ));
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    restrict_file(&temp)?;
    #[cfg(windows)]
    {
        if path.exists() {
            let backup = path.with_file_name(format!(
                ".{}.old-{}",
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("reqlivo"),
                crate::store::short_uuid()
            ));
            fs::rename(path, &backup)?;
            match fs::rename(&temp, path) {
                Ok(()) => {
                    let _ = fs::remove_file(backup);
                }
                Err(error) => {
                    let _ = fs::rename(&backup, path);
                    let _ = fs::remove_file(&temp);
                    return Err(error.into());
                }
            }
            return Ok(());
        }
    }
    fs::rename(&temp, path)?;
    Ok(())
}

pub fn atomic_write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let body = serde_json::to_vec_pretty(value)
        .map_err(|error| Error::Internal(format!("JSON interno inválido: {error}")))?;
    atomic_write(path, &body)
}

pub fn ensure_internal_dir(root: &Path, child: &str) -> Result<PathBuf> {
    let directory = root.join(INTERNAL_DIR).join(child);
    fs::create_dir_all(&directory)?;
    restrict_directory(&directory)?;
    if let Some(parent) = directory.parent() {
        restrict_directory(parent)?;
    }
    Ok(directory)
}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::metadata(path)?;
    if metadata.is_dir() {
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn restrict_directory(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn restrict_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_file(_path: &Path) -> Result<()> {
    Ok(())
}

/// Lock de escrita por coleção, recuperável após encerramento abrupto.
pub struct CollectionLock {
    path: PathBuf,
}

impl CollectionLock {
    pub fn acquire(root: &Path) -> Result<Self> {
        let directory = ensure_internal_dir(root, "")?;
        let path = directory.join("lock");
        let now = SystemTime::now();
        for attempt in 0..2 {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    let _ = write!(
                        file,
                        "pid={}\ncreated={}\n",
                        std::process::id(),
                        chrono::Utc::now().to_rfc3339()
                    );
                    let _ = file.sync_all();
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if attempt == 1 || !lock_is_stale(&path, now) {
                        return Err(Error::Conflict {
                            code: "collection_busy",
                            message: "outra operação está gravando nesta coleção; tente novamente"
                                .into(),
                        });
                    }
                    let _ = fs::remove_file(&path);
                }
                Err(error) => return Err(error.into()),
            }
        }
        Err(Error::Conflict {
            code: "collection_busy",
            message: "não foi possível obter o lock de escrita da coleção".into(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for CollectionLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn lock_is_stale(path: &Path, now: SystemTime) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return true;
    };
    let age = metadata
        .modified()
        .ok()
        .and_then(|modified| now.duration_since(modified).ok())
        .unwrap_or(Duration::ZERO);
    if age > Duration::from_secs(60) {
        return true;
    }
    let Ok(text) = fs::read_to_string(path) else {
        return true;
    };
    let pid = text
        .lines()
        .find_map(|line| line.strip_prefix("pid="))
        .and_then(|value| value.trim().parse::<u32>().ok());
    match pid {
        Some(pid) => !process_is_alive(pid),
        None => true,
    }
}

#[cfg(target_os = "linux")]
fn process_is_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(not(target_os = "linux"))]
fn process_is_alive(pid: u32) -> bool {
    // Sem uma API portátil de baixo custo, confia no prazo de expiração.
    let _ = pid;
    true
}

pub fn ensure_revision(path: &Path, expected: Option<&str>) -> Result<()> {
    if let Some(expected) = expected {
        if !path.exists() {
            return Err(Error::Conflict {
                code: "revision_conflict",
                message: "o arquivo não existe mais no disco".into(),
            });
        }
        let current = hash_file(path)?;
        if current != expected {
            return Err(Error::Conflict {
                code: "revision_conflict",
                message: "o arquivo foi alterado fora do Reqlivo; recarregue antes de salvar"
                    .into(),
            });
        }
    }
    Ok(())
}

pub fn yaml_with_header<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    let body = serde_yaml::to_string(value)?;
    let mut content = String::new();
    content.push_str("# Reqlivo — arquivo local, formato OpenCollection 1.0.0\n");
    content.push_str("# Editável fora do aplicativo. IDs ficam em extensions.reqlivo.id.\n");
    content.push_str(&body);
    Ok(content.into_bytes())
}
