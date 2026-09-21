use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, ErrorKind, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::{app_paths, AppError, Result};

const MAX_HISTORY: usize = 200;
const TARGET: &str = "compose_tunnel::command";
static WRITE_LOCK: Mutex<()> = Mutex::new(());
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

pub(crate) fn emit_persistence_error() {
    eprintln!("compose-tunnel: command log could not be persisted");
    log::warn!(target: TARGET, "{{\"kind\":\"persistence_error\"}}");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommandStatus {
    Running,
    Success,
    Failure,
    Timeout,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandLogEntry {
    pub id: String,
    pub revision: u64,
    pub started_at: String,
    pub updated_at: String,
    pub server: String,
    pub preview: String,
    pub status: CommandStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
    pub detail_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandLogDetail {
    pub id: String,
    pub program: String,
    pub args: Vec<String>,
    pub local_command: String,
    pub remote_command: Option<String>,
    pub stdout_base64: String,
    pub stderr_base64: String,
}

#[derive(Serialize, Deserialize)]
struct StoredDetail {
    id: String,
    program: String,
    args: Vec<String>,
    local_command: String,
    remote_command: Option<String>,
}

fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn private_file(path: &Path, append: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).append(append);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(0o600);
        let file = options.open(path)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        return Ok(file);
    }
    #[cfg(not(unix))]
    Ok(options.open(path)?)
}

fn command_dir(log_dir: &Path, id: &str) -> Result<PathBuf> {
    if id.len() < 12
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(AppError::msg("invalid command log ID"));
    }
    Ok(log_dir.join("commands").join(id))
}

fn append_entry(log_dir: &Path, entry: &CommandLogEntry) -> Result<()> {
    let _guard = WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    private_dir(log_dir)?;
    let path = log_dir.join(format!("commands-{}.jsonl", &entry.id[..10]));
    let mut file = private_file(&path, true)?;
    let mut line = serde_json::to_vec(entry)?;
    line.push(b'\n');
    file.write_all(&line)?;
    if let Ok(payload) = serde_json::to_string(entry) {
        log::info!(target: TARGET, "{payload}");
    }
    Ok(())
}

pub(crate) fn begin_command_in(
    log_dir: &Path,
    server: &str,
    preview: &str,
    program: &str,
    args: &[String],
    remote_command: Option<&str>,
) -> Result<CommandLogEntry> {
    let now = Utc::now();
    let started_at = now.to_rfc3339_opts(SecondsFormat::Millis, true);
    let id = format!(
        "{}-{}-{}",
        now.format("%Y-%m-%d-%H%M%S%6f"),
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    );
    let dir = command_dir(log_dir, &id)?;
    private_dir(log_dir)?;
    private_dir(&log_dir.join("commands"))?;
    private_dir(&dir)?;
    let local_command = std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(|arg| crate::shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let detail = StoredDetail {
        id: id.clone(),
        program: program.to_string(),
        args: args.to_vec(),
        local_command,
        remote_command: remote_command.map(str::to_string),
    };
    let mut file = private_file(&dir.join("meta.json"), false)?;
    file.write_all(&serde_json::to_vec(&detail)?)?;
    command_output_files_in(log_dir, &id)?;
    let entry = CommandLogEntry {
        id,
        revision: 0,
        started_at: started_at.clone(),
        updated_at: started_at,
        server: server.to_string(),
        preview: preview.to_string(),
        status: CommandStatus::Running,
        exit_code: None,
        duration_ms: None,
        detail_available: true,
    };
    append_entry(log_dir, &entry)?;
    Ok(entry)
}

pub(crate) fn finish_command_in(
    log_dir: &Path,
    entry: &CommandLogEntry,
    status: CommandStatus,
    exit_code: Option<i32>,
) -> Result<CommandLogEntry> {
    let mut finished = entry.clone();
    finished.revision += 1;
    finished.status = status;
    finished.exit_code = exit_code;
    finished.updated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    finished.duration_ms = chrono::DateTime::parse_from_rfc3339(&entry.started_at)
        .ok()
        .map(|start| (Utc::now().signed_duration_since(start).num_milliseconds()).max(0) as u64);
    append_entry(log_dir, &finished)?;
    Ok(finished)
}

pub(crate) fn finish_command_id_in(
    log_dir: &Path,
    id: &str,
    status: CommandStatus,
    exit_code: Option<i32>,
) {
    let entry = (|| -> Result<Option<CommandLogEntry>> {
        command_dir(log_dir, id)?;
        let path = log_dir.join(format!("commands-{}.jsonl", &id[..10]));
        let mut latest: Option<CommandLogEntry> = None;
        for line in BufReader::new(File::open(path)?).lines().flatten() {
            if let Ok(row) = serde_json::from_str::<CommandLogEntry>(&line) {
                if row.id == id
                    && latest
                        .as_ref()
                        .is_none_or(|old| row.revision > old.revision)
                {
                    latest = Some(row);
                }
            }
        }
        Ok(latest)
    })();
    if let Ok(Some(entry)) = entry {
        if entry.status == CommandStatus::Running {
            if finish_command_in(log_dir, &entry, status, exit_code).is_err() {
                emit_persistence_error();
            }
        }
    }
}

pub(crate) fn command_output_files_in(log_dir: &Path, id: &str) -> Result<(File, File)> {
    let dir = command_dir(log_dir, id)?;
    Ok((
        private_file(&dir.join("stdout.bin"), true)?,
        private_file(&dir.join("stderr.bin"), true)?,
    ))
}

pub fn read_command_logs(limit: usize) -> Result<Vec<CommandLogEntry>> {
    read_command_logs_at(&app_paths()?.logs_dir, limit)
}

pub(crate) fn read_command_logs_at(log_dir: &Path, limit: usize) -> Result<Vec<CommandLogEntry>> {
    let limit = limit.min(MAX_HISTORY);
    if limit == 0 {
        return Ok(Vec::new());
    }
    let files = match fs::read_dir(log_dir) {
        Ok(files) => files,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut entries: HashMap<String, CommandLogEntry> = HashMap::new();
    for file in files {
        let path = file?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(date) = name
            .strip_prefix("commands-")
            .and_then(|name| name.strip_suffix(".jsonl"))
        else {
            continue;
        };
        if chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
            continue;
        }
        for line in BufReader::new(File::open(path)?).lines().flatten() {
            if let Ok(row) = serde_json::from_str::<CommandLogEntry>(&line) {
                if command_dir(log_dir, &row.id).is_ok()
                    && entries
                        .get(&row.id)
                        .is_none_or(|old| row.revision >= old.revision)
                {
                    entries.insert(row.id.clone(), row);
                }
            }
        }
    }
    let mut rows = entries.into_values().collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        b.started_at
            .cmp(&a.started_at)
            .then_with(|| b.id.cmp(&a.id))
    });
    rows.truncate(limit);
    Ok(rows)
}

