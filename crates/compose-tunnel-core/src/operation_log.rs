use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, ErrorKind, Write},
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::{app_paths, Result};

const MAX_HISTORY: usize = 200;
const OPERATION_TARGET: &str = "compose_tunnel::operation";
static WRITE_LOCK: Mutex<()> = Mutex::new(());
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OperationLevel {
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationLogEntry {
    pub id: String,
    pub timestamp: String,
    pub level: OperationLevel,
    pub operation: String,
    pub target: String,
    pub outcome: String,
    pub detail: Option<String>,
}

fn new_entry(
    at: DateTime<Utc>,
    level: OperationLevel,
    operation: &str,
    target: &str,
    outcome: &str,
    detail: Option<&str>,
) -> OperationLogEntry {
    let timestamp = at.to_rfc3339_opts(SecondsFormat::Millis, true);
    OperationLogEntry {
        id: format!(
            "{}-{}-{}",
            timestamp,
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ),
        timestamp,
        level,
        operation: operation.to_string(),
        target: target.to_string(),
        outcome: outcome.to_string(),
        detail: detail.map(ToString::to_string),
    }
}

pub(crate) fn record_operation(
    level: OperationLevel,
    operation: &str,
    target: &str,
    outcome: &str,
    detail: Option<&str>,
) {
    match app_paths() {
        Ok(paths) => {
            record_operation_in(&paths.logs_dir, level, operation, target, outcome, detail)
        }
        Err(_) => eprintln!("compose-tunnel: operation log directory is unavailable"),
    }
}

pub(crate) fn record_operation_in(
    log_dir: &Path,
    level: OperationLevel,
    operation: &str,
    target: &str,
    outcome: &str,
    detail: Option<&str>,
) {
    let entry = new_entry(Utc::now(), level, operation, target, outcome, detail);
    match append_entry(log_dir, &entry) {
        Ok(()) => emit_entry(&entry),
        Err(_) => {
            eprintln!("compose-tunnel: operation log could not be persisted");
            let mut failed_entry = entry;
            failed_entry.detail = Some("operation log could not be persisted".to_string());
            emit_entry(&failed_entry);
        }
    }
}

#[cfg(test)]
pub(crate) fn record_operation_at(
    log_dir: &Path,
    at: DateTime<Utc>,
    level: OperationLevel,
    operation: &str,
    target: &str,
    outcome: &str,
    detail: Option<&str>,
) -> Result<OperationLogEntry> {
    let entry = new_entry(at, level, operation, target, outcome, detail);
    append_entry(log_dir, &entry)?;
    Ok(entry)
}

fn append_entry(log_dir: &Path, entry: &OperationLogEntry) -> Result<()> {
    let _guard = WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    fs::create_dir_all(log_dir)?;
    let date = entry
        .timestamp
        .get(..10)
        .expect("RFC3339 timestamp has a date");
    let path = log_dir.join(format!("operations-{date}.jsonl"));
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let mut line = serde_json::to_vec(entry)?;
    line.push(b'\n');
    file.write_all(&line)?;
    Ok(())
}

fn emit_entry(entry: &OperationLogEntry) {
    if let Ok(payload) = serde_json::to_string(entry) {
        let severity = match entry.level {
            OperationLevel::Info => log::Level::Info,
            OperationLevel::Error => log::Level::Error,
        };
        log::log!(target: OPERATION_TARGET, severity, "{payload}");
    }
}

pub fn read_operation_logs(limit: usize) -> Result<Vec<OperationLogEntry>> {
    read_operation_logs_at(&app_paths()?.logs_dir, limit)
}

pub(crate) fn read_operation_logs_at(
    log_dir: &Path,
    limit: usize,
) -> Result<Vec<OperationLogEntry>> {
    let limit = limit.min(MAX_HISTORY);
    if limit == 0 {
        return Ok(Vec::new());
    }
    let files = match fs::read_dir(log_dir) {
        Ok(files) => files,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut paths = files
        .filter_map(|file| file.ok().map(|file| file.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix("operations-"))
                .and_then(|name| name.strip_suffix(".jsonl"))
                .is_some_and(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok())
        })
        .collect::<Vec<_>>();
    paths.sort();

    let mut entries = Vec::new();
    for path in paths.into_iter().rev() {
        let file = fs::File::open(path)?;
        let remaining = limit - entries.len();
        let mut daily = VecDeque::with_capacity(remaining);
        for line in BufReader::new(file).lines() {
            if let Some(entry) = line
                .ok()
                .and_then(|line| serde_json::from_str::<OperationLogEntry>(&line).ok())
            {
                if daily.len() == remaining {
                    daily.pop_front();
                }
                daily.push_back(entry);
            }
        }
        entries.extend(daily.into_iter().rev());
        if entries.len() >= limit {
            break;
        }
    }
    entries.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct TempLogDir(PathBuf);

    impl TempLogDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "compose-tunnel-operation-log-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).expect("temporary log directory");
            Self(dir)
        }
    }

    impl Drop for TempLogDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn timestamp(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("valid timestamp")
            .with_timezone(&Utc)
    }

    #[test]
    fn operation_log_appends_daily_entries_and_reads_recent_history() {
        let dir = TempLogDir::new();
        let first = record_operation_at(
            &dir.0,
            timestamp("2026-09-20T23:59:59Z"),
            OperationLevel::Info,
            "Docker inspect",
            "staging/app/db",
            "attempt",
            None,
        )
        .expect("first operation");
        let second = record_operation_at(
            &dir.0,
            timestamp("2026-09-21T00:00:01Z"),
            OperationLevel::Error,
            "Docker inspect",
            "staging/app/db",
            "failure",
            Some("exit code 7"),
        )
        .expect("second operation");

        assert_ne!(first.id, second.id);
        assert!(dir.0.join("operations-2026-09-20.jsonl").exists());
        assert!(dir.0.join("operations-2026-09-21.jsonl").exists());
        assert_eq!(
            read_operation_logs_at(&dir.0, 200)
                .expect("history")
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            [first.id.as_str(), second.id.as_str()]
        );
        assert_eq!(
            read_operation_logs_at(&dir.0, 1).expect("latest")[0].id,
            second.id
        );
    }

    #[test]
    fn operation_log_keeps_only_recent_entries_from_a_busy_day() {
        let dir = TempLogDir::new();
        let at = timestamp("2026-09-21T01:00:00Z");
        let mut last_ids = Vec::new();
        for index in 0..250 {
            let entry = record_operation_at(
                &dir.0,
                at,
                OperationLevel::Info,
                "Docker inspect",
                "staging/app/db",
                "success",
                None,
            )
            .expect("operation");
            if index >= 248 {
                last_ids.push(entry.id);
            }
        }

        let entries = read_operation_logs_at(&dir.0, 2).expect("latest entries");
        assert_eq!(
            entries.iter().map(|entry| &entry.id).collect::<Vec<_>>(),
            last_ids.iter().collect::<Vec<_>>()
        );
    }

    #[test]
    fn operation_log_skips_broken_lines_without_losing_valid_entries() {
        let dir = TempLogDir::new();
        let entry = record_operation_at(
            &dir.0,
            timestamp("2026-09-21T01:00:00Z"),
            OperationLevel::Info,
            "SSH connectivity",
            "staging",
            "success",
            None,
        )
        .expect("operation");
        let path = dir.0.join("operations-2026-09-21.jsonl");
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .expect("log file");
        use std::io::Write;
        file.write_all(b"{\"partial\":\n").expect("partial line");

        assert_eq!(
            read_operation_logs_at(&dir.0, 200).expect("history")[0].id,
            entry.id
        );
    }

    #[test]
    fn operation_log_missing_directory_is_empty() {
        let dir = TempLogDir::new();
        assert!(read_operation_logs_at(&dir.0.join("missing"), 200)
            .expect("missing journal is empty")
            .is_empty());
    }
}
