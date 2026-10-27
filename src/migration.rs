//! Inspeção, aplicação e rollback verificável de migrações.
//!
//! A inspeção nunca altera arquivos do projeto; ela grava um plano e os bytes
//! propostos em `.reqlivo/backups/`. A aplicação só ocorre com o ID do plano e
//! verifica as revisões anteriores e posteriores. O rollback recusa apagar
//! trabalho posterior.

use crate::error::{Error, Result};
use crate::model::{CollectionManifest, FolderFile, RequestFile, SCHEMA_VERSION};
use crate::persist;
use crate::store::{FOLDER_FILE, MANIFEST_FILE, ensure_reqlivo_ext, new_id, short_uuid};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub const MIGRATION_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedFile {
    pub path: String,
    pub action: String,
    pub before_hash: String,
    pub after_hash: String,
    pub item_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationPlan {
    pub version: u32,
    pub id: String,
    pub revision: String,
    pub current_schema: u32,
    pub target_schema: u32,
    pub created_at: String,
    pub reasons: Vec<String>,
    pub files: Vec<PlannedFile>,
    pub state: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    pub required: bool,
    pub current_schema: u32,
    pub target_schema: u32,
    pub reasons: Vec<String>,
    pub files: Vec<PlannedFile>,
    pub plan_id: Option<String>,
    pub revision: Option<String>,
    pub recovery_required: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationOutcome {
    pub id: String,
    pub state: String,
    pub files: usize,
    pub backup: String,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationView {
    pub id: String,
    pub created_at: Option<String>,
    pub current_schema: Option<u32>,
    pub target_schema: Option<u32>,
    pub state: String,
    pub legacy: bool,
    pub rollback_eligible: bool,
    pub blocked_paths: Vec<String>,
    pub revision: Option<String>,
    pub error: Option<String>,
}

fn backups_dir(root: &Path) -> Result<PathBuf> {
    persist::ensure_internal_dir(root, "backups")
}

fn plan_dir(root: &Path, id: &str) -> Result<PathBuf> {
    if !persist::is_safe_id(id) {
        return Err(Error::Validation(
            "identificador de migração inválido".into(),
        ));
    }
    Ok(backups_dir(root)?.join(id))
}

fn plan_revision(plan: &MigrationPlan) -> String {
    let mut hasher = sha2::Sha256::new();
    use sha2::Digest;
    hasher.update(plan.version.to_be_bytes());
    hasher.update(plan.id.as_bytes());
    hasher.update(plan.current_schema.to_be_bytes());
    hasher.update(plan.target_schema.to_be_bytes());
    for file in &plan.files {
        hasher.update(file.path.as_bytes());
        hasher.update([0]);
        hasher.update(file.before_hash.as_bytes());
        hasher.update([0]);
        hasher.update(file.after_hash.as_bytes());
        hasher.update([b'\n']);
    }
    hex::encode(hasher.finalize())
}

fn read_plan(directory: &Path) -> std::result::Result<MigrationPlan, String> {
    let bytes = fs::read(directory.join("plan.json")).map_err(|error| error.to_string())?;
    let plan: MigrationPlan =
        serde_json::from_slice(&bytes).map_err(|error| format!("plano ilegível: {error}"))?;
    if plan.version > MIGRATION_VERSION {
        return Err("plano de versão futura".into());
    }
    if plan.version != MIGRATION_VERSION {
        return Err("versão de plano desconhecida".into());
    }
    if !persist::is_safe_id(&plan.id) {
        return Err("plano com identificador inválido".into());
    }
    Ok(plan)
}

fn yaml_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_yaml(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_yaml(root: &Path, dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "target" {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_yaml(root, &path, files)?;
        } else if matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("yml") | Some("yaml")
        ) {
            files.push(path);
        }
    }
    let _ = root;
    Ok(())
}

fn is_folder_file(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(FOLDER_FILE) | Some("folder.yaml")
    )
}

fn is_manifest(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(MANIFEST_FILE) | Some("opencollection.yaml")
    )
}