pub fn read_command_detail(id: &str) -> Result<CommandLogDetail> {
    read_command_detail_at(&app_paths()?.logs_dir, id)
}

pub(crate) fn read_command_detail_at(log_dir: &Path, id: &str) -> Result<CommandLogDetail> {
    let dir = command_dir(log_dir, id)?;
    let detail: StoredDetail = serde_json::from_slice(&fs::read(dir.join("meta.json"))?)?;
    if detail.id != id {
        return Err(AppError::msg("command detail ID mismatch"));
    }
    Ok(CommandLogDetail {
        id: detail.id,
        program: detail.program,
        args: detail.args,
        local_command: detail.local_command,
        remote_command: detail.remote_command,
        stdout_base64: STANDARD.encode(fs::read(dir.join("stdout.bin"))?),
        stderr_base64: STANDARD.encode(fs::read(dir.join("stderr.bin"))?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Result;
    use std::{
        fs::{self, OpenOptions},
        io::Write,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "compose-tunnel-commands-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn command_log_folds_revisions_and_limits_distinct_commands() -> Result<()> {
        let dir = TempDir::new();
        let args = vec!["deploy@example.com".to_string(), "docker ps".to_string()];
        let first = begin_command_in(
            &dir.0,
            "staging",
            "docker ps",
            "ssh",
            &args,
            Some("docker ps"),
        )?;
        let done = finish_command_in(&dir.0, &first, CommandStatus::Success, Some(0))?;
        assert_eq!(first.id, done.id);
        assert_eq!(done.revision, first.revision + 1);
        assert_eq!(read_command_logs_at(&dir.0, 200)?[0], done);
        for _ in 0..201 {
            begin_command_in(&dir.0, "staging", "docker ps", "ssh", &args, None)?;
        }
        let rows = read_command_logs_at(&dir.0, 200)?;
        assert_eq!(rows.len(), 200);
        assert!(!rows.iter().any(|row| row.id == first.id));
        let mut index = OpenOptions::new()
            .append(true)
            .open(dir.0.join(format!("commands-{}.jsonl", &first.id[..10])))?;
        index.write_all(b"{\"partial\":\n")?;
        assert_eq!(read_command_logs_at(&dir.0, 200)?.len(), 200);
        Ok(())
    }

    #[test]
    fn command_log_detail_is_private_and_rejects_path_traversal() -> Result<()> {
        let dir = TempDir::new();
        let row = begin_command_in(
            &dir.0,
            "staging",
            "docker ps",
            "ssh",
            &["a b".into()],
            Some("docker ps"),
        )?;
        assert_eq!(
            read_command_detail_at(&dir.0, &row.id)?.local_command,
            "ssh 'a b'"
        );
        assert!(read_command_detail_at(&dir.0, "../state.json").is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in [
                dir.0.clone(),
                dir.0.join("commands"),
                dir.0.join("commands").join(&row.id),
            ] {
                assert_eq!(fs::metadata(path)?.permissions().mode() & 0o777, 0o700);
            }
            for path in [
                dir.0.join(format!("commands-{}.jsonl", &row.id[..10])),
                dir.0.join("commands").join(&row.id).join("meta.json"),
                dir.0.join("commands").join(&row.id).join("stdout.bin"),
                dir.0.join("commands").join(&row.id).join("stderr.bin"),
            ] {
                assert_eq!(fs::metadata(path)?.permissions().mode() & 0o777, 0o600);
            }
        }
        Ok(())
    }
}
