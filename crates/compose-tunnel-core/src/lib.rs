use std::{
    collections::{BTreeMap, BTreeSet},
    net::TcpListener,
    path::{Path, PathBuf},
    process::Stdio,
};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use tokio::{
    fs,
    process::{Child, Command},
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
    #[serde(default = "default_socat_image")]
    pub socat_image: String,
    #[serde(default = "default_socat_command")]
    pub socat_command: String,
    #[serde(default = "default_ssh_binary")]
    pub ssh_binary: String,
    #[serde(default = "default_docker_timeout_secs")]
    pub docker_timeout_secs: u64,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            local_host: default_local_host(),
            socat_image: default_socat_image(),
            socat_command: default_socat_command(),
            ssh_binary: default_ssh_binary(),
            docker_timeout_secs: default_docker_timeout_secs(),
        }
    }
}

fn default_local_host() -> String {
    "127.0.0.1".to_string()
}

fn default_socat_image() -> String {
    "alpine/socat:latest".to_string()
}

fn default_socat_command() -> String {
    "socat".to_string()
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
    #[serde(default)]
    pub default_socat_image: Option<String>,
    #[serde(default = "default_docker_command")]
    pub docker_command: String,
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
    pub network: String,
    pub target_port: u16,
    pub socat_port: u16,
    pub local_host: String,
    pub local_port: u16,
    pub socat_container: String,
    pub socat_container_ip: String,
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
    SocatDirect,
}

