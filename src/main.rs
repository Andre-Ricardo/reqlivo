use clap::{Parser, Subcommand};
use reqlivo::error::Error;
use reqlivo::import_export::{self, ExportFormat, ImportFormat};
use reqlivo::log::LocalLog;
use reqlivo::runner::{self, ExecuteOptions, RunFilters, RunOptions, RunReport, RunStatus};
use reqlivo::store::CollectionStore;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(
    name = "reqlivo",
    version = reqlivo::VERSION,
    about = "Cliente de APIs local-first baseado em OpenCollection"
)]
struct Cli {
    /// Diretório da coleção (procura opencollection.yml no diretório ou acima).
    #[arg(long, global = true, default_value = ".")]
    collection: PathBuf,
    /// Emite resultados em JSON.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Cria uma coleção local.
    Init {
        /// Diretório da nova coleção (padrão: --collection).
        path: Option<PathBuf>,
        /// Nome exibido da coleção.
        #[arg(long)]
        name: Option<String>,
    },
    /// Executa a coleção ou um filtro dela.
    Run {
        #[arg(long)]
        env: Option<String>,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long = "request")]
        requests: Vec<String>,
        #[arg(long)]
        folder: Option<String>,
        /// Salva o relatório JSON no caminho informado.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Envia uma requisição salva e mostra a resposta.
    Send {
        request_id: String,
        #[arg(long)]
        env: Option<String>,
    },
    /// Importa cURL, Postman ou OpenCollection.
    Import {
        format: String,
        /// Arquivo, diretório OpenCollection ou comando cURL inline.
        source: String,
        #[arg(long)]
        folder: Option<String>,
    },
    /// Exporta cURL, Postman, OpenCollection ou o projeto inteiro.
    Export {
        format: String,
        request_id: Option<String>,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Valida a coleção e lista problemas.
    Validate,
    /// Lista requisições em ordem de execução.
    Requests,
    /// Inspeciona ambientes.
    Env {
        #[command(subcommand)]
        command: EnvCommand,
    },
    /// Mostra o histórico local de execuções.
    History {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
}

#[derive(Subcommand, Debug)]
enum EnvCommand {
    /// Lista ambientes.
    List,
    /// Mostra um ambiente sem revelar segredos.
    Show { name: String },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let code = match execute(cli).await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("erro: {error}");
            1
        }
    };
    std::process::exit(code);
}

