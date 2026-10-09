use anyhow::Result;
use clap::{ArgAction, Args, CommandFactory, Parser, Subcommand};
use serde::Serialize;

use crate::browser::{
    BrowserConfig, agent_host_port, agent_mcp_url, agent_profile_dir,
    apply_container_profile_default, default_config, generate_secure_secret, resolve_vnc_password,
};
use crate::constants::SURF_VERSION;
use crate::proxy::{ProxyConfig, run_proxy};
use crate::runtime::{
    build_runtime, container_identity, start_runtime, status_runtime, stop_runtime,
    stream_container_logs, switch_agent_workspace,
};
use crate::settings::{
    default_surf_settings, load_surf_settings_or_default, save_surf_settings, set_surf_config_value,
};
use crate::tunnel::{TunnelStartOptions, start_tunnel, stop_tunnel, tunnel_logs, tunnel_status};

#[derive(Debug, Parser)]
#[command(
    name = "surf",
    version = SURF_VERSION,
    about = "Playwright MCP server: one shared headed Chromium per profile, driven remotely over MCP with a noVNC viewer"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Build the browser runtime image
    Build(BuildArgs),
    /// Start the browser runtime container
    Start(StartArgs),
    /// Stop and remove the runtime container
    Stop(StopArgs),
    /// Check runtime health (container, MCP, noVNC)
    Status(StatusArgs),
    /// Stream runtime container logs
    Logs(LogsArgs),
    /// Manage the surf settings file
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Start the authenticated MCP gateway proxy
    Proxy(ProxyArgs),
    /// Manage browser agents (contexts) inside the runtime container
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    /// Manage cloudflare tunnels for public access
    Tunnel {
        #[command(subcommand)]
        command: TunnelCommand,
    },
    /// Print the surf version
    Version,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Print settings (TOML by default)
    Show(ConfigShowArgs),
    /// Alias for show
    Get(ConfigShowArgs),
    /// Set one settings key (e.g. tunnel.mode)
    Set(ConfigSetArgs),
    /// Print the settings file path
    Path,
    /// Write default settings (overwrites with --force)
    Init(ConfigInitArgs),
}

#[derive(Debug, Subcommand)]
enum AgentCommand {
    /// List agents with slots, workspaces, ports, and MCP URLs
    List(AgentListArgs),
    /// Switch the noVNC viewer to an agent's workspace
    View(AgentViewArgs),
}

#[derive(Debug, Subcommand)]
enum TunnelCommand {
    /// Start a tunnel container (quick or token mode)
    Start(TunnelStartArgs),
    /// Stop and remove a tunnel container
    Stop(TunnelNameArgs),
    /// Show tunnel status and public URL
    Status(TunnelNameArgs),
    /// Stream tunnel container logs
    Logs(TunnelLogsArgs),
}

#[derive(Debug, Args)]
struct ConfigShowArgs {
    #[arg(long, help = "Print as JSON")]
    json: bool,
}

#[derive(Debug, Args)]
struct ConfigSetArgs {
    #[arg(long, help = "Settings key, e.g. tunnel.mode or proxy.auth_token")]
    key: String,
    #[arg(long, default_value = "", help = "Value to store (empty clears)")]
    value: String,
    #[arg(long, help = "Print as JSON")]
    json: bool,
}

#[derive(Debug, Args)]
struct ConfigInitArgs {
    #[arg(long, help = "Overwrite an existing settings file")]
    force: bool,
    #[arg(long, help = "Print as JSON")]
    json: bool,
}