impl Default for TunnelMode {
    fn default() -> Self {
        Self::SocatDirect
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenTunnelRequest {
    pub server: String,
    pub project: String,
    pub service: String,
    pub target_port: u16,
    #[serde(default)]
    pub network: Option<String>,
    #[serde(default)]
    pub local_port: Option<u16>,
    #[serde(default)]
    pub local_host: Option<String>,
    #[serde(default)]
    pub socat_port: Option<u16>,
    #[serde(default)]
    pub socat_image: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteEnvProfileRequest {
    pub name: String,
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
    pub socat_image_ok: bool,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupResult {
    pub server: String,
    pub containers: Vec<String>,
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
    Ok(serde_json::from_str(&raw)?)
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

    let mut config = load_config().await?;
    let was_active = active_env_profiles_for_config(&config)
        .values()
        .any(|active_name| active_name == &profile_name);
    if let Some(existing) = config
        .env_profiles
        .iter_mut()
        .find(|item| item.name == profile_name)
    {
        *existing = profile;
    } else {
        config.env_profiles.push(profile);
    }
    config
        .env_profiles
        .sort_by(|left, right| left.name.cmp(&right.name));
    config.active_env_profiles = active_env_profiles_for_config(&config);
    config
        .active_env_profiles
        .retain(|_, active_name| active_name != &profile_name);
    if was_active {
        let saved = find_env_profile(&config, &profile_name)?;
        let target_key = env_profile_target_key(saved)?;
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

pub async fn delete_env_profile(name: String) -> Result<()> {
    let mut config = load_config().await?;
    config.env_profiles.retain(|profile| profile.name != name);
    config.active_env_profiles = active_env_profiles_for_config(&config);
    config
        .active_env_profiles
        .retain(|_, active_name| active_name != &name);
    config.active_env_profile = None;
    save_config(&config).await
}

pub async fn set_active_env_profile(name: String) -> Result<()> {
    let mut config = load_config().await?;
    let profile = find_env_profile(&config, &name)?;
    let target_key = env_profile_target_key(profile)?;
    config.active_env_profiles = active_env_profiles_for_config(&config);
    config.active_env_profiles.insert(target_key, name);
    config.active_env_profile = None;
    save_config(&config).await
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
        socat_image_ok: false,
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
            return Ok(result);
        }
    }

    let image = server
        .default_socat_image
        .as_ref()
        .unwrap_or(&config.defaults.socat_image);
    let image_cmd = format!(
        "{docker} image inspect {} >/dev/null 2>&1 || {docker} pull {} >/dev/null",
        shell_quote(image),
        shell_quote(image)
    );
    match run_ssh(&config.defaults, &server, &image_cmd).await {
        Ok(_) => {
            result.socat_image_ok = true;
            result
                .details
                .push(format!("socat image is available: {image}"));
        }
        Err(error) => {
            result
                .details
                .push(format!("socat image check failed for {image}: {error}"));
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

pub async fn open_tunnel(request: OpenTunnelRequest) -> Result<TunnelState> {
    validate_name("project", &request.project)?;
    validate_name("service", &request.service)?;
    if request.target_port == 0 {
        return Err(AppError::msg("target port is required"));
    }

    let config = load_config().await?;
    let server = find_server(&config, &request.server)?.clone();
    let services = list_compose_services(request.server.clone(), request.project.clone()).await?;
    let service = services
        .iter()
        .find(|item| item.service == request.service)
        .ok_or_else(|| {
            AppError::msg(format!(
                "service {} was not found in project {}",
                request.service, request.project
            ))
        })?;
    let network = match request.network.clone() {
        Some(network) => network,
        None => choose_network(&request.project, service)?,
    };

    let socat_port = request.socat_port.unwrap_or(request.target_port);
    let local_host = request
        .local_host
        .clone()
        .unwrap_or_else(|| config.defaults.local_host.clone());
    let local_port = match request.local_port {
        Some(port) => {
            ensure_local_port_available(&local_host, port)?;
            port
        }
        None => portpicker::pick_unused_port()
            .ok_or_else(|| AppError::msg("could not find an available local port"))?,
    };
    let image = request
        .socat_image
        .clone()
        .or_else(|| server.default_socat_image.clone())
        .unwrap_or_else(|| config.defaults.socat_image.clone());
    let container = tunnel_container_name(
        &server.name,
        &request.project,
        &request.service,
        request.target_port,
    );
    let existing_state = load_state().await?;
    let tunnel_id = tunnel_state_id(
        &existing_state,
        &request.server,
        &request.project,
        &request.service,
        request.target_port,
    );

    ensure_socat_container(
        &config.defaults,
        &server,
        &container,
        &network,
        &image,
        &request.service,
        request.target_port,
        socat_port,
    )
    .await?;
    let socat_container_ip =
        match inspect_container_ip(&config.defaults, &server, &container, &network).await {
            Ok(ip) => ip,
            Err(error) => {
                let _ = remove_socat_container(&config.defaults, &server, &container).await;
                return Err(error);
            }
        };
    let child = match spawn_ssh_forward(
        &config.defaults,
        &server,
        &local_host,
        local_port,
        &socat_container_ip,
        socat_port,
    )
    .await
    {
        Ok(child) => child,
        Err(error) => {
            let _ = remove_socat_container(&config.defaults, &server, &container).await;
            return Err(error);
        }
    };
    let ssh_pid = child.id();

    let state = TunnelState {
        id: tunnel_id,
        server: request.server.clone(),
        project: request.project.clone(),
        service: request.service.clone(),
        network,
        target_port: request.target_port,
        socat_port,
        local_host,
        local_port,
        socat_container: container.clone(),
        socat_container_ip,
        ssh_pid,
        status: TunnelStatus::Running,
        mode: TunnelMode::SocatDirect,
        started_at: Some(now_string()),
        last_error: None,
    };

    if let Err(error) = upsert_tunnel_state(state.clone()).await {
        if let Some(pid) = ssh_pid {
            let _ = kill_pid(pid).await;
        }
        let _ = remove_socat_container(&config.defaults, &server, &container).await;
        return Err(error);
    }
    Ok(state)
}

pub async fn close_tunnel(tunnel_id: String) -> Result<()> {
    let mut state = load_state().await?;
    let config = load_config().await?;
    let mut changed = false;

    for tunnel in &mut state.tunnels {
        if tunnel.id != tunnel_id {
            continue;
        }
        if let Some(pid) = tunnel.ssh_pid {
            kill_pid(pid).await?;
        }
        if let Ok(server) = find_server(&config, &tunnel.server) {
            let _ = remove_socat_container(&config.defaults, server, &tunnel.socat_container).await;
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

pub async fn cleanup(server_id: String) -> Result<CleanupResult> {
    let (config, server, containers) = cleanup_container_candidates(&server_id).await?;

    if !containers.is_empty() {
        let joined = containers
            .iter()
            .map(|container| shell_quote(container))
            .collect::<Vec<_>>()
            .join(" ");
        let command = format!("{} rm -f {joined}", docker_command(&server));
        run_ssh(&config.defaults, &server, &command).await?;
    }

    Ok(CleanupResult {
        server: server_id,
        containers,
    })
}

pub async fn preview_cleanup(server_id: String) -> Result<CleanupResult> {
    let (_, _, containers) = cleanup_container_candidates(&server_id).await?;

    Ok(CleanupResult {
        server: server_id,
        containers,
    })
}

async fn cleanup_container_candidates(
    server_id: &str,
) -> Result<(AppConfig, ServerConfig, Vec<String>)> {
    let config = load_config().await?;
    let server = find_server(&config, server_id)?.clone();
    let list_command = managed_container_list_command(&docker_command(&server));
    let output = run_ssh(&config.defaults, &server, &list_command).await?;
    let containers: Vec<String> = output
        .lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect();
    let state = load_refreshed_state().await?;
    let containers = cleanup_candidates(containers, &state, server_id);
    Ok((config, server, containers))
}

fn managed_container_list_command(docker: &str) -> String {
    format!("{docker} ps -a --filter label=compose-tunnel.managed=true --format '{{{{.Names}}}}'")
}

fn cleanup_candidates(containers: Vec<String>, state: &AppState, server_id: &str) -> Vec<String> {
    let active: BTreeSet<&str> = state
        .tunnels
        .iter()
        .filter(|tunnel| tunnel.server == server_id && tunnel.status == TunnelStatus::Running)
        .map(|tunnel| tunnel.socat_container.as_str())
        .collect();

    containers
        .into_iter()
        .filter(|container| !active.contains(container.as_str()))
        .collect()
}

pub async fn render_env_profile(name: String) -> Result<String> {
    let config = load_config().await?;
    let profile = find_env_profile(&config, &name)?;
    let state = load_state().await?;
    render_env_profile_inner(profile, &state)
}

pub async fn write_env_profile(request: WriteEnvProfileRequest) -> Result<PathBuf> {
    let config = load_config().await?;
    let profile = find_env_profile(&config, &request.name)?;
    let target_key = env_profile_target_key(profile)?;
    let active_profiles = active_env_profiles_for_config(&config);
    if active_profiles.get(&target_key) != Some(&request.name) {
        return Err(AppError::msg(format!(
            "env profile {} is not active for {}",
            request.name, target_key
        )));
    }
    let target_dir = env_profile_target_dir(profile)?;
    let env = render_env_profile(request.name.clone()).await?;
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

fn find_env_profile<'a>(config: &'a AppConfig, name: &str) -> Result<&'a EnvProfileConfig> {
    config
        .env_profiles
        .iter()
        .find(|profile| profile.name == name)
        .ok_or_else(|| AppError::msg(format!("env profile {name} was not found")))
}

fn active_env_profiles_for_config(config: &AppConfig) -> BTreeMap<String, String> {
    let mut active = BTreeMap::new();

    for active_name in config.active_env_profiles.values() {
        if let Some(profile) = config
            .env_profiles
            .iter()
            .find(|profile| &profile.name == active_name)
        {
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

fn env_profile_target_dir(profile: &EnvProfileConfig) -> Result<PathBuf> {
    let target_dir = profile
        .target_dir
        .as_ref()
        .ok_or_else(|| AppError::msg("target directory is required"))?;
    let target_dir = target_dir.to_string_lossy().trim().to_string();
    if target_dir.is_empty() {
        return Err(AppError::msg("target directory is required"));
    }
    let expanded = expand_home(&target_dir);
    if expanded.is_absolute() {
        Ok(expanded)
    } else {
        Ok(std::env::current_dir()?.join(expanded))
    }
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

async fn run_ssh(
    defaults: &Defaults,
    server: &ServerConfig,
    remote_command: &str,
) -> Result<String> {
    let mut args = ssh_base_args(server);
    args.push(remote_command.to_string());
    let output = Command::new(&defaults.ssh_binary)
        .args(args)
        .output()
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
    let mut args = ssh_base_args(server);
    args.insert(0, "-N".to_string());
    let forward = format!("{local_host}:{local_port}:{remote_host}:{remote_port}");
    let target_index = args
        .iter()
        .position(|value| value == &ssh_target(server))
        .ok_or_else(|| AppError::msg("could not build ssh forward command"))?;
    args.insert(target_index, "-L".to_string());
    args.insert(target_index + 1, forward);

    let child = Command::new(&defaults.ssh_binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(child)
}

async fn ensure_socat_container(
    defaults: &Defaults,
    server: &ServerConfig,
    container: &str,
    network: &str,
    image: &str,
    service: &str,
    target_port: u16,
    socat_port: u16,
) -> Result<()> {
    let docker = docker_command(server);
    let inspect_command = format!(
        "{docker} inspect {} >/dev/null 2>&1",
        shell_quote(container)
    );
    if run_ssh(defaults, server, &inspect_command).await.is_ok() {
        return Ok(());
    }

    let listen = format!("TCP-LISTEN:{socat_port},fork,reuseaddr");
    let target = format!("TCP:{service}:{target_port}");
    let command = format!(
        "{docker} run -d --rm --label compose-tunnel.managed=true --name {} --network {} {} {} {}",
        shell_quote(container),
        shell_quote(network),
        shell_quote(image),
        shell_quote(&listen),
        shell_quote(&target)
    );
    run_ssh(defaults, server, &command).await?;
    Ok(())
}

async fn inspect_container_ip(
    defaults: &Defaults,
    server: &ServerConfig,
    container: &str,
    network: &str,
) -> Result<String> {
    let docker = docker_command(server);
    let command = format!(
        "{docker} inspect -f {} {}",
        shell_quote(&format!(
            "{{{{ index .NetworkSettings.Networks \"{network}\" \"IPAddress\" }}}}"
        )),
        shell_quote(container)
    );
    let output = run_ssh(defaults, server, &command).await?;
    let ip = output.trim();
    if ip.is_empty() || ip == "<no value>" {
        return Err(AppError::msg(format!(
            "could not inspect container IP for {container} on network {network}"
        )));
    }
    Ok(ip.to_string())
}

async fn remove_socat_container(
    defaults: &Defaults,
    server: &ServerConfig,
    container: &str,
) -> Result<()> {
    let command = format!(
        "{} rm -f {} >/dev/null 2>&1 || true",
        docker_command(server),
        shell_quote(container)
    );
    run_ssh(defaults, server, &command).await.map(|_| ())
}

fn choose_network(project: &str, service: &ComposeService) -> Result<String> {
    let default_network = format!("{project}_default");
    if service
        .networks
        .iter()
        .any(|network| network == &default_network)
    {
        return Ok(default_network);
    }
    if service.networks.len() == 1 {
        return Ok(service.networks[0].clone());
    }
    Err(AppError::msg(format!(
        "service {} has multiple networks; pass --network",
        service.service
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

fn tunnel_container_name(server: &str, project: &str, service: &str, target_port: u16) -> String {
    format!(
        "compose-tunnel-{}-{}-{}-{}",
        sanitize_name(server),
        sanitize_name(project),
        sanitize_name(service),
        target_port
    )
}

fn tunnel_state_id(
    state: &AppState,
    server: &str,
    project: &str,
    service: &str,
    target_port: u16,
) -> String {
    let base = service.to_string();
    match state.tunnels.iter().find(|tunnel| tunnel.id == base) {
        None => base,
        Some(existing) if same_tunnel_target(existing, server, project, service, target_port) => {
            base
        }
        Some(_) => format!(
            "{}-{}-{}-{}",
            sanitize_name(server),
            sanitize_name(project),
            sanitize_name(service),
            target_port
        ),
    }
}

fn same_tunnel_target(
    tunnel: &TunnelState,
    server: &str,
    project: &str,
    service: &str,
    target_port: u16,
) -> bool {
    tunnel.server == server
        && tunnel.project == project
        && tunnel.service == service
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
    let mut lines = vec![format!("# compose-tunnel env: {}", profile.name)];
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
    let block = format!("{start}\n{}{end}\n", env.trim_end());
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

async fn load_refreshed_state() -> Result<AppState> {
    let mut state = load_state().await?;
    let changed = refresh_tunnel_process_statuses(&mut state, pid_is_running);
    if changed {
        save_state(&state).await?;
    }
    Ok(state)
}

fn refresh_tunnel_process_statuses<F>(state: &mut AppState, mut pid_running: F) -> bool
where
    F: FnMut(u32) -> bool,
{
    let mut changed = false;
    for tunnel in &mut state.tunnels {
        if tunnel.status != TunnelStatus::Running {
            continue;
        }

        match tunnel.ssh_pid {
            Some(pid) if pid_running(pid) => {}
            Some(pid) => {
                tunnel.status = TunnelStatus::Stopped;
                tunnel.ssh_pid = None;
                tunnel.last_error = Some(format!("ssh process {pid} is not running"));
                changed = true;
            }
            None => {
                tunnel.status = TunnelStatus::Error;
                tunnel.last_error = Some("running tunnel has no ssh process id".to_string());
                changed = true;
            }
        }
    }
    changed
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
    fn cleanup_lists_only_managed_labeled_containers() {
        let command = managed_container_list_command("sudo -n docker");

        assert!(command.contains("sudo -n docker ps -a"));
        assert!(command.contains("--filter label=compose-tunnel.managed=true"));
        assert!(!command.contains("--filter name="));
    }

    #[test]
    fn cleanup_skips_running_tunnel_containers_for_server() {
        let state = AppState {
            tunnels: vec![
                TunnelState {
                    id: "db".to_string(),
                    server: "staging".to_string(),
                    project: "app".to_string(),
                    service: "db".to_string(),
                    network: "app_default".to_string(),
                    target_port: 5432,
                    socat_port: 5432,
                    local_host: "127.0.0.1".to_string(),
                    local_port: 15432,
                    socat_container: "compose-tunnel-staging-app-db-5432".to_string(),
                    socat_container_ip: "172.18.0.20".to_string(),
                    ssh_pid: Some(1234),
                    status: TunnelStatus::Running,
                    mode: TunnelMode::SocatDirect,
                    started_at: None,
                    last_error: None,
                },
                TunnelState {
                    id: "redis".to_string(),
                    server: "staging".to_string(),
                    project: "app".to_string(),
                    service: "redis".to_string(),
                    network: "app_default".to_string(),
                    target_port: 6379,
                    socat_port: 6379,
                    local_host: "127.0.0.1".to_string(),
                    local_port: 16379,
                    socat_container: "compose-tunnel-staging-app-redis-6379".to_string(),
                    socat_container_ip: "172.18.0.21".to_string(),
                    ssh_pid: None,
                    status: TunnelStatus::Stopped,
                    mode: TunnelMode::SocatDirect,
                    started_at: None,
                    last_error: None,
                },
            ],
        };
        let containers = vec![
            "compose-tunnel-staging-app-db-5432".to_string(),
            "compose-tunnel-staging-app-redis-6379".to_string(),
            "compose-tunnel-orphan".to_string(),
        ];

        let candidates = cleanup_candidates(containers, &state, "staging");

        assert_eq!(
            candidates,
            vec![
                "compose-tunnel-staging-app-redis-6379".to_string(),
                "compose-tunnel-orphan".to_string()
            ]
        );
    }

    #[test]
    fn refresh_tunnel_status_marks_dead_ssh_process_as_stopped() {
        let mut state = AppState {
            tunnels: vec![TunnelState {
                id: "db".to_string(),
                server: "staging".to_string(),
                project: "app".to_string(),
                service: "db".to_string(),
                network: "app_default".to_string(),
                target_port: 5432,
                socat_port: 5432,
                local_host: "127.0.0.1".to_string(),
                local_port: 15432,
                socat_container: "compose-tunnel-staging-app-db-5432".to_string(),
                socat_container_ip: "172.18.0.20".to_string(),
                ssh_pid: Some(1234),
                status: TunnelStatus::Running,
                mode: TunnelMode::SocatDirect,
                started_at: None,
                last_error: None,
            }],
        };

        let changed = refresh_tunnel_process_statuses(&mut state, |_| false);

        assert!(changed);
        assert_eq!(state.tunnels[0].status, TunnelStatus::Stopped);
        assert_eq!(state.tunnels[0].ssh_pid, None);
        assert_eq!(
            state.tunnels[0].last_error.as_deref(),
            Some("ssh process 1234 is not running")
        );
    }

    #[test]
    fn refresh_tunnel_status_keeps_live_ssh_process_running() {
        let mut state = AppState {
            tunnels: vec![TunnelState {
                id: "db".to_string(),
                server: "staging".to_string(),
                project: "app".to_string(),
                service: "db".to_string(),
                network: "app_default".to_string(),
                target_port: 5432,
                socat_port: 5432,
                local_host: "127.0.0.1".to_string(),
                local_port: 15432,
                socat_container: "compose-tunnel-staging-app-db-5432".to_string(),
                socat_container_ip: "172.18.0.20".to_string(),
                ssh_pid: Some(1234),
                status: TunnelStatus::Running,
                mode: TunnelMode::SocatDirect,
                started_at: None,
                last_error: None,
            }],
        };

        let changed = refresh_tunnel_process_statuses(&mut state, |_| true);

        assert!(!changed);
        assert_eq!(state.tunnels[0].status, TunnelStatus::Running);
        assert_eq!(state.tunnels[0].ssh_pid, Some(1234));
        assert_eq!(state.tunnels[0].last_error, None);
    }

    #[test]
    fn refresh_tunnel_status_marks_running_without_pid_as_error() {
        let mut state = AppState {
            tunnels: vec![TunnelState {
                id: "db".to_string(),
                server: "staging".to_string(),
                project: "app".to_string(),
                service: "db".to_string(),
                network: "app_default".to_string(),
                target_port: 5432,
                socat_port: 5432,
                local_host: "127.0.0.1".to_string(),
                local_port: 15432,
                socat_container: "compose-tunnel-staging-app-db-5432".to_string(),
                socat_container_ip: "172.18.0.20".to_string(),
                ssh_pid: None,
                status: TunnelStatus::Running,
                mode: TunnelMode::SocatDirect,
                started_at: None,
                last_error: None,
            }],
        };

        let changed = refresh_tunnel_process_statuses(&mut state, |_| true);

        assert!(changed);
        assert_eq!(state.tunnels[0].status, TunnelStatus::Error);
        assert_eq!(
            state.tunnels[0].last_error.as_deref(),
            Some("running tunnel has no ssh process id")
        );
    }

    #[test]
    fn tunnel_id_scopes_when_service_id_already_belongs_to_another_target() {
        let state = AppState {
            tunnels: vec![TunnelState {
                id: "db".to_string(),
                server: "staging".to_string(),
                project: "app".to_string(),
                service: "db".to_string(),
                network: "app_default".to_string(),
                target_port: 5432,
                socat_port: 5432,
                local_host: "127.0.0.1".to_string(),
                local_port: 15432,
                socat_container: "compose-tunnel-staging-app-db-5432".to_string(),
                socat_container_ip: "172.18.0.20".to_string(),
                ssh_pid: Some(1234),
                status: TunnelStatus::Running,
                mode: TunnelMode::SocatDirect,
                started_at: None,
                last_error: None,
            }],
        };

        assert_eq!(tunnel_state_id(&state, "staging", "app", "db", 5432), "db");
        assert_eq!(
            tunnel_state_id(&state, "staging", "billing", "db", 5432),
            "staging-billing-db-5432"
        );
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
            tunnels: vec![TunnelState {
                id: "db".to_string(),
                server: "staging".to_string(),
                project: "app".to_string(),
                service: "db".to_string(),
                network: "app_default".to_string(),
                target_port: 5432,
                socat_port: 5432,
                local_host: "127.0.0.1".to_string(),
                local_port: 15432,
                socat_container: "compose-tunnel-staging-app-db-5432".to_string(),
                socat_container_ip: "172.18.0.20".to_string(),
                ssh_pid: None,
                status: TunnelStatus::Running,
                mode: TunnelMode::SocatDirect,
                started_at: None,
                last_error: None,
            }],
        };

        let rendered = render_env_profile_inner(&profile, &state).expect("profile should render");

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
            tunnels: vec![TunnelState {
                id: "db".to_string(),
                server: "staging".to_string(),
                project: "app".to_string(),
                service: "db".to_string(),
                network: "app_default".to_string(),
                target_port: 5432,
                socat_port: 5432,
                local_host: "127.0.0.1".to_string(),
                local_port: 15432,
                socat_container: "compose-tunnel-staging-app-db-5432".to_string(),
                socat_container_ip: "172.18.0.20".to_string(),
                ssh_pid: None,
                status: TunnelStatus::Running,
                mode: TunnelMode::SocatDirect,
                started_at: None,
                last_error: None,
            }],
        };

        let rendered = render_env_profile_inner(&profile, &state).expect("profile should render");

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
}
