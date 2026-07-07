use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
};

use clap::{Args, Parser, Subcommand};
use compose_tunnel_core::{
    active_env_profiles, cleanup, close_all_tunnels, close_tunnel, delete_env_profile,
    delete_server, init_config, list_compose_projects, list_compose_services, list_env_profiles,
    list_servers, list_tunnels, open_tunnel, preview_cleanup, render_env, render_env_profile,
    save_env_profile, save_server, set_active_env_profile, test_server, write_env_file,
    write_env_profile, EnvPlainEntry, EnvProfileConfig, EnvTunnelPort, OpenTunnelRequest,
    ServerConfig, WriteEnvFileRequest, WriteEnvProfileRequest,
};

#[derive(Debug, Parser)]
#[command(name = "compose-tunnel")]
#[command(about = "Forward remote Docker Compose internal services through SSH")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Init,
    Server {
        #[command(subcommand)]
        command: ServerCommand,
    },
    Compose {
        #[command(subcommand)]
        command: ComposeCommand,
    },
    Open(OpenArgs),
    Close(CloseArgs),
    Cleanup(CleanupArgs),
    Status,
    Env(EnvArgs),
}

#[derive(Debug, Subcommand)]
enum ServerCommand {
    Add(ServerAddArgs),
    List,
    Test { name: String },
    Delete(ServerDeleteArgs),
}

#[derive(Debug, Args)]
struct ServerAddArgs {
    name: String,
    #[arg(long)]
    host: String,
    #[arg(long)]
    user: String,
    #[arg(long, default_value_t = 22)]
    port: u16,
    #[arg(long)]
    identity_file: Option<String>,
    #[arg(long)]
    ssh_alias: Option<String>,
    #[arg(long)]
    socat_image: Option<String>,
    #[arg(long, default_value = "docker")]
    docker_command: String,
}

#[derive(Debug, Args)]
struct ServerDeleteArgs {
    name: String,
    #[arg(long)]
    yes: bool,
}

#[derive(Debug, Subcommand)]
enum ComposeCommand {
    List {
        #[arg(long)]
        server: String,
    },
    Services {
        #[arg(long)]
        server: String,
        #[arg(long)]
        project: String,
    },
}

#[derive(Debug, Args)]
struct OpenArgs {
    #[arg(long)]
    server: String,
    #[arg(long)]
    project: String,
    #[arg(long)]
    service: String,
    #[arg(long)]
    target_port: u16,
    #[arg(long)]
    network: Option<String>,
    #[arg(long)]
    local_port: Option<u16>,
    #[arg(long)]
    local_host: Option<String>,
    #[arg(long)]
    socat_port: Option<u16>,
    #[arg(long)]
    socat_image: Option<String>,
    #[arg(long)]
    env_prefix: Option<String>,
}

#[derive(Debug, Args)]
struct CloseArgs {
    tunnel_id: Option<String>,
    #[arg(long)]
    all: bool,
    #[arg(long)]
    yes: bool,
}

#[derive(Debug, Args)]
struct CleanupArgs {
    #[arg(long)]
    server: String,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    yes: bool,
}