/// Lê um plano somente-leitura e produz o plano persistido quando há trabalho.
pub fn inspect(root: &Path) -> Result<Inspection> {
    recover_interrupted(root)?;
    let manifest_path = crate::store::find_manifest(root).ok_or_else(|| {
        Error::Storage(format!(
            "'{}' não contém {MANIFEST_FILE}; crie uma coleção com `reqlivo init`",
            root.display()
        ))
    })?;
    let bytes = fs::read(&manifest_path)?;
    let mut manifest: CollectionManifest = serde_yaml::from_slice(&bytes)?;
    if manifest.bundled {
        return Err(Error::Storage(
            "coleção empacotada deve ser importada, não migrada".into(),
        ));
    }
    let current = manifest
        .extensions
        .reqlivo
        .as_ref()
        .map(|ext| ext.schema_version)
        .unwrap_or(0);
    if current > SCHEMA_VERSION {
        return Err(Error::Conflict {
            code: "schema_future",
            message: format!(
                "schemaVersion {current} é mais novo que o suportado ({SCHEMA_VERSION}); atualize o Reqlivo"
            ),
        });
    }

    let mut reasons = Vec::new();
    if current < SCHEMA_VERSION {
        reasons.push(format!(
            "manifesto será migrado de schemaVersion {current} para {SCHEMA_VERSION}"
        ));
    }
    let extension = manifest.extensions.reqlivo.as_ref();
    if extension.and_then(|ext| ext.id.as_deref()).is_none() {
        reasons.push("manifesto sem ID de coleção".into());
    }
    if extension.and_then(|ext| ext.settings.as_ref()).is_none() {
        reasons.push("manifesto sem configurações do Reqlivo".into());
    }

    let files = yaml_files(root)?;
    let mut seen = BTreeSet::new();
    if let Some(id) = manifest.collection_id() {
        seen.insert(id.to_string());
    }
    let mut requests: Vec<(PathBuf, RequestFile)> = Vec::new();
    let mut folders: Vec<(PathBuf, FolderFile)> = Vec::new();
    for path in &files {
        if is_manifest(path) {
            continue;
        }
        let raw = fs::read(path)?;
        if is_folder_file(path) {
            let folder: FolderFile = serde_yaml::from_slice(&raw).map_err(|error| {
                Error::Validation(format!(
                    "{}: folder.yml inválido: {error}",
                    persist::relative(root, &path)
                ))
            })?;
            if let Some(id) = folder.reqlivo_id() {
                if !seen.insert(id.to_string()) {
                    return Err(Error::Conflict {
                        code: "duplicate_id",
                        message: format!(
                            "ID duplicado '{id}' em {}",
                            persist::relative(root, &path)
                        ),
                    });
                }
            }
            folders.push((path.clone(), folder));
        } else {
            let value: serde_yaml::Value = match serde_yaml::from_slice(&raw) {
                Ok(value) => value,
                Err(_) => continue,
            };
            let kind = value
                .get("info")
                .and_then(|info| info.get("type"))
                .and_then(|kind| kind.as_str())
                .unwrap_or("http");
            if !kind.eq_ignore_ascii_case("http") {
                continue;
            }
            let Ok(request) = serde_yaml::from_slice::<RequestFile>(&raw) else {
                continue;
            };
            if let Some(id) = request.reqlivo_id() {
                if !seen.insert(id.to_string()) {
                    return Err(Error::Conflict {
                        code: "duplicate_id",
                        message: format!(
                            "ID duplicado '{id}' em {}",
                            persist::relative(root, &path)
                        ),
                    });
                }
            }
            requests.push((path.clone(), request));
        }
    }

    let mut planned = Vec::new();
    let mut new_files: Vec<(String, Vec<u8>)> = Vec::new();

    let manifest_needs_id = manifest
        .extensions
        .reqlivo
        .as_ref()
        .and_then(|ext| ext.id.clone())
        .is_none();
    let manifest_needs_settings = manifest
        .extensions
        .reqlivo
        .as_ref()
        .and_then(|ext| ext.settings.clone())
        .is_none();
    let mut missing_environment_ids = false;
    if let Some(ext) = manifest.extensions.reqlivo.as_ref() {
        missing_environment_ids = manifest
            .config
            .environments
            .iter()
            .any(|environment| !ext.environment_ids.contains_key(&environment.name));
    }
    if current < SCHEMA_VERSION
        || manifest_needs_id
        || manifest_needs_settings
        || missing_environment_ids
    {
        let ext = ensure_reqlivo_ext(&mut manifest.extensions);
        ext.schema_version = SCHEMA_VERSION;
        if ext.id.is_none() {
            ext.id = Some(format!("col_{}", short_uuid()));
        }
        if ext.settings.is_none() {
            ext.settings = Some(Default::default());
        }
        for environment in &manifest.config.environments {
            ext.environment_ids
                .entry(environment.name.clone())
                .or_insert_with(|| format!("env_{}", short_uuid()));
        }
        let after = persist::yaml_with_header(&manifest)?;
        let relative = persist::relative(root, &manifest_path);
        planned.push(PlannedFile {
            path: relative.clone(),
            action: "manifest".into(),
            before_hash: persist::hash_bytes(&bytes),
            after_hash: persist::hash_bytes(&after),
            item_id: manifest.collection_id().map(str::to_string),
        });
        new_files.push((relative, after));
    }

    let mut next_seen = seen;
    for (path, mut request) in requests {
        let mut changed = false;
        let mut item_id = request.reqlivo_id().map(str::to_string);
        if item_id.is_none() {
            let id = new_id("req", &mut next_seen);
            ensure_reqlivo_ext(&mut request.extensions).id = Some(id.clone());
            item_id = Some(id);
            changed = true;
            reasons.push(format!("ID atribuído a {}", persist::relative(root, &path)));
        }
        if changed {
            let after = persist::yaml_with_header(&request)?;
            let relative = persist::relative(root, &path);
            planned.push(PlannedFile {
                path: relative.clone(),
                action: "assign-id".into(),
                before_hash: persist::hash_bytes(&fs::read(&path)?),
                after_hash: persist::hash_bytes(&after),
                item_id,
            });
            new_files.push((relative, after));
        }
    }
    for (path, mut folder) in folders {
        let mut changed = false;
        let mut item_id = folder.reqlivo_id().map(str::to_string);
        if item_id.is_none() {
            let id = new_id("fld", &mut next_seen);
            ensure_reqlivo_ext(&mut folder.extensions).id = Some(id.clone());
            item_id = Some(id);
            changed = true;
            reasons.push(format!("ID atribuído a {}", persist::relative(root, &path)));
        }
        if changed {
            let after = persist::yaml_with_header(&folder)?;
            let relative = persist::relative(root, &path);
            planned.push(PlannedFile {
                path: relative.clone(),
                action: "assign-id".into(),
                before_hash: persist::hash_bytes(&fs::read(&path)?),
                after_hash: persist::hash_bytes(&after),
                item_id,
            });
            new_files.push((relative, after));
        }
    }

    if planned.is_empty() {
        return Ok(Inspection {
            required: false,
            current_schema: current,
            target_schema: SCHEMA_VERSION,
            reasons,
            files: Vec::new(),
            plan_id: None,
            revision: None,
            recovery_required: false,
        });
    }

    // Reutiliza um plano já preparado para os mesmos arquivos. A inspeção pode
    // ser repetida (abertura cancelada, tela reaberta, CLI) sem acumular planos
    // nem trocar os IDs propostos.
    if let Some(plan) = matching_prepared_plan(root, &planned)? {
        return Ok(inspection_from_plan(&plan));
    }

    let id = format!("mig_{}", short_uuid());
    let directory = plan_dir(root, &id)?;
    fs::create_dir_all(&directory)?;
    let mut plan = MigrationPlan {
        version: MIGRATION_VERSION,
        id: id.clone(),
        revision: String::new(),
        current_schema: current,
        target_schema: SCHEMA_VERSION,
        created_at: chrono::Utc::now().to_rfc3339(),
        reasons: reasons.clone(),
        files: planned.clone(),
        state: "prepared".into(),
        error: None,
    };
    plan.revision = plan_revision(&plan);
    persist::atomic_write_json(&directory.join("plan.json"), &plan)?;
    for (relative_path, bytes) in new_files {
        let path = directory.join("new").join(&relative_path);
        persist::atomic_write(&path, &bytes)?;
    }
    Ok(inspection_from_plan(&plan))
}