#[derive(Debug, Args, Clone, Default)]
struct RuntimeOverrides {
    #[arg(long, help = "Browser image tag")]
    image: Option<String>,
    #[arg(long = "name", help = "Container name")]
    container_name: Option<String>,
    #[arg(long, help = "Docker network")]
    network: Option<String>,
    #[arg(long, help = "Profile name (isolated browser state)")]
    profile: Option<String>,
    #[arg(long, help = "Comma-separated agent names (one browser context each)")]
    agents: Option<String>,
    #[arg(
        long = "profile-dir",
        help = "Host directory for the container profile"
    )]
    profile_dir: Option<String>,
    #[arg(long = "host-mcp-bind", help = "Host bind for raw MCP (keep loopback)")]
    host_mcp_bind: Option<String>,
    #[arg(long = "host-novnc-bind", help = "Host bind for the noVNC viewer")]
    host_novnc_bind: Option<String>,
    #[arg(long = "host-mcp-port", help = "Host port for raw MCP")]
    host_mcp_port: Option<i32>,
    #[arg(long = "host-novnc-port", help = "Host port for the noVNC viewer")]
    host_novnc_port: Option<i32>,
    #[arg(long = "mcp-port", help = "Container port for MCP")]
    mcp_port: Option<i32>,
    #[arg(long = "novnc-port", help = "Container port for noVNC")]
    novnc_port: Option<i32>,
    #[arg(long = "vnc-password", help = "Viewer password (generated when empty)")]
    vnc_password: Option<String>,
    #[arg(
        long = "browser",
        help = "Browser channel (chromium, chrome, firefox, webkit, msedge)"
    )]
    browser_channel: Option<String>,
    #[arg(long = "allowed-hosts", help = "MCP allowed-hosts value")]
    allowed_hosts: Option<String>,
}

#[derive(Debug, Args)]
struct BuildArgs {
    #[arg(long, help = "Surf repo root (auto-detected)")]
    repo: Option<String>,
    #[arg(long = "context", help = "Docker build context dir")]
    context_dir: Option<String>,
    #[arg(long, help = "Dockerfile path")]
    dockerfile: Option<String>,
    #[arg(long, help = "Image tag to build")]
    image: Option<String>,
    #[arg(long, help = "Print as JSON")]
    json: bool,
}

#[derive(Debug, Args)]
struct StartArgs {
    #[arg(long, help = "Print as JSON")]
    json: bool,
    #[command(flatten)]
    runtime: RuntimeOverrides,
}

#[derive(Debug, Args)]
struct StopArgs {
    #[arg(long = "name", help = "Container name")]
    container_name: Option<String>,
    #[arg(long, help = "Print as JSON")]
    json: bool,
}

#[derive(Debug, Args)]
struct StatusArgs {
    #[arg(long, help = "Print as JSON")]
    json: bool,
    #[command(flatten)]
    runtime: RuntimeOverrides,
}

#[derive(Debug, Args)]
struct LogsArgs {
    #[arg(long = "name", help = "Container name")]
    container_name: Option<String>,
    #[arg(long, default_value_t = 200, help = "Lines of history to show")]
    tail: i32,
    #[arg(long, default_value_t = true, action = ArgAction::Set, help = "Follow log output")]
    follow: bool,
}

#[derive(Debug, Args)]
struct TunnelStartArgs {
    #[arg(long, help = "Tunnel container name")]
    name: Option<String>,
    #[arg(
        long = "target-url",
        help = "Origin URL (quick mode; defaults to the viewer)"
    )]
    target_url: Option<String>,
    #[arg(long, help = "Tunnel mode: quick or token")]
    mode: Option<String>,
    #[arg(long, help = "Tunnel token (token mode)")]
    token: Option<String>,
    #[arg(
        long = "fort-key",
        alias = "vault-key",
        help = "Fort key holding the tunnel token"
    )]
    fort_key: Option<String>,
    #[arg(long = "fort-env-file", help = "safe env file selecting fort repo/env")]
    fort_env_file: Option<String>,
    #[arg(long, help = "cloudflared image")]
    image: Option<String>,
    #[arg(long, help = "Print as JSON")]
    json: bool,
}

#[derive(Debug, Args)]
struct TunnelNameArgs {
    #[arg(long, help = "Tunnel container name")]
    name: Option<String>,
    #[arg(long, help = "Print as JSON")]
    json: bool,
}

