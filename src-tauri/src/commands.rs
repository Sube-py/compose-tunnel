use compose_tunnel_core::{
    cleanup as core_cleanup, close_tunnel as core_close_tunnel,
    delete_server as core_delete_server, init_config as core_init_config,
    list_compose_projects as core_list_compose_projects,
    list_compose_services as core_list_compose_services, list_servers as core_list_servers,
    list_tunnels as core_list_tunnels, load_config, open_tunnel as core_open_tunnel,
    render_env as core_render_env, save_defaults as core_save_defaults,
    save_server as core_save_server, test_server as core_test_server,
    write_env_file as core_write_env_file, AppConfig, AppError, AppPaths, CleanupResult,
    ComposeProject, ComposeService, Defaults, OpenTunnelRequest, ServerConfig, ServerTestResult,
    TunnelState, WriteEnvFileRequest,
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
pub async fn list_servers() -> CommandResult<Vec<ServerConfig>> {
    core_list_servers().await.map_err(map_error)
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
pub async fn list_tunnels() -> CommandResult<Vec<TunnelState>> {
    core_list_tunnels().await.map_err(map_error)
}

#[tauri::command]
pub async fn render_env(tunnel_id: String) -> CommandResult<String> {
    core_render_env(tunnel_id).await.map_err(map_error)
}

#[tauri::command]
pub async fn write_env_file(request: WriteEnvFileRequest) -> CommandResult<()> {
    core_write_env_file(request).await.map_err(map_error)
}

#[tauri::command]
pub async fn cleanup(server_id: String) -> CommandResult<CleanupResult> {
    core_cleanup(server_id).await.map_err(map_error)
}