#[derive(Debug, Args)]
struct EnvArgs {
    #[command(subcommand)]
    command: Option<EnvCommand>,
    tunnel_id: Option<String>,
    #[arg(long)]
    write: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum EnvCommand {
    Profile {
        #[command(subcommand)]
        command: EnvProfileCommand,
    },
}

#[derive(Debug, Subcommand)]
enum EnvProfileCommand {
    List,
    Save(EnvProfileSaveArgs),
    Show { name: String },
    Use { name: String },
    Write(EnvProfileWriteArgs),
    Delete { name: String },
}

#[derive(Debug, Args)]
struct EnvProfileSaveArgs {
    name: String,
    #[arg(long)]
    target_dir: PathBuf,
    #[arg(long = "tunnel-port", value_name = "TUNNEL_ID:ALIAS[:ENV_KEY]")]
    tunnel_ports: Vec<String>,
    #[arg(long = "env", value_name = "KEY=VALUE")]
    env: Vec<String>,
}

#[derive(Debug, Args)]
struct EnvProfileWriteArgs {
    name: String,
    #[arg(long)]
    yes: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match cli.command {
        Command::Init => {
            let paths = init_config().await?;
            println!(
                "Initialized compose-tunnel config at {}",
                paths.config_dir.display()
            );
        }
        Command::Server { command } => handle_server(command).await?,
        Command::Compose { command } => handle_compose(command).await?,
        Command::Open(args) => {
            let tunnel = open_tunnel(OpenTunnelRequest {
                server: args.server,
                project: args.project,
                service: args.service,
                target_port: args.target_port,
                network: args.network,
                local_port: args.local_port,
                local_host: args.local_host,
                socat_port: args.socat_port,
                socat_image: args.socat_image,
                env_prefix: args.env_prefix,
            })
            .await?;
            println!("Tunnel started\n");
            println!(
                "Service:          {}/{}:{}",
                tunnel.project, tunnel.service, tunnel.target_port
            );
            println!(
                "Local:            {}:{}",
                tunnel.local_host, tunnel.local_port
            );
            println!(
                "Forward target:   {}:{}",
                tunnel.socat_container_ip, tunnel.socat_port
            );
            println!("Mode:             socat-direct");
            println!("Env:\n{}", render_env(tunnel.id).await?.trim_end());
        }
        Command::Close(args) => {
            if args.all {
                if !args.yes
                    && !confirm_dangerous_action(
                        "close --all requires --yes when stdin is not interactive",
                        "Stop all tunnels and remove their remote socat containers? [y/N] ",
                    )?
                {
                    println!("Close all cancelled");
                    return Ok(());
                }
                close_all_tunnels().await?;
                println!("All tunnels stopped");
            } else if let Some(tunnel_id) = args.tunnel_id {
                close_tunnel(tunnel_id.clone()).await?;
                println!("Tunnel {tunnel_id} stopped");
            } else {
                anyhow::bail!("pass a tunnel id or --all");
            }
        }
        Command::Cleanup(args) => {
            if args.dry_run {
                let result = preview_cleanup(args.server).await?;
                if result.containers.is_empty() {
                    println!("No compose-tunnel containers found on {}", result.server);
                    return Ok(());
                }
                println!("Containers that would be removed on {}:", result.server);
                for container in result.containers {
                    println!("  {container}");
                }
                return Ok(());
            }

            let preview = preview_cleanup(args.server.clone()).await?;
            if preview.containers.is_empty() {
                println!("No compose-tunnel containers found on {}", preview.server);
                return Ok(());
            }
            println!("Containers to remove on {}:", preview.server);
            for container in &preview.containers {
                println!("  {container}");
            }
            if !args.yes
                && !confirm_dangerous_action(
                    "cleanup requires --yes when stdin is not interactive",
                    "Remove these remote containers? [y/N] ",
                )?
            {
                println!("Cleanup cancelled");
                return Ok(());
            }

            let result = cleanup(args.server).await?;
            println!("Removed containers on {}:", result.server);
            for container in result.containers {
                println!("  {container}");
            }
        }
        Command::Status => {
            println!("ID\tSERVER\tPROJECT\tSERVICE\tLOCAL\tMODE\tSTATUS");
            for tunnel in list_tunnels().await? {
                println!(
                    "{}\t{}\t{}\t{}\t{}:{}\tsocat-direct\t{:?}",
                    tunnel.id,
                    tunnel.server,
                    tunnel.project,
                    tunnel.service,
                    tunnel.local_host,
                    tunnel.local_port,
                    tunnel.status
                );
            }
        }
        Command::Env(args) => handle_env(args).await?,
    }