fn inspection_from_plan(plan: &MigrationPlan) -> Inspection {
    Inspection {
        required: true,
        current_schema: plan.current_schema,
        target_schema: plan.target_schema,
        reasons: plan.reasons.clone(),
        files: plan.files.clone(),
        plan_id: Some(plan.id.clone()),
        revision: Some(plan.revision.clone()),
        recovery_required: false,
    }
}

/// Procura um plano `prepared` que descreva exatamente as mesmas entradas e
/// hashes anteriores. O plano anterior é reutilizado para preservar os IDs que
/// ele já havia proposto.
fn matching_prepared_plan(root: &Path, planned: &[PlannedFile]) -> Result<Option<MigrationPlan>> {
    let directory = backups_dir(root)?;
    let mut candidates = Vec::new();
    for entry in fs::read_dir(&directory)?.collect::<std::result::Result<Vec<_>, _>>()? {
        let path = entry.path();
        if !path.is_dir() || !path.join("plan.json").exists() {
            continue;
        }
        let Ok(plan) = read_plan(&path) else {
            continue;
        };
        if plan.state != "prepared" || plan.files.len() != planned.len() {
            continue;
        }
        let same_files = plan.files.iter().zip(planned.iter()).all(|(left, right)| {
            left.path == right.path
                && left.action == right.action
                && left.before_hash == right.before_hash
        });
        if !same_files {
            continue;
        }
        let has_proposed_bytes = plan.files.iter().all(|file| {
            fs::read(path.join("new").join(&file.path))
                .map(|bytes| persist::hash_bytes(&bytes) == file.after_hash)
                .unwrap_or(false)
        });
        if has_proposed_bytes {
            candidates.push(plan);
        }
    }
    candidates.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    Ok(candidates.pop())
}

