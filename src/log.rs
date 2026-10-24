use crate::redact::{redact_json, redact_text};
use crate::runner::RunReport;
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct LocalLog {
    directory: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl LocalLog {
    pub fn new(collection_root: impl AsRef<Path>) -> Self {
        let directory = collection_root.as_ref().join(".reqlivo");
        Self {
            directory,
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn event(
        &self,
        level: &str,
        event: &str,
        message: &str,
        mut fields: Value,
        secrets: &[String],
    ) {
        redact_json(&mut fields, secrets);
        let entry = json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "level": level,
            "event": event,
            "message": redact_text(message, secrets),
            "fields": fields,
        });
        self.append_jsonl("logs/events.jsonl", &entry);
    }

    pub fn history(&self, report: &RunReport) {
        let summary = json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "runId": report.id,
            "collection": report.collection,
            "environment": report.environment,
            "summary": report.summary,
            "deterministicHash": report.deterministic_hash,
            "statuses": report.requests.iter().map(|request| json!({
                "requestId": request.request_id,
                "name": request.name,
                "status": request.status,
                "httpStatus": request.http_status,
                "durationMs": request.duration_ms,
            })).collect::<Vec<_>>(),
        });
        self.append_jsonl("history/runs.jsonl", &summary);
    }

    pub fn read_events(&self, limit: usize) -> Vec<Value> {
        self.read_jsonl("logs/events.jsonl", limit)
    }

    pub fn read_history(&self, limit: usize) -> Vec<Value> {
        self.read_jsonl("history/runs.jsonl", limit)
    }

    pub fn clear_history(&self) -> std::io::Result<()> {
        let path = self.directory.join("history/runs.jsonl");
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    fn append_jsonl(&self, relative: &str, value: &Value) {
        let path = self.directory.join(relative);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            if let Ok(line) = serde_json::to_string(value) {
                let _ = writeln!(file, "{line}");
            }
        }
    }

    fn read_jsonl(&self, relative: &str, limit: usize) -> Vec<Value> {
        let path = self.directory.join(relative);
        let Ok(file) = fs::File::open(path) else {
            return Vec::new();
        };
        let mut values: Vec<Value> = BufReader::new(file)
            .lines()
            .map_while(Result::ok)
            .filter_map(|line| serde_json::from_str(&line).ok())
            .collect();
        if values.len() > limit {
            values = values.split_off(values.len() - limit);
        }
        values
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_redacts_secret_values_and_headers() {
        let dir = tempfile::tempdir().unwrap();
        let log = LocalLog::new(dir.path());
        log.event(
            "info",
            "request",
            "enviando com super-secret",
            json!({
                "authorization": "Bearer super-secret",
                "path": "/echo",
                "note": "super-secret"
            }),
            &["super-secret".to_string()],
        );
        let events = log.read_events(10);
        assert_eq!(events.len(), 1);
        let text = serde_json::to_string(&events[0]).unwrap();
        assert!(!text.contains("super-secret"));
    }
}