#[derive(Debug, Args)]
struct TunnelLogsArgs {
    #[arg(long, help = "Tunnel container name")]
    name: Option<String>,
    #[arg(long, default_value_t = 200, help = "Lines of history to show")]
    tail: i32,
    #[arg(long, default_value_t = true, action = ArgAction::Set, help = "Follow log output")]
    follow: bool,
}

#[derive(Debug, Args)]
struct AgentListArgs {
    #[arg(long = "name", help = "Container name")]
    container_name: Option<String>,
    #[arg(long, help = "Print as JSON")]
    json: bool,
}

#[derive(Debug, Args)]
struct AgentViewArgs {
    #[arg(help = "Agent name")]
    name: String,
    #[arg(long = "name", help = "Container name")]
    container_name: Option<String>,
}

#[derive(Debug, Args)]
struct ProxyArgs {
    #[arg(long, default_value = "0.0.0.0", help = "Interface to listen on")]
    bind: String,
    #[arg(long, default_value_t = 8931, help = "Port to listen on")]
    port: u16,
    #[arg(
        long,
        default_value = "http://127.0.0.1:8932",
        help = "Upstream raw MCP base URL"
    )]
    upstream: String,
    #[arg(
        long,
        help = "Auth token (defaults to settings, generated on first run)"
    )]
    token: Option<String>,
    #[arg(
        long,
        help = "Comma-separated agents to serve (must match the running container)"
    )]
    agents: Option<String>,
}

pub fn run(raw_args: &[String]) -> Result<i32> {
    if raw_args.is_empty() {
        print!("{}", Cli::command().render_help());
        return Ok(1);
    }

    match raw_args[0].as_str() {
        "help" | "-h" | "--help" => {
            return print_help_for(&raw_args[1..]);
        }
        "version" | "--version" | "-v" | "-V" => {
            println!("{SURF_VERSION}");
            return Ok(0);
        }
        _ => {}
    }

    let cli = Cli::parse_from(std::iter::once("surf".to_owned()).chain(raw_args.iter().cloned()));
    let Some(command) = cli.command else {
        print!("{}", Cli::command().render_help());
        return Ok(1);
    };

    match command {
        Command::Version => {
            println!("{SURF_VERSION}");
            Ok(0)
        }
        Command::Config { command } => handle_config(command),
        Command::Build(args) => handle_build(args),
        Command::Start(args) => handle_start(args),
        Command::Stop(args) => handle_stop(args),
        Command::Status(args) => handle_status(args),
        Command::Logs(args) => handle_logs(args),
        Command::Proxy(args) => handle_proxy(args),
        Command::Agent { command } => handle_agent(command),
        Command::Tunnel { command } => handle_tunnel(command),
    }
}

fn print_help_for(path: &[String]) -> Result<i32> {
    let mut command = Cli::command();
    for segment in path {
        let Some(next) = command
            .get_subcommands()
            .find(|sub| sub.get_name() == segment)
            .cloned()
        else {
            print!("{}", Cli::command().render_help());
            return Ok(0);
        };
        command = next;
    }
    print!("{}", command.render_long_help());
    Ok(0)
}

fn handle_config(command: ConfigCommand) -> Result<i32> {
    match command {
        ConfigCommand::Show(args) | ConfigCommand::Get(args) => {
            let settings = load_surf_settings_or_default();
            if args.json {
                print_json(&settings)?;
            } else {
                print!("{}", toml::to_string_pretty(&settings)?);
            }
            Ok(0)
        }
        ConfigCommand::Set(args) => {
            let mut settings = load_surf_settings_or_default();
            set_surf_config_value(&mut settings, &args.key, &args.value)?;
            save_surf_settings(&settings)?;
            if args.json {
                print_json(&serde_json::json!({
                    "ok": true,
                    "key": args.key.trim(),
                    "settings_file": crate::paths::surf_settings_path(),
                }))?;
            } else {
                println!("surf config set: {}", args.key.trim());
            }
            Ok(0)
        }
        ConfigCommand::Path => {
            println!("{}", crate::paths::surf_settings_path().display());
            Ok(0)
        }
        ConfigCommand::Init(args) => {
            let path = crate::paths::surf_settings_path();
            let created = if !args.force && path.exists() {
                false
            } else {
                let mut settings = default_surf_settings();
                crate::settings::apply_surf_settings_defaults(&mut settings);
                save_surf_settings(&settings)?;
                true
            };
            if args.json {
                print_json(&serde_json::json!({
                    "ok": true,
                    "settings_file": path,
                    "created": created,
                }))?;
            } else if created {
                println!("surf config init: wrote {}", path.display());
            } else {
                println!("surf config init: already exists at {}", path.display());
            }
            Ok(0)
        }
    }
}