pub fn apply(root: &Path, plan_id: &str) -> Result<MigrationOutcome> {
    let _lock = persist::CollectionLock::acquire(root)?;
    recover_interrupted(root)?;
    let directory = plan_dir(root, plan_id)?;
    let mut plan = read_plan(&directory).map_err(|message| Error::Conflict {
        code: "backup_invalid",
        message,
    })?;
    if plan.state == "applied" {
        return Ok(outcome(root, &plan));
    }
    if plan.state == "rolled_back" {
        return Err(Error::Conflict {
            code: "migration_state",
            message: "o plano já foi revertido; inspecione novamente antes de aplicar".into(),
        });
    }
    if plan.state != "prepared" && plan.state != "failed" {
        return Err(Error::Conflict {
            code: "migration_busy",
            message: format!("o plano está no estado '{}'", plan.state),
        });
    }
    if plan.revision != plan_revision(&plan) {
        return Err(Error::Conflict {
            code: "backup_invalid",
            message: "o plano foi alterado desde a inspeção".into(),
        });
    }
    // M-10: qualquer alteração entre a inspeção e a aplicação invalida o plano.
    for file in &plan.files {
        let path = persist::safe_path(root, &file.path)?;
        let current = if path.exists() {
            persist::hash_file(&path)?
        } else {
            String::new()
        };
        if current != file.before_hash {
            plan.state = "failed".into();
            plan.error = Some(format!(
                "{} mudou desde a inspeção; o plano foi invalidado",
                file.path
            ));
            persist::atomic_write_json(&directory.join("plan.json"), &plan)?;
            return Err(Error::Conflict {
                code: "revision_conflict",
                message: plan.error.clone().unwrap_or_default(),
            });
        }
    }

    // Backup completo antes da primeira escrita.
    for file in &plan.files {
        let source = persist::safe_path(root, &file.path)?;
        let backup = directory.join("originals").join(&file.path);
        if let Some(parent) = backup.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&source, &backup)?;
        if persist::hash_file(&backup)? != file.before_hash {
            return Err(Error::Conflict {
                code: "backup_invalid",
                message: format!("backup de {} não confere", file.path),
            });
        }
    }
    plan.state = "applying".into();
    persist::atomic_write_json(&directory.join("plan.json"), &plan)?;

    let result = (|| -> Result<()> {
        for file in &plan.files {
            let source = directory.join("new").join(&file.path);
            let bytes = fs::read(&source)?;
            if persist::hash_bytes(&bytes) != file.after_hash {
                return Err(Error::Conflict {
                    code: "backup_invalid",
                    message: format!("bytes propostos para {} não conferem", file.path),
                });
            }
            let destination = persist::safe_path(root, &file.path)?;
            persist::atomic_write(&destination, &bytes)?;
        }
        Ok(())
    })();

    match result {
        Ok(()) => {
            // Revalida o resultado antes de declarar sucesso.
            for file in &plan.files {
                let path = persist::safe_path(root, &file.path)?;
                if !path.exists() || persist::hash_file(&path)? != file.after_hash {
                    return fail_and_recover(root, directory, plan, "resultado não confere");
                }
            }
            let manifest_bytes = fs::read(crate::store::find_manifest(root).unwrap())?;
            let manifest: CollectionManifest = serde_yaml::from_slice(&manifest_bytes)?;
            if manifest
                .extensions
                .reqlivo
                .as_ref()
                .map(|ext| ext.schema_version)
                .unwrap_or(0)
                != SCHEMA_VERSION
            {
                return fail_and_recover(root, directory, plan, "schemaVersion não foi aplicado");
            }
            plan.state = "applied".into();
            plan.error = None;
            persist::atomic_write_json(&directory.join("plan.json"), &plan)?;
            Ok(outcome(root, &plan))
        }
        Err(error) => fail_and_recover(root, directory, plan, &error.to_string()),
    }
}

