use std::{
    collections::{BTreeMap, BTreeSet},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Output, Stdio},
    time::Duration,
};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use tokio::{
    fs,
    process::{Child, Command},
    time,
};

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
    fs::create_dir_all(&paths.config_dir).await?;
    fs::create_dir_all(&paths.logs_dir).await?;
    if !paths.config_file.exists() {
        save_config(&AppConfig::default()).await?;
    }
    if !paths.state_file.exists() {
        save_state(&AppState::default()).await?;
    }
    Ok(paths)
}

pub async fn load_config() -> Result<AppConfig> {
    let paths = init_config().await?;
    let raw = fs::read_to_string(paths.config_file).await?;
    Ok(toml::from_str(&raw)?)
}

pub async fn save_config(config: &AppConfig) -> Result<()> {
    let paths = app_paths()?;
    fs::create_dir_all(&paths.config_dir).await?;
    fs::write(&paths.config_file, toml::to_string_pretty(config)?).await?;
    Ok(())
}

pub async fn load_state() -> Result<AppState> {
    let paths = init_config().await?;
    let raw = fs::read_to_string(paths.state_file).await?;
    let migration = parse_stored_state(&raw)?;
    for pid in &migration.legacy_pids {
        if pid_is_running(*pid) {
            let _ = kill_pid(*pid).await;
        }
    }
    if migration.changed {
        save_state(&migration.state).await?;
    }
    Ok(migration.state)
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

pub async fn save_state(state: &AppState) -> Result<()> {
    let paths = app_paths()?;
    fs::create_dir_all(&paths.config_dir).await?;
    let raw = serde_json::to_string_pretty(state)?;
    fs::write(&paths.state_file, raw).await?;
    Ok(())
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

    match run_ssh(&config.defaults, &server, "true").await {
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
    match run_ssh(&config.defaults, &server, &version_command).await {
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
    let output = run_ssh(&config.defaults, server, &command).await?;

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
    let output = run_ssh(&config.defaults, server, &command).await?;

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
    let existing_state = load_state().await?;
    let tunnel_id = tunnel_state_id(
        &existing_state,
        &request.server,
        &request.project,
        &request.service,
        &target.container,
        request.target_port,
    );

    if let Some(previous) = existing_state
        .tunnels
        .iter()
        .find(|tunnel| tunnel.id == tunnel_id)
    {
        release_previous_tunnel(previous).await?;
    }

    let local_port =
        resolve_local_port(&existing_state, &tunnel_id, &local_host, request.local_port)?;
    let child = spawn_ssh_forward(
        &config.defaults,
        &server,
        &local_host,
        local_port,
        &target.ip,
        request.target_port,
    )
    .await?;
    let ssh_pid = child.id();

    let state = TunnelState {
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
        status: TunnelStatus::Running,
        mode: TunnelMode::ContainerDirect,
        started_at: Some(now_string()),
        last_error: None,
    };

    if let Err(error) = upsert_tunnel_state(state.clone()).await {
        if let Some(pid) = ssh_pid {
            let _ = kill_pid(pid).await;
        }
        return Err(error);
    }
    Ok(state)
}

pub async fn close_tunnel(tunnel_id: String) -> Result<()> {
    let mut state = load_state().await?;
    let mut changed = false;

    for tunnel in &mut state.tunnels {
        if tunnel.id != tunnel_id {
            continue;
        }
        if let Some(pid) = tunnel.ssh_pid {
            kill_pid(pid).await?;
        }
        tunnel.status = TunnelStatus::Stopped;
        tunnel.ssh_pid = None;
        changed = true;
    }

    if !changed {
        return Err(AppError::msg(format!("tunnel {tunnel_id} was not found")));
    }

    save_state(&state).await
}

pub async fn close_all_tunnels() -> Result<()> {
    let tunnels = load_state().await?.tunnels;
    for tunnel in tunnels {
        let _ = close_tunnel(tunnel.id).await;
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
) -> Result<String> {
    let mut args = ssh_base_args(server);
    args.push(remote_command.to_string());
    let output = run_command_with_timeout(
        &defaults.ssh_binary,
        &args,
        Duration::from_secs(defaults.docker_timeout_secs),
        "ssh",
    )
    .await?;
    if !output.status.success() {
        return Err(AppError::msg(command_error(
            "ssh",
            output.status.code(),
            &output.stderr,
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn spawn_ssh_forward(
    defaults: &Defaults,
    server: &ServerConfig,
    local_host: &str,
    local_port: u16,
    remote_host: &str,
    remote_port: u16,
) -> Result<Child> {
    let args = ssh_forward_args(server, local_host, local_port, remote_host, remote_port)?;

    let mut child = Command::new(&defaults.ssh_binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    time::sleep(Duration::from_millis(250)).await;
    if let Some(status) = child.try_wait()? {
        return Err(AppError::msg(format!(
            "ssh forward exited during startup with status {status}"
        )));
    }

    Ok(child)
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
    let command = format!(
        "{} inspect --type container {}",
        docker_command(server),
        shell_quote(container)
    );
    let output = run_ssh(defaults, server, &command).await?;
    parse_inspected_container(&output, container, project, service, network)
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
    let _ = kill_pid(pid).await;
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

async fn upsert_tunnel_state(tunnel: TunnelState) -> Result<()> {
    let mut state = load_state().await?;
    if let Some(existing) = state.tunnels.iter_mut().find(|item| item.id == tunnel.id) {
        *existing = tunnel;
    } else {
        state.tunnels.push(tunnel);
    }
    state.tunnels.sort_by(|left, right| left.id.cmp(&right.id));
    save_state(&state).await
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
    if let Some(pid) = tunnel.ssh_pid {
        if pid_is_running(pid) {
            let _ = kill_pid(pid).await;
        }
    }
    mark_reconciliation_error(tunnel, error)
}

async fn load_refreshed_state() -> Result<AppState> {
    let config = load_config().await?;
    let mut state = load_state().await?;
    let report = reconcile_tunnels(&config, &mut state).await;
    if report.changed {
        let saved = save_state(&state).await;
        finalize_refresh_save(saved, &report.spawned_pids).await?;
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

async fn reconcile_tunnels(config: &AppConfig, state: &mut AppState) -> ReconcileReport {
    let mut changed = false;
    let mut spawned_pids = Vec::new();

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
            changed |= terminate_and_mark_reconciliation_error(
                tunnel,
                format!("server {} was not found", tunnel.server),
            )
            .await;
            continue;
        };

        let inspected = match inspect_container(
            &config.defaults,
            &server,
            &tunnel.container,
            &tunnel.project,
            &tunnel.service,
            &tunnel.network,
        )
        .await
        {
            Ok(inspected) => inspected,
            Err(error) => {
                changed |= terminate_and_mark_reconciliation_error(tunnel, error.to_string()).await;
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
                    changed |= mark_reconciliation_error(tunnel, error.to_string());
                    continue;
                }
                tunnel.ssh_pid = None;

                match spawn_ssh_forward(
                    &config.defaults,
                    &server,
                    &tunnel.local_host,
                    tunnel.local_port,
                    &inspected.ip,
                    tunnel.target_port,
                )
                .await
                {
                    Ok(child) => {
                        let ssh_pid = child.id();
                        if let Some(pid) = ssh_pid {
                            spawned_pids.push(pid);
                        }
                        tunnel.container_id = inspected.id;
                        tunnel.container_ip = inspected.ip;
                        tunnel.ssh_pid = ssh_pid;
                        tunnel.status = TunnelStatus::Running;
                        tunnel.started_at = Some(now_string());
                        tunnel.last_error = None;
                        changed = true;
                    }
                    Err(error) => {
                        changed |= mark_reconciliation_error(tunnel, error.to_string());
                    }
                }
            }
        }
    }

    ReconcileReport {
        changed,
        spawned_pids,
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
        let _ = Command::new("kill").arg(pid.to_string()).output().await?;
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()
            .await?;
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
            status,
            mode: TunnelMode::ContainerDirect,
            started_at: Some("2026-09-20T12:00:00Z".to_string()),
            last_error: None,
        }
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
