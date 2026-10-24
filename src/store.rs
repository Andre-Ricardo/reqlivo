use crate::error::{Error, Result};
use crate::model::*;
use crate::variables::ScopeInput;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const MANIFEST_FILE: &str = "opencollection.yml";
pub const FOLDER_FILE: &str = "folder.yml";
const REQLIVO_DIR: &str = ".reqlivo";

#[derive(Debug, Clone)]
pub struct FolderEntry {
    pub id: String,
    pub name: String,
    pub dir: PathBuf,
    pub rel_dir: String,
    pub parent: Option<String>,
    pub seq: i64,
    pub tags: Vec<String>,
    pub file: FolderFile,
    pub revision: String,
}

impl FolderEntry {
    pub fn auth(&self) -> Option<&AuthConfig> {
        self.file
            .request
            .as_ref()
            .and_then(|request| request.auth.as_ref())
    }

    pub fn variables(&self) -> &[Variable] {
        self.file
            .request
            .as_ref()
            .map(|request| request.variables.as_slice())
            .unwrap_or(&[])
    }

    pub fn scripts(&self) -> &[Script] {
        self.file
            .request
            .as_ref()
            .map(|request| request.scripts.as_slice())
            .unwrap_or(&[])
    }

    pub fn headers(&self) -> &[Header] {
        self.file
            .request
            .as_ref()
            .map(|request| request.headers.as_slice())
            .unwrap_or(&[])
    }
}