fn fail_and_recover(
    root: &Path,
    directory: PathBuf,
    mut plan: MigrationPlan,
    message: &str,
) -> Result<MigrationOutcome> {
    for file in &plan.files {
        let backup = directory.join("originals").join(&file.path);
        if backup.exists() {
            let destination = persist::safe_path(root, &file.path)?;
            persist::atomic_write(&destination, &fs::read(&backup)?)?;
        }
    }
    plan.state = "failed".into();
    plan.error = Some(message.to_string());
    persist::atomic_write_json(&directory.join("plan.json"), &plan)?;
    Err(Error::Storage(format!(
        "migração falhou e os arquivos originais foram restaurados: {message}"
    )))
}

pub fn rollback(root: &Path, plan_id: &str) -> Result<MigrationOutcome> {
    let _lock = persist::CollectionLock::acquire(root)?;
    let directory = plan_dir(root, plan_id)?;
    let mut plan = read_plan(&directory).map_err(|message| Error::Conflict {
        code: "backup_invalid",
        message,
    })?;
    if plan.revision != plan_revision(&plan) {
        return Err(Error::Conflict {
            code: "backup_invalid",
            message: "o plano foi alterado desde a inspeção".into(),
        });
    }
    if plan.state == "rolled_back" {
        return Ok(outcome(root, &plan));
    }
    if plan.state != "applied" {
        return Err(Error::Conflict {
            code: "migration_state",
            message: format!("o plano no estado '{}' não pode ser revertido", plan.state),
        });
    }
    let mut blocked = Vec::new();
    for file in &plan.files {
        let path = persist::safe_path(root, &file.path)?;
        if !path.exists() || persist::hash_file(&path)? != file.after_hash {
            blocked.push(file.path.clone());
        }
    }
    if !blocked.is_empty() {
        return Err(Error::Conflict {
            code: "revision_conflict",
            message: format!(
                "há edições posteriores nos arquivos: {}; rollback bloqueado",
                blocked.join(", ")
            ),
        });
    }
    // Cópia de segurança do estado atual antes de restaurar.
    for file in &plan.files {
        let source = persist::safe_path(root, &file.path)?;
        let safety = directory.join("rollback-current").join(&file.path);
        if let Some(parent) = safety.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&source, &safety)?;
    }
    // Confere todos os backups antes de iniciar a restauração, para não deixar
    // a coleção em `rolling_back` por um backup adulterado.
    for file in &plan.files {
        let backup = directory.join("originals").join(&file.path);
        let bytes = fs::read(&backup).map_err(|error| Error::Conflict {
            code: "backup_invalid",
            message: format!("backup de {} ausente: {error}", file.path),
        })?;
        if persist::hash_bytes(&bytes) != file.before_hash {
            return Err(Error::Conflict {
                code: "backup_invalid",
                message: format!("backup de {} não confere", file.path),
            });
        }
    }
    plan.state = "rolling_back".into();
    persist::atomic_write_json(&directory.join("plan.json"), &plan)?;
    for file in &plan.files {
        let backup = directory.join("originals").join(&file.path);
        let bytes = fs::read(&backup)?;
        let destination = persist::safe_path(root, &file.path)?;
        persist::atomic_write(&destination, &bytes)?;
    }
    for file in &plan.files {
        let path = persist::safe_path(root, &file.path)?;
        if !path.exists() || persist::hash_file(&path)? != file.before_hash {
            return Err(Error::Storage(format!(
                "rollback não restaurou {} corretamente",
                file.path
            )));
        }
    }
    plan.state = "rolled_back".into();
    plan.error = None;
    persist::atomic_write_json(&directory.join("plan.json"), &plan)?;
    Ok(outcome(root, &plan))
}

fn outcome(root: &Path, plan: &MigrationPlan) -> MigrationOutcome {
    MigrationOutcome {
        id: plan.id.clone(),
        state: plan.state.clone(),
        files: plan.files.len(),
        backup: persist::relative(
            root,
            &plan_dir(root, &plan.id).unwrap_or_else(|_| root.to_path_buf()),
        ),
        revision: plan.revision.clone(),
    }
}