fn handle_build(args: BuildArgs) -> Result<i32> {
    let image = args.image.unwrap_or_else(|| default_config().image_name);
    let result = build_runtime(
        &image,
        args.repo.as_deref(),
        args.context_dir.as_deref(),
        args.dockerfile.as_deref(),
    )?;
    if args.json {
        print_json(&serde_json::json!({
            "ok": true,
            "command": "build",
            "image": result.image,
            "dockerfile": result.dockerfile,
            "context": result.context,
        }))?;
    } else {
        println!("surf build: image built {}", result.image);
    }
    Ok(0)
}

fn handle_start(args: StartArgs) -> Result<i32> {
    let mut cfg = default_config();
    let profile_dir_passed = apply_runtime_overrides(&mut cfg, &args.runtime);
    let result = start_runtime(&cfg, profile_dir_passed)?;
    // Persist a generated viewer password so `config show` stays truthful
    // across restarts instead of silently rotating the viewer credential.
    if result.viewer_password_generated
        && let Some(password) = result.viewer_password.as_ref()
    {
        let mut settings = load_surf_settings_or_default();
        settings.browser.vnc_password = password.clone();
        save_surf_settings(&settings)?;
    }
    if args.json {
        print_json(&serde_json::json!({
            "ok": true,
            "command": "start",
            "config": result.config,
            "status": result.status,
            "mcp_url": result.mcp_url,
            "novnc_url": result.novnc_url,
            "container_name": result.container_name,
            "viewer_password": result.viewer_password,
            "viewer_password_generated": result.viewer_password_generated,
            "warnings": result.warnings,
        }))?;
    } else {
        println!("surf start");
        println!(
            "  container={} image={} network={}",
            result.config.container_name, result.config.image_name, result.config.network
        );
        println!(
            "  profile={} profile_dir={}",
            result.config.profile_name, result.config.profile_dir
        );
        println!("  agents={}", result.config.agents.join(","));
        println!("  mcp_url={}", result.mcp_url);
        println!("  novnc_url={}", result.novnc_url);
        if let Some(password) = result.viewer_password.as_ref() {
            println!("  viewer_password={}", password);
        }
        for warning in &result.warnings {
            println!("  warning={}", warning);
        }
    }
    Ok(0)
}

fn handle_stop(args: StopArgs) -> Result<i32> {
    let cfg = default_config();
    let container_name = args.container_name.unwrap_or(cfg.container_name);
    stop_runtime(&container_name)?;
    if args.json {
        print_json(&serde_json::json!({
            "ok": true,
            "command": "stop",
            "container_name": container_name,
        }))?;
    } else {
        println!("surf stop: removed {container_name}");
    }
    Ok(0)
}