#[derive(Debug, Clone)]
pub struct RequestEntry {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub rel_path: String,
    pub parent: Option<String>,
    pub seq: i64,
    pub tags: Vec<String>,
    pub file: RequestFile,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnsupportedItem {
    pub path: String,
    pub kind: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TreeNode {
    Folder {
        id: String,
        name: String,
        seq: i64,
        tags: Vec<String>,
        children: Vec<TreeNode>,
    },
    Request {
        id: String,
        name: String,
        seq: i64,
        method: String,
        url: String,
        tags: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub struct CollectionStore {
    pub root: PathBuf,
    pub manifest: CollectionManifest,
    pub manifest_revision: String,
    pub folders: BTreeMap<String, FolderEntry>,
    pub requests: BTreeMap<String, RequestEntry>,
    pub unsupported: Vec<UnsupportedItem>,
    pub warnings: Vec<String>,
    children: BTreeMap<Option<String>, Vec<ItemRef>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ItemRef {
    Folder(String),
    Request(String),
}

impl ItemRef {
    fn id(&self) -> &str {
        match self {
            ItemRef::Folder(id) | ItemRef::Request(id) => id,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestPayload {
    pub id: String,
    pub revision: String,
    pub path: String,
    pub request: RequestFile,
}

#[derive(Debug, Clone, Serialize)]
pub struct CollectionSnapshot {
    pub root: String,
    pub manifest: CollectionManifest,
    pub manifest_revision: String,
    pub tree: Vec<TreeNode>,
    pub environments: Vec<Environment>,
    pub default_environment: Option<String>,
    pub unsupported: Vec<UnsupportedItem>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScopedScript {
    pub phase: &'static str,
    pub origin: String,
    pub code: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScopedHeader {
    pub name: String,
    pub value: String,
    pub disabled: bool,
    pub origin: String,
}

#[derive(Debug, Clone, Copy)]
pub struct EffectiveSettings {
    pub timeout_ms: u64,
    pub follow_redirects: bool,
    pub max_redirects: u32,
    pub encode_url: bool,
    pub offline_only: bool,
    pub max_response_bytes: usize,
    pub script_timeout_ms: u64,
}

impl CollectionStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let root = path.as_ref().to_path_buf();
        if !root.is_dir() {
            return Err(Error::Storage(format!(
                "'{}' não é um diretório de coleção",
                root.display()
            )));
        }
        let manifest_path = find_manifest(&root).ok_or_else(|| {
            Error::Storage(format!(
                "'{}' não contém {MANIFEST_FILE}; crie uma coleção com `reqlivo init`",
                root.display()
            ))
        })?;
        let manifest_bytes = fs::read(&manifest_path)?;
        let manifest_revision = hash_bytes(&manifest_bytes);
        let mut manifest: CollectionManifest = serde_yaml::from_slice(&manifest_bytes)?;
        if manifest.bundled {
            return Err(Error::Storage(format!(
                "'{}' é uma coleção OpenCollection empacotada; importe-a com `reqlivo import opencollection`",
                manifest_path.display()
            )));
        }

        let mut warnings = Vec::new();
        migrate_manifest(&root, &manifest_path, &mut manifest, &mut warnings)?;

        let mut store = Self {
            root: root.clone(),
            manifest,
            manifest_revision,
            folders: BTreeMap::new(),
            requests: BTreeMap::new(),
            unsupported: Vec::new(),
            warnings,
            children: BTreeMap::new(),
        };

        let mut seen_ids = BTreeSet::new();
        if let Some(id) = store.manifest.collection_id() {
            seen_ids.insert(id.to_string());
        }
        store.load_directory(&root, None, &mut seen_ids)?;
        store.rebuild_children();
        store.validate_tree()?;
        Ok(store)
    }

    fn load_directory(
        &mut self,
        dir: &Path,
        parent: Option<String>,
        seen_ids: &mut BTreeSet<String>,
    ) -> Result<()> {
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if is_yaml(&path) {
                files.push(path);
            }
        }
        dirs.sort_by_key(|path| path.file_name().map(|name| name.to_os_string()));
        files.sort();

        for path in files {
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if file_name == MANIFEST_FILE
                || file_name == "opencollection.yaml"
                || file_name == FOLDER_FILE
                || file_name == "folder.yaml"
            {
                continue;
            }
            let bytes = fs::read(&path)?;
            let revision = hash_bytes(&bytes);
            let value: serde_yaml::Value = match serde_yaml::from_slice(&bytes) {
                Ok(value) => value,
                Err(error) => {
                    self.unsupported.push(UnsupportedItem {
                        path: relative(&self.root, &path),
                        kind: "invalid".into(),
                        reason: format!("YAML inválido: {error}"),
                    });
                    continue;
                }
            };
            let kind = value
                .get("info")
                .and_then(|info| info.get("type"))
                .and_then(|kind| kind.as_str())
                .unwrap_or("http");
            if !kind.eq_ignore_ascii_case("http") {
                self.unsupported.push(UnsupportedItem {
                    path: relative(&self.root, &path),
                    kind: kind.to_string(),
                    reason: "tipo não suportado no MVP (apenas HTTP)".into(),
                });
                continue;
            }
            let mut request: RequestFile = match serde_yaml::from_slice(&bytes) {
                Ok(request) => request,
                Err(error) => {
                    self.unsupported.push(UnsupportedItem {
                        path: relative(&self.root, &path),
                        kind: "invalid".into(),
                        reason: format!("estrutura HTTP inválida: {error}"),
                    });
                    continue;
                }
            };
            let mut id = request.reqlivo_id().map(str::to_string);
            if id
                .as_deref()
                .map(|value| !seen_ids.insert(value.to_string()))
                .unwrap_or(false)
            {
                return Err(Error::Validation(format!(
                    "ID duplicado '{}' em {}",
                    id.unwrap_or_default(),
                    path.display()
                )));
            }
            if id.is_none() {
                id = Some(new_id("req", seen_ids));
                ensure_reqlivo_ext(&mut request.extensions).id = id.clone();
                write_yaml(&path, &request)?;
                self.warnings.push(format!(
                    "ID atribuído automaticamente a {}",
                    relative(&self.root, &path)
                ));
            }
            let id = id.expect("id gerado");
            let revision = if request.reqlivo_id() == Some(id.as_str()) {
                revision
            } else {
                hash_bytes(&fs::read(&path)?)
            };
            self.requests.insert(
                id.clone(),
                RequestEntry {
                    id,
                    name: request.info.name.clone(),
                    path: path.clone(),
                    rel_path: relative(&self.root, &path),
                    parent: parent.clone(),
                    seq: request.info.seq,
                    tags: request.info.tags.clone(),
                    file: request,
                    revision,
                },
            );
        }

        for path in dirs {
            let folder_file = [path.join(FOLDER_FILE), path.join("folder.yaml")]
                .into_iter()
                .find(|candidate| candidate.exists());
            let Some(folder_file) = folder_file else {
                self.warnings.push(format!(
                    "diretório ignorado por não conter {FOLDER_FILE}: {}",
                    relative(&self.root, &path)
                ));
                continue;
            };
            let bytes = fs::read(&folder_file)?;
            let revision = hash_bytes(&bytes);
            let mut folder: FolderFile = serde_yaml::from_slice(&bytes)?;
            let mut id = folder.reqlivo_id().map(str::to_string);
            if id
                .as_deref()
                .map(|value| !seen_ids.insert(value.to_string()))
                .unwrap_or(false)
            {
                return Err(Error::Validation(format!(
                    "ID de pasta duplicado '{}' em {}",
                    id.unwrap_or_default(),
                    path.display()
                )));
            }
            if id.is_none() {
                id = Some(new_id("fld", seen_ids));
                ensure_reqlivo_ext(&mut folder.extensions).id = id.clone();
                write_yaml(&folder_file, &folder)?;
                self.warnings.push(format!(
                    "ID atribuído automaticamente a {}",
                    relative(&self.root, &folder_file)
                ));
            }
            let id = id.expect("id gerado");
            let rev = if folder.reqlivo_id() == Some(id.as_str()) {
                revision
            } else {
                hash_bytes(&fs::read(&folder_file)?)
            };
            self.folders.insert(
                id.clone(),
                FolderEntry {
                    id: id.clone(),
                    name: folder.info.name.clone(),
                    dir: path.clone(),
                    rel_dir: relative(&self.root, &path),
                    parent: parent.clone(),
                    seq: folder.info.seq,
                    tags: folder.info.tags.clone(),
                    file: folder,
                    revision: rev,
                },
            );
            self.load_directory(&path, Some(id), seen_ids)?;
        }
        Ok(())
    }

    fn rebuild_children(&mut self) {
        self.children.clear();
        for folder in self.folders.values() {
            self.children
                .entry(folder.parent.clone())
                .or_default()
                .push(ItemRef::Folder(folder.id.clone()));
        }
        for request in self.requests.values() {
            self.children
                .entry(request.parent.clone())
                .or_default()
                .push(ItemRef::Request(request.id.clone()));
        }
        // A ordem é calculada antes do empréstimo mutável para evitar
        // conflito entre `children` e os mapas de pastas/requisições.
        let mut order: BTreeMap<String, (i64, String)> = BTreeMap::new();
        for folder in self.folders.values() {
            order.insert(folder.id.clone(), (folder.seq, folder.name.to_lowercase()));
        }
        for request in self.requests.values() {
            order.insert(
                request.id.clone(),
                (request.seq, request.name.to_lowercase()),
            );
        }
        for items in self.children.values_mut() {
            items.sort_by(|left, right| {
                let left_key = order.get(left.id()).cloned().unwrap_or((0, String::new()));
                let right_key = order.get(right.id()).cloned().unwrap_or((0, String::new()));
                left_key.cmp(&right_key)
            });
        }
    }

    fn validate_tree(&self) -> Result<()> {
        for folder in self.folders.values() {
            if let Some(parent) = &folder.parent {
                if !self.folders.contains_key(parent) {
                    return Err(Error::Validation(format!(
                        "pasta '{}' referencia pasta inexistente '{parent}'",
                        folder.name
                    )));
                }
            }
        }
        for request in self.requests.values() {
            if let Some(parent) = &request.parent {
                if !self.folders.contains_key(parent) {
                    return Err(Error::Validation(format!(
                        "requisição '{}' referencia pasta inexistente '{parent}'",
                        request.name
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn snapshot(&self) -> CollectionSnapshot {
        CollectionSnapshot {
            root: self.root.display().to_string(),
            manifest: self.manifest.clone(),
            manifest_revision: self.manifest_revision.clone(),
            tree: self.tree(),
            environments: self.manifest.environments().to_vec(),
            default_environment: self.default_environment(),
            unsupported: self.unsupported.clone(),
            warnings: self.warnings.clone(),
        }
    }

    pub fn tree(&self) -> Vec<TreeNode> {
        self.tree_for(None)
    }

    fn tree_for(&self, parent: Option<&str>) -> Vec<TreeNode> {
        let key = parent.map(str::to_string);
        let mut nodes = Vec::new();
        for item in self.children.get(&key).cloned().unwrap_or_default() {
            match item {
                ItemRef::Folder(id) => {
                    if let Some(folder) = self.folders.get(&id) {
                        nodes.push(TreeNode::Folder {
                            id: id.clone(),
                            name: folder.name.clone(),
                            seq: folder.seq,
                            tags: folder.tags.clone(),
                            children: self.tree_for(Some(&id)),
                        });
                    }
                }
                ItemRef::Request(id) => {
                    if let Some(request) = self.requests.get(&id) {
                        nodes.push(TreeNode::Request {
                            id: id.clone(),
                            name: request.name.clone(),
                            seq: request.seq,
                            method: request.file.http.method.clone(),
                            url: request.file.http.url.clone(),
                            tags: request.tags.clone(),
                        });
                    }
                }
            }
        }
        nodes
    }

    pub fn request(&self, id: &str) -> Result<&RequestEntry> {
        self.requests
            .get(id)
            .ok_or_else(|| Error::Storage(format!("requisição '{id}' não encontrada")))
    }

    pub fn folder(&self, id: &str) -> Result<&FolderEntry> {
        self.folders
            .get(id)
            .ok_or_else(|| Error::Storage(format!("pasta '{id}' não encontrada")))
    }

    pub fn request_payload(&self, id: &str) -> Result<RequestPayload> {
        let entry = self.request(id)?;
        Ok(RequestPayload {
            id: entry.id.clone(),
            revision: entry.revision.clone(),
            path: entry.rel_path.clone(),
            request: entry.file.clone(),
        })
    }

    /// Cadeia de pastas da raiz até a pasta do item.
    pub fn folder_chain(&self, parent: Option<&str>) -> Vec<&FolderEntry> {
        let mut chain = Vec::new();
        let mut current = parent.map(str::to_string);
        let mut guard = 0;
        while let Some(id) = current {
            if let Some(folder) = self.folders.get(&id) {
                chain.push(folder);
                current = folder.parent.clone();
            } else {
                break;
            }
            guard += 1;
            if guard > 128 {
                break;
            }
        }
        chain.reverse();
        chain
    }

    pub fn default_environment(&self) -> Option<String> {
        let settings = self.manifest.settings();
        settings
            .default_environment
            .filter(|name| {
                self.manifest
                    .environments()
                    .iter()
                    .any(|env| &env.name == name)
            })
            .or_else(|| {
                self.manifest
                    .environments()
                    .first()
                    .map(|env| env.name.clone())
            })
    }

    pub fn environment_chain(&self, name: &str) -> Result<Vec<Environment>> {
        let mut chain = Vec::new();
        let mut current = Some(name.to_string());
        let mut seen = BTreeSet::new();
        while let Some(name) = current {
            if !seen.insert(name.clone()) {
                return Err(Error::Validation(format!(
                    "herança circular de ambientes: {}",
                    seen.into_iter().collect::<Vec<_>>().join(" → ")
                )));
            }
            let env = self
                .manifest
                .environments()
                .iter()
                .find(|env| env.name == name)
                .ok_or_else(|| {
                    Error::Validation(format!("ambiente '{name}' não encontrado na coleção"))
                })?;
            chain.push(env.clone());
            current = env.extends.clone();
        }
        chain.reverse();
        Ok(chain)
    }

    pub fn scope_input(
        &self,
        request_id: &str,
        environment: Option<&str>,
        runtime: &BTreeMap<String, String>,
    ) -> Result<ScopeInput> {
        let request = self.request(request_id)?;
        self.scope_input_with(request_id, &request.file, environment, runtime)
    }

    /// Variante usada para testar um rascunho da UI sem salvar antes.
    pub fn scope_input_with(
        &self,
        request_id: &str,
        request: &RequestFile,
        environment: Option<&str>,
        runtime: &BTreeMap<String, String>,
    ) -> Result<ScopeInput> {
        let entry = self.request(request_id)?;
        let collection = self
            .manifest
            .request
            .as_ref()
            .map(|defaults| defaults.variables.clone())
            .unwrap_or_default();
        let environments = match environment {
            Some(name) => self
                .environment_chain(name)?
                .into_iter()
                .flat_map(|env| env.variables)
                .collect(),
            None => Vec::new(),
        };
        let folders = self
            .folder_chain(entry.parent.as_deref())
            .into_iter()
            .flat_map(|folder| folder.variables().to_vec())
            .collect();
        let mut secret_refs = self.manifest.secret_refs().to_vec();
        secret_refs.extend(request.secret_refs().iter().cloned());
        Ok(ScopeInput {
            collection,
            environments,
            folders,
            request: request.runtime.variables.clone(),
            runtime: runtime.clone(),
            secret_refs,
            environment_name: environment.map(str::to_string),
            allow_process_env: self.manifest.settings().allow_process_env,
        })
    }

    pub fn effective_settings(&self, request_id: &str) -> Result<EffectiveSettings> {
        let request = self.request(request_id)?;
        self.effective_settings_with(&request.file)
    }

    pub fn effective_settings_with(&self, request: &RequestFile) -> Result<EffectiveSettings> {
        let collection = self.manifest.settings();
        Ok(EffectiveSettings {
            timeout_ms: request
                .settings
                .timeout
                .unwrap_or(collection.default_timeout_ms),
            follow_redirects: request
                .settings
                .follow_redirects
                .unwrap_or(collection.follow_redirects),
            max_redirects: request
                .settings
                .max_redirects
                .unwrap_or(collection.max_redirects),
            encode_url: request.settings.encode_url.unwrap_or(true),
            offline_only: collection.offline_only
                || std::env::var("REQLIVO_OFFLINE")
                    .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
                    .unwrap_or(false),
            max_response_bytes: collection.max_response_bytes,
            script_timeout_ms: collection.script_timeout_ms,
        })
    }

    pub fn effective_headers(&self, request_id: &str) -> Result<Vec<ScopedHeader>> {
        let request = self.request(request_id)?;
        self.effective_headers_with(request_id, &request.file)
    }

    pub fn effective_headers_with(
        &self,
        request_id: &str,
        request: &RequestFile,
    ) -> Result<Vec<ScopedHeader>> {
        let entry = self.request(request_id)?;
        let mut merged: Vec<ScopedHeader> = Vec::new();
        let mut push = |header: &Header, origin: String| {
            if let Some(existing) = merged
                .iter_mut()
                .find(|item| item.name.eq_ignore_ascii_case(&header.name))
            {
                *existing = ScopedHeader {
                    name: header.name.clone(),
                    value: header.value.clone(),
                    disabled: header.disabled,
                    origin,
                };
            } else {
                merged.push(ScopedHeader {
                    name: header.name.clone(),
                    value: header.value.clone(),
                    disabled: header.disabled,
                    origin,
                });
            }
        };
        if let Some(defaults) = &self.manifest.request {
            for header in &defaults.headers {
                push(header, "coleção".into());
            }
        }
        for folder in self.folder_chain(entry.parent.as_deref()) {
            for header in folder.headers() {
                push(header, format!("pasta '{}'", folder.name));
            }
        }
        for header in &request.http.headers {
            push(header, "requisição".into());
        }
        Ok(merged)
    }

    pub fn auth_inputs(
        &self,
        request_id: &str,
    ) -> Result<(
        Option<AuthConfig>,
        Vec<(String, Option<AuthConfig>)>,
        Option<AuthConfig>,
    )> {
        let request = self.request(request_id)?;
        self.auth_inputs_with(request_id, &request.file)
    }

    pub fn auth_inputs_with(
        &self,
        request_id: &str,
        request: &RequestFile,
    ) -> Result<(
        Option<AuthConfig>,
        Vec<(String, Option<AuthConfig>)>,
        Option<AuthConfig>,
    )> {
        let entry = self.request(request_id)?;
        let collection = self
            .manifest
            .request
            .as_ref()
            .and_then(|defaults| defaults.auth.clone());
        let folders = self
            .folder_chain(entry.parent.as_deref())
            .into_iter()
            .rev()
            .map(|folder| (folder.name.clone(), folder.auth().cloned()))
            .collect();
        Ok((request.http.auth.clone(), folders, collection))
    }

    /// Ordem sandwich: coleção.pre → pasta.pre (raiz→folha) → requisição.pre.
    /// Para pós/testes, a ordem de pasta é folha→raiz e a coleção vem por último
    /// no pós; testes rodam depois do pós.
    pub fn scoped_scripts(
        &self,
        request_id: &str,
        phase: ScriptPhase,
    ) -> Result<Vec<ScopedScript>> {
        let request = self.request(request_id)?;
        self.scoped_scripts_with(request_id, &request.file, phase)
    }

    pub fn scoped_scripts_with(
        &self,
        request_id: &str,
        request: &RequestFile,
        phase: ScriptPhase,
    ) -> Result<Vec<ScopedScript>> {
        let entry = self.request(request_id)?;
        let folders = self.folder_chain(entry.parent.as_deref());
        let mut scripts = Vec::new();
        let add = |scripts: &[Script], origin: String, scripts_out: &mut Vec<ScopedScript>| {
            for script in scripts.iter().filter(|script| script.phase == phase) {
                scripts_out.push(ScopedScript {
                    phase: phase.as_str(),
                    origin: origin.clone(),
                    code: script.code.clone(),
                });
            }
        };
        match phase {
            ScriptPhase::BeforeRequest => {
                if let Some(defaults) = &self.manifest.request {
                    add(&defaults.scripts, "coleção".into(), &mut scripts);
                }
                for folder in &folders {
                    add(
                        folder.scripts(),
                        format!("pasta '{}'", folder.name),
                        &mut scripts,
                    );
                }
                add(&request.runtime.scripts, "requisição".into(), &mut scripts);
            }
            ScriptPhase::AfterResponse | ScriptPhase::Tests | ScriptPhase::Hooks => {
                add(&request.runtime.scripts, "requisição".into(), &mut scripts);
                for folder in folders.iter().rev() {
                    add(
                        folder.scripts(),
                        format!("pasta '{}'", folder.name),
                        &mut scripts,
                    );
                }
                if let Some(defaults) = &self.manifest.request {
                    add(&defaults.scripts, "coleção".into(), &mut scripts);
                }
            }
        }
        Ok(scripts)
    }

    pub fn assertions(&self, request_id: &str) -> Result<Vec<Assertion>> {
        Ok(self.request(request_id)?.file.runtime.assertions.clone())
    }

    pub fn assertions_with(&self, request: &RequestFile) -> Vec<Assertion> {
        request.runtime.assertions.clone()
    }

    pub fn request_tags(&self, request_id: &str) -> Result<Vec<String>> {
        let entry = self.request(request_id)?;
        let mut tags = entry.tags.clone();
        for folder in self.folder_chain(entry.parent.as_deref()) {
            tags.extend(folder.tags.clone());
        }
        tags.sort();
        tags.dedup();
        Ok(tags)
    }

    pub fn is_in_folder(&self, request_id: &str, folder_id: &str) -> bool {
        let Ok(entry) = self.request(request_id) else {
            return false;
        };
        let mut current = entry.parent.clone();
        while let Some(id) = current {
            if id == folder_id {
                return true;
            }
            current = self
                .folders
                .get(&id)
                .and_then(|folder| folder.parent.clone());
        }
        false
    }

    pub fn save_request(
        &mut self,
        id: &str,
        mut request: RequestFile,
        expected_revision: Option<&str>,
    ) -> Result<RequestPayload> {
        request.validate()?;
        let entry = self.request(id)?.clone();
        let current = hash_bytes(&fs::read(&entry.path)?);
        if let Some(expected) = expected_revision {
            if expected != current {
                return Err(Error::Storage(
                    "o arquivo foi alterado fora do Reqlivo; recarregue antes de salvar".into(),
                ));
            }
        }
        let ext = ensure_reqlivo_ext(&mut request.extensions);
        if ext.id.as_deref() != Some(id) {
            ext.id = Some(id.to_string());
        }
        // Preserva campos desconhecidos lidos do arquivo original.
        if request.extra.is_empty() {
            request.extra = entry.file.extra.clone();
        }
        write_yaml(&entry.path, &request)?;
        let revision = hash_bytes(&fs::read(&entry.path)?);
        let path = entry.path.clone();
        let rel_path = entry.rel_path.clone();
        let parent = entry.parent.clone();
        let updated = RequestEntry {
            id: id.to_string(),
            name: request.info.name.clone(),
            path,
            rel_path: rel_path.clone(),
            parent,
            seq: request.info.seq,
            tags: request.info.tags.clone(),
            file: request,
            revision: revision.clone(),
        };
        self.requests.insert(id.to_string(), updated);
        self.rebuild_children();
        Ok(RequestPayload {
            id: id.to_string(),
            revision,
            path: rel_path,
            request: self.request(id)?.file.clone(),
        })
    }

    pub fn create_request(
        &mut self,
        parent: Option<&str>,
        name: impl Into<String>,
        method: impl Into<String>,
        url: impl Into<String>,
    ) -> Result<RequestPayload> {
        let parent = match parent {
            Some(id) => Some(self.folder(id)?.clone()),
            None => None,
        };
        let name = name.into();
        if name.trim().is_empty() {
            return Err(Error::Validation(
                "o nome da requisição é obrigatório".into(),
            ));
        }
        let seq = self.next_seq(parent.as_ref().map(|folder| folder.id.as_str()));
        let mut seen = self.all_ids();
        let id = new_id("req", &mut seen);
        let mut extensions = Extensions::default();
        extensions.reqlivo = Some(ReqlivoExtension {
            id: Some(id.clone()),
            ..ReqlivoExtension::default()
        });
        let request = RequestFile {
            info: RequestInfo {
                name,
                kind: "http".into(),
                seq,
                tags: Vec::new(),
                description: None,
                extra: Extra::new(),
            },
            http: HttpDetails {
                method: method.into(),
                url: url.into(),
                headers: Vec::new(),
                params: Vec::new(),
                body: None,
                auth: Some(AuthConfig::inherit()),
            },
            runtime: RequestRuntime::default(),
            settings: HttpSettings::default(),
            docs: None,
            extensions,
            extra: Extra::new(),
        };
        let dir = parent
            .as_ref()
            .map(|folder| folder.dir.clone())
            .unwrap_or_else(|| self.root.clone());
        let path = dir.join(format!("{id}.yml"));
        write_yaml(&path, &request)?;
        let revision = hash_bytes(&fs::read(&path)?);
        let rel_path = relative(&self.root, &path);
        let entry = RequestEntry {
            id: id.clone(),
            name: request.info.name.clone(),
            path,
            rel_path: rel_path.clone(),
            parent: parent.as_ref().map(|folder| folder.id.clone()),
            seq,
            tags: Vec::new(),
            file: request.clone(),
            revision: revision.clone(),
        };
        self.requests.insert(id.clone(), entry);
        self.rebuild_children();
        Ok(RequestPayload {
            id,
            revision,
            path: rel_path,
            request,
        })
    }

    pub fn create_folder(
        &mut self,
        parent: Option<&str>,
        name: impl Into<String>,
    ) -> Result<String> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(Error::Validation("o nome da pasta é obrigatório".into()));
        }
        let parent_entry = match parent {
            Some(id) => Some(self.folder(id)?.clone()),
            None => None,
        };
        let base = parent_entry
            .as_ref()
            .map(|folder| folder.dir.clone())
            .unwrap_or_else(|| self.root.clone());
        let mut slug = slugify(&name);
        let mut candidate = base.join(&slug);
        let mut counter = 2;
        while candidate.exists() {
            slug = format!("{}-{counter}", slugify(&name));
            candidate = base.join(&slug);
            counter += 1;
        }
        fs::create_dir_all(&candidate)?;
        let mut seen = self.all_ids();
        let id = new_id("fld", &mut seen);
        let seq = self.next_seq(parent);
        let mut extensions = Extensions::default();
        extensions.reqlivo = Some(ReqlivoExtension {
            id: Some(id.clone()),
            ..ReqlivoExtension::default()
        });
        let folder = FolderFile {
            info: FolderInfo {
                name: name.clone(),
                kind: "folder".into(),
                seq,
                tags: Vec::new(),
                description: None,
                extra: Extra::new(),
            },
            request: None,
            docs: None,
            extensions,
            extra: Extra::new(),
        };
        let path = candidate.join(FOLDER_FILE);
        write_yaml(&path, &folder)?;
        let revision = hash_bytes(&fs::read(&path)?);
        self.folders.insert(
            id.clone(),
            FolderEntry {
                id: id.clone(),
                name,
                dir: candidate.clone(),
                rel_dir: relative(&self.root, &candidate),
                parent: parent.map(str::to_string),
                seq,
                tags: Vec::new(),
                file: folder,
                revision,
            },
        );
        self.rebuild_children();
        Ok(id)
    }

    pub fn rename_folder(&mut self, id: &str, name: impl Into<String>) -> Result<()> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(Error::Validation("o nome da pasta é obrigatório".into()));
        }
        let entry = self.folder(id)?.clone();
        let mut file = entry.file.clone();
        file.info.name = name.clone();
        write_yaml(&entry.dir.join(FOLDER_FILE), &file)?;
        let revision = hash_bytes(&fs::read(entry.dir.join(FOLDER_FILE))?);
        self.folders.insert(
            id.to_string(),
            FolderEntry {
                name,
                file,
                revision,
                ..entry
            },
        );
        self.rebuild_children();
        Ok(())
    }

    pub fn set_folder_defaults(
        &mut self,
        id: &str,
        defaults: Option<RequestDefaults>,
        docs: Option<String>,
    ) -> Result<()> {
        let entry = self.folder(id)?.clone();
        let mut file = entry.file.clone();
        file.request = defaults;
        if docs.is_some() {
            file.docs = docs;
        }
        write_yaml(&entry.dir.join(FOLDER_FILE), &file)?;
        let revision = hash_bytes(&fs::read(entry.dir.join(FOLDER_FILE))?);
        self.folders.insert(
            id.to_string(),
            FolderEntry {
                file,
                revision,
                ..entry
            },
        );
        self.rebuild_children();
        Ok(())
    }

    pub fn set_collection_defaults(&mut self, defaults: Option<RequestDefaults>) -> Result<()> {
        let mut manifest = self.manifest.clone();
        manifest.request = defaults;
        self.manifest = manifest;
        self.save_manifest()
    }

    pub fn delete_request(&mut self, id: &str) -> Result<String> {
        let entry = self.request(id)?.clone();
        let trash = self.root.join(REQLIVO_DIR).join("trash");
        fs::create_dir_all(&trash)?;
        let target = trash.join(format!(
            "{}-{}",
            chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
            entry
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("request.yml")
        ));
        fs::rename(&entry.path, &target)?;
        self.requests.remove(id);
        self.rebuild_children();
        Ok(target.display().to_string())
    }

    pub fn delete_folder(&mut self, id: &str) -> Result<String> {
        let entry = self.folder(id)?.clone();
        let has_children = self
            .children
            .get(&Some(id.to_string()))
            .map(|children| !children.is_empty())
            .unwrap_or(false);
        if has_children {
            return Err(Error::Validation(
                "a pasta não está vazia; mova ou apague os itens primeiro".into(),
            ));
        }
        let trash = self.root.join(REQLIVO_DIR).join("trash");
        fs::create_dir_all(&trash)?;
        let target = trash.join(format!(
            "{}-{}",
            chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
            entry
                .dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("folder")
        ));
        fs::rename(&entry.dir, &target)?;
        self.folders.remove(id);
        self.rebuild_children();
        Ok(target.display().to_string())
    }

    pub fn add_request(
        &mut self,
        parent: Option<&str>,
        mut request: RequestFile,
    ) -> Result<RequestPayload> {
        let parent_entry = match parent {
            Some(id) => Some(self.folder(id)?.clone()),
            None => None,
        };
        let mut seen = self.all_ids();
        let id = new_id("req", &mut seen);
        let ext = ensure_reqlivo_ext(&mut request.extensions);
        ext.id = Some(id.clone());
        if request.info.seq == 0 {
            request.info.seq = self.next_seq(parent);
        }
        if request.info.kind.is_empty() {
            request.info.kind = "http".into();
        }
        let dir = parent_entry
            .as_ref()
            .map(|folder| folder.dir.clone())
            .unwrap_or_else(|| self.root.clone());
        let path = dir.join(format!("{id}.yml"));
        write_yaml(&path, &request)?;
        let revision = hash_bytes(&fs::read(&path)?);
        self.requests.insert(
            id.clone(),
            RequestEntry {
                id: id.clone(),
                name: request.info.name.clone(),
                path: path.clone(),
                rel_path: relative(&self.root, &path),
                parent: parent.map(str::to_string),
                seq: request.info.seq,
                tags: request.info.tags.clone(),
                file: request,
                revision: revision.clone(),
            },
        );
        self.rebuild_children();
        Ok(RequestPayload {
            id: id.clone(),
            revision,
            path: relative(&self.root, &path),
            request: self.request(&id)?.file.clone(),
        })
    }

    pub fn add_folder(&mut self, parent: Option<&str>, name: impl Into<String>) -> Result<String> {
        self.create_folder(parent, name)
    }

    pub fn upsert_environment(
        &mut self,
        name: &str,
        variables: Vec<Variable>,
        secret_refs: Vec<SecretRef>,
        extends: Option<String>,
    ) -> Result<()> {
        if name.trim().is_empty() {
            return Err(Error::Validation("o nome do ambiente é obrigatório".into()));
        }
        let mut manifest = self.manifest.clone();
        let index = manifest
            .config
            .environments
            .iter()
            .position(|env| env.name == name);
        let environment = Environment {
            name: name.to_string(),
            color: index.and_then(|index| manifest.config.environments[index].color.clone()),
            description: index
                .and_then(|index| manifest.config.environments[index].description.clone()),
            variables,
            extends,
            dot_env_file_path: None,
            extra: Extra::new(),
        };
        match index {
            Some(index) => manifest.config.environments[index] = environment,
            None => manifest.config.environments.push(environment),
        }
        let ext = ensure_reqlivo_ext(&mut manifest.extensions);
        ext.secret_refs
            .retain(|reference| reference.environment.as_deref() != Some(name));
        for mut reference in secret_refs {
            reference.environment = Some(name.to_string());
            ext.secret_refs.push(reference);
        }
        ext.environment_ids
            .entry(name.to_string())
            .or_insert_with(|| format!("env_{}", short_uuid()));
        if manifest.settings().default_environment.as_deref().is_none() {
            if let Some(settings) = manifest
                .extensions
                .reqlivo
                .as_mut()
                .and_then(|ext| ext.settings.as_mut())
            {
                settings.default_environment = Some(name.to_string());
            }
        }
        self.manifest = manifest;
        self.save_manifest()?;
        Ok(())
    }

    pub fn delete_environment(&mut self, name: &str) -> Result<()> {
        let mut manifest = self.manifest.clone();
        let before = manifest.config.environments.len();
        manifest.config.environments.retain(|env| env.name != name);
        if manifest.config.environments.len() == before {
            return Err(Error::Storage(format!("ambiente '{name}' não encontrado")));
        }
        for environment in &mut manifest.config.environments {
            if environment.extends.as_deref() == Some(name) {
                environment.extends = None;
            }
        }
        if let Some(ext) = manifest.extensions.reqlivo.as_mut() {
            ext.secret_refs
                .retain(|reference| reference.environment.as_deref() != Some(name));
            ext.environment_ids.remove(name);
            if let Some(settings) = ext.settings.as_mut() {
                if settings.default_environment.as_deref() == Some(name) {
                    settings.default_environment = manifest
                        .config
                        .environments
                        .first()
                        .map(|env| env.name.clone());
                }
            }
        }
        self.manifest = manifest;
        self.save_manifest()?;
        Ok(())
    }

    pub fn save_manifest(&mut self) -> Result<()> {
        let mut manifest = self.manifest.clone();
        ensure_reqlivo_ext(&mut manifest.extensions);
        let path = find_manifest(&self.root).unwrap_or_else(|| self.root.join(MANIFEST_FILE));
        write_yaml(&path, &manifest)?;
        self.manifest = manifest;
        self.manifest_revision = hash_bytes(&fs::read(&path)?);
        Ok(())
    }

    pub fn update_settings(
        &mut self,
        settings: CollectionSettings,
        default_environment: Option<String>,
    ) -> Result<()> {
        let mut manifest = self.manifest.clone();
        let ext = ensure_reqlivo_ext(&mut manifest.extensions);
        let mut settings = settings;
        settings.default_environment = default_environment;
        ext.settings = Some(settings);
        self.manifest = manifest;
        self.save_manifest()
    }

    fn seq_of(&self, item: &ItemRef) -> i64 {
        match item {
            ItemRef::Folder(id) => self.folders.get(id).map(|folder| folder.seq).unwrap_or(0),
            ItemRef::Request(id) => self
                .requests
                .get(id)
                .map(|request| request.seq)
                .unwrap_or(0),
        }
    }

    pub fn all_ids(&self) -> BTreeSet<String> {
        let mut ids: BTreeSet<String> = self.folders.keys().cloned().collect();
        ids.extend(self.requests.keys().cloned());
        if let Some(id) = self.manifest.collection_id() {
            ids.insert(id.to_string());
        }
        ids
    }

    fn next_seq(&self, parent: Option<&str>) -> i64 {
        self.children
            .get(&parent.map(str::to_string))
            .map(|items| {
                items
                    .iter()
                    .map(|item| self.seq_of(item))
                    .max()
                    .unwrap_or(0)
                    + 1
            })
            .unwrap_or(1)
    }

    pub fn all_requests_in_order(&self) -> Vec<&RequestEntry> {
        let mut out = Vec::new();
        self.collect_requests(None, &mut out);
        out
    }

    fn collect_requests<'a>(&'a self, parent: Option<&str>, out: &mut Vec<&'a RequestEntry>) {
        for item in self
            .children
            .get(&parent.map(str::to_string))
            .cloned()
            .unwrap_or_default()
        {
            match item {
                ItemRef::Folder(id) => self.collect_requests(Some(&id), out),
                ItemRef::Request(id) => {
                    if let Some(request) = self.requests.get(&id) {
                        out.push(request);
                    }
                }
            }
        }
    }

    pub fn export_project(&self, destination: &Path) -> Result<Vec<String>> {
        let mut copied = Vec::new();
        if destination.exists() {
            return Err(Error::Export(format!(
                "destino já existe: {}",
                destination.display()
            )));
        }
        copy_tree(&self.root, destination, &mut copied)?;
        Ok(copied)
    }

    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        for environment in self.manifest.environments() {
            if let Err(error) = self.environment_chain(&environment.name) {
                issues.push(format!("ambiente '{}': {error}", environment.name));
            }
        }
        if let Some(extension) = &self.manifest.extensions.reqlivo {
            for reference in &extension.secret_refs {
                if reference.key.trim().is_empty() {
                    issues.push(format!(
                        "segredo '{}' sem chave de referência",
                        reference.name
                    ));
                }
            }
        }
        for request in self.requests.values() {
            if request.file.http.url.contains("{{") {
                // Templates são válidos; a validação ocorre na execução.
            }
            if request.file.http.method.trim().is_empty() {
                issues.push(format!("{}: método vazio", request.rel_path));
            }
            if request.file.http.url.trim().is_empty() {
                issues.push(format!("{}: URL vazia", request.rel_path));
            }
            if let Some(Body::Json { data }) = &request.file.http.body {
                if !data.trim().is_empty()
                    && serde_json::from_str::<serde_json::Value>(data).is_err()
                {
                    issues.push(format!("{}: corpo JSON inválido", request.rel_path));
                }
            }
            if let Some(Body::Xml { data }) = &request.file.http.body {
                if !data.trim().is_empty() && roxmltree::Document::parse(data).is_err() {
                    issues.push(format!("{}: corpo XML inválido", request.rel_path));
                }
            }
        }
        issues
    }
}

// ---------------------------------------------------------------------------
// Inicialização e migração
// ---------------------------------------------------------------------------

pub fn init_collection(path: impl AsRef<Path>, name: &str) -> Result<CollectionStore> {
    let root = path.as_ref().to_path_buf();
    if root.join(MANIFEST_FILE).exists() {
        return Err(Error::Storage(format!(
            "já existe uma coleção em {}",
            root.display()
        )));
    }
    if root.exists() && fs::read_dir(&root)?.next().is_some() {
        return Err(Error::Storage(format!(
            "o diretório de destino não está vazio: {}",
            root.display()
        )));
    }
    fs::create_dir_all(&root)?;
    let mut seen = BTreeSet::new();
    let collection_id = new_id("col", &mut seen);
    let environment_id = format!("env_{}", short_uuid());
    let mut environment_ids = BTreeMap::new();
    environment_ids.insert("dev".to_string(), environment_id);
    let settings = CollectionSettings {
        default_environment: Some("dev".into()),
        ..CollectionSettings::default()
    };
    let mut extensions = Extensions::default();
    extensions.reqlivo = Some(ReqlivoExtension {
        schema_version: SCHEMA_VERSION,
        id: Some(collection_id),
        secret_refs: Vec::new(),
        settings: Some(settings),
        environment_ids,
        extra: Extra::new(),
    });
    let manifest = CollectionManifest {
        opencollection: OPENCOLLECTION_VERSION.to_string(),
        info: CollectionInfo {
            name: name.to_string(),
            summary: Some("Coleção local criada pelo Reqlivo.".into()),
            version: None,
            authors: None,
            extra: Extra::new(),
        },
        config: CollectionConfig {
            environments: vec![Environment {
                name: "dev".into(),
                color: None,
                description: None,
                variables: vec![Variable::plain("baseUrl", "http://127.0.0.1:18080")],
                extends: None,
                dot_env_file_path: None,
                extra: Extra::new(),
            }],
            extra: Extra::new(),
        },
        request: None,
        docs: None,
        bundled: false,
        extensions,
        extra: Extra::new(),
    };
    write_yaml(&root.join(MANIFEST_FILE), &manifest)?;
    fs::create_dir_all(root.join(REQLIVO_DIR))?;
    let gitignore = root.join(".gitignore");
    if !gitignore.exists() {
        fs::write(&gitignore, ".reqlivo/\n")?;
    }
    let readme = root.join("README.md");
    if !readme.exists() {
        fs::write(
            &readme,
            format!(
                "# {name}\n\nColeção local do [Reqlivo](https://github.com/reqlivo).\n\n- Formato: OpenCollection 1.0.0 + extensões `reqlivo`.\n- Estado local (cache, histórico, logs) fica em `.reqlivo/` e não deve ser versionado.\n- Segredos são referências a variáveis de ambiente, nunca valores neste diretório.\n"
            ),
        )?;
    }
    CollectionStore::open(&root)
}

fn migrate_manifest(
    root: &Path,
    path: &Path,
    manifest: &mut CollectionManifest,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let current = manifest
        .extensions
        .reqlivo
        .as_ref()
        .map(|ext| ext.schema_version)
        .unwrap_or(0);
    if current > SCHEMA_VERSION {
        return Err(Error::Storage(format!(
            "schemaVersion {current} é mais novo que o suportado ({SCHEMA_VERSION}); atualize o Reqlivo"
        )));
    }
    if current == SCHEMA_VERSION {
        return Ok(());
    }

    let backup_dir = root.join(REQLIVO_DIR).join("backups");
    fs::create_dir_all(&backup_dir)?;
    let backup = backup_dir.join(format!(
        "{}-opencollection.yml",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ")
    ));
    fs::copy(path, &backup)?;
    warnings.push(format!(
        "manifesto migrado de schemaVersion {current} para {SCHEMA_VERSION}; backup em {}",
        backup.display()
    ));

    let ext = ensure_reqlivo_ext(&mut manifest.extensions);
    ext.schema_version = SCHEMA_VERSION;
    if ext.id.is_none() {
        ext.id = Some(format!("col_{}", short_uuid()));
    }
    if ext.settings.is_none() {
        ext.settings = Some(CollectionSettings::default());
    }
    let mut seen = BTreeSet::new();
    seen.insert(ext.id.clone().unwrap_or_default());
    for environment in &manifest.config.environments {
        if !ext.environment_ids.contains_key(&environment.name) {
            ext.environment_ids
                .insert(environment.name.clone(), format!("env_{}", short_uuid()));
        }
    }
    let mut manifest_clone = manifest.clone();
    write_yaml(path, &manifest_clone)?;
    // Garante que a versão serializada continua legível.
    let reparsed: CollectionManifest = serde_yaml::from_slice(&fs::read(path)?)?;
    *manifest = reparsed;
    let _ = &mut manifest_clone;
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers de arquivos
// ---------------------------------------------------------------------------

pub fn find_manifest(root: &Path) -> Option<PathBuf> {
    for name in [MANIFEST_FILE, "opencollection.yaml"] {
        let candidate = root.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn ensure_reqlivo_ext(extensions: &mut Extensions) -> &mut ReqlivoExtension {
    if extensions.reqlivo.is_none() {
        extensions.reqlivo = Some(ReqlivoExtension {
            schema_version: SCHEMA_VERSION,
            ..ReqlivoExtension::default()
        });
    }
    extensions.reqlivo.as_mut().expect("extensão criada")
}

pub fn write_yaml<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let body = serde_yaml::to_string(value)?;
    let mut content = String::new();
    content.push_str("# Reqlivo — arquivo local, formato OpenCollection 1.0.0\n");
    content.push_str("# Editável fora do aplicativo. IDs ficam em extensions.reqlivo.id.\n");
    content.push_str(&body);
    atomic_write(path, content.as_bytes())
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_file_name(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("reqlivo"),
        short_uuid()
    ));
    {
        use std::io::Write;
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&temp, path)?;
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path, copied: &mut Vec<String>) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name == REQLIVO_DIR || name == ".git" || name == "target" {
            continue;
        }
        let from = entry.path();
        let to = destination.join(&name);
        if from.is_dir() {
            copy_tree(&from, &to, copied)?;
        } else {
            fs::copy(&from, &to)?;
            copied.push(name);
        }
    }
    Ok(())
}

#[cfg(test)]
fn find_manifest_in_parent(path: &Path) -> Option<PathBuf> {
    find_manifest(path)
}

fn is_yaml(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("yml") | Some("yaml")
    )
}

pub fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn new_id(prefix: &str, seen: &mut BTreeSet<String>) -> String {
    loop {
        let id = format!("{prefix}_{}", short_uuid());
        if seen.insert(id.clone()) {
            return id;
        }
    }
}

pub fn short_uuid() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..16].to_string()
}

pub fn slugify(input: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for character in input.chars() {
        let mapped = match character {
            'a'..='z' | '0'..='9' => Some(character),
            'A'..='Z' => Some(character.to_ascii_lowercase()),
            'á' | 'à' | 'â' | 'ã' | 'ä' => Some('a'),
            'é' | 'è' | 'ê' | 'ë' => Some('e'),
            'í' | 'ì' | 'î' | 'ï' => Some('i'),
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => Some('o'),
            'ú' | 'ù' | 'û' | 'ü' => Some('u'),
            'ç' => Some('c'),
            'ñ' => Some('n'),
            _ => None,
        };
        match mapped {
            Some(character) => {
                slug.push(character);
                last_dash = false;
            }
            None if !last_dash && !slug.is_empty() => {
                slug.push('-');
                last_dash = true;
            }
            None => {}
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "folder".to_string()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_collection() -> (tempfile::TempDir, CollectionStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = init_collection(dir.path(), "Teste").unwrap();
        (dir, store)
    }

    #[test]
    fn init_creates_manifest_and_environment() {
        let (dir, store) = temp_collection();
        assert!(dir.path().join(MANIFEST_FILE).exists());
        assert_eq!(store.manifest.environments().len(), 1);
        assert!(store.manifest.collection_id().is_some());
    }

    #[test]
    fn create_and_reload_request() {
        let (dir, mut store) = temp_collection();
        let payload = store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        let reopened = CollectionStore::open(dir.path()).unwrap();
        assert_eq!(
            reopened.request(&payload.id).unwrap().file.info.name,
            "Echo"
        );
    }

    #[test]
    fn save_rejects_external_change() {
        let (_dir, mut store) = temp_collection();
        let payload = store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        let entry = store.request(&payload.id).unwrap().clone();
        fs::write(&entry.path, "# alterado\n").unwrap();
        let error = store
            .save_request(&payload.id, entry.file.clone(), Some(&payload.revision))
            .unwrap_err()
            .to_string();
        assert!(error.contains("alterado fora"));
    }

    #[test]
    fn folders_keep_ids_when_renamed() {
        let (_dir, mut store) = temp_collection();
        let folder_id = store.create_folder(None, "Users").unwrap();
        let payload = store
            .create_request(Some(&folder_id), "List", "GET", "http://127.0.0.1/users")
            .unwrap();
        store.rename_folder(&folder_id, "Pessoas").unwrap();
        assert_eq!(store.folder(&folder_id).unwrap().name, "Pessoas");
        assert_eq!(
            store.request(&payload.id).unwrap().parent.as_deref(),
            Some(folder_id.as_str())
        );
    }

    #[test]
    fn environment_chain_detects_cycle() {
        let (_dir, mut store) = temp_collection();
        store
            .upsert_environment("a", vec![], vec![], Some("b".into()))
            .unwrap();
        store
            .upsert_environment("b", vec![], vec![], Some("a".into()))
            .unwrap();
        let error = store.environment_chain("a").unwrap_err().to_string();
        assert!(error.contains("circular"));
    }

    #[test]
    fn delete_moves_to_trash() {
        let (_dir, mut store) = temp_collection();
        let payload = store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        let trash = store.delete_request(&payload.id).unwrap();
        assert!(Path::new(&trash).exists());
        assert!(store.request(&payload.id).is_err());
    }

    #[test]
    fn slugify_handles_accents() {
        assert_eq!(slugify("Configuração Local"), "configuracao-local");
    }

    #[test]
    fn unused_helper_is_kept_for_import() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_manifest_in_parent(dir.path()).is_none());
    }
}