pub fn list_operations(root: &Path) -> Result<Vec<OperationView>> {
    let directory = backups_dir(root)?;
    let mut operations = Vec::new();
    let mut entries: Vec<_> =
        fs::read_dir(&directory)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() && path.join("plan.json").exists() {
            match read_plan(&path) {
                Ok(plan) => {
                    let mut blocked = Vec::new();
                    if plan.state == "applied" {
                        for file in &plan.files {
                            let path = persist::safe_path(root, &file.path)?;
                            if !path.exists() || persist::hash_file(&path)? != file.after_hash {
                                blocked.push(file.path.clone());
                            }
                        }
                    }
                    let rollback_eligible = blocked.is_empty() && plan.state == "applied";
                    operations.push(OperationView {
                        id: plan.id,
                        created_at: Some(plan.created_at),
                        current_schema: Some(plan.current_schema),
                        target_schema: Some(plan.target_schema),
                        state: plan.state,
                        legacy: false,
                        rollback_eligible,
                        blocked_paths: blocked,
                        revision: Some(plan.revision),
                        error: plan.error,
                    });
                }
                Err(message) => operations.push(OperationView {
                    id: path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("desconhecido")
                        .to_string(),
                    created_at: None,
                    current_schema: None,
                    target_schema: None,
                    state: "corrupt".into(),
                    legacy: false,
                    rollback_eligible: false,
                    blocked_paths: Vec::new(),
                    revision: None,
                    error: Some(message),
                }),
            }
        } else if path.is_file() {
            operations.push(OperationView {
                id: path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("legado")
                    .to_string(),
                created_at: fs::metadata(&path)
                    .ok()
                    .and_then(|metadata| metadata.modified().ok())
                    .map(|time| chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339()),
                current_schema: None,
                target_schema: None,
                state: "legacy".into(),
                legacy: true,
                rollback_eligible: false,
                blocked_paths: Vec::new(),
                revision: None,
                error: Some(
                    "backup da v0.2.0 sem plano/hashes; recuperação manual, sem rollback automático"
                        .into(),
                ),
            });
        }
    }
    operations.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(operations)
}

