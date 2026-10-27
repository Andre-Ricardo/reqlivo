//! Benchmark reproduzível de reconciliação com muitas requisições.
//!
//! O exemplo cria uma coleção temporária com N arquivos YAML de requisição,
//! mede a abertura inicial, a verificação de alterações (o mesmo caminho usado
//! pelo polling da UI), o snapshot e a recarga após uma edição externa.
//!
//! Uso:
//!   cargo run --release --example bench_reconciliation -- 1000
//!
//! O diretório temporário é removido ao final. O resultado é JSON no stdout.

use reqlivo::model::{
    Extensions, HttpDetails, ReqlivoExtension, RequestFile, RequestInfo, RequestRuntime,
    SCHEMA_VERSION,
};
use reqlivo::store::{CollectionStore, init_collection};
use serde_json::json;
use std::fs;
use std::hint::black_box;
use std::path::Path;
use std::time::{Duration, Instant};

fn main() -> reqlivo::Result<()> {
    let count = std::env::args()
        .nth(1)
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|error| reqlivo::Error::Validation(format!("contagem inválida: {error}")))?
        .unwrap_or(1_000);
    if count == 0 {
        return Err(reqlivo::Error::Validation(
            "a contagem deve ser maior que zero".into(),
        ));
    }

    let temp = tempfile::tempdir()?;
    let root = temp.path().join("bench");
    fs::create_dir_all(&root)?;
    init_collection(&root, "Benchmark 1.000 requisições")?;

    let request_dir = root.clone();
    let mut bytes = 0usize;
    for index in 0..count {
        let id = format!("req_{index:016x}");
        let file = RequestFile {
            info: RequestInfo {
                name: format!("Requisição {index:04}"),
                kind: "http".into(),
                seq: index as i64 + 1,
                tags: Vec::new(),
                description: None,
                extra: Default::default(),
            },
            http: HttpDetails {
                method: "GET".into(),
                url: "http://127.0.0.1:18080/echo".into(),
                headers: Vec::new(),
                params: Vec::new(),
                body: None,
                auth: None,
            },
            runtime: RequestRuntime::default(),
            settings: Default::default(),
            docs: None,
            extensions: Extensions {
                reqlivo: Some(ReqlivoExtension {
                    schema_version: SCHEMA_VERSION,
                    id: Some(id),
                    ..ReqlivoExtension::default()
                }),
                extra: Default::default(),
            },
            extra: Default::default(),
        };
        let path = request_dir.join(format!("request-{index:04}.yml"));
        let body = serde_yaml::to_string(&file)?;
        bytes += body.len();
        fs::write(path, body)?;
    }

    let open_started = Instant::now();
    let store = CollectionStore::open(&root)?;
    let open = open_started.elapsed();

    let mut change_times = Vec::with_capacity(20);
    for _ in 0..20 {
        let started = Instant::now();
        let (generation, paths) = store.changes(&root)?;
        black_box(generation);
        black_box(paths);
        change_times.push(started.elapsed());
    }

    let mut tree_times = Vec::with_capacity(20);
    let mut request_count = 0usize;
    for _ in 0..20 {
        let started = Instant::now();
        let tree = store.tree();
        request_count = count_requests(&tree);
        tree_times.push(started.elapsed());
        black_box(tree);
    }

    let mut snapshot_times = Vec::with_capacity(20);
    let mut snapshot_bytes = 0usize;
    for _ in 0..20 {
        let started = Instant::now();
        let snapshot = store.snapshot();
        snapshot_bytes = serde_json::to_vec(&snapshot)?.len();
        snapshot_times.push(started.elapsed());
        black_box(snapshot);
    }

    // Edição externa de um único arquivo: mede a detecção incremental e a
    // recarga completa, que é o custo pago quando a UI precisa reconciliar.
    let changed_path = request_dir.join("request-0000.yml");
    let original = fs::read_to_string(&changed_path)?;
    fs::write(
        &changed_path,
        original.replace(
            "http://127.0.0.1:18080/echo",
            "http://127.0.0.1:18080/echo?external=1",
        ),
    )?;
    let detect_started = Instant::now();
    let (_, changed_paths) = store.changes(&root)?;
    let detect = detect_started.elapsed();
    let reload_started = Instant::now();
    let reloaded = CollectionStore::open(&root)?;
    let reload = reload_started.elapsed();
    black_box(reloaded);

    let output = json!({
        "requests": count,
        "requestFiles": count,
        "requestBytes": bytes,
        "manifestAndStateBytes": directory_size(&root),
        "version": env!("CARGO_PKG_VERSION"),
        "openMs": ms(open),
        "changesMs": stats(&change_times),
        "treeMs": stats(&tree_times),
        "treeRequests": request_count,
        "snapshotMs": stats(&snapshot_times),
        "snapshotBytes": snapshot_bytes,
        "externalDetectionMs": ms(detect),
        "externalChangedPaths": changed_paths.len(),
        "reloadMs": ms(reload),
        "maxRssKb": max_rss_kb(),
        "machine": machine(),
        "platform": std::env::consts::OS,
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn stats(values: &[Duration]) -> serde_json::Value {
    let mut millis: Vec<f64> = values.iter().map(|value| ms(*value)).collect();
    millis.sort_by(f64::total_cmp);
    let sum: f64 = millis.iter().sum();
    json!({
        "samples": millis.len(),
        "minMs": millis.first().copied().unwrap_or(0.0),
        "avgMs": if millis.is_empty() { 0.0 } else { sum / millis.len() as f64 },
        "p95Ms": percentile(&millis, 0.95),
        "maxMs": millis.last().copied().unwrap_or(0.0),
    })
}

fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f64 * fraction).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

fn count_requests(tree: &[reqlivo::store::TreeNode]) -> usize {
    tree.iter()
        .map(|node| match node {
            reqlivo::store::TreeNode::Request { .. } => 1,
            reqlivo::store::TreeNode::Folder { children, .. } => count_requests(children),
        })
        .sum()
}

fn directory_size(root: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let Ok(entries) = fs::read_dir(&path) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(metadata) = entry.metadata() {
                total += metadata.len();
            }
        }
    }
    total
}

fn max_rss_kb() -> u64 {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status.lines().find_map(|line| {
                line.strip_prefix("VmHWM:")
                    .and_then(|value| value.split_whitespace().next())
                    .and_then(|value| value.parse().ok())
            })
        })
        .unwrap_or(0)
}

fn machine() -> String {
    fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|cpuinfo| {
            cpuinfo.lines().find_map(|line| {
                line.strip_prefix("model name")
                    .and_then(|value| value.split_once(':'))
                    .map(|(_, value)| value.trim().to_string())
            })
        })
        .unwrap_or_else(|| "desconhecida".into())
}