async fn execute(cli: Cli) -> reqlivo::Result<i32> {
    match cli.command {
        Command::Init { path, name } => {
            let root = path.unwrap_or(cli.collection);
            if root.exists() && root.read_dir()?.next().is_some() {
                return Err(Error::Storage(format!(
                    "o diretório '{}' não está vazio",
                    root.display()
                )));
            }
            let name = name.unwrap_or_else(|| {
                root.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Minha coleção")
                    .to_string()
            });
            let store = reqlivo::store::init_collection(&root, &name)?;
            if cli.json {
                print_json(&serde_json::json!({
                    "root": store.root,
                    "name": store.manifest.info.name,
                    "manifestRevision": store.manifest_revision,
                }))?;
            } else {
                println!("Coleção '{}' criada em {}", name, store.root.display());
                println!(
                    "Abra esta pasta no aplicativo Reqlivo ou execute: reqlivo --collection {} validate",
                    store.root.display()
                );
            }
            Ok(0)
        }
        Command::Run {
            env,
            tags,
            requests,
            folder,
            output,
        } => {
            let store = open_collection(&cli.collection)?;
            let options = RunOptions {
                environment: env,
                filters: RunFilters {
                    tags,
                    request_ids: requests,
                    folder_id: folder,
                },
                cookie_jar: Arc::new(reqwest::cookie::Jar::default()),
                cancellation: None,
            };
            let report = runner::run_collection(&store, options, BTreeMap::new()).await?;
            LocalLog::new(&store.root).history(&report);
            if let Some(path) = output {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&path, serde_json::to_string_pretty(&report)?)?;
            }
            if cli.json {
                print_json(&report)?;
            } else {
                print_report(&report);
            }
            Ok(exit_for_report(&report))
        }
        Command::Send { request_id, env } => {
            let store = open_collection(&cli.collection)?;
            let request = store.request(&request_id)?.file.clone();
            let outcome = runner::execute_single(
                &store,
                &request_id,
                &request,
                ExecuteOptions {
                    environment: env,
                    cookie_jar: Arc::new(reqwest::cookie::Jar::default()),
                    cancellation: None,
                },
            )
            .await?;
            LocalLog::new(&store.root).event(
                "info",
                "cli.send",
                "requisição enviada pela CLI",
                serde_json::json!({
                    "requestId": request_id,
                    "status": outcome.run.status,
                    "httpStatus": outcome.run.http_status,
                }),
                &[],
            );
            if cli.json {
                print_json(&serde_json::json!({
                    "run": outcome.run,
                    "response": outcome.response,
                }))?;
            } else {
                print_send(&outcome.run, outcome.response.as_ref());
            }
            Ok(
                if matches!(
                    outcome.run.status,
                    RunStatus::Failed | RunStatus::Error | RunStatus::Cancelled
                ) {
                    1
                } else {
                    0
                },
            )
        }
        Command::Import {
            format,
            source,
            folder,
        } => {
            let format = ImportFormat::parse(&format)?;
            let mut store = open_collection(&cli.collection)?;
            let report = if source.trim_start().starts_with("curl ") {
                import_export::import_content(
                    &mut store,
                    folder.as_deref(),
                    format,
                    &source,
                    Some("inline"),
                )?
            } else {
                import_export::import_path(
                    &mut store,
                    folder.as_deref(),
                    format,
                    &PathBuf::from(&source),
                )?
            };
            if cli.json {
                print_json(&report)?;
            } else {
                println!(
                    "Importação {}: {} requisição(ões), {} pasta(s), {} ambiente(s).",
                    report.format,
                    report.requests_imported,
                    report.folders_imported,
                    report.environments_imported
                );
                for warning in &report.warnings {
                    println!("aviso: {warning}");
                }
                for loss in &report.losses {
                    println!("perda: {} · {} · {}", loss.item, loss.field, loss.reason);
                }
                if let Some(original) = &report.original_saved {
                    println!("original preservado em {original}");
                }
            }
            Ok(0)
        }
        Command::Export {
            format,
            request_id,
            output,
        } => {
            let store = open_collection(&cli.collection)?;
            let format = ExportFormat::parse(&format)?;
            let report = if format == ExportFormat::Project {
                let destination = output.ok_or_else(|| {
                    Error::Export("exportação de projeto exige --output <diretório>".into())
                })?;
                import_export::export_project(&store, &destination)?
            } else {
                let mut report = import_export::export(&store, format, request_id.as_deref())?;
                if let Some(path) = output {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    if let Some(content) = &report.content {
                        std::fs::write(&path, content)?;
                        report.destination = Some(path.display().to_string());
                    }
                }
                report
            };
            if cli.json {
                print_json(&report)?;
            } else if let Some(content) = &report.content {
                if report.destination.is_none() {
                    print!("{content}");
                } else {
                    println!(
                        "Exportação {} gravada em {}.",
                        report.format,
                        report.destination.as_deref().unwrap_or_default()
                    );
                }
            } else {
                println!(
                    "Exportação {} gravada em {} ({} arquivo(s)).",
                    report.format,
                    report.destination.as_deref().unwrap_or_default(),
                    report.files
                );
            }
            for warning in &report.warnings {
                eprintln!("aviso: {warning}");
            }
            for loss in &report.losses {
                eprintln!("perda: {} · {} · {}", loss.item, loss.field, loss.reason);
            }
            Ok(0)
        }
        Command::Validate => {
            let store = open_collection(&cli.collection)?;
            let issues = store.validate();
            if cli.json {
                print_json(&serde_json::json!({
                    "valid": issues.is_empty() && store.unsupported.is_empty(),
                    "issues": issues,
                    "warnings": store.warnings,
                    "unsupported": store.unsupported,
                }))?;
            } else {
                if issues.is_empty() && store.unsupported.is_empty() {
                    println!("Coleção válida: {} requisição(ões).", store.requests.len());
                } else {
                    for issue in &issues {
                        println!("problema: {issue}");
                    }
                    for item in &store.unsupported {
                        println!(
                            "não suportado: {} ({}) · {}",
                            item.path, item.kind, item.reason
                        );
                    }
                }
                for warning in &store.warnings {
                    println!("aviso: {warning}");
                }
            }
            Ok(if issues.is_empty() && store.unsupported.is_empty() {
                0
            } else {
                1
            })
        }
        Command::Requests => {
            let store = open_collection(&cli.collection)?;
            if cli.json {
                let requests: Vec<serde_json::Value> = store
                    .all_requests_in_order()
                    .into_iter()
                    .map(|request| {
                        serde_json::json!({
                            "id": request.id,
                            "name": request.name,
                            "method": request.file.http.method,
                            "url": request.file.http.url,
                            "path": request.rel_path,
                            "tags": request.tags,
                        })
                    })
                    .collect();
                print_json(&serde_json::json!({ "requests": requests }))?;
            } else {
                for request in store.all_requests_in_order() {
                    println!(
                        "{}  {:<6} {}  [{}]",
                        request.id, request.file.http.method, request.name, request.rel_path
                    );
                }
            }
            Ok(0)
        }
        Command::Env { command } => {
            let store = open_collection(&cli.collection)?;
            match command {
                EnvCommand::List => {
                    if cli.json {
                        print_json(&serde_json::json!({
                            "default": store.default_environment(),
                            "environments": store.manifest.environments(),
                        }))?;
                    } else {
                        for environment in store.manifest.environments() {
                            let marker = if store.default_environment().as_deref()
                                == Some(environment.name.as_str())
                            {
                                " (padrão)"
                            } else {
                                ""
                            };
                            println!(
                                "{}{} · {} variável(is)",
                                environment.name,
                                marker,
                                environment.variables.len()
                            );
                        }
                    }
                    Ok(0)
                }
                EnvCommand::Show { name } => {
                    let environment = store
                        .manifest
                        .environments()
                        .iter()
                        .find(|environment| environment.name == name)
                        .ok_or_else(|| {
                            Error::Storage(format!("ambiente '{name}' não encontrado"))
                        })?;
                    let refs: Vec<_> = store
                        .manifest
                        .secret_refs()
                        .iter()
                        .filter(|reference| reference.environment.as_deref() == Some(name.as_str()))
                        .collect();
                    if cli.json {
                        print_json(&serde_json::json!({
                            "environment": environment,
                            "secretRefs": refs.iter().map(|reference| serde_json::json!({
                                "name": reference.name,
                                "provider": reference.provider,
                                "key": reference.key,
                                "available": reference.provider == reqlivo::model::SecretProvider::Env
                                    && std::env::var(&reference.key).is_ok(),
                            })).collect::<Vec<_>>(),
                        }))?;
                    } else {
                        for variable in &environment.variables {
                            if variable.secret {
                                let reference = refs
                                    .iter()
                                    .find(|reference| reference.name == variable.name);
                                let state = reference
                                    .map(|reference| {
                                        if reference.provider == reqlivo::model::SecretProvider::Env
                                        {
                                            if std::env::var(&reference.key).is_ok() {
                                                "definido no processo"
                                            } else {
                                                "ausente no processo"
                                            }
                                        } else {
                                            "provider não implementado"
                                        }
                                    })
                                    .unwrap_or("sem referência");
                                println!("{} = <segredo; {}>", variable.name, state);
                            } else {
                                println!(
                                    "{} = {}",
                                    variable.name,
                                    variable.text_value().unwrap_or_default()
                                );
                            }
                        }
                    }
                    Ok(0)
                }
            }
        }
        Command::History { limit } => {
            let store = open_collection(&cli.collection)?;
            let runs = LocalLog::new(&store.root).read_history(limit);
            if cli.json {
                print_json(&serde_json::json!({ "runs": runs }))?;
            } else if runs.is_empty() {
                println!("Sem execuções no histórico local.");
            } else {
                for run in runs {
                    println!(
                        "{} · {} · {}",
                        run.get("ts").and_then(|value| value.as_str()).unwrap_or(""),
                        run.get("runId")
                            .and_then(|value| value.as_str())
                            .unwrap_or(""),
                        run.get("deterministicHash")
                            .and_then(|value| value.as_str())
                            .unwrap_or("")
                    );
                }
            }
            Ok(0)
        }
    }
}