fn handle_status(args: StatusArgs) -> Result<i32> {
    let mut cfg = default_config();
    let profile_dir_passed = apply_runtime_overrides(&mut cfg, &args.runtime);
    // Prefer the running container's actual profile/agents over settings.
    if let Some((profile, agents)) = container_identity(&cfg.container_name) {
        if args.runtime.profile.is_none() && !profile.is_empty() {
            cfg.profile_name = profile;
            if !profile_dir_passed {
                cfg.profile_dir.clear();
                apply_container_profile_default(false, &mut cfg);
            }
        }
        if args.runtime.agents.is_none() {
            cfg.agents = agents;
        }
    }
    let status = status_runtime(&cfg, profile_dir_passed)?;
    if args.json {
        print_json(&serde_json::json!({
            "ok": status.ok,
            "command": "status",
            "status": status,
        }))?;
    } else {
        println!("surf status");
        println!(
            "  container={} running={}",
            status.container_name, status.container_running
        );
        println!(
            "  profile={} profile_dir={}",
            cfg.profile_name, cfg.profile_dir
        );
        if !status.container_status_line.trim().is_empty() {
            println!("  docker_ps={}", status.container_status_line);
        }
        println!("  mcp_url={}", status.mcp_url);
        println!("  novnc_url={}", status.novnc_url);
        println!(
            "  mcp_ready={} host_code={} container_code={}",
            status.mcp_ready, status.mcp_host_code, status.mcp_container_code
        );
        for agent in &status.agents {
            println!(
                "  agent={} slot={} ready={} mcp_url={}",
                agent.name, agent.slot, agent.ready, agent.mcp_url
            );
        }
        println!(
            "  novnc_ready={} host_code={} container_code={}",
            status.novnc_ready, status.novnc_host_code, status.novnc_container_code
        );
        if !status.error.trim().is_empty() {
            println!("  error={}", status.error);
        }
    }
    Ok(if status.ok { 0 } else { 1 })
}

fn handle_logs(args: LogsArgs) -> Result<i32> {
    let cfg = default_config();
    let container_name = args.container_name.unwrap_or(cfg.container_name);
    stream_container_logs(&container_name, args.tail, args.follow)?;
    Ok(0)
}

