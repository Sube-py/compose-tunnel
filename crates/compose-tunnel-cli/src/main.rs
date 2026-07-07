use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use compose_tunnel_core::{
    active_env_profiles, cleanup, close_all_tunnels, close_tunnel, delete_server, init_config,
    list_compose_projects, list_compose_services, list_env_profiles, list_servers, list_tunnels,
    open_tunnel, render_env, render_env_profile, save_server, set_active_env_profile, test_server,
    write_env_file, write_env_profile, OpenTunnelRequest, ServerConfig, WriteEnvFileRequest,
    WriteEnvProfileRequest,
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
    Cleanup {
        #[arg(long)]
        server: String,
    },
    Status,
    Env(EnvArgs),
}

#[derive(Debug, Subcommand)]
enum ServerCommand {
    Add(ServerAddArgs),
    List,
    Test { name: String },
    Delete { name: String },
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
    Show { name: String },
    Use { name: String },
    Write { name: String },
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
                close_all_tunnels().await?;
                println!("All tunnels stopped");
            } else if let Some(tunnel_id) = args.tunnel_id {
                close_tunnel(tunnel_id.clone()).await?;
                println!("Tunnel {tunnel_id} stopped");
            } else {
                anyhow::bail!("pass a tunnel id or --all");
            }
        }
        Command::Cleanup { server } => {
            let result = cleanup(server).await?;
            if result.containers.is_empty() {
                println!("No compose-tunnel containers found on {}", result.server);
            } else {
                println!("Removed containers on {}:", result.server);
                for container in result.containers {
                    println!("  {container}");
                }
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
        EnvProfileCommand::Show { name } => {
            print!("{}", render_env_profile(name).await?);
        }
        EnvProfileCommand::Use { name } => {
            set_active_env_profile(name.clone()).await?;
            println!("Activated env profile {name}");
        }
        EnvProfileCommand::Write { name } => {
            set_active_env_profile(name.clone()).await?;
            let path = write_env_profile(WriteEnvProfileRequest { name: name.clone() }).await?;
            println!("Wrote env profile {name} to {}", path.display());
        }
    }
    Ok(())
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
        ServerCommand::Delete { name } => {
            delete_server(name.clone()).await?;
            println!("Deleted server {name}");
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