fn open_collection(path: &Path) -> reqlivo::Result<CollectionStore> {
    let root = discover_root(path);
    CollectionStore::open(&root)
}

fn discover_root(path: &Path) -> PathBuf {
    let mut current = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    if current.is_file() {
        current.pop();
    }
    loop {
        if current.join("opencollection.yml").is_file()
            || current.join("opencollection.yaml").is_file()
        {
            return current;
        }
        if !current.pop() {
            return path.to_path_buf();
        }
    }
}

fn print_json(value: &impl serde::Serialize) -> reqlivo::Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|error| Error::Internal(error.to_string()))?
    );
    Ok(())
}

fn print_report(report: &RunReport) {
    println!(
        "Execução {} · ambiente {}",
        report.id,
        report.environment.as_deref().unwrap_or("nenhum")
    );
    println!(
        "Total {} · executadas {} · passaram {} · falharam {} · ignoradas {} · erros {} · canceladas {}",
        report.summary.total,
        report.summary.executed,
        report.summary.passed,
        report.summary.failed,
        report.summary.skipped,
        report.summary.errors,
        report.summary.cancelled
    );
    for request in &report.requests {
        let status = match request.status {
            RunStatus::Executed => "executada",
            RunStatus::Passed => "passou",
            RunStatus::Failed => "falhou",
            RunStatus::Skipped => "ignorada",
            RunStatus::Cancelled => "cancelada",
            RunStatus::Error => "erro",
        };
        println!(
            "[{}] {} · {} · {} ms",
            request
                .http_status
                .map(|status| status.to_string())
                .unwrap_or_else(|| "-".into()),
            request.name,
            status,
            request.duration_ms
        );
        for test in request.tests.iter().filter(|test| !test.passed) {
            println!("    teste falhou: {} · {}", test.name, test.message);
        }
        if let Some(error) = &request.error {
            println!("    erro [{}]: {}", error.code, error.message);
        }
        if let Some(reason) = &request.skip_reason {
            println!("    motivo: {reason}");
        }
    }
    println!("hash determinístico: {}", report.deterministic_hash);
}