fn handle_proxy(args: ProxyArgs) -> Result<i32> {
    let mut cfg = default_config();
    if let Some(agents) = args.agents.as_deref() {
        let names: Vec<String> = agents
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect();
        cfg.agents = crate::browser::normalize_agents(&names);
    } else if let Some((_, agents)) = container_identity(&cfg.container_name) {
        // Serve the running container's agents when no explicit list is given.
        cfg.agents = agents;
    }
    let token = args
        .token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| crate::paths::env_trimmed("SURF_PROXY_TOKEN"))
        .or_else(|| {
            let token = load_surf_settings_or_default().proxy.auth_token;
            (!token.trim().is_empty()).then(|| token.trim().to_owned())
        });
    let mut settings = load_surf_settings_or_default();
    let mut dirty = false;
    let token = match token {
        Some(token) => token,
        None => {
            // First run: generate a stable token and persist it so agents can
            // be configured once. Printed only here; afterwards read it back
            // with `surf config show`.
            let token = generate_secure_secret(48);
            settings.proxy.auth_token = token.clone();
            println!("generated proxy auth token: {token}");
            println!("configure MCP clients with this token (it is not shown again)");
            dirty = true;
            token
        }
    };
    for agent in &cfg.agents {
        let missing = settings
            .proxy
            .tokens
            .get(agent)
            .map(|t| t.trim().is_empty())
            .unwrap_or(true);
        if missing {
            let agent_token = generate_secure_secret(48);
            println!("generated proxy token for agent '{agent}': {agent_token}");
            settings.proxy.tokens.insert(agent.clone(), agent_token);
            dirty = true;
        }
    }
    if dirty {
        save_surf_settings(&settings)?;
    }
    run_proxy(&ProxyConfig {
        bind: args.bind,
        port: args.port,
        upstream: args.upstream,
        main_token: token,
        agents: cfg.agents,
        tokens: settings.proxy.tokens,
    })?;
    Ok(0)
}
fn handle_agent(command: AgentCommand) -> Result<i32> {
    match command {
        AgentCommand::List(args) => {
            let mut cfg = default_config();
            if let Some(name) = args.container_name.as_deref() {
                cfg.container_name = name.trim().to_owned();
            }
            // Prefer the running container's actual agents over settings.
            if let Some((profile, agents)) = container_identity(&cfg.container_name) {
                if !profile.is_empty() {
                    cfg.profile_name = profile;
                }
                cfg.agents = agents;
                // The container's profile may differ from settings; re-resolve.
                cfg.profile_dir.clear();
                apply_container_profile_default(false, &mut cfg);
            }
            let settings = load_surf_settings_or_default();
            let rows: Vec<serde_json::Value> = cfg
                .agents
                .iter()
                .enumerate()
                .map(|(slot, agent)| {
                    serde_json::json!({
                        "name": agent,
                        "slot": slot,
                        "workspace": format!("{}/{}", cfg.profile_name, agent),
                        "host_port": agent_host_port(&cfg, slot),
                        "mcp_url": agent_mcp_url(&cfg, slot),
                        "proxy_path": format!("/agent/{agent}/mcp"),
                        "profile_dir": agent_profile_dir(&cfg, agent),
                        "token_configured": settings
                            .proxy
                            .tokens
                            .get(agent)
                            .is_some_and(|t| !t.trim().is_empty()),
                    })
                })
                .collect();
            if args.json {
                print_json(&serde_json::json!({
                    "ok": true,
                    "command": "agent list",
                    "profile": cfg.profile_name,
                    "agents": rows,
                }))?;
            } else {
                println!("surf agent list (profile={})", cfg.profile_name);
                for row in &rows {
                    println!(
                        "  slot={} name={} workspace={} port={} token={}",
                        row["slot"],
                        row["name"].as_str().unwrap_or(""),
                        row["workspace"].as_str().unwrap_or(""),
                        row["host_port"],
                        if row["token_configured"].as_bool().unwrap_or(false) {
                            "yes"
                        } else {
                            "no"
                        },
                    );
                    println!("    mcp_url={}", row["mcp_url"].as_str().unwrap_or(""));
                    println!(
                        "    proxy_path={} profile_dir={}",
                        row["proxy_path"].as_str().unwrap_or(""),
                        row["profile_dir"].as_str().unwrap_or("")
                    );
                }
            }
            Ok(0)
        }
        AgentCommand::View(args) => {
            let mut cfg = default_config();
            if let Some(name) = args.container_name.as_deref() {
                cfg.container_name = name.trim().to_owned();
            }
            if let Some((profile, agents)) = container_identity(&cfg.container_name) {
                if !profile.is_empty() {
                    cfg.profile_name = profile;
                }
                cfg.agents = agents;
            }
            let wanted = crate::paths::sanitize_profile_name(&args.name);
            let Some(slot) = cfg.agents.iter().position(|a| a == &wanted) else {
                anyhow::bail!(
                    "unknown agent '{wanted}' (profile {}: {})",
                    cfg.profile_name,
                    cfg.agents.join(", ")
                );
            };
            switch_agent_workspace(&cfg.container_name, slot)?;
            println!(
                "surf agent view: workspace {slot} ({}/{wanted})",
                cfg.profile_name
            );
            Ok(0)
        }
    }
}