    Ok(())
}

async fn handle_env(args: EnvArgs) -> anyhow::Result<()> {
    match args.command {
        Some(EnvCommand::Profile { command }) => handle_env_profile(command).await,
        None => {
            let Some(tunnel_id) = args.tunnel_id else {
                anyhow::bail!("pass a tunnel id or an env subcommand");
            };
            let env = render_env(tunnel_id.clone()).await?;
            if let Some(path) = args.write {
                write_env_file(WriteEnvFileRequest {
                    tunnel_id,
                    path: path.clone(),
                })
                .await?;
                println!("Wrote compose-tunnel env block to {}", path.display());
            } else {
                print!("{env}");
            }
            Ok(())
        }
    }
}

async fn handle_env_profile(command: EnvProfileCommand) -> anyhow::Result<()> {
    match command {
        EnvProfileCommand::List => {
            let active = active_env_profiles().await?;
            println!("NAME\tTARGET DIR\tACTIVE\tTUNNEL PORTS\tEXTRA ENV");
            for profile in list_env_profiles().await? {
                let target_dir = profile
                    .target_dir
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default();
                let active_text = if active.get(&target_dir) == Some(&profile.name) {
                    "yes"
                } else {
                    ""
                };
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    profile.name,
                    target_dir,
                    active_text,
                    profile.tunnel_ports.len(),
                    profile.extra_env.len()
                );
            }
        }
        EnvProfileCommand::Save(args) => {
            let profile = EnvProfileConfig {
                name: args.name.clone(),
                target_dir: Some(args.target_dir),
                tunnel_ports: args
                    .tunnel_ports
                    .iter()
                    .map(|value| parse_env_tunnel_port(value))
                    .collect::<anyhow::Result<Vec<_>>>()?,
                extra_env: args
                    .env
                    .iter()
                    .map(|value| parse_plain_env(value))
                    .collect::<anyhow::Result<Vec<_>>>()?,
            };
            save_env_profile(profile).await?;
            println!("Saved env profile {}", args.name);
        }
        EnvProfileCommand::Show { name } => {
            print!("{}", render_env_profile(name).await?);
        }
        EnvProfileCommand::Use { name } => {
            set_active_env_profile(name.clone()).await?;
            println!("Activated env profile {name}");
        }
        EnvProfileCommand::Write(args) => {
            let env = render_env_profile(args.name.clone()).await?;
            let sensitive_keys = sensitive_env_keys(&env);
            if !sensitive_keys.is_empty()
                && !args.yes
                && !confirm_sensitive_env_write(&sensitive_keys)?
            {
                println!("Env profile write cancelled");
                return Ok(());
            }
            set_active_env_profile(args.name.clone()).await?;
            let path = write_env_profile(WriteEnvProfileRequest {
                name: args.name.clone(),
            })
            .await?;
            println!("Wrote env profile {} to {}", args.name, path.display());
        }
        EnvProfileCommand::Delete { name } => {
            delete_env_profile(name.clone()).await?;
            println!("Deleted env profile {name}");
        }
    }
    Ok(())
}

fn parse_env_tunnel_port(value: &str) -> anyhow::Result<EnvTunnelPort> {
    let parts: Vec<&str> = value.splitn(3, ':').collect();
    if parts.len() < 2 || parts[0].trim().is_empty() || parts[1].trim().is_empty() {
        anyhow::bail!("tunnel port must be TUNNEL_ID:ALIAS[:ENV_KEY]");
    }
    Ok(EnvTunnelPort {
        tunnel_id: parts[0].trim().to_string(),
        alias: parts[1].trim().to_string(),
        env_key: parts
            .get(2)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
    })
}

fn parse_plain_env(value: &str) -> anyhow::Result<EnvPlainEntry> {
    let Some((key, entry_value)) = value.split_once('=') else {
        anyhow::bail!("env must be KEY=VALUE");
    };
    let key = key.trim();
    if key.is_empty() {
        anyhow::bail!("env key is required");
    }
    Ok(EnvPlainEntry {
        key: key.to_string(),
        value: entry_value.to_string(),
    })
}

