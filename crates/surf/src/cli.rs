use anyhow::{Result, bail};
use clap::{ArgAction, Args, Parser, Subcommand};
use serde::Serialize;

use crate::browser::{BrowserConfig, default_config};
use crate::constants::{SURF_VERSION, USAGE_TEXT};
use crate::extension::{extension_doctor, extension_path, install_extension};
use crate::host::{
    default_host_profile_name, host_logs, host_status, start_host_browser, stop_host_browser,
};
use crate::runtime::{
    build_runtime, start_runtime, status_runtime, stop_runtime, stream_container_logs,
};
use crate::settings::{
    default_surf_settings, load_surf_settings_or_default, save_surf_settings, set_surf_config_value,
};
use crate::tunnel::{start_tunnel, stop_tunnel, tunnel_logs, tunnel_status};

#[derive(Debug, Parser)]
#[command(disable_help_flag = true, disable_version_flag = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Build(BuildArgs),
    Start(StartArgs),
    Stop(StopArgs),
    Status(StatusArgs),
    Logs(LogsArgs),
    Session,
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Proxy,
    Host {
        #[command(subcommand)]
        command: HostCommand,
    },
    Tunnel {
        #[command(subcommand)]
        command: TunnelCommand,
    },
    Extension {
        #[command(subcommand)]
        command: ExtensionCommand,
    },
    Version,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Show(ConfigShowArgs),
    Get(ConfigShowArgs),
    Set(ConfigSetArgs),
    Path,
    Init(ConfigInitArgs),
}

#[derive(Debug, Subcommand)]
enum HostCommand {
    Start(HostStartArgs),
    Stop(HostProfileArgs),
    Status(HostProfileArgs),
    Logs(HostLogsArgs),
}

#[derive(Debug, Subcommand)]
enum TunnelCommand {
    Start(TunnelStartArgs),
    Stop(TunnelNameArgs),
    Status(TunnelNameArgs),
    Logs(TunnelLogsArgs),
}

#[derive(Debug, Subcommand)]
enum ExtensionCommand {
    Install(ExtensionInstallArgs),
    Path(ExtensionPathArgs),
    Doctor(ExtensionDoctorArgs),
}

