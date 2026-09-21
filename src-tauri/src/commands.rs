use std::collections::BTreeMap;

use compose_tunnel_core::{
    active_env_profile as core_active_env_profile, active_env_profiles as core_active_env_profiles,
    clear_active_env_profile as core_clear_active_env_profile,
    close_all_tunnels as core_close_all_tunnels, close_tunnel as core_close_tunnel,
    delete_env_profile as core_delete_env_profile, delete_server as core_delete_server,
    init_config as core_init_config, list_compose_projects as core_list_compose_projects,
    list_compose_services as core_list_compose_services,
    list_env_profiles as core_list_env_profiles, list_servers as core_list_servers,
    list_ssh_config_hosts as core_list_ssh_config_hosts, list_tunnels as core_list_tunnels,
    load_config, open_tunnel as core_open_tunnel, read_command_detail as core_read_command_detail,
    read_command_logs as core_read_command_logs, read_operation_logs as core_read_operation_logs,
    render_env_profile as core_render_env_profile, save_defaults as core_save_defaults,
    save_env_profile as core_save_env_profile, save_server as core_save_server,
    set_active_env_profile as core_set_active_env_profile, test_server as core_test_server,
    write_env_profile as core_write_env_profile, AppConfig, AppError, AppPaths, CommandLogDetail,
    CommandLogEntry, ComposeProject, ComposeService, Defaults, EnvProfileConfig, OpenTunnelRequest,
    OperationLogEntry, ServerConfig, ServerTestResult, SshConfigHost, TunnelState,
    WriteEnvProfileRequest,
};

type CommandResult<T> = std::result::Result<T, String>;

fn map_error(error: AppError) -> String {
    error.to_string()
}

#[tauri::command]
pub async fn init_config() -> CommandResult<AppPaths> {
    core_init_config().await.map_err(map_error)
}

#[tauri::command]
pub async fn get_config() -> CommandResult<AppConfig> {
    load_config().await.map_err(map_error)
}

#[tauri::command]
pub async fn save_defaults(defaults: Defaults) -> CommandResult<()> {
    core_save_defaults(defaults).await.map_err(map_error)
}

#[tauri::command]
pub async fn list_env_profiles() -> CommandResult<Vec<EnvProfileConfig>> {
    core_list_env_profiles().await.map_err(map_error)
}

#[tauri::command]
pub async fn active_env_profile() -> CommandResult<Option<String>> {
    core_active_env_profile().await.map_err(map_error)
}

#[tauri::command]
pub async fn active_env_profiles() -> CommandResult<BTreeMap<String, String>> {
    core_active_env_profiles().await.map_err(map_error)
}

#[tauri::command]
pub async fn save_env_profile(profile: EnvProfileConfig) -> CommandResult<()> {
    core_save_env_profile(profile).await.map_err(map_error)
}

#[tauri::command]
pub async fn delete_env_profile(name: String, target_dir: Option<String>) -> CommandResult<()> {
    core_delete_env_profile(name, target_dir)
        .await
        .map_err(map_error)
}

#[tauri::command]
pub async fn set_active_env_profile(name: String, target_dir: Option<String>) -> CommandResult<()> {
    core_set_active_env_profile(name, target_dir)
        .await
        .map_err(map_error)
}

#[tauri::command]
pub async fn clear_active_env_profile(target_dir: String) -> CommandResult<String> {
    core_clear_active_env_profile(target_dir)
        .await
        .map(|path| path.display().to_string())
        .map_err(map_error)
}

#[tauri::command]
pub async fn list_servers() -> CommandResult<Vec<ServerConfig>> {
    core_list_servers().await.map_err(map_error)
}

#[tauri::command]
pub async fn list_ssh_config_hosts() -> CommandResult<Vec<SshConfigHost>> {
    core_list_ssh_config_hosts().await.map_err(map_error)
}

#[tauri::command]
pub async fn save_server(server: ServerConfig) -> CommandResult<()> {
    core_save_server(server).await.map_err(map_error)
}

#[tauri::command]
pub async fn delete_server(name: String) -> CommandResult<()> {
    core_delete_server(name).await.map_err(map_error)
}

#[tauri::command]
pub async fn test_server(server_id: String) -> CommandResult<ServerTestResult> {
    core_test_server(server_id).await.map_err(map_error)
}

#[tauri::command]
pub async fn list_compose_projects(server_id: String) -> CommandResult<Vec<ComposeProject>> {
    core_list_compose_projects(server_id)
        .await
        .map_err(map_error)
}

#[tauri::command]
pub async fn list_compose_services(
    server_id: String,
    project: String,
) -> CommandResult<Vec<ComposeService>> {
    core_list_compose_services(server_id, project)
        .await
        .map_err(map_error)
}

#[tauri::command]
pub async fn open_tunnel(request: OpenTunnelRequest) -> CommandResult<TunnelState> {
    core_open_tunnel(request).await.map_err(map_error)
}

#[tauri::command]
pub async fn close_tunnel(tunnel_id: String) -> CommandResult<()> {
    core_close_tunnel(tunnel_id).await.map_err(map_error)
}

#[tauri::command]
pub async fn close_all_tunnels() -> CommandResult<()> {
    core_close_all_tunnels().await.map_err(map_error)
}

#[tauri::command]
pub async fn list_tunnels() -> CommandResult<Vec<TunnelState>> {
    core_list_tunnels().await.map_err(map_error)
}

#[tauri::command]
pub async fn read_operation_logs(limit: usize) -> CommandResult<Vec<OperationLogEntry>> {
    tauri::async_runtime::spawn_blocking(move || core_read_operation_logs(limit))
        .await
        .map_err(|_| "operation log reader failed".to_string())?
        .map_err(map_error)
}

#[tauri::command]
pub async fn read_command_logs(limit: usize) -> CommandResult<Vec<CommandLogEntry>> {
    tauri::async_runtime::spawn_blocking(move || core_read_command_logs(limit))
        .await
        .map_err(|_| "command log reader failed".to_string())?
        .map_err(map_error)
}

#[tauri::command]
pub async fn read_command_detail(id: String) -> CommandResult<CommandLogDetail> {
    tauri::async_runtime::spawn_blocking(move || core_read_command_detail(&id))
        .await
        .map_err(|_| "command detail reader failed".to_string())?
        .map_err(map_error)
}

#[tauri::command]
pub async fn render_env_profile(name: String, target_dir: Option<String>) -> CommandResult<String> {
    core_render_env_profile(name, target_dir)
        .await
        .map_err(map_error)
}

#[tauri::command]
pub async fn write_env_profile(request: WriteEnvProfileRequest) -> CommandResult<String> {
    core_write_env_profile(request)
        .await
        .map(|path| path.display().to_string())
        .map_err(map_error)
}