fn handle_tunnel(command: TunnelCommand) -> Result<i32> {
    match command {
        TunnelCommand::Start(args) => {
            let cfg = default_config();
            let settings = load_surf_settings_or_default();
            let name = args
                .name
                .or_else(|| crate::paths::env_trimmed("SURF_TUNNEL_NAME"))
                .unwrap_or(settings.tunnel.container_name);
            let payload = start_tunnel(
                &cfg,
                &TunnelStartOptions {
                    name: &name,
                    target_url: args.target_url.as_deref(),
                    mode: args.mode.as_deref(),
                    token: args.token.as_deref(),
                    fort_key: args.fort_key.as_deref(),
                    fort_env_file: args.fort_env_file.as_deref(),
                    image: args.image.as_deref(),
                },
            )?;
            if args.json {
                print_json(&payload)?;
            } else {
                println!("surf tunnel start");
                println!(
                    "  container={} mode={}",
                    payload.container_name, payload.mode
                );
                if let Some(target) = args.target_url.as_deref() {
                    println!("  target={}", target.trim());
                }
                if !payload.url.trim().is_empty() {
                    println!("  public_url={}", payload.url);
                } else {
                    println!("  public_url=pending (run `surf tunnel status`)");
                }
            }
            Ok(0)
        }
        TunnelCommand::Stop(args) => {
            let settings = load_surf_settings_or_default();
            let name = args.name.unwrap_or(settings.tunnel.container_name);
            stop_tunnel(&name)?;
            if args.json {
                print_json(&serde_json::json!({
                    "ok": true,
                    "container_name": name,
                }))?;
            } else {
                println!("surf tunnel stop: removed {name}");
            }
            Ok(0)
        }
        TunnelCommand::Status(args) => {
            let settings = load_surf_settings_or_default();
            let name = args.name.unwrap_or(settings.tunnel.container_name);
            let payload = tunnel_status(&name)?;
            if args.json {
                print_json(&payload)?;
            } else {
                println!("surf tunnel status");
                println!(
                    "  container={} running={}",
                    payload.container_name, payload.running
                );
                if !payload.url.trim().is_empty() {
                    println!("  public_url={}", payload.url);
                }
            }
            Ok(if payload.ok { 0 } else { 1 })
        }
        TunnelCommand::Logs(args) => {
            let settings = load_surf_settings_or_default();
            let name = args.name.unwrap_or(settings.tunnel.container_name);
            tunnel_logs(&name, args.tail, args.follow)?;
            Ok(0)
        }
    }
}
fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn apply_runtime_overrides(cfg: &mut BrowserConfig, overrides: &RuntimeOverrides) -> bool {
    if let Some(value) = overrides.image.as_ref() {
        cfg.image_name = value.trim().to_owned();
    }
    if let Some(value) = overrides.container_name.as_ref() {
        cfg.container_name = value.trim().to_owned();
    }
    if let Some(value) = overrides.network.as_ref() {
        cfg.network = value.trim().to_owned();
    }
    if let Some(value) = overrides.profile.as_ref() {
        cfg.profile_name = value.trim().to_owned();
        // A new profile name invalidates the resolved dir unless an explicit
        // dir was also passed; clearing lets the default re-resolve.
        if overrides.profile_dir.is_none() {
            cfg.profile_dir.clear();
        }
    }
    if let Some(value) = overrides.agents.as_ref() {
        let names: Vec<String> = value
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect();
        cfg.agents = crate::browser::normalize_agents(&names);
    }
    let profile_dir_passed = overrides.profile_dir.is_some();
    if let Some(value) = overrides.profile_dir.as_ref() {
        cfg.profile_dir = value.trim().to_owned();
    }
    if let Some(value) = overrides.host_mcp_bind.as_ref() {
        cfg.host_mcp_bind = value.trim().to_owned();
    }
    if let Some(value) = overrides.host_novnc_bind.as_ref() {
        cfg.host_novnc_bind = value.trim().to_owned();
    }
    if let Some(value) = overrides.host_mcp_port {
        cfg.host_mcp_port = value;
    }
    if let Some(value) = overrides.host_novnc_port {
        cfg.host_novnc_port = value;
    }
    if let Some(value) = overrides.mcp_port {
        cfg.mcp_port = value;
    }
    if let Some(value) = overrides.novnc_port {
        cfg.novnc_port = value;
    }
    if let Some(value) = overrides.vnc_password.as_ref() {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            let (password, generated) = resolve_vnc_password(None, "");
            cfg.vnc_password = password;
            cfg.vnc_password_generated = generated;
        } else {
            cfg.vnc_password = trimmed.to_owned();
            cfg.vnc_password_generated = false;
        }
    }
    if let Some(value) = overrides.browser_channel.as_ref() {
        cfg.browser_channel = value.trim().to_owned();
    }
    if let Some(value) = overrides.allowed_hosts.as_ref() {
        cfg.allowed_hosts = value.trim().to_owned();
    }
    profile_dir_passed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::env_lock;
    use crate::paths::set_env;

    #[test]
    fn run_with_no_args_shows_usage_code() {
        let result = run(&[]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1);
    }

    #[test]
    fn run_help_and_version_flags() {
        assert_eq!(run(&["help".to_owned()]).expect("help"), 0);
        assert_eq!(run(&["-h".to_owned()]).expect("short help"), 0);
        assert_eq!(run(&["--help".to_owned()]).expect("long help"), 0);
        assert_eq!(run(&["version".to_owned()]).expect("version"), 0);
        assert_eq!(run(&["-v".to_owned()]).expect("short version"), 0);
        assert_eq!(run(&["--version".to_owned()]).expect("long version"), 0);
    }

    #[test]
    fn run_version_config_paths_do_not_fail() {
        let lock = env_lock().lock().unwrap_or_else(|error| error.into_inner());
        let previous_settings_file = std::env::var_os("SURF_SETTINGS_FILE");
        set_env(
            "SURF_SETTINGS_FILE",
            Some("/tmp/surf-config-test-settings.toml"),
        );
        assert_eq!(
            run(&["config".to_owned(), "show".to_owned()]).expect("config show"),
            0
        );
        assert_eq!(
            run(&["config".to_owned(), "path".to_owned()]).expect("config path"),
            0
        );
        set_env(
            "SURF_SETTINGS_FILE",
            previous_settings_file
                .as_ref()
                .and_then(|value| value.to_str())
                .map(|value| value.trim()),
        );
        drop(lock);
    }

    #[test]
    fn profile_override_without_dir_resets_resolved_dir() {
        let mut cfg = default_config();
        cfg.profile_dir = "/tmp/stale".to_owned();
        let overrides = RuntimeOverrides {
            profile: Some("fresh".to_owned()),
            ..Default::default()
        };
        assert!(!apply_runtime_overrides(&mut cfg, &overrides));
        assert_eq!(cfg.profile_name, "fresh");
        assert!(cfg.profile_dir.is_empty());
    }

    #[test]
    fn apply_runtime_overrides_updates_fields() {
        let mut cfg = default_config();
        let overrides = RuntimeOverrides {
            image: Some("my-image".to_owned()),
            container_name: Some("my-container".to_owned()),
            network: Some("bridge".to_owned()),
            profile: Some("my-profile".to_owned()),
            agents: Some("alice,Bob".to_owned()),
            profile_dir: Some("/tmp/profile".to_owned()),
            host_mcp_bind: Some("127.0.0.1".to_owned()),
            host_novnc_bind: Some("0.0.0.0".to_owned()),
            host_mcp_port: Some(1),
            host_novnc_port: Some(2),
            mcp_port: Some(3),
            novnc_port: Some(4),
            vnc_password: Some("pass".to_owned()),
            browser_channel: Some("stable".to_owned()),
            allowed_hosts: Some("localhost".to_owned()),
        };
        let profile_dir_passed = apply_runtime_overrides(&mut cfg, &overrides);
        assert!(profile_dir_passed);
        assert_eq!(cfg.image_name, "my-image");
        assert_eq!(cfg.container_name, "my-container");
        assert_eq!(cfg.network, "bridge");
        assert_eq!(cfg.profile_name, "my-profile");
        assert_eq!(cfg.agents, vec!["alice".to_owned(), "bob".to_owned()]);
        assert_eq!(cfg.profile_dir, "/tmp/profile");
        assert_eq!(cfg.host_mcp_bind, "127.0.0.1");
        assert_eq!(cfg.host_novnc_bind, "0.0.0.0");
        assert_eq!(cfg.host_mcp_port, 1);
        assert_eq!(cfg.host_novnc_port, 2);
        assert_eq!(cfg.mcp_port, 3);
        assert_eq!(cfg.novnc_port, 4);
        assert_eq!(cfg.vnc_password, "pass");
        assert_eq!(cfg.browser_channel, "stable");
        assert_eq!(cfg.allowed_hosts, "localhost");
    }
}
