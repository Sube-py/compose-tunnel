use std::{
    collections::{BTreeMap, BTreeSet},
    io::ErrorKind,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Output, Stdio},
    sync::LazyLock,
    time::Duration,
};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use tokio::{
    fs,
    process::{Child, Command},
    sync::{Mutex, MutexGuard},
    time,
};

mod command_log;
pub use command_log::{
    read_command_detail, read_command_logs, CommandLogDetail, CommandLogEntry, CommandStatus,
};
mod operation_log;
use command_log::{begin_command_in, command_output_files_in, finish_command_in};
pub use operation_log::{read_operation_logs, OperationLevel, OperationLogEntry};
use operation_log::{record_operation, record_operation_in};

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    TomlDe(#[from] toml::de::Error),
    #[error(transparent)]
    TomlSer(#[from] toml::ser::Error),
}

impl AppError {
    pub fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub servers: Vec<ServerConfig>,
    #[serde(default)]
    pub profiles: Vec<ProfileConfig>,
    #[serde(default)]
    pub env_profiles: Vec<EnvProfileConfig>,
    #[serde(default)]
    pub active_env_profiles: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_env_profile: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            defaults: Defaults::default(),
            servers: Vec::new(),
            profiles: Vec::new(),
            env_profiles: Vec::new(),
            active_env_profiles: BTreeMap::new(),
            active_env_profile: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Defaults {
    #[serde(default = "default_local_host")]
    pub local_host: String,
    #[serde(default = "default_ssh_binary")]
    pub ssh_binary: String,
    #[serde(default = "default_docker_timeout_secs")]
    pub docker_timeout_secs: u64,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            local_host: default_local_host(),
            ssh_binary: default_ssh_binary(),
            docker_timeout_secs: default_docker_timeout_secs(),
        }
    }
}

fn default_local_host() -> String {
    "127.0.0.1".to_string()
}

fn default_ssh_binary() -> String {
    "ssh".to_string()
}