/// Restaura operações interrompidas. Chamado antes de abrir uma coleção.
pub fn recover_interrupted(root: &Path) -> Result<()> {
    let directory = backups_dir(root)?;
    let entries: Vec<_> = fs::read_dir(&directory)?.collect::<std::result::Result<Vec<_>, _>>()?;
    for entry in entries {
        let path = entry.path();
        if !path.is_dir() || !path.join("plan.json").exists() {
            continue;
        }
        let Ok(mut plan) = read_plan(&path) else {
            continue;
        };
        if plan.state != "applying" && plan.state != "rolling_back" {
            continue;
        }
        for file in &plan.files {
            let backup = path.join("originals").join(&file.path);
            let bytes = fs::read(&backup).map_err(|_| Error::Conflict {
                code: "backup_invalid",
                message: format!(
                    "migração interrompida sem backup válido de {}; recuperação manual necessária",
                    file.path
                ),
            })?;
            if persist::hash_bytes(&bytes) != file.before_hash {
                return Err(Error::Conflict {
                    code: "backup_invalid",
                    message: format!("backup de {} não confere; nada foi alterado", file.path),
                });
            }
            let destination = persist::safe_path(root, &file.path)?;
            persist::atomic_write(&destination, &bytes)?;
        }
        if plan.state == "rolling_back" {
            plan.state = "rolled_back".into();
        } else {
            plan.state = "failed".into();
            plan.error = Some("aplicação interrompida; originais restaurados".into());
        }
        persist::atomic_write_json(&path.join("plan.json"), &plan)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{CollectionStore, init_collection};

    fn downgrade(root: &Path) {
        let path = crate::store::find_manifest(root).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let text = text.replace("schemaVersion: 1", "schemaVersion: 0");
        fs::write(&path, text).unwrap();
    }

    #[test]
    fn inspection_does_not_change_project_and_apply_creates_backup() {
        let dir = tempfile::tempdir().unwrap();
        init_collection(dir.path(), "Migração").unwrap();
        downgrade(dir.path());
        let before = persist::scan_tree(dir.path()).unwrap();
        let inspection = inspect(dir.path()).unwrap();
        assert!(inspection.required);
        assert_eq!(persist::scan_tree(dir.path()).unwrap(), before);
        let plan_id = inspection.plan_id.unwrap();
        let outcome = apply(dir.path(), &plan_id).unwrap();
        assert_eq!(outcome.state, "applied");
        assert!(CollectionStore::open(dir.path()).is_ok());
        let operations = list_operations(dir.path()).unwrap();
        assert!(
            operations
                .iter()
                .any(|operation| operation.id == plan_id && operation.state == "applied")
        );
    }

    #[test]
    fn rollback_restores_original_bytes_and_blocks_later_edits() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Migração").unwrap();
        let payload = store
            .create_request(None, "Echo", "GET", "http://127.0.0.1/echo")
            .unwrap();
        let request_path = store.request(&payload.id).unwrap().path.clone();
        let original = fs::read(&request_path).unwrap();
        downgrade(dir.path());
        let inspection = inspect(dir.path()).unwrap();
        let plan_id = inspection.plan_id.clone().unwrap();
        apply(dir.path(), &plan_id).unwrap();
        // Uma edição posterior fora do Reqlivo impede o rollback.
        let manifest_path = crate::store::find_manifest(dir.path()).unwrap();
        fs::write(
            &manifest_path,
            [
                fs::read(&manifest_path).unwrap(),
                b"# edicao externa\n".to_vec(),
            ]
            .concat(),
        )
        .unwrap();
        let blocked = rollback(dir.path(), &plan_id).unwrap_err();
        assert_eq!(blocked.info().code, "revision_conflict");
        let _ = (&request_path, &original);
        // Reaplica com rollback imediatamente após a migração.
        let dir2 = tempfile::tempdir().unwrap();
        init_collection(dir2.path(), "Migração").unwrap();
        downgrade(dir2.path());
        let inspection = inspect(dir2.path()).unwrap();
        let plan_id = inspection.plan_id.unwrap();
        apply(dir2.path(), &plan_id).unwrap();
        let outcome = rollback(dir2.path(), &plan_id).unwrap();
        assert_eq!(outcome.state, "rolled_back");
        assert!(inspect(dir2.path()).unwrap().required);
    }

    #[test]
    fn changed_file_between_inspection_and_apply_invalidates_plan() {
        let dir = tempfile::tempdir().unwrap();
        init_collection(dir.path(), "Migração").unwrap();
        downgrade(dir.path());
        let inspection = inspect(dir.path()).unwrap();
        let plan_id = inspection.plan_id.unwrap();
        let path = crate::store::find_manifest(dir.path()).unwrap();
        fs::write(&path, fs::read_to_string(&path).unwrap() + "# externo\n").unwrap();
        let error = apply(dir.path(), &plan_id).unwrap_err();
        assert_eq!(error.info().code, "revision_conflict");
    }

    #[test]
    fn repeating_inspection_reuses_the_same_plan_and_ids() {
        let dir = tempfile::tempdir().unwrap();
        init_collection(dir.path(), "Migração").unwrap();
        downgrade(dir.path());
        let first = inspect(dir.path()).unwrap();
        let second = inspect(dir.path()).unwrap();
        assert_eq!(first.plan_id, second.plan_id);
        assert_eq!(first.revision, second.revision);
        assert_eq!(first.files, second.files);
        let plans = fs::read_dir(backups_dir(dir.path()).unwrap())
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .map(|entry| entry.path().join("plan.json").exists())
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(plans, 1);
    }

    #[test]
    fn future_schema_is_rejected_without_touching_files() {
        let dir = tempfile::tempdir().unwrap();
        init_collection(dir.path(), "Migração").unwrap();
        let path = crate::store::find_manifest(dir.path()).unwrap();
        fs::write(
            &path,
            fs::read_to_string(&path)
                .unwrap()
                .replace("schemaVersion: 1", "schemaVersion: 9"),
        )
        .unwrap();
        let before = persist::scan_tree(dir.path()).unwrap();
        let error = inspect(dir.path()).unwrap_err();
        assert_eq!(error.info().code, "schema_future");
        assert_eq!(persist::scan_tree(dir.path()).unwrap(), before);
    }

    #[test]
    fn interrupted_application_restores_originals_and_marks_failed() {
        let dir = tempfile::tempdir().unwrap();
        init_collection(dir.path(), "Migração").unwrap();
        downgrade(dir.path());
        let inspection = inspect(dir.path()).unwrap();
        let plan_id = inspection.plan_id.unwrap();
        let directory = plan_dir(dir.path(), &plan_id).unwrap();
        let mut plan = read_plan(&directory).unwrap();
        for file in &plan.files {
            let source = persist::safe_path(dir.path(), &file.path).unwrap();
            let backup = directory.join("originals").join(&file.path);
            fs::create_dir_all(backup.parent().unwrap()).unwrap();
            fs::copy(&source, &backup).unwrap();
        }
        let before = persist::scan_tree(dir.path()).unwrap();
        let manifest = crate::store::find_manifest(dir.path()).unwrap();
        fs::write(&manifest, b"parcial: [\n").unwrap();
        plan.state = "applying".into();
        persist::atomic_write_json(&directory.join("plan.json"), &plan).unwrap();
        recover_interrupted(dir.path()).unwrap();
        assert_eq!(persist::scan_tree(dir.path()).unwrap(), before);
        let recovered = read_plan(&directory).unwrap();
        assert_eq!(recovered.state, "failed");
    }

    #[test]
    fn missing_proposed_bytes_fail_without_partial_success() {
        let dir = tempfile::tempdir().unwrap();
        init_collection(dir.path(), "Migração").unwrap();
        downgrade(dir.path());
        let inspection = inspect(dir.path()).unwrap();
        let plan_id = inspection.plan_id.unwrap();
        let before = persist::scan_tree(dir.path()).unwrap();
        let directory = plan_dir(dir.path(), &plan_id).unwrap();
        let plan = read_plan(&directory).unwrap();
        fs::remove_file(directory.join("new").join(&plan.files[0].path)).unwrap();
        let error = apply(dir.path(), &plan_id).unwrap_err();
        assert_eq!(error.info().category, crate::error::ErrorCategory::Storage);
        assert_eq!(persist::scan_tree(dir.path()).unwrap(), before);
        assert_eq!(read_plan(&directory).unwrap().state, "failed");
    }

    #[test]
    fn backup_originals_match_pre_migration_bytes() {
        let dir = tempfile::tempdir().unwrap();
        init_collection(dir.path(), "Migração").unwrap();
        downgrade(dir.path());
        let before = persist::scan_tree(dir.path()).unwrap();
        let inspection = inspect(dir.path()).unwrap();
        let plan_id = inspection.plan_id.unwrap();
        apply(dir.path(), &plan_id).unwrap();
        let directory = plan_dir(dir.path(), &plan_id).unwrap();
        let plan = read_plan(&directory).unwrap();
        for file in &plan.files {
            let backup = fs::read(directory.join("originals").join(&file.path)).unwrap();
            assert_eq!(persist::hash_bytes(&backup), file.before_hash);
            assert_eq!(before.get(&file.path), Some(&file.before_hash));
        }
    }

    #[test]
    fn rollback_keeps_independent_files_and_restores_touched_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = init_collection(dir.path(), "Migração").unwrap();
        let payload = store
            .create_request(None, "Independente", "GET", "http://127.0.0.1/echo")
            .unwrap();
        let request_path = store.request(&payload.id).unwrap().path.clone();
        let manifest_path = crate::store::find_manifest(dir.path()).unwrap();
        downgrade(dir.path());
        let request_before = fs::read(&request_path).unwrap();
        let manifest_before = fs::read(&manifest_path).unwrap();
        let inspection = inspect(dir.path()).unwrap();
        let plan_id = inspection.plan_id.unwrap();
        apply(dir.path(), &plan_id).unwrap();
        let independent = dir.path().join("independente.yml");
        fs::write(&independent, b"nao: [migrado]\n").unwrap();
        let outcome = rollback(dir.path(), &plan_id).unwrap();
        assert_eq!(outcome.state, "rolled_back");
        assert_eq!(fs::read(&manifest_path).unwrap(), manifest_before);
        assert_eq!(fs::read(&request_path).unwrap(), request_before);
        assert_eq!(fs::read(&independent).unwrap(), b"nao: [migrado]\n");
    }

    #[test]
    fn corrupt_and_legacy_backups_are_listed_without_writes() {
        let dir = tempfile::tempdir().unwrap();
        init_collection(dir.path(), "Migração").unwrap();
        let legacy = backups_dir(dir.path()).unwrap().join("v020-backup.tar.gz");
        fs::write(&legacy, b"historico").unwrap();
        let corrupt = backups_dir(dir.path()).unwrap().join("mig_corrompido");
        fs::create_dir_all(&corrupt).unwrap();
        fs::write(corrupt.join("plan.json"), b"{ invalido").unwrap();
        let operations = list_operations(dir.path()).unwrap();
        assert!(operations.iter().any(|operation| operation.legacy));
        assert!(
            operations
                .iter()
                .any(|operation| operation.state == "corrupt" && operation.id == "mig_corrompido")
        );
        assert_eq!(fs::read(&legacy).unwrap(), b"historico");
    }
}
