mod commands;

use tauri_plugin_log::{Target, TargetKind};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .clear_targets()
                .clear_format()
                .level(log::LevelFilter::Info)
                .target(
                    Target::new(TargetKind::Webview)
                        .filter(|metadata| metadata.target() == "compose_tunnel::operation"),
                )
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::init_config,
            commands::get_config,
            commands::save_defaults,
            commands::list_env_profiles,
            commands::active_env_profile,
            commands::active_env_profiles,
            commands::save_env_profile,
            commands::delete_env_profile,
            commands::set_active_env_profile,
            commands::clear_active_env_profile,
            commands::list_servers,
            commands::list_ssh_config_hosts,
            commands::save_server,
            commands::delete_server,
            commands::test_server,
            commands::list_compose_projects,
            commands::list_compose_services,
            commands::open_tunnel,
            commands::close_tunnel,
            commands::close_all_tunnels,
            commands::list_tunnels,
            commands::read_operation_logs,
            commands::render_env_profile,
            commands::write_env_profile
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