fn default_docker_timeout_secs() -> u64 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerConfig {
    pub name: String,
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    pub user: String,
    #[serde(default)]
    pub identity_file: Option<String>,
    #[serde(default)]
    pub ssh_alias: Option<String>,
    #[serde(default = "default_docker_command")]
    pub docker_command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SshConfigHost {
    pub alias: String,
    pub hostname: String,
    pub user: String,
    pub port: u16,
}

fn default_ssh_port() -> u16 {
    22
}

fn default_docker_command() -> String {
    "docker".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProfileConfig {
    pub name: String,
    pub server: String,
    pub project: String,
    pub service: String,
    #[serde(default)]
    pub network: Option<String>,
    pub target_port: u16,
    #[serde(default)]
    pub env: Vec<EnvEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EnvEntry {
    pub key: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EnvProfileConfig {
    pub name: String,
    #[serde(default)]
    pub target_dir: Option<PathBuf>,
    #[serde(default)]
    pub tunnel_ports: Vec<EnvTunnelPort>,
    #[serde(default)]
    pub extra_env: Vec<EnvPlainEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EnvTunnelPort {
    pub tunnel_id: String,
    pub alias: String,
    #[serde(default)]
    pub env_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EnvPlainEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppState {
    #[serde(default)]
    pub tunnels: Vec<TunnelState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TunnelStatus {
    Running,
    Stopped,
    Error,
}

impl Default for TunnelStatus {
    fn default() -> Self {
        Self::Stopped
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelState {
    pub id: String,
    pub server: String,
    pub project: String,
    pub service: String,
    pub container: String,
    pub container_id: String,
    pub container_ip: String,
    pub network: String,
    pub target_port: u16,
    pub local_host: String,
    pub local_port: u16,
    #[serde(default)]
    pub ssh_pid: Option<u32>,
    #[serde(default)]
    pub command_log_id: Option<String>,
    #[serde(default)]
    pub status: TunnelStatus,
    #[serde(default)]
    pub mode: TunnelMode,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TunnelMode {
    ContainerDirect,
}

impl Default for TunnelMode {
    fn default() -> Self {
        Self::ContainerDirect
    }
}

#[derive(Debug)]
struct StateMigration {
    state: AppState,
    legacy_pids: Vec<u32>,
    changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenTunnelRequest {
    pub server: String,
    pub project: String,
    pub service: String,
    pub target_port: u16,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub network: Option<String>,
    #[serde(default)]
    pub local_port: Option<u16>,
    #[serde(default)]
    pub local_host: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteEnvProfileRequest {
    pub name: String,
    #[serde(default)]
    pub target_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeProject {
    pub server: String,
    pub project: String,
    pub services: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeService {
    pub service: String,
    pub container: String,
    pub status: String,
    pub ports: Vec<String>,
    pub networks: Vec<String>,
    pub image: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerTestResult {
    pub ssh_ok: bool,
    pub docker_ok: bool,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub state_file: PathBuf,
    pub logs_dir: PathBuf,
}

pub fn app_paths() -> Result<AppPaths> {
    let dirs = ProjectDirs::from("", "", "compose-tunnel")
        .ok_or_else(|| AppError::msg("could not resolve user config directory"))?;
    let config_dir = dirs.config_dir().to_path_buf();
    Ok(AppPaths {
        config_file: config_dir.join("config.toml"),
        state_file: config_dir.join("state.json"),
        logs_dir: config_dir.join("logs"),
        config_dir,
    })
}

pub async fn init_config() -> Result<AppPaths> {
    let paths = app_paths()?;
    init_dirs(&paths).await?;
    if !paths.config_file.exists() {
        save_config(&AppConfig::default()).await?;
    }
    if !paths.state_file.exists() {
        save_state(&AppState::default()).await?;
    }
    Ok(paths)
}

async fn init_dirs(paths: &AppPaths) -> Result<()> {
    fs::create_dir_all(&paths.config_dir).await?;
    fs::create_dir_all(&paths.logs_dir).await?;
    Ok(())
}

pub async fn load_config() -> Result<AppConfig> {
    load_config_at(&app_paths()?.config_file).await
}

async fn load_config_at(config_file: &Path) -> Result<AppConfig> {
    let raw = match fs::read_to_string(config_file).await {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let config = AppConfig::default();
            save_config_at(config_file, &config).await?;
            return Ok(config);
        }
        Err(error) => return Err(error.into()),
    };
    Ok(toml::from_str(&raw)?)
}

pub async fn save_config(config: &AppConfig) -> Result<()> {
    save_config_at(&app_paths()?.config_file, config).await
}

async fn save_config_at(config_file: &Path, config: &AppConfig) -> Result<()> {
    ensure_parent_dir(config_file).await?;
    fs::write(config_file, toml::to_string_pretty(config)?).await?;
    Ok(())
}

/// Serializes every state-file transaction in this process.
///
/// Tokio mutexes are not reentrant, so the locked helpers (`*_locked` and the
/// transaction functions that take the guard) must never call a public wrapper
/// that acquires this lock again.
static STATE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

async fn lock_state() -> MutexGuard<'static, ()> {
    STATE_LOCK.lock().await
}

pub async fn load_state() -> Result<AppState> {
    let _guard = lock_state().await;
    load_state_unlocked(&app_paths()?.state_file).await
}

/// Reads, migrates, and persists the state file.
///
/// The caller must hold the state transaction lock.
async fn load_state_unlocked(state_file: &Path) -> Result<AppState> {
    let raw = match fs::read_to_string(state_file).await {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let state = AppState::default();
            save_state_unlocked(state_file, &state).await?;
            return Ok(state);
        }
        Err(error) => return Err(error.into()),
    };
    let migration = parse_stored_state(&raw)?;
    for pid in &migration.legacy_pids {
        if pid_is_running(*pid) {
            let _ = kill_pid(*pid).await;
        }
    }
    if migration.changed {
        save_state_unlocked(state_file, &migration.state).await?;
    }
    Ok(migration.state)
}

pub async fn save_state(state: &AppState) -> Result<()> {
    let _guard = lock_state().await;
    save_state_unlocked(&app_paths()?.state_file, state).await
}

/// Writes the state file.
///
/// The caller must hold the state transaction lock.
async fn save_state_unlocked(state_file: &Path, state: &AppState) -> Result<()> {
    ensure_parent_dir(state_file).await?;
    fs::write(state_file, serde_json::to_string_pretty(state)?).await?;
    Ok(())
}

async fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    Ok(())
}

fn parse_stored_state(raw: &str) -> Result<StateMigration> {
    let value: serde_json::Value = serde_json::from_str(raw)?;
    let mut legacy_pids = Vec::new();
    let mut changed = false;
    let mut tunnels = Vec::new();

    if let Some(rows) = value.get("tunnels") {
        let rows = rows
            .as_array()
            .ok_or_else(|| AppError::msg("state tunnels must be an array"))?;
        for row in rows {
            if row.get("mode").and_then(|mode| mode.as_str()) == Some("socat-direct") {
                if let Some(pid) = row.get("ssh_pid").and_then(serde_json::Value::as_u64) {
                    if let Ok(pid) = u32::try_from(pid) {
                        legacy_pids.push(pid);
                    }
                }
                changed = true;
                continue;
            }
            tunnels.push(serde_json::from_value(row.clone())?);
        }
    }

    Ok(StateMigration {
        state: AppState { tunnels },
        legacy_pids,
        changed,
    })
}

pub async fn list_servers() -> Result<Vec<ServerConfig>> {
    Ok(load_config().await?.servers)
}

pub async fn list_ssh_config_hosts() -> Result<Vec<SshConfigHost>> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Ok(Vec::new());
    };
    let aliases = ssh_aliases_from_file(&home.join(".ssh/config"), &home.join(".ssh"))?;
    let ssh_binary = load_config()
        .await
        .map(|config| config.defaults.ssh_binary)
        .unwrap_or_else(|_| default_ssh_binary());
    let mut hosts = Vec::new();

    for alias in aliases {
        let output = Command::new(&ssh_binary)
            .args(["-G", "--", &alias])
            .output()
            .await?;
        if !output.status.success() {
            continue;
        }
        let resolved = String::from_utf8_lossy(&output.stdout);
        let hostname = ssh_config_value(&resolved, "hostname").unwrap_or_else(|| alias.clone());
        let user = ssh_config_value(&resolved, "user").unwrap_or_default();
        let port = ssh_config_value(&resolved, "port")
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(default_ssh_port);
        hosts.push(SshConfigHost {
            alias,
            hostname,
            user,
            port,
        });
    }
    Ok(hosts)
}

fn ssh_aliases_from_file(path: &Path, ssh_dir: &Path) -> Result<Vec<String>> {
    let mut aliases = BTreeSet::new();
    let mut visited = BTreeSet::new();
    collect_ssh_aliases(path, ssh_dir, &mut aliases, &mut visited)?;
    Ok(aliases.into_iter().collect())
}

fn collect_ssh_aliases(
    path: &Path,
    ssh_dir: &Path,
    aliases: &mut BTreeSet<String>,
    visited: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    let path = path.to_path_buf();
    if !visited.insert(path.clone()) || !path.is_file() {
        return Ok(());
    }
    let content = std::fs::read_to_string(&path)?;
    for raw_line in content.lines() {
        let line = raw_line.split('#').next().unwrap_or_default().trim();
        let Some((keyword, value)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        if keyword.eq_ignore_ascii_case("host") {
            aliases.extend(
                value
                    .split_whitespace()
                    .filter(|alias| !alias.starts_with('!') && !alias.contains(['*', '?']))
                    .map(str::to_string),
            );
        } else if keyword.eq_ignore_ascii_case("include") {
            for pattern in value.split_whitespace() {
                let expanded = if let Some(rest) = pattern.strip_prefix("~/") {
                    ssh_dir.parent().unwrap_or(ssh_dir).join(rest)
                } else {
                    let candidate = PathBuf::from(pattern);
                    if candidate.is_absolute() {
                        candidate
                    } else {
                        ssh_dir.join(candidate)
                    }
                };
                if let Some(pattern) = expanded.to_str() {
                    for included in
                        glob::glob(pattern).map_err(|error| AppError::msg(error.to_string()))?
                    {
                        if let Ok(included) = included {
                            collect_ssh_aliases(&included, ssh_dir, aliases, visited)?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn ssh_config_value(config: &str, key: &str) -> Option<String> {
    config.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        (parts.next() == Some(key)).then(|| parts.collect::<Vec<_>>().join(" "))
    })
}

pub async fn save_server(server: ServerConfig) -> Result<()> {
    validate_name("server name", &server.name)?;
    if server.host.trim().is_empty() {
        return Err(AppError::msg("server host is required"));
    }
    if server.user.trim().is_empty() {
        return Err(AppError::msg("server user is required"));
    }

    let mut server = server;
    if server.docker_command.trim().is_empty() {
        server.docker_command = default_docker_command();
    }

    let mut config = load_config().await?;
    if let Some(existing) = config
        .servers
        .iter_mut()
        .find(|item| item.name == server.name)
    {
        *existing = server;
    } else {
        config.servers.push(server);
    }
    config
        .servers
        .sort_by(|left, right| left.name.cmp(&right.name));
    save_config(&config).await
}

pub async fn delete_server(name: String) -> Result<()> {
    let mut config = load_config().await?;
    config.servers.retain(|server| server.name != name);
    save_config(&config).await
}

pub async fn save_defaults(defaults: Defaults) -> Result<()> {
    let mut config = load_config().await?;
    config.defaults = defaults;
    save_config(&config).await
}

pub async fn list_env_profiles() -> Result<Vec<EnvProfileConfig>> {
    Ok(env_profiles_for_display(&load_config().await?))
}

pub async fn active_env_profile() -> Result<Option<String>> {
    Ok(active_env_profiles_for_config(&load_config().await?)
        .into_values()
        .next())
}

pub async fn active_env_profiles() -> Result<BTreeMap<String, String>> {
    Ok(active_env_profiles_for_config(&load_config().await?))
}

pub async fn save_env_profile(profile: EnvProfileConfig) -> Result<()> {
    validate_name("env profile name", &profile.name)?;
    let profile = normalize_env_profile(profile)?;
    let profile_name = profile.name.clone();
    let target_key = env_profile_target_key(&profile)?;

    let mut config = load_config().await?;
    let was_active =
        active_env_profiles_for_config(&config).get(&target_key) == Some(&profile_name);
    if let Some(existing) = config.env_profiles.iter_mut().find(|item| {
        item.name == profile_name
            && env_profile_target_key(item).is_ok_and(|value| value == target_key)
    }) {
        *existing = profile;
    } else {
        config.env_profiles.push(profile);
    }
    config.env_profiles.sort_by(|left, right| {
        env_profile_target_key(left)
            .unwrap_or_default()
            .cmp(&env_profile_target_key(right).unwrap_or_default())
            .then_with(|| left.name.cmp(&right.name))
    });
    config.active_env_profiles = active_env_profiles_for_config(&config);
    if was_active {
        config.active_env_profiles.insert(target_key, profile_name);
    }
    config.active_env_profile = None;
    save_config(&config).await
}

fn normalize_env_profile(mut profile: EnvProfileConfig) -> Result<EnvProfileConfig> {
    profile.target_dir = Some(env_profile_target_dir(&profile)?);
    profile.tunnel_ports.retain(|item| {
        !item.tunnel_id.trim().is_empty()
            && !item.alias.trim().is_empty()
            && item
                .env_key
                .as_ref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(true)
    });
    profile.extra_env.retain(|item| !item.key.trim().is_empty());
    for binding in &mut profile.tunnel_ports {
        binding.alias = normalize_env_name(&binding.alias);
        binding.env_key = binding
            .env_key
            .as_ref()
            .map(|value| normalize_env_name(value));
    }
    let mut seen_tunnel_aliases = BTreeSet::new();
    profile.tunnel_ports.reverse();
    profile
        .tunnel_ports
        .retain(|binding| seen_tunnel_aliases.insert(binding.alias.clone()));
    profile.tunnel_ports.reverse();
    for entry in &mut profile.extra_env {
        entry.key = normalize_env_name(&entry.key);
        if entry.value.contains('\n') || entry.value.contains('\r') {
            return Err(AppError::msg(format!(
                "env value for {} may not contain newlines",
                entry.key
            )));
        }
    }
    let mut seen_extra_keys = BTreeSet::new();
    profile.extra_env.reverse();
    profile
        .extra_env
        .retain(|entry| seen_extra_keys.insert(entry.key.clone()));
    profile.extra_env.reverse();
    Ok(profile)
}

pub async fn delete_env_profile(name: String, target_dir: Option<String>) -> Result<()> {
    let mut config = load_config().await?;
    let profile = find_env_profile(&config, &name, target_dir.as_deref())?;
    let target_key = env_profile_target_key(profile)?;
    config.env_profiles.retain(|profile| {
        !(profile.name == name
            && env_profile_target_key(profile).is_ok_and(|value| value == target_key))
    });
    config.active_env_profiles = active_env_profiles_for_config(&config);
    config.active_env_profiles.remove(&target_key);
    config.active_env_profile = None;
    save_config(&config).await
}

pub async fn set_active_env_profile(name: String, target_dir: Option<String>) -> Result<()> {
    let mut config = load_config().await?;
    let profile = find_env_profile(&config, &name, target_dir.as_deref())?;
    let target_key = env_profile_target_key(profile)?;
    config.active_env_profiles = active_env_profiles_for_config(&config);
    config.active_env_profiles.insert(target_key, name);
    config.active_env_profile = None;
    save_config(&config).await
}

pub async fn clear_active_env_profile(target_dir: String) -> Result<PathBuf> {
    let mut config = load_config().await?;
    let target_key = normalize_target_dir_key(&target_dir)?;
    config.active_env_profiles = active_env_profiles_for_config(&config);
    config.active_env_profiles.remove(&target_key);
    if config
        .active_env_profile
        .as_ref()
        .and_then(|name| {
            config
                .env_profiles
                .iter()
                .find(|profile| &profile.name == name)
        })
        .and_then(|profile| env_profile_target_key(profile).ok())
        .as_ref()
        == Some(&target_key)
    {
        config.active_env_profile = None;
    }
    save_config(&config).await?;

    let env_file = PathBuf::from(&target_key).join(".env");
    clear_env_profile_block(&env_file).await?;
    Ok(env_file)
}

pub async fn list_tunnels() -> Result<Vec<TunnelState>> {
    Ok(load_refreshed_state().await?.tunnels)
}

pub async fn test_server(server_id: String) -> Result<ServerTestResult> {
    let config = load_config().await?;
    let server = find_server(&config, &server_id)?.clone();
    let mut result = ServerTestResult {
        ssh_ok: false,
        docker_ok: false,
        details: Vec::new(),
    };

    match run_ssh(
        &config.defaults,
        &server,
        "true",
        "SSH connectivity",
        &server.name,
    )
    .await
    {
        Ok(_) => {
            result.ssh_ok = true;
            result.details.push("SSH connection succeeded".to_string());
        }
        Err(error) => {
            result
                .details
                .push(format!("SSH connection failed: {error}"));
            return Ok(result);
        }
    }

    let docker = docker_command(&server);
    let version_command = format!("{docker} version --format '{{{{.Server.Version}}}}'");
    match run_ssh(
        &config.defaults,
        &server,
        &version_command,
        "Docker version",
        &server.name,
    )
    .await
    {
        Ok(output) => {
            result.docker_ok = true;
            result
                .details
                .push(format!("{docker} is available: {}", output.trim()));
        }
        Err(error) => {
            result.details.push(format!("Docker check failed: {error}"));
        }
    }

    Ok(result)
}

pub async fn list_compose_projects(server_id: String) -> Result<Vec<ComposeProject>> {
    let config = load_config().await?;
    let server = find_server(&config, &server_id)?;
    let format =
        "{{.Label \"com.docker.compose.project\"}}\\t{{.Label \"com.docker.compose.service\"}}";
    let command = format!(
        "{} ps --format {}",
        docker_command(server),
        shell_quote(format)
    );
    let output = run_ssh(
        &config.defaults,
        server,
        &command,
        "Docker project discovery",
        &server.name,
    )
    .await?;

    let mut projects: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for line in output.lines() {
        let mut parts = line.split('\t');
        let Some(project) = parts
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let Some(service) = parts
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        projects
            .entry(project.to_string())
            .or_default()
            .insert(service.to_string());
    }

    Ok(projects
        .into_iter()
        .map(|(project, services)| ComposeProject {
            server: server_id.clone(),
            project,
            services: services.into_iter().collect(),
        })
        .collect())
}

pub async fn list_compose_services(
    server_id: String,
    project: String,
) -> Result<Vec<ComposeService>> {
    let config = load_config().await?;
    let server = find_server(&config, &server_id)?;
    let format = [
        "{{.Label \"com.docker.compose.service\"}}",
        "{{.Names}}",
        "{{.Status}}",
        "{{.Ports}}",
        "{{.Image}}",
        "{{.Networks}}",
    ]
    .join("\\t");
    let command = format!(
        "{} ps --filter label=com.docker.compose.project={} --format {}",
        docker_command(server),
        shell_quote(&project),
        shell_quote(&format)
    );
    let output = run_ssh(
        &config.defaults,
        server,
        &command,
        "Docker service discovery",
        &format!("{}/{project}", server.name),
    )
    .await?;

    let mut services = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 6 {
            continue;
        }
        services.push(ComposeService {
            service: parts[0].to_string(),
            container: parts[1].to_string(),
            status: parts[2].to_string(),
            ports: parse_ports(parts[3]),
            image: parts[4].to_string(),
            networks: parts[5]
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect(),
        });
    }
    services.sort_by(|left, right| left.service.cmp(&right.service));
    Ok(services)
}

fn select_compose_container<'a>(
    services: &'a [ComposeService],
    project: &str,
    service: &str,
    requested_container: Option<&str>,
) -> Result<&'a ComposeService> {
    let candidates: Vec<&ComposeService> = services
        .iter()
        .filter(|item| item.service == service)
        .collect();

    if let Some(container) = requested_container {
        return candidates
            .into_iter()
            .find(|item| item.container == container)
            .ok_or_else(|| {
                AppError::msg(format!(
                    "container {container} does not belong to {project}/{service}"
                ))
            });
    }

    match candidates.as_slice() {
        [] => Err(AppError::msg(format!(
            "service {service} was not found in project {project}"
        ))),
        [container] => Ok(*container),
        many => Err(AppError::msg(format!(
            "service {project}/{service} has multiple containers; pass one of: {}",
            many.iter()
                .map(|item| item.container.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

pub async fn open_tunnel(request: OpenTunnelRequest) -> Result<TunnelState> {
    let log_dir = app_paths().ok().map(|paths| paths.logs_dir);
    log_open_attempt(log_dir.as_deref(), &request);
    let result = open_tunnel_inner(request.clone()).await;
    log_open_result(log_dir.as_deref(), &request, &result);
    result
}

fn open_log_target(request: &OpenTunnelRequest) -> String {
    format!(
        "{}/{}/{}/{}:{}",
        request.server,
        request.project,
        request.service,
        request.container.as_deref().unwrap_or("auto"),
        request.target_port
    )
}

fn log_open_attempt(log_dir: Option<&Path>, request: &OpenTunnelRequest) {
    record_command_event(
        log_dir,
        OperationLevel::Info,
        "Tunnel open",
        &open_log_target(request),
        "attempt",
        None,
    );
}

fn log_open_result(
    log_dir: Option<&Path>,
    request: &OpenTunnelRequest,
    result: &Result<TunnelState>,
) {
    match result {
        Ok(tunnel) => record_command_event(
            log_dir,
            OperationLevel::Info,
            "Tunnel open",
            &open_log_target(request),
            "success",
            Some(&format!(
                "{}:{} -> {}:{} on {} (PID {})",
                tunnel.local_host,
                tunnel.local_port,
                tunnel.container_ip,
                tunnel.target_port,
                tunnel.network,
                tunnel
                    .ssh_pid
                    .map_or_else(|| "unknown".to_string(), |pid| pid.to_string())
            )),
        ),
        Err(_) => record_command_event(
            log_dir,
            OperationLevel::Error,
            "Tunnel open",
            &open_log_target(request),
            "failure",
            Some("tunnel could not be opened"),
        ),
    }
}

async fn open_tunnel_inner(request: OpenTunnelRequest) -> Result<TunnelState> {
    validate_name("project", &request.project)?;
    validate_name("service", &request.service)?;
    if request.target_port == 0 {
        return Err(AppError::msg("target port is required"));
    }

    let config = load_config().await?;
    let server = find_server(&config, &request.server)?.clone();
    let services = list_compose_services(request.server.clone(), request.project.clone()).await?;
    let selected = select_compose_container(
        &services,
        &request.project,
        &request.service,
        request.container.as_deref(),
    )?;
    let network = resolve_network(&request.project, selected, request.network.as_deref())?;
    let target = inspect_container(
        &config.defaults,
        &server,
        &selected.container,
        &request.project,
        &request.service,
        &network,
    )
    .await?;

    let local_host = request
        .local_host
        .clone()
        .unwrap_or_else(|| config.defaults.local_host.clone());
    let state_file = app_paths()?.state_file;

    // Everything from here on changes the state file or the local SSH process
    // that the state file points at: hold the transaction lock so a long
    // refresh cannot release, replace, or resurrect this tunnel behind us.
    let _guard = lock_state().await;
    let mut state = load_state_unlocked(&state_file).await?;
    let tunnel_id = tunnel_state_id(
        &state,
        &request.server,
        &request.project,
        &request.service,
        &target.container,
        request.target_port,
    );

    let previous_command_id = state
        .tunnels
        .iter()
        .find(|tunnel| tunnel.id == tunnel_id)
        .and_then(|tunnel| tunnel.command_log_id.clone());
    if let Some(previous) = state.tunnels.iter().find(|tunnel| tunnel.id == tunnel_id) {
        release_previous_tunnel(previous).await?;
    }

    let local_port = resolve_local_port(&state, &tunnel_id, &local_host, request.local_port)?;
    let started = spawn_ssh_forward(
        &config.defaults,
        &server,
        &local_host,
        local_port,
        &target.ip,
        request.target_port,
    )
    .await?;
    let ssh_pid = started.child.id();

    let tunnel = TunnelState {
        id: tunnel_id,
        server: request.server.clone(),
        project: request.project.clone(),
        service: request.service.clone(),
        container: target.container,
        container_id: target.id,
        container_ip: target.ip,
        network,
        target_port: request.target_port,
        local_host,
        local_port,
        ssh_pid,
        command_log_id: started.command_log_id.clone(),
        status: TunnelStatus::Running,
        mode: TunnelMode::ContainerDirect,
        started_at: Some(now_string()),
        last_error: None,
    };

    upsert_tunnel(&mut state, tunnel.clone());
    if let Err(error) = save_state_unlocked(&state_file, &state).await {
        if let Some(pid) = ssh_pid {
            let _ = kill_pid(pid).await;
        }
        finish_forward(
            log_dir_for_state(&state_file).as_deref(),
            started.command_log_id.as_deref(),
            CommandStatus::Failure,
        );
        return Err(error);
    }
    finish_forward(
        log_dir_for_state(&state_file).as_deref(),
        previous_command_id.as_deref(),
        CommandStatus::Stopped,
    );
    Ok(tunnel)
}

fn log_dir_for_state(state_file: &Path) -> Option<PathBuf> {
    state_file.parent().map(|parent| parent.join("logs"))
}

fn finish_forward(log_dir: Option<&Path>, id: Option<&str>, status: CommandStatus) {
    if let (Some(dir), Some(id)) = (log_dir, id) {
        command_log::finish_command_id_in(dir, id, status, None);
    }
}

pub async fn close_tunnel(tunnel_id: String) -> Result<()> {
    close_tunnel_at(&app_paths()?.state_file, &tunnel_id).await
}

async fn close_tunnel_at(state_file: &Path, tunnel_id: &str) -> Result<()> {
    let log_dir = state_file.parent().map(|parent| parent.join("logs"));
    let _guard = lock_state().await;
    let mut state = load_state_unlocked(state_file).await?;
    let mut found = false;
    let mut target = None;
    let mut finished_id = None;

    for tunnel in &mut state.tunnels {
        if tunnel.id != tunnel_id {
            continue;
        }
        found = true;
        let tunnel_target = tunnel_log_target(tunnel);
        record_command_event(
            log_dir.as_deref(),
            OperationLevel::Info,
            "Tunnel stop",
            &tunnel_target,
            "attempt",
            None,
        );
        target = Some(tunnel_target);
        if let Some(pid) = tunnel.ssh_pid {
            if let Err(error) = kill_pid(pid).await {
                record_command_event(
                    log_dir.as_deref(),
                    OperationLevel::Error,
                    "Tunnel stop",
                    target.as_deref().unwrap_or(tunnel_id),
                    "failure",
                    Some("local SSH process could not be stopped"),
                );
                return Err(error);
            }
        }
        tunnel.status = TunnelStatus::Stopped;
        tunnel.ssh_pid = None;
        finished_id = tunnel.command_log_id.clone();
    }

    if !found {
        return Err(AppError::msg(format!("tunnel {tunnel_id} was not found")));
    }

    if let Err(error) = save_state_unlocked(state_file, &state).await {
        record_command_event(
            log_dir.as_deref(),
            OperationLevel::Error,
            "Tunnel stop",
            target.as_deref().unwrap_or(tunnel_id),
            "failure",
            Some("stopped state could not be saved"),
        );
        return Err(error);
    }
    finish_forward(
        log_dir.as_deref(),
        finished_id.as_deref(),
        CommandStatus::Stopped,
    );
    record_command_event(
        log_dir.as_deref(),
        OperationLevel::Info,
        "Tunnel stop",
        target.as_deref().unwrap_or(tunnel_id),
        "success",
        Some("local forward stopped"),
    );
    Ok(())
}

pub async fn close_all_tunnels() -> Result<()> {
    close_all_tunnels_at(&app_paths()?.state_file).await
}

async fn close_all_tunnels_at(state_file: &Path) -> Result<()> {
    let log_dir = state_file.parent().map(|parent| parent.join("logs"));
    let _guard = lock_state().await;
    let mut state = load_state_unlocked(state_file).await?;
    let mut changed = false;
    let mut stopped_targets = Vec::new();
    let mut failed_targets = Vec::new();
    let mut finished_ids = Vec::new();

    for tunnel in &mut state.tunnels {
        let needs_stop = tunnel.status != TunnelStatus::Stopped || tunnel.ssh_pid.is_some();
        let target = tunnel_log_target(tunnel);
        if needs_stop {
            record_command_event(
                log_dir.as_deref(),
                OperationLevel::Info,
                "Tunnel stop",
                &target,
                "attempt",
                None,
            );
        }
        if let Some(pid) = tunnel.ssh_pid {
            if kill_pid(pid).await.is_err() {
                record_command_event(
                    log_dir.as_deref(),
                    OperationLevel::Error,
                    "Tunnel stop",
                    &target,
                    "failure",
                    Some("local SSH process could not be stopped"),
                );
                failed_targets.push(target);
                continue;
            }
        }
        if needs_stop {
            stopped_targets.push(target);
        }
        changed |= needs_stop;
        tunnel.status = TunnelStatus::Stopped;
        tunnel.ssh_pid = None;
        if needs_stop {
            finished_ids.push(tunnel.command_log_id.clone());
        }
    }

    if changed {
        if let Err(error) = save_state_unlocked(state_file, &state).await {
            record_command_event(
                log_dir.as_deref(),
                OperationLevel::Error,
                "Tunnel stop all",
                "all tunnels",
                "failure",
                Some("stopped state could not be saved"),
            );
            return Err(error);
        }
        for target in &stopped_targets {
            record_command_event(
                log_dir.as_deref(),
                OperationLevel::Info,
                "Tunnel stop",
                target,
                "success",
                Some("local forward stopped"),
            );
        }
        for id in &finished_ids {
            finish_forward(log_dir.as_deref(), id.as_deref(), CommandStatus::Stopped);
        }
    }
    record_command_event(
        log_dir.as_deref(),
        if failed_targets.is_empty() {
            OperationLevel::Info
        } else {
            OperationLevel::Error
        },
        "Tunnel stop all",
        "all tunnels",
        if failed_targets.is_empty() {
            "success"
        } else {
            "failure"
        },
        Some(&format!(
            "{} stopped, {} could not be stopped",
            stopped_targets.len(),
            failed_targets.len()
        )),
    );
    if !failed_targets.is_empty() {
        return Err(AppError::msg(format!(
            "could not stop {} local SSH forward(s)",
            failed_targets.len()
        )));
    }
    Ok(())
}

pub async fn render_env_profile(name: String, target_dir: Option<String>) -> Result<String> {
    let config = load_config().await?;
    let profile = find_env_profile(&config, &name, target_dir.as_deref())?;
    let state = load_state().await?;
    render_env_profile_inner(profile, &state)
}

pub async fn write_env_profile(request: WriteEnvProfileRequest) -> Result<PathBuf> {
    let config = load_config().await?;
    let profile = find_env_profile(&config, &request.name, request.target_dir.as_deref())?;
    let target_key = env_profile_target_key(profile)?;
    let active_profiles = active_env_profiles_for_config(&config);
    if active_profiles.get(&target_key) != Some(&request.name) {
        return Err(AppError::msg(format!(
            "env profile {} is not active for {}",
            request.name, target_key
        )));
    }
    let target_dir = env_profile_target_dir(profile)?;
    let state = load_state().await?;
    let env = render_env_profile_inner(profile, &state)?;
    let env_file = target_dir.join(".env");
    write_env_profile_block(&env_file, &env).await?;
    Ok(env_file)
}

fn find_server<'a>(config: &'a AppConfig, name: &str) -> Result<&'a ServerConfig> {
    config
        .servers
        .iter()
        .find(|server| server.name == name)
        .ok_or_else(|| AppError::msg(format!("server {name} was not found")))
}

fn find_env_profile<'a>(
    config: &'a AppConfig,
    name: &str,
    target_dir: Option<&str>,
) -> Result<&'a EnvProfileConfig> {
    let target_key = target_dir.map(normalize_target_dir_key).transpose()?;
    let matches = config
        .env_profiles
        .iter()
        .filter(|profile| {
            profile.name == name
                && target_key.as_ref().is_none_or(|target| {
                    env_profile_target_key(profile).is_ok_and(|value| &value == target)
                })
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [profile] => Ok(profile),
        [] => Err(AppError::msg(format!("env profile {name} was not found"))),
        _ => Err(AppError::msg(format!(
            "env profile {name} exists in multiple projects; target directory is required"
        ))),
    }
}

fn active_env_profiles_for_config(config: &AppConfig) -> BTreeMap<String, String> {
    let mut active = BTreeMap::new();

    for (stored_target, active_name) in &config.active_env_profiles {
        let Ok(stored_target) = normalize_target_dir_key(stored_target) else {
            continue;
        };
        if let Some(profile) = config.env_profiles.iter().find(|profile| {
            &profile.name == active_name
                && env_profile_target_key(profile).is_ok_and(|value| value == stored_target)
        }) {
            if let Ok(target_key) = env_profile_target_key(profile) {
                active.insert(target_key, active_name.clone());
            }
        }
    }

    if let Some(legacy_name) = &config.active_env_profile {
        if active.values().all(|name| name != legacy_name) {
            if let Some(profile) = config
                .env_profiles
                .iter()
                .find(|profile| &profile.name == legacy_name)
            {
                if let Ok(target_key) = env_profile_target_key(profile) {
                    active.insert(target_key, legacy_name.clone());
                }
            }
        }
    }
    active
}

fn env_profiles_for_display(config: &AppConfig) -> Vec<EnvProfileConfig> {
    config
        .env_profiles
        .iter()
        .cloned()
        .map(|mut profile| {
            if let Ok(target_dir) = env_profile_target_dir(&profile) {
                profile.target_dir = Some(target_dir);
            }
            profile
        })
        .collect()
}

fn env_profile_target_key(profile: &EnvProfileConfig) -> Result<String> {
    Ok(env_profile_target_dir(profile)?
        .to_string_lossy()
        .trim()
        .to_string())
}

fn normalize_target_dir_key(target_dir: &str) -> Result<String> {
    let target_dir = target_dir.trim();
    if target_dir.is_empty() {
        return Err(AppError::msg("target directory is required"));
    }
    let expanded = expand_home(target_dir);
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()?.join(expanded)
    };
    Ok(absolute.to_string_lossy().trim().to_string())
}

fn env_profile_target_dir(profile: &EnvProfileConfig) -> Result<PathBuf> {
    Ok(PathBuf::from(normalize_target_dir_key(
        profile
            .target_dir
            .as_ref()
            .ok_or_else(|| AppError::msg("target directory is required"))?
            .to_string_lossy()
            .as_ref(),
    )?))
}

fn validate_name(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(AppError::msg(format!("{label} is required")));
    }
    if value
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))
    {
        return Err(AppError::msg(format!(
            "{label} may only contain letters, numbers, dashes, and underscores"
        )));
    }
    Ok(())
}

fn ssh_target(server: &ServerConfig) -> String {
    if let Some(alias) = server
        .ssh_alias
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        return alias.clone();
    }
    format!("{}@{}", server.user, server.host)
}

fn docker_command(server: &ServerConfig) -> String {
    let value = server.docker_command.trim();
    if value.is_empty() {
        default_docker_command()
    } else {
        value.to_string()
    }
}

fn ssh_base_args(server: &ServerConfig) -> Vec<String> {
    let mut args = Vec::new();
    if server.ssh_alias.is_none() {
        args.push("-p".to_string());
        args.push(server.port.to_string());
    }
    if let Some(identity_file) = &server.identity_file {
        args.push("-i".to_string());
        args.push(expand_home(identity_file).to_string_lossy().to_string());
    }
    args.push("-o".to_string());
    args.push("BatchMode=yes".to_string());
    args.push("-o".to_string());
    args.push("ExitOnForwardFailure=yes".to_string());
    args.push(ssh_target(server));
    args
}

fn format_forward_host(host: &str) -> String {
    if host.contains(':') && !(host.starts_with('[') && host.ends_with(']')) {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

fn ssh_forward_spec(
    local_host: &str,
    local_port: u16,
    remote_host: &str,
    remote_port: u16,
) -> String {
    format!(
        "{}:{local_port}:{}:{remote_port}",
        format_forward_host(local_host),
        format_forward_host(remote_host)
    )
}

fn ssh_forward_args(
    server: &ServerConfig,
    local_host: &str,
    local_port: u16,
    remote_host: &str,
    remote_port: u16,
) -> Result<Vec<String>> {
    let mut args = ssh_base_args(server);
    args.insert(0, "-N".to_string());
    let forward = ssh_forward_spec(local_host, local_port, remote_host, remote_port);
    let target_index = args
        .iter()
        .position(|value| value == &ssh_target(server))
        .ok_or_else(|| AppError::msg("could not build ssh forward command"))?;
    args.insert(target_index, "-L".to_string());
    args.insert(target_index + 1, forward);
    Ok(args)
}

async fn run_command_with_timeout(
    program: &str,
    args: &[String],
    timeout: Duration,
    label: &str,
) -> Result<Output> {
    let mut command = Command::new(program);
    command.args(args);
    command.kill_on_drop(true);
    match time::timeout(timeout, command.output()).await {
        Ok(output) => Ok(output?),
        Err(_) => Err(AppError::msg(format!(
            "{label} timed out after {} seconds",
            timeout.as_secs_f64()
        ))),
    }
}

async fn run_ssh(
    defaults: &Defaults,
    server: &ServerConfig,
    remote_command: &str,
    operation: &'static str,
    target: &str,
) -> Result<String> {
    let log_dir = app_paths().ok().map(|paths| paths.logs_dir);
    run_ssh_at(
        defaults,
        server,
        remote_command,
        operation,
        target,
        log_dir.as_deref(),
    )
    .await
}

fn record_command_event(
    log_dir: Option<&Path>,
    level: OperationLevel,
    operation: &str,
    target: &str,
    outcome: &str,
    detail: Option<&str>,
) {
    match log_dir {
        Some(log_dir) => record_operation_in(log_dir, level, operation, target, outcome, detail),
        None => record_operation(level, operation, target, outcome, detail),
    }
}

async fn run_ssh_at(
    defaults: &Defaults,
    server: &ServerConfig,
    remote_command: &str,
    operation: &'static str,
    target: &str,
    log_dir: Option<&Path>,
) -> Result<String> {
    record_command_event(
        log_dir,
        OperationLevel::Info,
        operation,
        target,
        "attempt",
        None,
    );
    let mut args = ssh_base_args(server);
    args.push(remote_command.to_string());
    let journal = log_dir.and_then(|dir| {
        match begin_command_in(
            dir,
            &server.name,
            remote_command,
            &defaults.ssh_binary,
            &args,
            Some(remote_command),
        ) {
            Ok(entry) => Some((dir, entry)),
            Err(_) => {
                command_log::emit_persistence_error();
                None
            }
        }
    });
    let timeout = Duration::from_secs(defaults.docker_timeout_secs);
    let output = match &journal {
        Some((dir, entry)) => match command_output_files_in(dir, &entry.id) {
            Ok((stdout, stderr)) => {
                run_ssh_to_files(
                    &defaults.ssh_binary,
                    &args,
                    timeout,
                    stdout,
                    stderr,
                    dir,
                    &entry.id,
                )
                .await
            }
            Err(_) => run_command_with_timeout(&defaults.ssh_binary, &args, timeout, "ssh").await,
        },
        None => run_command_with_timeout(&defaults.ssh_binary, &args, timeout, "ssh").await,
    };
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            let detail = if matches!(&error, AppError::Message(message) if message.starts_with("ssh timed out after"))
            {
                "timed out"
            } else {
                "SSH command could not start or complete"
            };
            if let Some((dir, entry)) = &journal {
                let status = if detail == "timed out" {
                    CommandStatus::Timeout
                } else {
                    CommandStatus::Failure
                };
                if finish_command_in(dir, entry, status, None).is_err() {
                    command_log::emit_persistence_error();
                }
            }
            record_command_event(
                log_dir,
                OperationLevel::Error,
                operation,
                target,
                "failure",
                Some(detail),
            );
            return Err(error);
        }
    };
    if let Some((dir, entry)) = &journal {
        let status = if output.status.success() {
            CommandStatus::Success
        } else {
            CommandStatus::Failure
        };
        if finish_command_in(dir, entry, status, output.status.code()).is_err() {
            command_log::emit_persistence_error();
        }
    }
    if !output.status.success() {
        let detail = match output.status.code() {
            Some(code) => format!("exit code {code}"),
            None => "SSH command terminated by signal".to_string(),
        };
        record_command_event(
            log_dir,
            OperationLevel::Error,
            operation,
            target,
            "failure",
            Some(&detail),
        );
        return Err(AppError::msg(command_error(
            "ssh",
            output.status.code(),
            &output.stderr,
        )));
    }
    record_command_event(
        log_dir,
        OperationLevel::Info,
        operation,
        target,
        "success",
        None,
    );
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn run_ssh_to_files(
    program: &str,
    args: &[String],
    timeout: Duration,
    stdout: std::fs::File,
    stderr: std::fs::File,
    log_dir: &Path,
    id: &str,
) -> Result<Output> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .kill_on_drop(true)
        .spawn()?;
    let status = match time::timeout(timeout, child.wait()).await {
        Ok(status) => status?,
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(AppError::msg(format!(
                "ssh timed out after {} seconds",
                timeout.as_secs_f64()
            )));
        }
    };
    let dir = log_dir.join("commands").join(id);
    Ok(Output {
        status,
        stdout: std::fs::read(dir.join("stdout.bin"))?,
        stderr: std::fs::read(dir.join("stderr.bin"))?,
    })
}

async fn spawn_ssh_forward(
    defaults: &Defaults,
    server: &ServerConfig,
    local_host: &str,
    local_port: u16,
    remote_host: &str,
    remote_port: u16,
) -> Result<ForwardStart> {
    let log_dir = app_paths().ok().map(|paths| paths.logs_dir);
    spawn_ssh_forward_at(
        defaults,
        server,
        local_host,
        local_port,
        remote_host,
        remote_port,
        log_dir.as_deref(),
    )
    .await
}

async fn spawn_ssh_forward_at(
    defaults: &Defaults,
    server: &ServerConfig,
    local_host: &str,
    local_port: u16,
    remote_host: &str,
    remote_port: u16,
    log_dir: Option<&Path>,
) -> Result<ForwardStart> {
    let target = format!(
        "{} {local_host}:{local_port} -> {remote_host}:{remote_port}",
        server.name
    );
    record_command_event(
        log_dir,
        OperationLevel::Info,
        "SSH forward",
        &target,
        "attempt",
        None,
    );
    let args = match ssh_forward_args(server, local_host, local_port, remote_host, remote_port) {
        Ok(args) => args,
        Err(error) => {
            record_command_event(
                log_dir,
                OperationLevel::Error,
                "SSH forward",
                &target,
                "failure",
                Some("invalid forward address or port"),
            );
            return Err(error);
        }
    };

    let journal = log_dir.and_then(|dir| {
        match begin_command_in(
            dir,
            &server.name,
            &format!(
                "ssh -N -L {}",
                ssh_forward_spec(local_host, local_port, remote_host, remote_port)
            ),
            &defaults.ssh_binary,
            &args,
            None,
        ) {
            Ok(entry) => Some((dir, entry)),
            Err(_) => {
                command_log::emit_persistence_error();
                None
            }
        }
    });
    let output_files =
        journal.as_ref().and_then(
            |(dir, entry)| match command_output_files_in(dir, &entry.id) {
                Ok(files) => Some(files),
                Err(error) => {
                    eprintln!("compose-tunnel: forward output files unavailable: {error}");
                    command_log::emit_persistence_error();
                    None
                }
            },
        );
    let (stdout, stderr) = match output_files {
        Some((stdout, stderr)) => (Stdio::from(stdout), Stdio::from(stderr)),
        None => (Stdio::null(), Stdio::null()),
    };
    let mut child = match Command::new(&defaults.ssh_binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            if let Some((dir, entry)) = &journal {
                if finish_command_in(dir, entry, CommandStatus::Failure, None).is_err() {
                    command_log::emit_persistence_error();
                }
            }
            record_command_event(
                log_dir,
                OperationLevel::Error,
                "SSH forward",
                &target,
                "failure",
                Some("SSH process could not start"),
            );
            return Err(error.into());
        }
    };

    match time::timeout(Duration::from_secs(1), child.wait()).await {
        Ok(Ok(status)) => {
            if let Some((dir, entry)) = &journal {
                if finish_command_in(dir, entry, CommandStatus::Failure, status.code()).is_err() {
                    command_log::emit_persistence_error();
                }
            }
            let detail = match status.code() {
                Some(code) => format!("exit code {code}"),
                None => "SSH process terminated by signal".to_string(),
            };
            record_command_event(
                log_dir,
                OperationLevel::Error,
                "SSH forward",
                &target,
                "failure",
                Some(&detail),
            );
            Err(AppError::msg(format!(
                "ssh forward exited during startup with status {status}"
            )))
        }
        Err(_) => {
            let detail = child.id().map(|pid| format!("PID {pid}"));
            record_command_event(
                log_dir,
                OperationLevel::Info,
                "SSH forward",
                &target,
                "success",
                detail.as_deref(),
            );
            Ok(ForwardStart {
                child,
                command_log_id: journal.map(|(_, entry)| entry.id),
            })
        }
        Ok(Err(error)) => {
            // The child state is unknown, so it may still be forwarding: kill
            // and reap it instead of dropping a live SSH process.
            let _ = child.kill().await;
            let _ = child.wait().await;
            if let Some((dir, entry)) = &journal {
                if finish_command_in(dir, entry, CommandStatus::Failure, None).is_err() {
                    command_log::emit_persistence_error();
                }
            }
            record_command_event(
                log_dir,
                OperationLevel::Error,
                "SSH forward",
                &target,
                "failure",
                Some("SSH process could not be checked during startup"),
            );
            Err(AppError::msg(format!(
                "ssh forward could not be checked during startup: {error}"
            )))
        }
    }
}

#[derive(Debug)]
struct ForwardStart {
    child: Child,
    command_log_id: Option<String>,
}

impl std::ops::Deref for ForwardStart {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.child
    }
}

impl std::ops::DerefMut for ForwardStart {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

#[derive(Debug, Deserialize)]
struct DockerInspectContainer {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "State")]
    state: DockerInspectState,
    #[serde(rename = "Config")]
    config: DockerInspectConfig,
    #[serde(rename = "NetworkSettings")]
    network_settings: DockerInspectNetworkSettings,
}

#[derive(Debug, Deserialize)]
struct DockerInspectState {
    #[serde(rename = "Running")]
    running: bool,
}

#[derive(Debug, Deserialize)]
struct DockerInspectConfig {
    #[serde(rename = "Labels", default)]
    labels: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct DockerInspectNetworkSettings {
    #[serde(rename = "Networks", default)]
    networks: BTreeMap<String, DockerInspectNetwork>,
}

#[derive(Debug, Deserialize)]
struct DockerInspectNetwork {
    #[serde(rename = "IPAddress", default)]
    ipv4: String,
    #[serde(rename = "GlobalIPv6Address", default)]
    ipv6: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InspectedContainer {
    container: String,
    id: String,
    network: String,
    ip: String,
}

fn parse_inspected_container(
    raw: &str,
    container: &str,
    project: &str,
    service: &str,
    network: &str,
) -> Result<InspectedContainer> {
    let mut containers: Vec<DockerInspectContainer> = serde_json::from_str(raw)?;
    if containers.len() != 1 {
        return Err(AppError::msg(format!(
            "expected exactly one container for {container}, got {}",
            containers.len()
        )));
    }

    let inspected = containers.remove(0);
    if !inspected.state.running {
        return Err(AppError::msg(format!("{container} is not running")));
    }

    let project_label = inspected.config.labels.get("com.docker.compose.project");
    let service_label = inspected.config.labels.get("com.docker.compose.service");
    if project_label.map(String::as_str) != Some(project)
        || service_label.map(String::as_str) != Some(service)
    {
        return Err(AppError::msg(format!(
            "container {container} does not belong to {project}/{service}"
        )));
    }

    let attached = inspected
        .network_settings
        .networks
        .get(network)
        .ok_or_else(|| {
            AppError::msg(format!("network {network} is not attached to {container}"))
        })?;
    let ip = if attached.ipv4.trim().is_empty() {
        attached.ipv6.trim()
    } else {
        attached.ipv4.trim()
    };
    if ip.is_empty() {
        return Err(AppError::msg(format!(
            "container {container} has no IP address on network {network}"
        )));
    }

    Ok(InspectedContainer {
        container: container.to_string(),
        id: inspected.id,
        network: network.to_string(),
        ip: ip.to_string(),
    })
}

async fn inspect_container(
    defaults: &Defaults,
    server: &ServerConfig,
    container: &str,
    project: &str,
    service: &str,
    network: &str,
) -> Result<InspectedContainer> {
    let log_dir = app_paths().ok().map(|paths| paths.logs_dir);
    inspect_container_at(
        defaults,
        server,
        container,
        project,
        service,
        network,
        log_dir.as_deref(),
    )
    .await
}

async fn inspect_container_at(
    defaults: &Defaults,
    server: &ServerConfig,
    container: &str,
    project: &str,
    service: &str,
    network: &str,
    log_dir: Option<&Path>,
) -> Result<InspectedContainer> {
    let command = format!(
        "{} inspect --type container {}",
        docker_command(server),
        shell_quote(container)
    );
    let target = format!("{}/{project}/{service}/{container}", server.name);
    let output = run_ssh_at(
        defaults,
        server,
        &command,
        "Docker inspect",
        &target,
        log_dir,
    )
    .await?;
    let inspected = parse_inspected_container(&output, container, project, service, network);
    if inspected.is_err() {
        record_command_event(
            log_dir,
            OperationLevel::Error,
            "Docker inspect validation",
            &target,
            "failure",
            Some("container state, labels, network, or IP did not match"),
        );
    }
    inspected
}

fn resolve_network(
    project: &str,
    container: &ComposeService,
    requested_network: Option<&str>,
) -> Result<String> {
    if let Some(network) = requested_network {
        return container
            .networks
            .iter()
            .find(|item| item.as_str() == network)
            .cloned()
            .ok_or_else(|| {
                AppError::msg(format!(
                    "network {network} is not attached to {}",
                    container.container
                ))
            });
    }

    let default_network = format!("{project}_default");
    if container.networks.contains(&default_network) {
        return Ok(default_network);
    }
    if let [network] = container.networks.as_slice() {
        return Ok(network.clone());
    }

    let mut networks = container.networks.clone();
    networks.sort();
    Err(AppError::msg(format!(
        "container {} has multiple networks; pass one of: {}",
        container.container,
        networks.join(", ")
    )))
}

fn parse_ports(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn ensure_local_port_available(host: &str, port: u16) -> Result<()> {
    TcpListener::bind((host, port))
        .map(|_| ())
        .map_err(|error| AppError::msg(format!("local port {host}:{port} is unavailable: {error}")))
}

fn local_port_available(host: &str, port: u16) -> bool {
    TcpListener::bind((host, port)).is_ok()
}

async fn release_previous_tunnel(previous: &TunnelState) -> Result<()> {
    let Some(pid) = previous.ssh_pid else {
        return Ok(());
    };
    kill_pid(pid).await?;
    wait_for_local_port_release(&previous.local_host, previous.local_port).await
}

async fn wait_for_local_port_release(host: &str, port: u16) -> Result<()> {
    for _ in 0..20 {
        if local_port_available(host, port) {
            return Ok(());
        }
        time::sleep(Duration::from_millis(25)).await;
    }
    ensure_local_port_available(host, port)
}

fn resolve_local_port(
    state: &AppState,
    tunnel_id: &str,
    host: &str,
    requested: Option<u16>,
) -> Result<u16> {
    match requested {
        Some(port) => {
            ensure_local_port_available(host, port)?;
            Ok(port)
        }
        None => state
            .tunnels
            .iter()
            .find(|tunnel| tunnel.id == tunnel_id)
            .map(|tunnel| tunnel.local_port)
            .filter(|port| local_port_available(host, *port))
            .or_else(portpicker::pick_unused_port)
            .ok_or_else(|| AppError::msg("could not find an available local port")),
    }
}

fn tunnel_state_id(
    state: &AppState,
    server: &str,
    project: &str,
    service: &str,
    container: &str,
    target_port: u16,
) -> String {
    let base = service.to_string();
    match state.tunnels.iter().find(|tunnel| tunnel.id == base) {
        None => base,
        Some(existing)
            if same_tunnel_target(existing, server, project, service, container, target_port) =>
        {
            base
        }
        Some(_) => format!(
            "{}-{}-{}-{}-{}",
            sanitize_name(server),
            sanitize_name(project),
            sanitize_name(service),
            sanitize_name(container),
            target_port
        ),
    }
}

fn same_tunnel_target(
    tunnel: &TunnelState,
    server: &str,
    project: &str,
    service: &str,
    container: &str,
    target_port: u16,
) -> bool {
    tunnel.server == server
        && tunnel.project == project
        && tunnel.service == service
        && tunnel.container == container
        && tunnel.target_port == target_port
}

fn sanitize_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn normalize_env_name(value: &str) -> String {
    let mut output: String = value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if output
        .chars()
        .next()
        .map(|ch| ch.is_ascii_digit())
        .unwrap_or(true)
    {
        output.insert(0, '_');
    }
    output
}

fn render_env_profile_inner(profile: &EnvProfileConfig, state: &AppState) -> Result<String> {
    render_env_profile_at(profile, state, &local_timestamp_string())
}

fn render_env_profile_at(
    profile: &EnvProfileConfig,
    state: &AppState,
    created_at: &str,
) -> Result<String> {
    let mut lines = vec![
        format!("# compose-tunnel env: {}", profile.name),
        format!("# created_at: {created_at}"),
    ];
    let mut port_values = BTreeMap::new();

    for binding in &profile.tunnel_ports {
        let tunnel = state
            .tunnels
            .iter()
            .find(|tunnel| tunnel.id == binding.tunnel_id)
            .ok_or_else(|| AppError::msg(format!("tunnel {} was not found", binding.tunnel_id)))?;
        if binding.alias.trim().is_empty() {
            continue;
        }
        let alias = normalize_env_name(&binding.alias);
        let port = tunnel.local_port.to_string();
        port_values.insert(alias.clone(), port.clone());
        lines.push(format!("# tunnel: {alias}"));
        lines.push(format!("#   server: {}", tunnel.server));
        lines.push(format!("#   project: {}", tunnel.project));
        lines.push(format!("#   service: {}", tunnel.service));
        lines.push(format!("#   container: {}", tunnel.container));
        lines.push(format!("#   remote_port: {}", tunnel.target_port));
        lines.push(format!(
            "#   local: {}:{}",
            tunnel.local_host, tunnel.local_port
        ));
        lines.push(format!("{alias}={port}"));
        if let Some(env_key) = binding.env_key.as_ref().map(|value| value.trim()) {
            if !env_key.is_empty() {
                let env_key = normalize_env_name(env_key);
                lines.push(format!("{env_key}={port}"));
            }
        }
    }

    for entry in &profile.extra_env {
        let key = entry.key.trim();
        if key.is_empty() {
            continue;
        }
        let key = normalize_env_name(key);
        let value = resolve_env_profile_value(&entry.value, &port_values);
        lines.push(format!("{key}={value}"));
    }

    lines.push(String::new());
    Ok(lines.join("\n"))
}

fn local_timestamp_string() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn resolve_env_profile_value(value: &str, port_values: &BTreeMap<String, String>) -> String {
    let mut resolved = String::with_capacity(value.len());
    let mut cursor = 0;

    while let Some(start) = value[cursor..].find("${") {
        let absolute_start = cursor + start;
        resolved.push_str(&value[cursor..absolute_start]);

        let token_start = absolute_start + 2;
        let Some(end) = value[token_start..].find('}') else {
            resolved.push_str(&value[absolute_start..]);
            return resolved;
        };

        let token_end = token_start + end;
        let token = &value[token_start..token_end];
        let normalized_token = normalize_env_name(token);
        if let Some(port) = port_values.get(&normalized_token) {
            resolved.push_str(port);
        } else {
            resolved.push_str(&value[absolute_start..=token_end]);
        }
        cursor = token_end + 1;
    }

    resolved.push_str(&value[cursor..]);
    resolved
}

async fn write_env_profile_block(path: &Path, env: &str) -> Result<()> {
    let start = "# compose-tunnel:start env";
    let end = "# compose-tunnel:end env";
    let block = format_env_profile_block(env);
    let existing = match fs::read_to_string(path).await {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };

    let cleaned = remove_env_profile_blocks(&existing);
    let updated = replace_block(&cleaned, start, end, &block);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    fs::write(path, updated).await?;
    Ok(())
}

async fn clear_env_profile_block(path: &Path) -> Result<()> {
    let existing = match fs::read_to_string(path).await {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let cleaned = remove_env_profile_blocks(&existing);
    if cleaned == existing {
        return Ok(());
    }
    if cleaned.trim().is_empty() {
        if path.exists() {
            fs::remove_file(path).await?;
        }
        return Ok(());
    }
    fs::write(path, cleaned).await?;
    Ok(())
}

fn format_env_profile_block(env: &str) -> String {
    let start = "# compose-tunnel:start env";
    let end = "# compose-tunnel:end env";
    format!("{start}\n{}\n{end}\n", env.trim_end())
}

fn replace_block(existing: &str, start: &str, end: &str, block: &str) -> String {
    let Some(start_index) = existing.find(start) else {
        let separator = if existing.is_empty() || existing.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        return format!("{existing}{separator}{block}");
    };
    let Some(relative_end_index) = existing[start_index..].find(end) else {
        let separator = if existing.ends_with('\n') { "" } else { "\n" };
        return format!("{existing}{separator}{block}");
    };
    let end_index = start_index + relative_end_index + end.len();
    let mut output = String::new();
    output.push_str(&existing[..start_index]);
    output.push_str(block);
    if let Some(rest) = existing.get(end_index..) {
        output.push_str(rest.strip_prefix('\n').unwrap_or(rest));
    }
    output
}

fn remove_env_profile_blocks(existing: &str) -> String {
    let mut output = String::new();
    let mut skipping = false;

    for line in existing.split_inclusive('\n') {
        let line_text = line.trim_end_matches(['\r', '\n']);
        if !skipping && is_env_profile_block_start(line_text) {
            skipping = true;
            continue;
        }
        if skipping {
            if is_env_profile_block_end(line_text) {
                skipping = false;
            }
            continue;
        }
        output.push_str(line);
    }

    output
}

fn is_env_profile_block_start(line: &str) -> bool {
    line.strip_prefix("# compose-tunnel:start ")
        .map(is_env_profile_block_id)
        .unwrap_or(false)
}

fn is_env_profile_block_end(line: &str) -> bool {
    line.strip_prefix("# compose-tunnel:end ")
        .map(is_env_profile_block_id)
        .unwrap_or(false)
}

fn is_env_profile_block_id(block_id: &str) -> bool {
    block_id == "env" || block_id.starts_with("env:")
}

fn upsert_tunnel(state: &mut AppState, tunnel: TunnelState) {
    if let Some(existing) = state.tunnels.iter_mut().find(|item| item.id == tunnel.id) {
        *existing = tunnel;
    } else {
        state.tunnels.push(tunnel);
    }
    state.tunnels.sort_by(|left, right| left.id.cmp(&right.id));
}

fn tunnel_log_target(tunnel: &TunnelState) -> String {
    format!(
        "{}/{}/{}/{} {}:{} -> {}:{}",
        tunnel.server,
        tunnel.project,
        tunnel.service,
        tunnel.container,
        format_forward_host(&tunnel.local_host),
        tunnel.local_port,
        format_forward_host(&tunnel.container_ip),
        tunnel.target_port
    )
}

#[derive(Debug, PartialEq, Eq)]
enum ReconcileDecision {
    Skip,
    Keep,
    UpdateIdentity,
    Restart,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ReconcileReport {
    changed: bool,
    spawned_pids: Vec<u32>,
    spawned_ids: Vec<String>,
    finished_ids: Vec<String>,
    events: Vec<ReconcileEvent>,
}

#[derive(Debug, PartialEq, Eq)]
struct ReconcileEvent {
    level: OperationLevel,
    target: String,
    outcome: &'static str,
    detail: String,
}

fn reconciliation_decision(
    tunnel: &TunnelState,
    inspected: &InspectedContainer,
    pid_running: bool,
) -> ReconcileDecision {
    if tunnel.status == TunnelStatus::Stopped {
        return ReconcileDecision::Skip;
    }
    if tunnel.status == TunnelStatus::Error || !pid_running || inspected.ip != tunnel.container_ip {
        return ReconcileDecision::Restart;
    }
    if inspected.id != tunnel.container_id {
        return ReconcileDecision::UpdateIdentity;
    }
    ReconcileDecision::Keep
}

fn mark_reconciliation_error(tunnel: &mut TunnelState, error: impl Into<String>) -> bool {
    let error = error.into();
    let changed = tunnel.status != TunnelStatus::Error
        || tunnel.ssh_pid.is_some()
        || tunnel.last_error.as_deref() != Some(error.as_str());
    tunnel.status = TunnelStatus::Error;
    tunnel.ssh_pid = None;
    tunnel.last_error = Some(error);
    changed
}

async fn terminate_and_mark_reconciliation_error(
    tunnel: &mut TunnelState,
    error: impl Into<String>,
) -> bool {
    let mut failed_pid = None;
    if let Some(pid) = tunnel.ssh_pid {
        if kill_pid(pid).await.is_err() {
            failed_pid = Some(pid);
        }
    }
    let changed = mark_reconciliation_error(tunnel, error);
    tunnel.ssh_pid = failed_pid;
    changed
}

async fn load_refreshed_state() -> Result<AppState> {
    let paths = app_paths()?;
    refresh_state_at(&paths.config_file, &paths.state_file).await
}

/// Reconciles every desired tunnel and persists the result once.
///
/// The lock is held for the whole reconciliation so a slow refresh can neither
/// overwrite a concurrent open or close nor orphan the SSH processes it
/// replaces.
async fn refresh_state_at(config_file: &Path, state_file: &Path) -> Result<AppState> {
    let config = load_config_at(config_file).await?;
    let log_dir = state_file.parent().map(|parent| parent.join("logs"));
    let _guard = lock_state().await;
    let mut state = load_state_unlocked(state_file).await?;
    let report = reconcile_tunnels(&config, &mut state, log_dir.as_deref()).await;
    if report.changed {
        let saved = save_state_unlocked(state_file, &state).await;
        if let Err(error) = finalize_refresh_save(saved, &report.spawned_pids).await {
            for id in &report.spawned_ids {
                finish_forward(log_dir.as_deref(), Some(id), CommandStatus::Failure);
            }
            record_command_event(
                log_dir.as_deref(),
                OperationLevel::Error,
                "Tunnel reconciliation",
                "saved tunnels",
                "failure",
                Some("state could not be saved; new SSH forwards stopped"),
            );
            return Err(error);
        }
        for id in &report.finished_ids {
            finish_forward(log_dir.as_deref(), Some(id), CommandStatus::Stopped);
        }
        for event in report.events {
            record_command_event(
                log_dir.as_deref(),
                event.level,
                "Tunnel reconnect",
                &event.target,
                event.outcome,
                Some(&event.detail),
            );
        }
    }
    Ok(state)
}

async fn finalize_refresh_save(save_result: Result<()>, spawned_pids: &[u32]) -> Result<()> {
    let Err(error) = save_result else {
        return Ok(());
    };
    for pid in spawned_pids {
        let _ = kill_pid(*pid).await;
    }
    Err(error)
}

async fn reconcile_tunnels(
    config: &AppConfig,
    state: &mut AppState,
    log_dir: Option<&Path>,
) -> ReconcileReport {
    let mut changed = false;
    let mut spawned_pids = Vec::new();
    let mut spawned_ids = Vec::new();
    let mut finished_ids = Vec::new();
    let mut events = Vec::new();

    for tunnel in &mut state.tunnels {
        if tunnel.status == TunnelStatus::Stopped {
            continue;
        }

        let Some(server) = config
            .servers
            .iter()
            .find(|server| server.name == tunnel.server)
            .cloned()
        else {
            events.push(ReconcileEvent {
                level: OperationLevel::Error,
                target: tunnel_log_target(tunnel),
                outcome: "failure",
                detail: "server is no longer configured".to_string(),
            });
            changed |= terminate_and_mark_reconciliation_error(
                tunnel,
                format!("server {} was not found", tunnel.server),
            )
            .await;
            if let Some(id) = &tunnel.command_log_id {
                if tunnel.ssh_pid.is_none() {
                    finished_ids.push(id.clone());
                }
            }
            continue;
        };

        let inspected = match inspect_container_at(
            &config.defaults,
            &server,
            &tunnel.container,
            &tunnel.project,
            &tunnel.service,
            &tunnel.network,
            log_dir,
        )
        .await
        {
            Ok(inspected) => inspected,
            Err(error) => {
                events.push(ReconcileEvent {
                    level: OperationLevel::Error,
                    target: tunnel_log_target(tunnel),
                    outcome: "failure",
                    detail: "container inspect failed".to_string(),
                });
                changed |= terminate_and_mark_reconciliation_error(tunnel, error.to_string()).await;
                if let Some(id) = &tunnel.command_log_id {
                    if tunnel.ssh_pid.is_none() {
                        finished_ids.push(id.clone());
                    }
                }
                continue;
            }
        };

        let pid_running = tunnel.ssh_pid.is_some_and(pid_is_running);
        match reconciliation_decision(tunnel, &inspected, pid_running) {
            ReconcileDecision::Skip | ReconcileDecision::Keep => {}
            ReconcileDecision::UpdateIdentity => {
                tunnel.container_id = inspected.id;
                tunnel.last_error = None;
                changed = true;
            }
            ReconcileDecision::Restart => {
                if let Err(error) = release_previous_tunnel(tunnel).await {
                    events.push(ReconcileEvent {
                        level: OperationLevel::Error,
                        target: tunnel_log_target(tunnel),
                        outcome: "failure",
                        detail: "previous local forward could not be released".to_string(),
                    });
                    let pid = tunnel.ssh_pid;
                    changed |= mark_reconciliation_error(tunnel, error.to_string());
                    tunnel.ssh_pid = pid;
                    continue;
                }
                if let Some(id) = &tunnel.command_log_id {
                    finished_ids.push(id.clone());
                }
                tunnel.ssh_pid = None;

                match spawn_ssh_forward_at(
                    &config.defaults,
                    &server,
                    &tunnel.local_host,
                    tunnel.local_port,
                    &inspected.ip,
                    tunnel.target_port,
                    log_dir,
                )
                .await
                {
                    Ok(started) => {
                        let ssh_pid = started.child.id();
                        events.push(ReconcileEvent {
                            level: OperationLevel::Info,
                            target: tunnel_log_target(tunnel),
                            outcome: "success",
                            detail: format!(
                                "{}:{} -> {}:{} on {} (PID {})",
                                tunnel.local_host,
                                tunnel.local_port,
                                inspected.ip,
                                tunnel.target_port,
                                tunnel.network,
                                ssh_pid
                                    .map_or_else(|| "unknown".to_string(), |pid| pid.to_string())
                            ),
                        });
                        if let Some(pid) = ssh_pid {
                            spawned_pids.push(pid);
                        }
                        if let Some(id) = &started.command_log_id {
                            spawned_ids.push(id.clone());
                        }
                        tunnel.container_id = inspected.id;
                        tunnel.container_ip = inspected.ip;
                        tunnel.ssh_pid = ssh_pid;
                        tunnel.command_log_id = started.command_log_id;
                        tunnel.status = TunnelStatus::Running;
                        tunnel.started_at = Some(now_string());
                        tunnel.last_error = None;
                        changed = true;
                    }
                    Err(error) => {
                        events.push(ReconcileEvent {
                            level: OperationLevel::Error,
                            target: tunnel_log_target(tunnel),
                            outcome: "failure",
                            detail: "new local SSH forward could not start".to_string(),
                        });
                        changed |= mark_reconciliation_error(tunnel, error.to_string());
                    }
                }
            }
        }
    }

    ReconcileReport {
        changed,
        spawned_pids,
        spawned_ids,
        finished_ids,
        events,
    }
}

fn pid_is_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
    #[cfg(windows)]
    {
        let Ok(output) = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
        else {
            return false;
        };
        if !output.status.success() {
            return false;
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line.contains(&format!("\"{pid}\"")))
    }
}

async fn kill_pid(pid: u32) -> Result<()> {
    #[cfg(unix)]
    {
        let pid = i32::try_from(pid).map_err(|_| AppError::msg("invalid process id"))?;
        if pid == 0 {
            return Err(AppError::msg("invalid process id"));
        }
        if unsafe { libc::kill(pid, libc::SIGTERM) } == -1 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error.into());
            }
        }
    }
    #[cfg(windows)]
    {
        let output = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()
            .await?;
        if !output.status.success() && pid_is_running(pid) {
            return Err(AppError::msg("could not stop local SSH process"));
        }
    }
    Ok(())
}

fn command_error(command: &str, code: Option<i32>, stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    let stderr = mask_secrets(stderr.trim());
    match code {
        Some(code) => format!("{command} exited with code {code}: {stderr}"),
        None => format!("{command} failed: {stderr}"),
    }
}

pub fn mask_secrets(value: &str) -> String {
    let mut output = Vec::new();
    for token in value.split_whitespace() {
        let lowered = token.to_ascii_lowercase();
        if lowered.contains("password")
            || lowered.contains("token")
            || lowered.contains("secret")
            || lowered.contains("private_key")
        {
            output.push("[masked]");
        } else {
            output.push(token);
        }
    }
    output.join(" ")
}

fn shell_quote(value: &str) -> String {
    shell_words::quote(value).to_string()
}

fn expand_home(value: &str) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(value)
}

fn now_string() -> String {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => duration.as_secs().to_string(),
        Err(_) => "0".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn direct_tunnel(status: TunnelStatus) -> TunnelState {
        TunnelState {
            id: "db".to_string(),
            server: "staging".to_string(),
            project: "app".to_string(),
            service: "db".to_string(),
            container: "app-db-1".to_string(),
            container_id: "sha256:abc123".to_string(),
            container_ip: "172.22.0.4".to_string(),
            network: "app_default".to_string(),
            target_port: 5432,
            local_host: "127.0.0.1".to_string(),
            local_port: 15432,
            ssh_pid: Some(1234),
            command_log_id: None,
            status,
            mode: TunnelMode::ContainerDirect,
            started_at: Some("2026-09-20T12:00:00Z".to_string()),
            last_error: None,
        }
    }

    #[test]
    fn tunnel_log_target_distinguishes_local_and_remote_ports() {
        let first = direct_tunnel(TunnelStatus::Running);
        let mut second = first.clone();
        second.local_port = 15433;
        second.target_port = 5433;

        assert!(tunnel_log_target(&first).contains("127.0.0.1:15432"));
        assert!(tunnel_log_target(&first).contains("172.22.0.4:5432"));
        assert_ne!(tunnel_log_target(&first), tunnel_log_target(&second));
    }

    #[tokio::test]
    async fn stop_all_does_not_report_success_when_process_termination_fails() {
        let app = TempApp::new("stop-all-kill-error");
        let mut tunnel = direct_tunnel(TunnelStatus::Running);
        tunnel.ssh_pid = Some(u32::MAX);
        app.write_state(&AppState {
            tunnels: vec![tunnel],
        });

        assert!(close_all_tunnels_at(&app.state_file).await.is_err());
        let entries = operation_log::read_operation_logs_at(&app.dir.join("logs"), 200)
            .expect("logged operations");
        assert!(entries
            .iter()
            .any(|entry| { entry.operation == "Tunnel stop all" && entry.outcome == "failure" }));
        assert!(!entries.iter().any(|entry| entry.outcome == "success"));
        let state = load_state_unlocked(&app.state_file)
            .await
            .expect("saved state");
        assert_eq!(state.tunnels[0].status, TunnelStatus::Running);
        assert_eq!(state.tunnels[0].ssh_pid, Some(u32::MAX));
    }

    #[test]
    fn legacy_socat_state_is_discarded_and_returns_its_pid() {
        let raw = r#"{
          "tunnels": [{
            "id": "db",
            "server": "staging",
            "project": "app",
            "service": "db",
            "network": "app_default",
            "target_port": 5432,
            "socat_port": 5432,
            "local_host": "127.0.0.1",
            "local_port": 15432,
            "socat_container": "compose-tunnel-staging-app-db-5432",
            "socat_container_ip": "172.22.0.9",
            "ssh_pid": 4242,
            "status": "running",
            "mode": "socat-direct"
          }]
        }"#;

        let migration = parse_stored_state(raw).expect("legacy state should migrate");

        assert!(migration.state.tunnels.is_empty());
        assert_eq!(migration.legacy_pids, vec![4242]);
        assert!(migration.changed);
    }

    #[test]
    fn tunnel_id_distinguishes_service_replicas() {
        let state = AppState {
            tunnels: vec![direct_tunnel(TunnelStatus::Stopped)],
        };

        assert_eq!(
            tunnel_state_id(&state, "staging", "app", "db", "app-db-2", 5432),
            "staging-app-db-app-db-2-5432"
        );
    }

    #[test]
    fn old_socat_config_keys_are_ignored_when_reserialized() {
        let raw = r#"
            [defaults]
            local_host = "127.0.0.1"
            socat_image = "alpine/socat:latest"
            socat_command = "socat"
            ssh_binary = "ssh"
            docker_timeout_secs = 20

            [[servers]]
            name = "staging"
            host = "example.com"
            port = 22
            user = "deploy"
            default_socat_image = "alpine/socat:latest"
            docker_command = "docker"
        "#;

        let config: AppConfig = toml::from_str(raw).expect("old config should load");
        let serialized = toml::to_string(&config).expect("new config should serialize");

        assert!(!serialized.contains("socat"));
    }

    #[test]
    fn env_profile_names_are_scoped_to_target_directory() {
        let config = AppConfig {
            env_profiles: vec![
                EnvProfileConfig {
                    name: "test".to_string(),
                    target_dir: Some(PathBuf::from("/apps/alpha")),
                    ..EnvProfileConfig::default()
                },
                EnvProfileConfig {
                    name: "test".to_string(),
                    target_dir: Some(PathBuf::from("/apps/beta")),
                    ..EnvProfileConfig::default()
                },
            ],
            ..AppConfig::default()
        };

        let alpha = find_env_profile(&config, "test", Some("/apps/alpha"))
            .expect("project-scoped env should resolve");
        assert_eq!(alpha.target_dir.as_deref(), Some(Path::new("/apps/alpha")));

        let error = find_env_profile(&config, "test", None)
            .expect_err("an unscoped duplicate env name should be ambiguous");
        assert_eq!(
            error.to_string(),
            "env profile test exists in multiple projects; target directory is required"
        );
    }

    #[test]
    fn ssh_config_hosts_exclude_patterns_and_follow_includes() {
        let root = std::env::temp_dir().join(format!(
            "compose-tunnel-ssh-config-test-{}",
            std::process::id()
        ));
        let ssh_dir = root.join(".ssh");
        std::fs::create_dir_all(ssh_dir.join("config.d")).expect("ssh test dir should exist");
        std::fs::write(
            ssh_dir.join("config"),
            "Host * !blocked\n  ServerAliveInterval 30\nInclude config.d/*\nHost staging prod\n",
        )
        .expect("main ssh config should be written");
        std::fs::write(
            ssh_dir.join("config.d/work"),
            "Host jump-*\nHost bastion # comment\n",
        )
        .expect("included ssh config should be written");

        let aliases = ssh_aliases_from_file(&ssh_dir.join("config"), &ssh_dir)
            .expect("ssh aliases should parse");

        assert_eq!(aliases, vec!["bastion", "prod", "staging"]);
        std::fs::remove_dir_all(root).expect("ssh test dir should be removed");
    }

    #[test]
    fn ssh_config_value_reads_resolved_property() {
        let config = "host staging\nuser deploy\nhostname 10.0.0.5\nport 2202\n";

        assert_eq!(
            ssh_config_value(config, "hostname").as_deref(),
            Some("10.0.0.5")
        );
        assert_eq!(ssh_config_value(config, "user").as_deref(), Some("deploy"));
        assert_eq!(ssh_config_value(config, "port").as_deref(), Some("2202"));
    }

    #[test]
    fn replace_managed_block_keeps_other_content() {
        let existing = "A=1\n# compose-tunnel:start db\nOLD=1\n# compose-tunnel:end db\nB=2\n";
        let updated = replace_block(
            existing,
            "# compose-tunnel:start db",
            "# compose-tunnel:end db",
            "# compose-tunnel:start db\nDB_HOST=127.0.0.1\n# compose-tunnel:end db\n",
        );

        assert_eq!(
            updated,
            "A=1\n# compose-tunnel:start db\nDB_HOST=127.0.0.1\n# compose-tunnel:end db\nB=2\n"
        );
    }

    #[test]
    fn env_profile_block_keeps_end_marker_on_its_own_line() {
        let block = format_env_profile_block(
            "# compose-tunnel env: test\nDATABASE_PORT=15432\nDATABASE_HOST=127.0.0.1\n",
        );

        assert_eq!(
            block,
            [
                "# compose-tunnel:start env\n",
                "# compose-tunnel env: test\n",
                "DATABASE_PORT=15432\n",
                "DATABASE_HOST=127.0.0.1\n",
                "# compose-tunnel:end env\n",
            ]
            .concat()
        );
        assert!(!block.contains("127.0.0.1# compose-tunnel:end env"));
    }

    #[test]
    fn remove_env_profile_blocks_keeps_user_env_content() {
        let existing = [
            "APP_NAME=demo\n",
            "# compose-tunnel:start env\n",
            "DATABASE_PORT=15432\n",
            "# compose-tunnel:end env\n",
            "USER_KEY=keep-me\n",
        ]
        .concat();

        let cleaned = remove_env_profile_blocks(&existing);

        assert_eq!(cleaned, "APP_NAME=demo\nUSER_KEY=keep-me\n");
        assert!(!cleaned.contains("compose-tunnel"));
        assert!(!cleaned.contains("DATABASE_PORT"));
    }

    #[test]
    fn env_profile_block_replaces_previous_profile_blocks_only() {
        let existing = [
            "A=1\n",
            "# compose-tunnel:start env:test\n",
            "DATABASE_PORT=15432\n",
            "# compose-tunnel:end env:test\n",
            "# compose-tunnel:start db\n",
            "DB_PORT=15432\n",
            "# compose-tunnel:end db\n",
            "# compose-tunnel:start env:prod\n",
            "DATABASE_PORT=25432\n",
            "# compose-tunnel:end env:prod\n",
            "B=2\n",
        ]
        .concat();

        let cleaned = remove_env_profile_blocks(&existing);
        let updated = replace_block(
            &cleaned,
            "# compose-tunnel:start env",
            "# compose-tunnel:end env",
            "# compose-tunnel:start env\nDATABASE_PORT=35432\n# compose-tunnel:end env\n",
        );

        assert_eq!(
            updated,
            [
                "A=1\n",
                "# compose-tunnel:start db\n",
                "DB_PORT=15432\n",
                "# compose-tunnel:end db\n",
                "B=2\n",
                "# compose-tunnel:start env\n",
                "DATABASE_PORT=35432\n",
                "# compose-tunnel:end env\n",
            ]
            .concat()
        );
    }
    #[test]
    fn env_names_are_normalized_for_dotenv_output() {
        assert_eq!(normalize_env_name("server-db"), "server_db");
        assert_eq!(normalize_env_name("DATABASE-PORT"), "DATABASE_PORT");
        assert_eq!(normalize_env_name("1_PORT"), "_1_PORT");
    }

    #[test]
    fn reconciliation_skips_user_stopped_tunnel() {
        let tunnel = direct_tunnel(TunnelStatus::Stopped);
        let inspected = InspectedContainer {
            container: tunnel.container.clone(),
            id: tunnel.container_id.clone(),
            network: tunnel.network.clone(),
            ip: tunnel.container_ip.clone(),
        };

        assert_eq!(
            reconciliation_decision(&tunnel, &inspected, false),
            ReconcileDecision::Skip
        );
    }

    #[test]
    fn reconciliation_keeps_matching_live_forward() {
        let tunnel = direct_tunnel(TunnelStatus::Running);
        let inspected = InspectedContainer {
            container: tunnel.container.clone(),
            id: tunnel.container_id.clone(),
            network: tunnel.network.clone(),
            ip: tunnel.container_ip.clone(),
        };

        assert_eq!(
            reconciliation_decision(&tunnel, &inspected, true),
            ReconcileDecision::Keep
        );
    }

    #[test]
    fn reconciliation_updates_identity_without_restart_when_ip_is_reused() {
        let tunnel = direct_tunnel(TunnelStatus::Running);
        let inspected = InspectedContainer {
            container: tunnel.container.clone(),
            id: "sha256:new".to_string(),
            network: tunnel.network.clone(),
            ip: tunnel.container_ip.clone(),
        };

        assert_eq!(
            reconciliation_decision(&tunnel, &inspected, true),
            ReconcileDecision::UpdateIdentity
        );
    }

    #[test]
    fn reconciliation_restarts_for_changed_ip_dead_pid_or_error_state() {
        let running = direct_tunnel(TunnelStatus::Running);
        let changed = InspectedContainer {
            container: running.container.clone(),
            id: "sha256:new".to_string(),
            network: running.network.clone(),
            ip: "172.22.0.8".to_string(),
        };
        assert_eq!(
            reconciliation_decision(&running, &changed, true),
            ReconcileDecision::Restart
        );
        assert_eq!(
            reconciliation_decision(&running, &changed, false),
            ReconcileDecision::Restart
        );

        let errored = direct_tunnel(TunnelStatus::Error);
        assert_eq!(
            reconciliation_decision(&errored, &changed, false),
            ReconcileDecision::Restart
        );
    }

    #[test]
    fn reconciliation_error_clears_pid_and_preserves_desired_target() {
        let mut tunnel = direct_tunnel(TunnelStatus::Running);

        mark_reconciliation_error(&mut tunnel, "container app-db-1 is not running");

        assert_eq!(tunnel.status, TunnelStatus::Error);
        assert_eq!(tunnel.ssh_pid, None);
        assert_eq!(tunnel.container, "app-db-1");
        assert_eq!(
            tunnel.last_error.as_deref(),
            Some("container app-db-1 is not running")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn refresh_save_failure_terminates_forwards_spawned_in_the_same_pass() {
        let mut child = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn a long-running child");
        let pid = child.id().expect("spawned child should have a pid");

        let result = finalize_refresh_save(Err(AppError::msg("state write failed")), &[pid]).await;

        let mut exited = false;
        for _ in 0..100 {
            if child.try_wait().expect("try_wait").is_some() {
                exited = true;
                break;
            }
            time::sleep(Duration::from_millis(20)).await;
        }
        if !exited {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }

        assert!(
            exited,
            "a forward spawned during the failed refresh must be terminated, not left holding its port"
        );
        let error = result.expect_err("the original save error must be returned");
        assert_eq!(error.to_string(), "state write failed");
    }

    /// An isolated config/state directory so state transactions can be tested
    /// without touching the real user profile.
    struct TempApp {
        dir: PathBuf,
        config_file: PathBuf,
        state_file: PathBuf,
    }

    impl TempApp {
        fn new(label: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!(
                "compose-tunnel-core-{label}-{}-{unique}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("temp dir should be created");
            Self {
                config_file: dir.join("config.toml"),
                state_file: dir.join("state.json"),
                dir,
            }
        }

        fn write_state(&self, state: &AppState) {
            std::fs::write(
                &self.state_file,
                serde_json::to_string_pretty(state).expect("state should serialize"),
            )
            .expect("state file should be written");
        }

        fn read_state(&self) -> AppState {
            let raw = std::fs::read_to_string(&self.state_file).expect("state file should exist");
            serde_json::from_str(&raw).expect("state file should parse")
        }
    }

    impl Drop for TempApp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn staging_config(ssh_binary: &str) -> AppConfig {
        AppConfig {
            defaults: Defaults {
                local_host: "127.0.0.1".to_string(),
                ssh_binary: ssh_binary.to_string(),
                docker_timeout_secs: 10,
            },
            servers: vec![ServerConfig {
                name: "staging".to_string(),
                host: "staging.example.com".to_string(),
                port: 22,
                user: "deploy".to_string(),
                identity_file: None,
                ssh_alias: None,
                docker_command: "docker".to_string(),
            }],
            ..AppConfig::default()
        }
    }

    /// A running tunnel whose SSH process is already gone, so a refresh has to
    /// rebuild it. The local port is only carried as tunnel metadata: nothing
    /// in these tests binds it, which keeps the tests off the ephemeral range.
    fn desired_running_tunnel() -> TunnelState {
        TunnelState {
            ssh_pid: None,
            ..direct_tunnel(TunnelStatus::Running)
        }
    }

    #[cfg(unix)]
    fn write_stalling_ssh(app: &TempApp, marker: &Path, inspect_json: &str) -> PathBuf {
        let script = app.dir.join("stalling-ssh");
        let body = format!(
            "#!/bin/sh\n: > {marker}\nsleep 1\ncat <<'INSPECT'\n{inspect_json}\nINSPECT\n",
            marker = marker.display(),
        );
        std::fs::write(&script, body).expect("fake ssh should be written");
        let mut permissions = std::fs::metadata(&script)
            .expect("fake ssh metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("fake ssh should be executable");
        script
    }

    #[cfg(unix)]
    fn write_remote_log_test_ssh(app: &TempApp, name: &str, body: &str) -> PathBuf {
        let script = app.dir.join(name);
        std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).expect("fake SSH script");
        let mut permissions = std::fs::metadata(&script)
            .expect("fake SSH metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("fake SSH executable");
        script
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn remote_command_capture_preserves_argv_and_raw_bytes() {
        use base64::{engine::general_purpose::STANDARD, Engine};
        let app = TempApp::new("command-capture");
        let script = write_remote_log_test_ssh(
            &app,
            "fake ssh",
            "printf 'stdout\\377'; printf 'stderr\\376' >&2",
        );
        let config = staging_config(&script.to_string_lossy());
        let log_dir = app.dir.join("logs");
        run_ssh_at(
            &config.defaults,
            &config.servers[0],
            "docker ps --format '{{.Names}}'",
            "Docker project discovery",
            "staging",
            Some(&log_dir),
        )
        .await
        .unwrap();
        let rows = command_log::read_command_logs_at(&log_dir, 200).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, CommandStatus::Success);
        assert_eq!(rows[0].exit_code, Some(0));
        let detail = command_log::read_command_detail_at(&log_dir, &rows[0].id).unwrap();
        assert_eq!(
            detail.args.last().unwrap(),
            "docker ps --format '{{.Names}}'"
        );
        assert_eq!(
            detail.remote_command.as_deref(),
            Some("docker ps --format '{{.Names}}'")
        );
        assert!(detail.local_command.contains("docker ps --format"));
        assert_eq!(
            STANDARD.decode(detail.stdout_base64).unwrap(),
            b"stdout\xff"
        );
        assert_eq!(
            STANDARD.decode(detail.stderr_base64).unwrap(),
            b"stderr\xfe"
        );
        assert!(!serde_json::to_string(&rows).unwrap().contains("stdout"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn remote_command_capture_failure_timeout_and_spawn_failure() {
        use base64::{engine::general_purpose::STANDARD, Engine};
        let app = TempApp::new("command-failures");
        let log_dir = app.dir.join("logs");
        let script = write_remote_log_test_ssh(&app, "fake-ssh", "printf fail >&2; exit 7");
        let mut config = staging_config(&script.to_string_lossy());
        assert!(run_ssh_at(
            &config.defaults,
            &config.servers[0],
            "docker ps",
            "Discovery",
            "staging",
            Some(&log_dir)
        )
        .await
        .is_err());
        let failure = &command_log::read_command_logs_at(&log_dir, 200).unwrap()[0];
        assert_eq!(failure.status, CommandStatus::Failure);
        assert_eq!(failure.exit_code, Some(7));
        let script =
            write_remote_log_test_ssh(&app, "slow-ssh", "printf partial; printf err >&2; sleep 3");
        config.defaults.ssh_binary = script.to_string_lossy().into_owned();
        config.defaults.docker_timeout_secs = 1;
        assert!(run_ssh_at(
            &config.defaults,
            &config.servers[0],
            "docker inspect x",
            "Inspect",
            "staging",
            Some(&log_dir)
        )
        .await
        .is_err());
        let timeout = &command_log::read_command_logs_at(&log_dir, 200).unwrap()[0];
        assert_eq!(timeout.status, CommandStatus::Timeout);
        let detail = command_log::read_command_detail_at(&log_dir, &timeout.id).unwrap();
        assert_eq!(STANDARD.decode(detail.stdout_base64).unwrap(), b"partial");
        assert_eq!(STANDARD.decode(detail.stderr_base64).unwrap(), b"err");
        config.defaults.ssh_binary = app.dir.join("missing-ssh").to_string_lossy().into_owned();
        assert!(run_ssh_at(
            &config.defaults,
            &config.servers[0],
            "docker ps",
            "Discovery",
            "staging",
            Some(&log_dir)
        )
        .await
        .is_err());
        assert_eq!(
            command_log::read_command_logs_at(&log_dir, 200).unwrap()[0].status,
            CommandStatus::Failure
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn remote_command_logs_failure_without_remote_output() {
        let app = TempApp::new("remote-log-failure");
        let script = write_remote_log_test_ssh(
            &app,
            "fail-ssh",
            "echo STDOUT_OUTPUT_MARKER; echo STDERR_OUTPUT_MARKER >&2; exit 7",
        );
        let config = staging_config(&script.to_string_lossy());
        let error = run_ssh_at(
            &config.defaults,
            &config.servers[0],
            "printf RAW_COMMAND_OUTPUT_MARKER",
            "Docker inspect",
            "staging/app/db/app-db-1",
            Some(&app.dir.join("logs")),
        )
        .await
        .expect_err("remote SSH failure");
        assert!(error.to_string().contains("STDERR_OUTPUT_MARKER"));

        let entries = operation_log::read_operation_logs_at(&app.dir.join("logs"), 200)
            .expect("logged operations");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].outcome, "attempt");
        assert_eq!(entries[1].outcome, "failure");
        assert_eq!(entries[1].operation, "Docker inspect");
        assert_eq!(entries[1].target, "staging/app/db/app-db-1");
        assert_eq!(entries[1].detail.as_deref(), Some("exit code 7"));
        let journal = serde_json::to_string(&entries).expect("serialize journal");
        assert!(!journal.contains("OUTPUT_MARKER"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn remote_command_logs_success_without_stdout() {
        let app = TempApp::new("remote-log-success");
        let script = write_remote_log_test_ssh(&app, "success-ssh", "echo STDOUT_OUTPUT_MARKER");
        let config = staging_config(&script.to_string_lossy());
        let output = run_ssh_at(
            &config.defaults,
            &config.servers[0],
            "true",
            "SSH connectivity",
            "staging",
            Some(&app.dir.join("logs")),
        )
        .await
        .expect("remote SSH success");
        assert!(output.contains("STDOUT_OUTPUT_MARKER"));
        let entries = operation_log::read_operation_logs_at(&app.dir.join("logs"), 200)
            .expect("logged operations");
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.outcome.as_str())
                .collect::<Vec<_>>(),
            ["attempt", "success"]
        );
        assert!(!serde_json::to_string(&entries)
            .unwrap()
            .contains("OUTPUT_MARKER"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn remote_command_logs_timeout_without_command_output() {
        let app = TempApp::new("remote-log-timeout");
        let script = write_remote_log_test_ssh(&app, "timeout-ssh", "sleep 3");
        let mut config = staging_config(&script.to_string_lossy());
        config.defaults.docker_timeout_secs = 1;
        let error = run_ssh_at(
            &config.defaults,
            &config.servers[0],
            "printf RAW_COMMAND_OUTPUT_MARKER",
            "Docker version",
            "staging",
            Some(&app.dir.join("logs")),
        )
        .await
        .expect_err("remote SSH timeout");
        assert!(error.to_string().contains("timed out"));
        let entries = operation_log::read_operation_logs_at(&app.dir.join("logs"), 200)
            .expect("logged operations");
        assert_eq!(entries[1].outcome, "failure");
        assert_eq!(entries[1].detail.as_deref(), Some("timed out"));
        assert!(!serde_json::to_string(&entries)
            .unwrap()
            .contains("OUTPUT_MARKER"));
    }

    #[cfg(unix)]
    async fn wait_for_path(path: &Path) {
        for _ in 0..500 {
            if path.exists() {
                return;
            }
            time::sleep(Duration::from_millis(10)).await;
        }
        panic!("{} was never created", path.display());
    }

    #[cfg(unix)]
    const REFRESHED_INSPECT: &str = r#"[{"Id":"sha256:new","State":{"Running":true},"Config":{"Labels":{"com.docker.compose.project":"app","com.docker.compose.service":"db"}},"NetworkSettings":{"Networks":{"app_default":{"IPAddress":"172.22.0.8","GlobalIPv6Address":""}}}}]"#;

    #[cfg(unix)]
    #[tokio::test]
    async fn reconciliation_logs_restarted_target() {
        let app = TempApp::new("reconciliation-log-restart");
        let marker = app.dir.join("inspect.started");
        let ssh = write_stalling_ssh(&app, &marker, REFRESHED_INSPECT);
        std::fs::write(
            &app.config_file,
            toml::to_string_pretty(&staging_config(&ssh.to_string_lossy()))
                .expect("config should serialize"),
        )
        .expect("config file should be written");
        app.write_state(&AppState {
            tunnels: vec![desired_running_tunnel()],
        });

        let refreshed = refresh_state_at(&app.config_file, &app.state_file)
            .await
            .expect("refresh should reconnect");
        let entries = operation_log::read_operation_logs_at(&app.dir.join("logs"), 200)
            .expect("logged operations");
        assert!(entries.iter().any(|entry| {
            entry.operation == "Tunnel reconnect"
                && entry.outcome == "success"
                && entry.target.contains("staging/app/db/app-db-1")
                && entry
                    .detail
                    .as_deref()
                    .is_some_and(|detail| detail.contains("172.22.0.8:5432"))
        }));

        close_tunnel_at(&app.state_file, &refreshed.tunnels[0].id)
            .await
            .expect("stop fake forward");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn refresh_transaction_does_not_overwrite_a_concurrent_close() {
        let app = TempApp::new("refresh-close-race");
        let marker = app.dir.join("inspect.started");
        let ssh = write_stalling_ssh(&app, &marker, REFRESHED_INSPECT);
        std::fs::write(
            &app.config_file,
            toml::to_string_pretty(&staging_config(&ssh.to_string_lossy()))
                .expect("config should serialize"),
        )
        .expect("config file should be written");
        app.write_state(&AppState {
            tunnels: vec![desired_running_tunnel()],
        });

        // The refresh holds the state lock for the whole reconciliation and is
        // stalled inside its remote inspect until the fake ssh finishes.
        let refresh = refresh_state_at(&app.config_file, &app.state_file);
        let close = async {
            wait_for_path(&marker).await;
            close_tunnel_at(&app.state_file, "db").await
        };
        let (refresh_result, close_result) = tokio::join!(refresh, close);

        refresh_result.expect("refresh should reconcile the tunnel");
        close_result.expect("a close requested during a refresh should succeed");

        let state = app.read_state();
        assert_eq!(state.tunnels.len(), 1);
        assert_eq!(
            state.tunnels[0].status,
            TunnelStatus::Stopped,
            "the refresh must not resurrect a tunnel stopped while it was reconciling"
        );
        assert_eq!(state.tunnels[0].ssh_pid, None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn close_all_transaction_stops_every_tunnel_and_terminates_its_process() {
        let app = TempApp::new("close-all");
        let mut child = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn a long-running child");
        let pid = child.id().expect("spawned child should have a pid");

        let mut running = direct_tunnel(TunnelStatus::Running);
        running.ssh_pid = Some(pid);
        let mut errored = direct_tunnel(TunnelStatus::Error);
        errored.id = "cache".to_string();
        errored.ssh_pid = None;
        let stopped = direct_tunnel(TunnelStatus::Stopped);
        app.write_state(&AppState {
            tunnels: vec![running, errored, stopped],
        });

        time::timeout(
            Duration::from_secs(10),
            close_all_tunnels_at(&app.state_file),
        )
        .await
        .expect("close all must not self-deadlock on the state lock")
        .expect("close all should succeed");

        let mut exited = false;
        for _ in 0..100 {
            if child.try_wait().expect("try_wait").is_some() {
                exited = true;
                break;
            }
            time::sleep(Duration::from_millis(20)).await;
        }
        if !exited {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        assert!(exited, "close all must terminate the recorded SSH process");

        let state = app.read_state();
        assert_eq!(state.tunnels.len(), 3);
        assert!(state
            .tunnels
            .iter()
            .all(|tunnel| tunnel.status == TunnelStatus::Stopped && tunnel.ssh_pid.is_none()));
        let entries = operation_log::read_operation_logs_at(&app.dir.join("logs"), 200)
            .expect("stop-all journal");
        assert!(entries.iter().any(|entry| {
            entry.operation == "Tunnel stop"
                && entry.outcome == "success"
                && entry.target.contains("staging/app/db/app-db-1")
        }));
    }

    #[tokio::test]
    async fn tunnel_stop_logs_after_state_is_saved() {
        let app = TempApp::new("stop-log-success");
        app.write_state(&AppState {
            tunnels: vec![TunnelState {
                ssh_pid: None,
                ..direct_tunnel(TunnelStatus::Running)
            }],
        });

        close_tunnel_at(&app.state_file, "db")
            .await
            .expect("tunnel stopped");
        let entries = operation_log::read_operation_logs_at(&app.dir.join("logs"), 200)
            .expect("stop journal");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].outcome, "attempt");
        assert_eq!(entries[1].operation, "Tunnel stop");
        assert_eq!(entries[1].outcome, "success");
        assert_eq!(app.read_state().tunnels[0].status, TunnelStatus::Stopped);
    }

    #[test]
    fn tunnel_open_logs_result_without_raw_error_text() {
        let app = TempApp::new("open-log-result");
        let log_dir = app.dir.join("logs");
        let request = OpenTunnelRequest {
            server: "staging".to_string(),
            project: "app".to_string(),
            service: "db".to_string(),
            container: Some("app-db-1".to_string()),
            target_port: 5432,
            network: Some("app_default".to_string()),
            local_host: Some("127.0.0.1".to_string()),
            local_port: Some(15432),
        };
        log_open_attempt(Some(&log_dir), &request);
        let result = Ok(direct_tunnel(TunnelStatus::Running));
        log_open_result(Some(&log_dir), &request, &result);
        let failure: Result<TunnelState> = Err(AppError::msg("PRIVATE_OUTPUT_MARKER"));
        log_open_result(Some(&log_dir), &request, &failure);

        let entries = operation_log::read_operation_logs_at(&log_dir, 200).expect("open journal");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].outcome, "attempt");
        assert_eq!(entries[1].outcome, "success");
        assert_eq!(entries[1].target, "staging/app/db/app-db-1:5432");
        assert!(entries[1]
            .detail
            .as_deref()
            .unwrap()
            .contains("127.0.0.1:15432 -> 172.22.0.4:5432 on app_default"));
        assert_eq!(entries[2].outcome, "failure");
        assert!(!serde_json::to_string(&entries)
            .unwrap()
            .contains("PRIVATE_OUTPUT_MARKER"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ssh_forward_startup_exit_is_reported_as_an_open_failure() {
        let app = TempApp::new("forward-startup-exit");
        let defaults = Defaults {
            local_host: "127.0.0.1".to_string(),
            ssh_binary: "sh".to_string(),
            docker_timeout_secs: 5,
        };
        let server = staging_config("sh").servers.remove(0);

        let error = spawn_ssh_forward_at(
            &defaults,
            &server,
            "127.0.0.1",
            15432,
            "172.22.0.4",
            5432,
            Some(&app.dir.join("logs")),
        )
        .await
        .expect_err("a forward that exits during startup must fail");

        assert!(
            error.to_string().contains("exited during startup"),
            "unexpected startup error: {error}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn forward_logs_successful_local_binding_without_command_args() {
        let app = TempApp::new("forward-log-success");
        let ssh = write_remote_log_test_ssh(&app, "forward-ssh", "exec sleep 30");
        let config = staging_config(&ssh.to_string_lossy());
        let mut child = spawn_ssh_forward_at(
            &config.defaults,
            &config.servers[0],
            "127.0.0.1",
            15432,
            "172.22.0.4",
            5432,
            Some(&app.dir.join("logs")),
        )
        .await
        .expect("fake forward starts");
        let pid = child.id().expect("fake forward PID");
        let _ = child.kill().await;
        let _ = child.wait().await;

        let entries = operation_log::read_operation_logs_at(&app.dir.join("logs"), 200)
            .expect("forward events");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].outcome, "attempt");
        assert_eq!(entries[1].operation, "SSH forward");
        assert_eq!(entries[1].outcome, "success");
        assert!(entries[1].target.contains("127.0.0.1:15432"));
        assert!(entries[1].target.contains("172.22.0.4:5432"));
        assert_eq!(
            entries[1].detail.as_deref(),
            Some(format!("PID {pid}").as_str())
        );
        assert!(!serde_json::to_string(&entries)
            .unwrap()
            .contains("forward-ssh"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn forward_command_log_keeps_one_id_through_stop() {
        use base64::{engine::general_purpose::STANDARD, Engine};
        let app = TempApp::new("forward-command-lifecycle");
        let ssh = write_remote_log_test_ssh(
            &app,
            "forward-ssh",
            "printf forward-stderr >&2; exec sleep 30",
        );
        let config = staging_config(&ssh.to_string_lossy());
        let log_dir = app.dir.join("logs");
        let started = spawn_ssh_forward_at(
            &config.defaults,
            &config.servers[0],
            "127.0.0.1",
            15432,
            "172.22.0.4",
            5432,
            Some(&log_dir),
        )
        .await
        .unwrap();
        let id = started.command_log_id.clone().expect("forward command ID");
        let mut tunnel = direct_tunnel(TunnelStatus::Running);
        tunnel.ssh_pid = started.child.id();
        tunnel.command_log_id = Some(id.clone());
        app.write_state(&AppState {
            tunnels: vec![tunnel],
        });
        let rows = command_log::read_command_logs_at(&log_dir, 200).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, CommandStatus::Running);
        let stderr_path = log_dir.join("commands").join(&id).join("stderr.bin");
        for _ in 0..100 {
            if std::fs::metadata(&stderr_path).is_ok_and(|metadata| metadata.len() > 0) {
                break;
            }
            time::sleep(Duration::from_millis(10)).await;
        }
        let detail = command_log::read_command_detail_at(&log_dir, &id).unwrap();
        assert!(detail.local_command.contains("-N"));
        assert!(detail.local_command.contains("-L"));
        assert_eq!(
            STANDARD.decode(detail.stderr_base64).unwrap(),
            b"forward-stderr"
        );
        close_tunnel_at(&app.state_file, "db").await.unwrap();
        let rows = command_log::read_command_logs_at(&log_dir, 200).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].status, CommandStatus::Stopped);
        assert!(rows[0].revision >= 1);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn forward_command_log_reconnect_finishes_old_and_starts_new() {
        let app = TempApp::new("forward-command-reconnect");
        let body = format!(
            "case \"$*\" in *-N*) exec sleep 30 ;; *) printf '%s\\n' '{}' ;; esac",
            REFRESHED_INSPECT
        );
        let ssh = write_remote_log_test_ssh(&app, "forward-ssh", &body);
        let config = staging_config(&ssh.to_string_lossy());
        std::fs::write(&app.config_file, toml::to_string_pretty(&config).unwrap()).unwrap();
        let log_dir = app.dir.join("logs");
        let old = spawn_ssh_forward_at(
            &config.defaults,
            &config.servers[0],
            "127.0.0.1",
            15432,
            "172.22.0.4",
            5432,
            Some(&log_dir),
        )
        .await
        .unwrap();
        let old_id = old.command_log_id.clone().unwrap();
        let mut tunnel = direct_tunnel(TunnelStatus::Running);
        tunnel.ssh_pid = old.child.id();
        tunnel.command_log_id = Some(old_id.clone());
        app.write_state(&AppState {
            tunnels: vec![tunnel],
        });
        let refreshed = refresh_state_at(&app.config_file, &app.state_file)
            .await
            .unwrap();
        let new_id = refreshed.tunnels[0].command_log_id.clone().unwrap();
        assert_ne!(new_id, old_id);
        let rows = command_log::read_command_logs_at(&log_dir, 200).unwrap();
        assert_eq!(
            rows.iter().find(|row| row.id == old_id).unwrap().status,
            CommandStatus::Stopped
        );
        assert_eq!(
            rows.iter().find(|row| row.id == new_id).unwrap().status,
            CommandStatus::Running
        );
        close_tunnel_at(&app.state_file, "db").await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn forward_command_log_does_not_claim_stop_when_kill_fails() {
        let app = TempApp::new("forward-command-stop-failure");
        let log_dir = app.dir.join("logs");
        let row = begin_command_in(&log_dir, "staging", "ssh -N -L", "ssh", &[], None).unwrap();
        let mut tunnel = direct_tunnel(TunnelStatus::Running);
        tunnel.ssh_pid = Some(u32::MAX);
        tunnel.command_log_id = Some(row.id.clone());
        app.write_state(&AppState {
            tunnels: vec![tunnel],
        });
        assert!(close_tunnel_at(&app.state_file, "db").await.is_err());
        assert_eq!(
            command_log::read_command_logs_at(&log_dir, 200).unwrap()[0].status,
            CommandStatus::Running
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn forward_detects_failure_during_startup_grace_period() {
        let app = TempApp::new("forward-log-failure");
        let ssh = write_remote_log_test_ssh(&app, "forward-ssh", "sleep 0.35; exit 9");
        let config = staging_config(&ssh.to_string_lossy());
        spawn_ssh_forward_at(
            &config.defaults,
            &config.servers[0],
            "127.0.0.1",
            15432,
            "172.22.0.4",
            5432,
            Some(&app.dir.join("logs")),
        )
        .await
        .expect_err("fake forward exits during startup");

        let entries = operation_log::read_operation_logs_at(&app.dir.join("logs"), 200)
            .expect("forward events");
        assert_eq!(entries[1].operation, "SSH forward");
        assert_eq!(entries[1].outcome, "failure");
        assert_eq!(entries[1].detail.as_deref(), Some("exit code 9"));
    }

    #[test]
    fn tunnel_id_scopes_when_service_id_already_belongs_to_another_target() {
        let state = AppState {
            tunnels: vec![direct_tunnel(TunnelStatus::Running)],
        };

        assert_eq!(
            tunnel_state_id(&state, "staging", "app", "db", "app-db-1", 5432),
            "db"
        );
        assert_eq!(
            tunnel_state_id(&state, "staging", "billing", "db", "app-db-1", 5432),
            "staging-billing-db-app-db-1-5432"
        );
    }

    fn stopped_tunnel_with_local_port(id: &str, local_port: u16) -> TunnelState {
        TunnelState {
            id: id.to_string(),
            local_port,
            status: TunnelStatus::Stopped,
            ssh_pid: None,
            ..direct_tunnel(TunnelStatus::Stopped)
        }
    }

    #[test]
    fn local_port_is_reused_when_previous_port_is_available() {
        let ephemeral = TcpListener::bind("127.0.0.1:0").expect("ephemeral listener should bind");
        let port = ephemeral
            .local_addr()
            .expect("local address should resolve")
            .port();
        drop(ephemeral);
        let state = AppState {
            tunnels: vec![stopped_tunnel_with_local_port("db", port)],
        };

        let resolved =
            resolve_local_port(&state, "db", "127.0.0.1", None).expect("local port should resolve");

        assert_eq!(resolved, port);
    }

    #[test]
    fn local_port_falls_back_to_a_new_port_when_previous_port_is_occupied() {
        let occupied = TcpListener::bind("127.0.0.1:0").expect("ephemeral listener should bind");
        let port = occupied
            .local_addr()
            .expect("local address should resolve")
            .port();
        let state = AppState {
            tunnels: vec![stopped_tunnel_with_local_port("db", port)],
        };

        let resolved =
            resolve_local_port(&state, "db", "127.0.0.1", None).expect("local port should resolve");

        assert_ne!(resolved, port);
    }

    #[tokio::test]
    async fn stopped_previous_tunnel_without_pid_does_not_wait_for_its_old_port() {
        let occupied = TcpListener::bind("127.0.0.1:0").expect("ephemeral listener should bind");
        let port = occupied
            .local_addr()
            .expect("local address should resolve")
            .port();
        let previous = stopped_tunnel_with_local_port("db", port);

        release_previous_tunnel(&previous)
            .await
            .expect("a previous tunnel without a pid must not wait for its old port");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn previous_forward_termination_failure_is_not_ignored() {
        let mut previous = direct_tunnel(TunnelStatus::Running);
        previous.ssh_pid = Some(u32::MAX);
        assert!(release_previous_tunnel(&previous).await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reconciliation_preserves_pid_when_termination_fails() {
        let mut tunnel = direct_tunnel(TunnelStatus::Running);
        tunnel.ssh_pid = Some(u32::MAX);
        terminate_and_mark_reconciliation_error(&mut tunnel, "inspect failed").await;
        assert_eq!(tunnel.status, TunnelStatus::Error);
        assert_eq!(tunnel.ssh_pid, Some(u32::MAX));
    }

    #[test]
    fn local_port_is_assigned_automatically_without_a_previous_tunnel() {
        let state = AppState::default();

        let resolved =
            resolve_local_port(&state, "db", "127.0.0.1", None).expect("local port should resolve");

        assert!(resolved > 0);
    }

    #[test]
    fn old_server_config_defaults_to_docker_command() {
        let raw = r#"
            [defaults]

            [[servers]]
            name = "staging"
            host = "staging.example.com"
            port = 22
            user = "deploy"
        "#;

        let config: AppConfig = toml::from_str(raw).expect("config should parse");

        assert_eq!(config.servers[0].docker_command, "docker");
    }

    #[test]
    fn env_profile_renders_tunnel_port_alias_and_extra_env() {
        let profile = EnvProfileConfig {
            name: "test".to_string(),
            target_dir: Some(PathBuf::from("/tmp/app")),
            tunnel_ports: vec![EnvTunnelPort {
                tunnel_id: "db".to_string(),
                alias: "server-db".to_string(),
                env_key: Some("DATABASE-PORT".to_string()),
            }],
            extra_env: vec![EnvPlainEntry {
                key: "DATABASE-HOST".to_string(),
                value: "127.0.0.1".to_string(),
            }],
        };
        let state = AppState {
            tunnels: vec![direct_tunnel(TunnelStatus::Running)],
        };

        let rendered = render_env_profile_at(&profile, &state, "2026-07-21 16:30:45")
            .expect("profile should render");

        assert!(rendered.contains("# compose-tunnel env: test"));
        assert!(rendered.contains("# created_at: 2026-07-21 16:30:45"));
        assert!(rendered.contains("# tunnel: server_db"));
        assert!(rendered.contains("#   server: staging"));
        assert!(rendered.contains("#   project: app"));
        assert!(rendered.contains("#   service: db"));
        assert!(rendered.contains("#   container: app-db-1"));
        assert!(rendered.contains("#   remote_port: 5432"));
        assert!(rendered.contains("#   local: 127.0.0.1:15432"));
        assert!(rendered.contains("server_db=15432"));
        assert!(rendered.contains("DATABASE_PORT=15432"));
        assert!(rendered.contains("DATABASE_HOST=127.0.0.1"));
    }

    #[test]
    fn env_profile_replaces_port_references_in_extra_env() {
        let profile = EnvProfileConfig {
            name: "test".to_string(),
            target_dir: Some(PathBuf::from("/tmp/app")),
            tunnel_ports: vec![EnvTunnelPort {
                tunnel_id: "db".to_string(),
                alias: "server_name-container_name".to_string(),
                env_key: None,
            }],
            extra_env: vec![
                EnvPlainEntry {
                    key: "DATABASE-PORT".to_string(),
                    value: "${server_name-container_name}".to_string(),
                },
                EnvPlainEntry {
                    key: "DATABASE-URL".to_string(),
                    value: "postgres://127.0.0.1:${server_name-container_name}/app".to_string(),
                },
                EnvPlainEntry {
                    key: "UNKNOWN-REF".to_string(),
                    value: "${not_configured}".to_string(),
                },
            ],
        };
        let state = AppState {
            tunnels: vec![direct_tunnel(TunnelStatus::Running)],
        };

        let rendered = render_env_profile_at(&profile, &state, "2026-07-21 16:30:45")
            .expect("profile should render");

        assert!(rendered.contains("server_name_container_name=15432"));
        assert!(rendered.contains("DATABASE_PORT=15432"));
        assert!(rendered.contains("DATABASE_URL=postgres://127.0.0.1:15432/app"));
        assert!(rendered.contains("UNKNOWN_REF=${not_configured}"));
    }

    #[test]
    fn active_env_profiles_are_scoped_by_target_directory() {
        let config = AppConfig {
            env_profiles: vec![
                EnvProfileConfig {
                    name: "app-test".to_string(),
                    target_dir: Some(PathBuf::from("/tmp/app")),
                    ..EnvProfileConfig::default()
                },
                EnvProfileConfig {
                    name: "admin-prod".to_string(),
                    target_dir: Some(PathBuf::from("/tmp/admin")),
                    ..EnvProfileConfig::default()
                },
            ],
            active_env_profiles: BTreeMap::from([
                ("/tmp/app".to_string(), "app-test".to_string()),
                ("/tmp/admin".to_string(), "admin-prod".to_string()),
            ]),
            ..AppConfig::default()
        };

        let active = active_env_profiles_for_config(&config);

        assert_eq!(active.get("/tmp/app"), Some(&"app-test".to_string()));
        assert_eq!(active.get("/tmp/admin"), Some(&"admin-prod".to_string()));
    }

    #[test]
    fn active_env_profiles_normalize_stored_target_keys() {
        let config = AppConfig {
            env_profiles: vec![EnvProfileConfig {
                name: "app-test".to_string(),
                target_dir: Some(PathBuf::from("relative-app")),
                ..EnvProfileConfig::default()
            }],
            active_env_profiles: BTreeMap::from([(
                "relative-app".to_string(),
                "app-test".to_string(),
            )]),
            ..AppConfig::default()
        };

        let active = active_env_profiles_for_config(&config);
        let normalized_key = active
            .keys()
            .next()
            .expect("active profile should be preserved");

        assert!(normalized_key.ends_with("relative-app"));
        assert_eq!(active.get(normalized_key), Some(&"app-test".to_string()));
    }

    #[test]
    fn env_profiles_for_display_normalizes_target_directories() {
        let config = AppConfig {
            env_profiles: vec![EnvProfileConfig {
                name: "app-test".to_string(),
                target_dir: Some(PathBuf::from("relative-app")),
                ..EnvProfileConfig::default()
            }],
            ..AppConfig::default()
        };

        let profiles = env_profiles_for_display(&config);
        let target_dir = profiles[0]
            .target_dir
            .as_ref()
            .expect("target dir should be present");

        assert!(target_dir.is_absolute());
        assert!(target_dir.ends_with("relative-app"));
    }

    #[test]
    fn env_profile_target_directory_is_required() {
        let profile = EnvProfileConfig {
            name: "test".to_string(),
            target_dir: None,
            ..EnvProfileConfig::default()
        };

        let error = env_profile_target_key(&profile).expect_err("target dir should be required");

        assert_eq!(error.to_string(), "target directory is required");
    }

    #[test]
    fn env_profile_target_directory_is_normalized_to_absolute_path() {
        let profile = EnvProfileConfig {
            name: "test".to_string(),
            target_dir: Some(PathBuf::from("relative-app")),
            ..EnvProfileConfig::default()
        };

        let target_dir = env_profile_target_dir(&profile).expect("target dir should normalize");

        assert!(target_dir.is_absolute());
        assert!(target_dir.ends_with("relative-app"));
    }

    #[test]
    fn env_profile_plain_values_reject_newlines() {
        let profile = EnvProfileConfig {
            name: "test".to_string(),
            target_dir: Some(PathBuf::from("/tmp/app")),
            extra_env: vec![EnvPlainEntry {
                key: "DATABASE_PASSWORD".to_string(),
                value: "line1\nline2".to_string(),
            }],
            ..EnvProfileConfig::default()
        };

        let error = normalize_env_profile(profile).expect_err("newline should be rejected");

        assert_eq!(
            error.to_string(),
            "env value for DATABASE_PASSWORD may not contain newlines"
        );
    }

    #[test]
    fn env_profile_extra_env_deduplicates_normalized_keys() {
        let profile = EnvProfileConfig {
            name: "test".to_string(),
            target_dir: Some(PathBuf::from("/tmp/app")),
            extra_env: vec![
                EnvPlainEntry {
                    key: "DATABASE-HOST".to_string(),
                    value: "old.example.com".to_string(),
                },
                EnvPlainEntry {
                    key: "DATABASE_HOST".to_string(),
                    value: "new.example.com".to_string(),
                },
            ],
            ..EnvProfileConfig::default()
        };

        let profile = normalize_env_profile(profile).expect("profile should normalize");

        assert_eq!(profile.extra_env.len(), 1);
        assert_eq!(profile.extra_env[0].key, "DATABASE_HOST");
        assert_eq!(profile.extra_env[0].value, "new.example.com");
    }

    #[test]
    fn env_profile_tunnel_ports_deduplicate_normalized_aliases() {
        let profile = EnvProfileConfig {
            name: "test".to_string(),
            target_dir: Some(PathBuf::from("/tmp/app")),
            tunnel_ports: vec![
                EnvTunnelPort {
                    tunnel_id: "db".to_string(),
                    alias: "server-db".to_string(),
                    env_key: Some("OLD_PORT".to_string()),
                },
                EnvTunnelPort {
                    tunnel_id: "redis".to_string(),
                    alias: "server_db".to_string(),
                    env_key: Some("NEW-PORT".to_string()),
                },
            ],
            ..EnvProfileConfig::default()
        };

        let profile = normalize_env_profile(profile).expect("profile should normalize");

        assert_eq!(profile.tunnel_ports.len(), 1);
        assert_eq!(profile.tunnel_ports[0].tunnel_id, "redis");
        assert_eq!(profile.tunnel_ports[0].alias, "server_db");
        assert_eq!(profile.tunnel_ports[0].env_key.as_deref(), Some("NEW_PORT"));
    }

    fn compose_service(service: &str, container: &str, networks: &[&str]) -> ComposeService {
        ComposeService {
            service: service.to_string(),
            container: container.to_string(),
            status: "Up 1 minute".to_string(),
            ports: Vec::new(),
            networks: networks.iter().map(|value| (*value).to_string()).collect(),
            image: "postgres:16".to_string(),
        }
    }

    #[test]
    fn selects_only_container_when_replica_is_not_specified() {
        let services = vec![compose_service("db", "app-db-1", &["app_default"])];

        let selected = select_compose_container(&services, "app", "db", None)
            .expect("single replica should be selected");

        assert_eq!(selected.container, "app-db-1");
    }

    #[test]
    fn selects_requested_container_among_replicas() {
        let services = vec![
            compose_service("db", "app-db-1", &["app_default"]),
            compose_service("db", "app-db-2", &["app_default"]),
        ];

        let selected = select_compose_container(&services, "app", "db", Some("app-db-2"))
            .expect("explicit replica should be selected");

        assert_eq!(selected.container, "app-db-2");
    }

    #[test]
    fn rejects_ambiguous_service_without_container() {
        let services = vec![
            compose_service("db", "app-db-1", &["app_default"]),
            compose_service("db", "app-db-2", &["app_default"]),
        ];

        let error = select_compose_container(&services, "app", "db", None)
            .expect_err("multiple replicas must require a container");

        assert!(error.to_string().contains("app-db-1, app-db-2"));
    }

    #[test]
    fn rejects_container_from_another_service() {
        let services = vec![
            compose_service("db", "app-db-1", &["app_default"]),
            compose_service("cache", "app-cache-1", &["app_default"]),
        ];

        let error = select_compose_container(&services, "app", "db", Some("app-cache-1"))
            .expect_err("container must belong to the requested service");

        assert!(error.to_string().contains("does not belong to app/db"));
    }

    #[test]
    fn network_resolution_prefers_project_default() {
        let service = compose_service("api", "app-api-1", &["shared", "app_default"]);

        assert_eq!(
            resolve_network("app", &service, None).expect("default network should resolve"),
            "app_default"
        );
    }

    #[test]
    fn network_resolution_selects_the_only_attached_network() {
        let service = compose_service("api", "app-api-1", &["shared"]);

        assert_eq!(
            resolve_network("app", &service, None)
                .expect("the only attached network should resolve"),
            "shared"
        );
    }

    #[test]
    fn network_resolution_rejects_unattached_requested_network() {
        let service = compose_service("api", "app-api-1", &["app_default"]);

        let error = resolve_network("app", &service, Some("private"))
            .expect_err("unattached network must fail");

        assert!(error
            .to_string()
            .contains("private is not attached to app-api-1"));
    }

    #[test]
    fn network_resolution_lists_ambiguous_networks() {
        let service = compose_service("api", "app-api-1", &["frontend", "backend"]);

        let error = resolve_network("app", &service, None)
            .expect_err("ambiguous networks must require selection");

        assert!(error.to_string().contains("backend, frontend"));
    }

    const RUNNING_INSPECT: &str = r#"[
  {
    "Id": "sha256:abc123",
    "Name": "/app-db-1",
    "State": { "Running": true },
    "Config": {
      "Labels": {
        "com.docker.compose.project": "app",
        "com.docker.compose.service": "db"
      }
    },
    "NetworkSettings": {
      "Networks": {
        "app_default": {
          "IPAddress": "172.22.0.4",
          "GlobalIPv6Address": "fd00::4"
        }
      }
    }
  }
]"#;

    #[test]
    fn parses_running_container_and_prefers_ipv4() {
        let target =
            parse_inspected_container(RUNNING_INSPECT, "app-db-1", "app", "db", "app_default")
                .expect("inspect result should parse");

        assert_eq!(target.container, "app-db-1");
        assert_eq!(target.id, "sha256:abc123");
        assert_eq!(target.network, "app_default");
        assert_eq!(target.ip, "172.22.0.4");
    }

    #[test]
    fn parses_ipv6_only_container_address() {
        let raw = RUNNING_INSPECT.replace("\"IPAddress\": \"172.22.0.4\"", "\"IPAddress\": \"\"");

        let target = parse_inspected_container(&raw, "app-db-1", "app", "db", "app_default")
            .expect("IPv6-only inspect result should parse");

        assert_eq!(target.ip, "fd00::4");
    }

    #[test]
    fn rejects_stopped_container() {
        let raw = RUNNING_INSPECT.replace("\"Running\": true", "\"Running\": false");

        let error = parse_inspected_container(&raw, "app-db-1", "app", "db", "app_default")
            .expect_err("stopped container must fail");

        assert!(error.to_string().contains("app-db-1 is not running"));
    }

    #[test]
    fn rejects_mismatched_compose_labels() {
        let raw = RUNNING_INSPECT.replace(
            "\"com.docker.compose.project\": \"app\"",
            "\"com.docker.compose.project\": \"billing\"",
        );

        let error = parse_inspected_container(&raw, "app-db-1", "app", "db", "app_default")
            .expect_err("mismatched project label must fail");

        assert!(error.to_string().contains("does not belong to app/db"));
    }

    #[test]
    fn rejects_missing_network_and_empty_addresses() {
        let missing =
            parse_inspected_container(RUNNING_INSPECT, "app-db-1", "app", "db", "private")
                .expect_err("missing network must fail");
        assert!(missing.to_string().contains("private is not attached"));

        let raw = RUNNING_INSPECT
            .replace("\"IPAddress\": \"172.22.0.4\"", "\"IPAddress\": \"\"")
            .replace(
                "\"GlobalIPv6Address\": \"fd00::4\"",
                "\"GlobalIPv6Address\": \"\"",
            );
        let empty = parse_inspected_container(&raw, "app-db-1", "app", "db", "app_default")
            .expect_err("empty addresses must fail");
        assert!(empty.to_string().contains("has no IP address"));
    }

    #[test]
    fn rejects_malformed_or_ambiguous_inspect_output() {
        assert!(
            parse_inspected_container("not-json", "app-db-1", "app", "db", "app_default").is_err()
        );

        let empty = parse_inspected_container("[]", "app-db-1", "app", "db", "app_default")
            .expect_err("empty inspect output must fail");
        assert!(empty.to_string().contains("exactly one container"));

        let object = RUNNING_INSPECT
            .trim()
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
            .expect("fixture should be an array");
        let duplicate = format!("[{object},{object}]");
        let multiple =
            parse_inspected_container(&duplicate, "app-db-1", "app", "db", "app_default")
                .expect_err("multiple inspect objects must fail");
        assert!(multiple.to_string().contains("exactly one container"));
    }

    #[test]
    fn rejects_missing_compose_labels() {
        let raw = RUNNING_INSPECT.replace(
            "\"com.docker.compose.service\": \"db\"",
            "\"unrelated.label\": \"db\"",
        );

        let error = parse_inspected_container(&raw, "app-db-1", "app", "db", "app_default")
            .expect_err("missing Compose label must fail");

        assert!(error.to_string().contains("does not belong to app/db"));
    }

    #[test]
    fn ssh_forward_spec_formats_ipv4_hosts() {
        assert_eq!(
            ssh_forward_spec("127.0.0.1", 15432, "172.22.0.4", 5432),
            "127.0.0.1:15432:172.22.0.4:5432"
        );
    }

    #[test]
    fn ssh_forward_spec_brackets_ipv6_hosts() {
        assert_eq!(
            ssh_forward_spec("::1", 15432, "fd00::4", 5432),
            "[::1]:15432:[fd00::4]:5432"
        );
    }

    #[test]
    fn ssh_forward_args_place_local_forward_before_target() {
        let server = ServerConfig {
            name: "staging".to_string(),
            host: "staging.example.com".to_string(),
            port: 22,
            user: "deploy".to_string(),
            identity_file: None,
            ssh_alias: None,
            docker_command: "docker".to_string(),
        };

        let args = ssh_forward_args(&server, "127.0.0.1", 15432, "172.22.0.4", 5432)
            .expect("forward args should build");
        let forward_index = args
            .iter()
            .position(|value| value == "-L")
            .expect("-L should be present");

        assert_eq!(args[forward_index + 1], "127.0.0.1:15432:172.22.0.4:5432");
        assert_eq!(
            args.last().map(String::as_str),
            Some("deploy@staging.example.com")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn command_timeout_stops_a_slow_process() {
        let args = vec!["-c".to_string(), "sleep 1".to_string()];

        let error = run_command_with_timeout(
            "sh",
            &args,
            std::time::Duration::from_millis(10),
            "test command",
        )
        .await
        .expect_err("slow command must time out");

        assert!(error.to_string().contains("test command timed out"));
    }
}