fn confirm_dangerous_action(non_interactive_message: &str, prompt: &str) -> anyhow::Result<bool> {
    if !io::stdin().is_terminal() {
        anyhow::bail!(non_interactive_message.to_string());
    }

    print!("{prompt}");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn confirm_sensitive_env_write(keys: &[String]) -> anyhow::Result<bool> {
    if !io::stdin().is_terminal() {
        anyhow::bail!("env profile write requires --yes when sensitive keys are present and stdin is not interactive");
    }

    println!(
        "Env profile contains sensitive key(s): {}",
        format_sensitive_keys(keys)
    );
    print!("Write these values to .env? [y/N] ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn sensitive_env_keys(env: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for line in env.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, _)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let lowered = key.to_ascii_lowercase();
        let is_sensitive = lowered.contains("password")
            || lowered.contains("token")
            || lowered.contains("secret")
            || lowered.contains("private_key")
            || lowered.contains("private-key");
        if is_sensitive && !keys.iter().any(|existing| existing == key) {
            keys.push(key.to_string());
        }
    }
    keys
}

fn format_sensitive_keys(keys: &[String]) -> String {
    let visible = keys
        .iter()
        .take(5)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let hidden_count = keys.len().saturating_sub(5);
    if hidden_count > 0 {
        format!("{visible}, and {hidden_count} more")
    } else {
        visible
    }
}

async fn handle_server(command: ServerCommand) -> anyhow::Result<()> {
    match command {
        ServerCommand::Add(args) => {
            save_server(ServerConfig {
                name: args.name.clone(),
                host: args.host,
                port: args.port,
                user: args.user,
                identity_file: args.identity_file,
                ssh_alias: args.ssh_alias,
                default_socat_image: args.socat_image,
                docker_command: args.docker_command,
            })
            .await?;
            println!("Saved server {}", args.name);
        }
        ServerCommand::List => {
            println!("NAME\tHOST\tPORT\tUSER\tDOCKER\tSSH ALIAS");
            for server in list_servers().await? {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    server.name,
                    server.host,
                    server.port,
                    server.user,
                    server.docker_command,
                    server.ssh_alias.unwrap_or_default()
                );
            }
        }
        ServerCommand::Test { name } => {
            let result = test_server(name).await?;
            println!("SSH:          {}", ok_text(result.ssh_ok));
            println!("Docker:       {}", ok_text(result.docker_ok));
            println!("socat image:  {}", ok_text(result.socat_image_ok));
            for detail in result.details {
                println!("  {detail}");
            }
        }
        ServerCommand::Delete(args) => {
            if !args.yes
                && !confirm_dangerous_action(
                    "server delete requires --yes when stdin is not interactive",
                    "Delete this server config? Existing tunnels and env profiles may still reference it. [y/N] ",
                )?
            {
                println!("Server delete cancelled");
                return Ok(());
            }
            delete_server(args.name.clone()).await?;
            println!("Deleted server {}", args.name);
        }
    }
    Ok(())
}

async fn handle_compose(command: ComposeCommand) -> anyhow::Result<()> {
    match command {
        ComposeCommand::List { server } => {
            println!("SERVER\tPROJECT\tSERVICES");
            for project in list_compose_projects(server).await? {
                println!(
                    "{}\t{}\t{}",
                    project.server,
                    project.project,
                    project.services.join(", ")
                );
            }
        }
        ComposeCommand::Services { server, project } => {
            println!("SERVICE\tCONTAINER\tSTATUS\tPORTS\tNETWORKS\tIMAGE");
            for service in list_compose_services(server, project).await? {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    service.service,
                    service.container,
                    service.status,
                    service.ports.join(", "),
                    service.networks.join(", "),
                    service.image
                );
            }
        }
    }
    Ok(())
}