fn print_send(
    run: &reqlivo::runner::RequestRun,
    response: Option<&reqlivo::transport::HttpResponse>,
) {
    if let Some(error) = &run.error {
        println!(
            "{} {} · erro [{}]: {}",
            run.method, run.url, error.code, error.message
        );
        return;
    }
    if let Some(response) = response {
        println!(
            "{} {} -> {} {} · {} ms · {} bytes{}",
            run.method,
            run.url,
            response.status,
            response.status_text,
            response.time_ms,
            response.size_bytes,
            if response.truncated {
                " (truncado)"
            } else {
                ""
            }
        );
        if response.redirected {
            println!("URL final: {}", response.final_url);
        }
        if let Some(content_type) = &response.content_type {
            println!("content-type: {content_type}");
        }
        let passed = run.tests.iter().filter(|test| test.passed).count();
        let failed = run.tests.iter().filter(|test| !test.passed).count();
        if passed + failed > 0 {
            println!("testes: {passed} passaram, {failed} falharam");
            for test in run.tests.iter().filter(|test| !test.passed) {
                println!("  falhou: {} · {}", test.name, test.message);
            }
        }
        if let Some(body) = &response.body_text {
            println!("--- corpo ---");
            let limit = 4000;
            let mut end = body.len().min(limit);
            while !body.is_char_boundary(end) {
                end -= 1;
            }
            print!("{}", &body[..end]);
            if body.len() > limit {
                println!("\n… (saída truncada em {limit} bytes)");
            } else if !body.ends_with('\n') {
                println!();
            }
        } else if response.is_binary {
            println!("--- corpo binário ({} bytes) ---", response.size_bytes);
        }
    }
}

fn exit_for_report(report: &RunReport) -> i32 {
    if report.summary.failed > 0 || report.summary.errors > 0 || report.summary.cancelled > 0 {
        1
    } else {
        0
    }
}