#[derive(Debug, Args)]
struct ConfigShowArgs {
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ConfigSetArgs {
    #[arg(long)]
    key: String,
    #[arg(long, default_value = "")]
    value: String,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ConfigInitArgs {
    #[arg(long)]
    force: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone, Default)]
struct RuntimeOverrides {
    #[arg(long)]
    image: Option<String>,
    #[arg(long = "name")]
    container_name: Option<String>,
    #[arg(long)]
    network: Option<String>,
    #[arg(long)]
    profile: Option<String>,
    #[arg(long = "profile-dir")]
    profile_dir: Option<String>,
    #[arg(long = "host-bind")]
    host_bind: Option<String>,
    #[arg(long = "host-mcp-port")]
    host_mcp_port: Option<i32>,
    #[arg(long = "host-novnc-port")]
    host_novnc_port: Option<i32>,
    #[arg(long = "mcp-port")]
    mcp_port: Option<i32>,
    #[arg(long = "novnc-port")]
    novnc_port: Option<i32>,
    #[arg(long = "vnc-password")]
    vnc_password: Option<String>,
    #[arg(long = "mcp-version")]
    mcp_version: Option<String>,
    #[arg(long = "browser")]
    browser_channel: Option<String>,
    #[arg(long = "allowed-hosts")]
    allowed_hosts: Option<String>,
}

#[derive(Debug, Args)]
struct BuildArgs {
    #[arg(long)]
    repo: Option<String>,
    #[arg(long = "context")]
    context_dir: Option<String>,
    #[arg(long)]
    dockerfile: Option<String>,
    #[arg(long)]
    image: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct StartArgs {
    #[arg(long)]
    repo: Option<String>,
    #[arg(long = "skip-build")]
    skip_build: bool,
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    runtime: RuntimeOverrides,
}

#[derive(Debug, Args)]
struct StopArgs {
    #[arg(long = "name")]
    container_name: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct StatusArgs {
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    runtime: RuntimeOverrides,
}

#[derive(Debug, Args)]
struct LogsArgs {
    #[arg(long = "name")]
    container_name: Option<String>,
    #[arg(long, default_value_t = 200)]
    tail: i32,
    #[arg(long, default_value_t = true, action = ArgAction::Set)]
    follow: bool,
}

#[derive(Debug, Args)]
struct HostStartArgs {
    #[arg(long)]
    profile: Option<String>,
    #[arg(long = "profile-dir")]
    profile_dir: Option<String>,
    #[arg(long = "browser-path")]
    browser_path: Option<String>,
    #[arg(long = "cdp-port")]
    cdp_port: Option<i32>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct HostProfileArgs {
    #[arg(long)]
    profile: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct HostLogsArgs {
    #[arg(long)]
    profile: Option<String>,
}

#[derive(Debug, Args)]
struct TunnelStartArgs {
    #[arg(long)]
    name: Option<String>,
    #[arg(long = "target-url")]
    target_url: Option<String>,
    #[arg(long)]
    mode: Option<String>,
    #[arg(long)]
    token: Option<String>,
    #[arg(long = "vault-key")]
    vault_key: Option<String>,
    #[arg(long)]
    image: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct TunnelNameArgs {
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct TunnelLogsArgs {
    #[arg(long)]
    name: Option<String>,
    #[arg(long, default_value_t = 200)]
    tail: i32,
    #[arg(long, default_value_t = true, action = ArgAction::Set)]
    follow: bool,
}

#[derive(Debug, Args)]
struct ExtensionInstallArgs {
    #[arg(long)]
    repo: Option<String>,
    #[arg(long)]
    dest: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ExtensionPathArgs {
    #[arg(long)]
    repo: Option<String>,
    #[arg(long)]
    source: bool,
}

#[derive(Debug, Args)]
struct ExtensionDoctorArgs {
    #[arg(long)]
    path: Option<String>,
    #[arg(long)]
    json: bool,
}

pub fn run(raw_args: &[String]) -> Result<i32> {
    if raw_args.is_empty() {
        print!("{USAGE_TEXT}");
        return Ok(1);
    }

    match raw_args[0].as_str() {
        "help" | "-h" | "--help" => {
            print!("{USAGE_TEXT}");
            return Ok(0);
        }
        "version" | "--version" | "-v" => {
            println!("{SURF_VERSION}");
            return Ok(0);
        }
        _ => {}
    }

    let cli = Cli::parse_from(std::iter::once("surf".to_owned()).chain(raw_args.iter().cloned()));
    let Some(command) = cli.command else {
        print!("{USAGE_TEXT}");
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
        Command::Session => not_yet("session"),
        Command::Proxy => not_yet("proxy"),
        Command::Host { command } => handle_host(command),
        Command::Tunnel { command } => handle_tunnel(command),
        Command::Extension { command } => handle_extension(command),
    }
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
    let result = start_runtime(
        &cfg,
        args.repo.as_deref(),
        args.skip_build,
        profile_dir_passed,
    )?;
    if args.json {
        print_json(&serde_json::json!({
            "ok": true,
            "command": "start",
            "config": result.config,
            "status": result.status,
            "mcp_url": result.mcp_url,
            "novnc_url": result.novnc_url,
            "container_name": result.container_name,
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
        println!("  mcp_url={}", result.mcp_url);
        println!("  novnc_url={}", result.novnc_url);
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

fn handle_host(command: HostCommand) -> Result<i32> {
    match command {
        HostCommand::Start(args) => {
            let profile = args.profile.unwrap_or_else(default_host_profile_name);
            let state = start_host_browser(
                &profile,
                args.profile_dir.as_deref(),
                args.browser_path.as_deref(),
                args.cdp_port,
            )?;
            if args.json {
                print_json(&serde_json::json!({
                    "ok": true,
                    "state": state,
                    "cdp_url": format!("http://127.0.0.1:{}", state.cdp_port),
                }))?;
            } else {
                println!("surf host start");
                println!("  profile={} pid={}", state.profile, state.pid);
                println!("  browser={}", state.browser_path);
                println!("  profile_dir={}", state.profile_dir);
                println!("  cdp_url=http://127.0.0.1:{}", state.cdp_port);
                println!("  log={}", state.log_file);
            }
            Ok(0)
        }
        HostCommand::Stop(args) => {
            let profile = args.profile.unwrap_or_else(default_host_profile_name);
            let state = stop_host_browser(&profile)?;
            if args.json {
                print_json(&serde_json::json!({
                    "ok": true,
                    "profile": profile,
                }))?;
            } else {
                println!("surf host stop: profile={} pid={}", profile, state.pid);
            }
            Ok(0)
        }
        HostCommand::Status(args) => {
            let profile = args.profile.unwrap_or_else(default_host_profile_name);
            let status = host_status(&profile)?;
            if args.json {
                print_json(&status)?;
            } else {
                println!("surf host status");
                println!(
                    "  profile={} pid={} alive={}",
                    status.profile, status.pid, status.alive
                );
                println!("  cdp_url={} status={}", status.cdp_url, status.cdp_status);
                println!("  profile_dir={}", status.state.profile_dir);
            }
            Ok(if status.ok { 0 } else { 1 })
        }
        HostCommand::Logs(args) => {
            let profile = args.profile.unwrap_or_else(default_host_profile_name);
            print!("{}", host_logs(&profile)?);
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
                &name,
                args.target_url.as_deref(),
                args.mode.as_deref(),
                args.token.as_deref(),
                args.vault_key.as_deref(),
                args.image.as_deref(),
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

fn handle_extension(command: ExtensionCommand) -> Result<i32> {
    match command {
        ExtensionCommand::Install(args) => {
            let path = install_extension(args.repo.as_deref(), args.dest.as_deref())?;
            if args.json {
                print_json(&serde_json::json!({
                    "ok": true,
                    "path": path,
                }))?;
            } else {
                println!("surf extension installed: {}", path.display());
                println!("Load unpacked in chrome://extensions");
            }
            Ok(0)
        }
        ExtensionCommand::Path(args) => {
            println!(
                "{}",
                extension_path(args.repo.as_deref(), args.source)?.display()
            );
            Ok(0)
        }
        ExtensionCommand::Doctor(args) => {
            let doctor = extension_doctor(args.path.as_deref())?;
            if args.json {
                print_json(&doctor)?;
            } else if !doctor.ok {
                bail!(
                    "extension not installed at {} (run `surf extension install`)",
                    doctor.path
                );
            } else {
                println!("surf extension doctor: ok");
                println!("  path={}", doctor.path);
                println!(
                    "  next=chrome://extensions -> Load unpacked -> {}",
                    doctor.path
                );
            }
            Ok(if doctor.ok { 0 } else { 1 })
        }
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn not_yet(command: &str) -> Result<i32> {
    bail!("{command} is not implemented in Rust yet")
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
    }
    let profile_dir_passed = overrides.profile_dir.is_some();
    if let Some(value) = overrides.profile_dir.as_ref() {
        cfg.profile_dir = value.trim().to_owned();
    }
    if let Some(value) = overrides.host_bind.as_ref() {
        cfg.host_bind = value.trim().to_owned();
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
        cfg.vnc_password = value.trim().to_owned();
    }
    if let Some(value) = overrides.mcp_version.as_ref() {
        cfg.mcp_version = value.trim().to_owned();
    }
    if let Some(value) = overrides.browser_channel.as_ref() {
        cfg.browser_channel = value.trim().to_owned();
    }
    if let Some(value) = overrides.allowed_hosts.as_ref() {
        cfg.allowed_hosts = value.trim().to_owned();
    }
    profile_dir_passed
}