fn ok_text(value: bool) -> &'static str {
    if value {
        "ok"
    } else {
        "failed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tunnel_port_binding_with_env_key() {
        let binding =
            parse_env_tunnel_port("db:staging_db:DATABASE_PORT").expect("binding should parse");

        assert_eq!(binding.tunnel_id, "db");
        assert_eq!(binding.alias, "staging_db");
        assert_eq!(binding.env_key.as_deref(), Some("DATABASE_PORT"));
    }

    #[test]
    fn parses_tunnel_port_binding_without_env_key() {
        let binding = parse_env_tunnel_port("redis:staging_redis").expect("binding should parse");

        assert_eq!(binding.tunnel_id, "redis");
        assert_eq!(binding.alias, "staging_redis");
        assert_eq!(binding.env_key, None);
    }

    #[test]
    fn rejects_invalid_tunnel_port_binding() {
        assert!(parse_env_tunnel_port("db").is_err());
        assert!(parse_env_tunnel_port(":alias").is_err());
        assert!(parse_env_tunnel_port("db:").is_err());
    }

    #[test]
    fn parses_plain_env_and_keeps_equals_in_value() {
        let entry = parse_plain_env("DATABASE_URL=postgres://u:p@localhost/db?sslmode=disable")
            .expect("env should parse");

        assert_eq!(entry.key, "DATABASE_URL");
        assert_eq!(entry.value, "postgres://u:p@localhost/db?sslmode=disable");
    }

    #[test]
    fn rejects_plain_env_without_key_or_separator() {
        assert!(parse_plain_env("DATABASE_URL").is_err());
        assert!(parse_plain_env("=value").is_err());
    }

    #[test]
    fn parses_cleanup_dry_run() {
        let cli = Cli::try_parse_from([
            "compose-tunnel",
            "cleanup",
            "--server",
            "staging",
            "--dry-run",
        ])
        .expect("cleanup dry run should parse");

        match cli.command {
            Command::Cleanup(args) => {
                assert_eq!(args.server, "staging");
                assert!(args.dry_run);
            }
            _ => panic!("expected cleanup command"),
        }
    }

    #[test]
    fn parses_close_all_yes() {
        let cli = Cli::try_parse_from(["compose-tunnel", "close", "--all", "--yes"])
            .expect("close all yes should parse");

        match cli.command {
            Command::Close(args) => {
                assert!(args.all);
                assert!(args.yes);
            }
            _ => panic!("expected close command"),
        }
    }

    #[test]
    fn parses_server_delete_yes() {
        let cli = Cli::try_parse_from(["compose-tunnel", "server", "delete", "staging", "--yes"])
            .expect("server delete yes should parse");

        match cli.command {
            Command::Server {
                command: ServerCommand::Delete(args),
            } => {
                assert_eq!(args.name, "staging");
                assert!(args.yes);
            }
            _ => panic!("expected server delete command"),
        }
    }

    #[test]
    fn parses_cleanup_yes() {
        let cli =
            Cli::try_parse_from(["compose-tunnel", "cleanup", "--server", "staging", "--yes"])
                .expect("cleanup yes should parse");

        match cli.command {
            Command::Cleanup(args) => {
                assert_eq!(args.server, "staging");
                assert!(args.yes);
                assert!(!args.dry_run);
            }
            _ => panic!("expected cleanup command"),
        }
    }

    #[test]
    fn parses_env_profile_write_yes() {
        let cli = Cli::try_parse_from([
            "compose-tunnel",
            "env",
            "profile",
            "write",
            "staging-db",
            "--yes",
        ])
        .expect("env profile write yes should parse");

        match cli.command {
            Command::Env(args) => match args.command {
                Some(EnvCommand::Profile {
                    command: EnvProfileCommand::Write(write_args),
                }) => {
                    assert_eq!(write_args.name, "staging-db");
                    assert!(write_args.yes);
                }
                _ => panic!("expected env profile write command"),
            },
            _ => panic!("expected env command"),
        }
    }

    #[test]
    fn sensitive_env_keys_are_detected_and_deduplicated() {
        let keys = sensitive_env_keys(
            "DATABASE_PASSWORD=secret\nDATABASE_PASSWORD=secret\nAPI_TOKEN=token\nSAFE=value\n",
        );

        assert_eq!(
            keys,
            vec!["DATABASE_PASSWORD".to_string(), "API_TOKEN".to_string()]
        );
    }

    #[test]
    fn sensitive_env_key_list_is_compacted() {
        let keys = vec![
            "A_PASSWORD".to_string(),
            "B_TOKEN".to_string(),
            "C_SECRET".to_string(),
            "D_PRIVATE_KEY".to_string(),
            "E_PASSWORD".to_string(),
            "F_TOKEN".to_string(),
        ];

        assert_eq!(
            format_sensitive_keys(&keys),
            "A_PASSWORD, B_TOKEN, C_SECRET, D_PRIVATE_KEY, E_PASSWORD, and 1 more"
        );
    }
}
