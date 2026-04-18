use anyhow::{Result, bail};
use clap::{ArgAction, Args, Parser, Subcommand};
use serde::Serialize;

use crate::browser::{BrowserConfig, default_config, resolve_vnc_password};
use crate::constants::{SURF_VERSION, USAGE_TEXT};
use crate::extension::{extension_doctor, extension_path, install_extension};
use crate::host::{
    default_host_profile_name, host_logs, host_status, start_host_browser, stop_host_browser,
};
use crate::paths::sanitize_profile_name;
use crate::proxy::run_proxy;
use crate::runtime::{
    build_runtime, start_runtime, status_runtime, stop_runtime, stream_container_logs,
};
use crate::session::{
    AttachedSession, SessionActionRequest, SessionHumanOptions, choose_chrome_target,
    current_timestamp_rfc3339, default_session_attach_timeout, default_session_browser,
    default_session_chrome_host, default_session_chrome_port, default_session_human_max_delay_ms,
    default_session_human_min_delay_ms, default_session_human_mouse_steps,
    default_session_human_scroll_step_px, default_session_human_type_max_delay_ms,
    default_session_human_type_min_delay_ms, default_session_humanize, default_session_mode,
    delete_attached_session, discover_chrome_targets, list_attached_sessions,
    normalize_session_mode, read_attached_session, run_session_action, write_attached_session,
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
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Proxy(ProxyArgs),
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

#[derive(Debug, Subcommand)]
enum SessionCommand {
    Discover(SessionDiscoverArgs),
    Scan(SessionDiscoverArgs),
    Attach(SessionAttachArgs),
    List(SessionListArgs),
    Ls(SessionListArgs),
    Detach(SessionDetachArgs),
    Rm(SessionDetachArgs),
    Remove(SessionDetachArgs),
    Act(SessionActArgs),
    Action(SessionActArgs),
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

#[derive(Debug, Args)]
struct SessionDiscoverArgs {
    #[arg(long, default_value_t = default_session_browser())]
    browser: String,
    #[arg(long, default_value_t = default_session_chrome_host())]
    host: String,
    #[arg(long = "cdp-port", default_value_t = default_session_chrome_port())]
    cdp_port: i32,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct SessionAttachArgs {
    #[arg(long, default_value_t = default_session_browser())]
    browser: String,
    #[arg(long, default_value_t = default_session_chrome_host())]
    host: String,
    #[arg(long = "cdp-port", default_value_t = default_session_chrome_port())]
    cdp_port: i32,
    #[arg(long = "id")]
    target_id: Option<String>,
    #[arg(long = "url-contains")]
    url_contains: Option<String>,
    #[arg(long = "title-contains")]
    title_contains: Option<String>,
    #[arg(long = "session")]
    session_name: Option<String>,
    #[arg(long, default_value_t = default_session_mode())]
    mode: String,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct SessionListArgs {
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct SessionDetachArgs {
    #[arg(long = "session")]
    session_name: String,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct SessionActArgs {
    #[arg(long = "session")]
    session_name: String,
    #[arg(long)]
    action: String,
    #[arg(long)]
    selector: Option<String>,
    #[arg(long)]
    text: Option<String>,
    #[arg(long)]
    expr: Option<String>,
    #[arg(long)]
    out: Option<String>,
    #[arg(long = "delta-y", default_value_t = 0)]
    delta_y: i32,
    #[arg(long, default_value_t = 0)]
    steps: i32,
    #[arg(long, default_value_t = default_session_humanize(), action = ArgAction::Set)]
    human: bool,
    #[arg(long = "min-delay-ms", default_value_t = default_session_human_min_delay_ms())]
    min_delay_ms: i32,
    #[arg(long = "max-delay-ms", default_value_t = default_session_human_max_delay_ms())]
    max_delay_ms: i32,
    #[arg(long = "type-min-delay-ms", default_value_t = default_session_human_type_min_delay_ms())]
    type_min_delay_ms: i32,
    #[arg(long = "type-max-delay-ms", default_value_t = default_session_human_type_max_delay_ms())]
    type_max_delay_ms: i32,
    #[arg(long = "mouse-steps", default_value_t = default_session_human_mouse_steps())]
    mouse_steps: i32,
    #[arg(long = "scroll-step-px", default_value_t = default_session_human_scroll_step_px())]
    scroll_step_px: i32,
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
    #[arg(long = "fort-key", alias = "vault-key")]
    fort_key: Option<String>,
    #[arg(long = "fort-repo")]
    fort_repo: Option<String>,
    #[arg(long = "fort-env")]
    fort_env: Option<String>,
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

#[derive(Debug, Args)]
struct ProxyArgs {
    #[arg(long, default_value = "127.0.0.1")]
    bind: String,
    #[arg(long, default_value_t = 8931)]
    port: u16,
    #[arg(long, default_value = "http://127.0.0.1:8932")]
    upstream: String,
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
        Command::Session { command } => handle_session(command),
        Command::Proxy(args) => {
            run_proxy(&args.bind, args.port, &args.upstream)?;
            Ok(0)
        }
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
                args.fort_key.as_deref(),
                args.fort_repo.as_deref(),
                args.fort_env.as_deref(),
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

fn handle_session(command: SessionCommand) -> Result<i32> {
    match command {
        SessionCommand::Discover(args) | SessionCommand::Scan(args) => {
            let adapter = args.browser.trim().to_lowercase();
            match adapter.as_str() {
                "chrome" | "chromium" => {
                    let targets = discover_chrome_targets(
                        &args.host,
                        args.cdp_port,
                        default_session_attach_timeout(),
                    )?;
                    if args.json {
                        print_json(&serde_json::json!({
                            "ok": true,
                            "browser": "chrome",
                            "targets": targets,
                        }))?;
                    } else if targets.is_empty() {
                        println!("no browser targets discovered");
                    } else {
                        println!("available browser targets:");
                        for (index, target) in targets.iter().enumerate() {
                            let title = if target.title.trim().is_empty() {
                                "(untitled)"
                            } else {
                                target.title.trim()
                            };
                            println!("{}. [{}] {}", index + 1, target.id, title);
                            println!("   {}", target.url.trim());
                        }
                    }
                    Ok(0)
                }
                "safari" => bail!(
                    "safari existing-session discovery is not implemented yet; use chrome/chromium"
                ),
                _ => bail!(
                    "unsupported browser {:?} (expected chrome|safari)",
                    args.browser
                ),
            }
        }
        SessionCommand::Attach(args) => {
            let adapter = args.browser.trim().to_lowercase();
            let mode = normalize_session_mode(&args.mode)?;
            match adapter.as_str() {
                "chrome" | "chromium" => {
                    let targets = discover_chrome_targets(
                        &args.host,
                        args.cdp_port,
                        default_session_attach_timeout(),
                    )?;
                    let target = choose_chrome_target(
                        &targets,
                        args.target_id.as_deref().unwrap_or_default(),
                        args.url_contains.as_deref().unwrap_or_default(),
                        args.title_contains.as_deref().unwrap_or_default(),
                    )?;
                    let session_name = args
                        .session_name
                        .as_deref()
                        .map(sanitize_profile_name)
                        .unwrap_or_else(|| {
                            sanitize_profile_name(if !target.id.is_empty() {
                                &target.id
                            } else {
                                &target.title
                            })
                        });
                    let session = AttachedSession {
                        session: session_name.clone(),
                        browser: "chrome".to_owned(),
                        mode,
                        target_id: target.id.clone(),
                        title: target.title.clone(),
                        url: target.url.clone(),
                        ws_url: target.websocket_debugger_url.clone(),
                        cdp_host: args.host.trim().to_owned(),
                        cdp_port: args.cdp_port,
                        created_at: current_timestamp_rfc3339(),
                    };
                    write_attached_session(&session)?;
                    if args.json {
                        print_json(&serde_json::json!({"ok": true, "session": session}))?;
                    } else {
                        println!("attached: {} ({})", session.session, session.mode);
                        println!("  target_id={}", session.target_id);
                        println!("  title={}", session.title.trim());
                        println!("  url={}", session.url.trim());
                    }
                    Ok(0)
                }
                "safari" => bail!(
                    "safari existing-session attach is not implemented yet; use chrome/chromium"
                ),
                _ => bail!(
                    "unsupported browser {:?} (expected chrome|safari)",
                    args.browser
                ),
            }
        }
        SessionCommand::List(args) | SessionCommand::Ls(args) => {
            let sessions = list_attached_sessions()?;
            if args.json {
                print_json(&serde_json::json!({"ok": true, "sessions": sessions}))?;
            } else if sessions.is_empty() {
                println!("no attached sessions");
            } else {
                for (index, session) in sessions.iter().enumerate() {
                    println!(
                        "{}. {} [{}/{}]",
                        index + 1,
                        session.session,
                        session.browser,
                        session.mode
                    );
                    println!("   {}", session.url.trim());
                }
            }
            Ok(0)
        }
        SessionCommand::Detach(args) | SessionCommand::Rm(args) | SessionCommand::Remove(args) => {
            let session_name = sanitize_profile_name(&args.session_name);
            delete_attached_session(&session_name)?;
            if args.json {
                print_json(&serde_json::json!({"ok": true, "session": session_name}))?;
            } else {
                println!("detached: {session_name}");
            }
            Ok(0)
        }
        SessionCommand::Act(args) | SessionCommand::Action(args) => {
            let session = read_attached_session(&sanitize_profile_name(&args.session_name))?;
            let result = run_session_action(
                &session,
                &SessionActionRequest {
                    action: args.action.trim().to_lowercase(),
                    selector: args.selector.unwrap_or_default(),
                    text: args.text.unwrap_or_default(),
                    expr: args.expr.unwrap_or_default(),
                    out: args.out.unwrap_or_default(),
                    delta_y: args.delta_y,
                    steps: args.steps,
                    human: SessionHumanOptions {
                        enabled: args.human,
                        min_delay_ms: args.min_delay_ms,
                        max_delay_ms: args.max_delay_ms,
                        type_min_delay: args.type_min_delay_ms,
                        type_max_delay: args.type_max_delay_ms,
                        mouse_steps: args.mouse_steps,
                        scroll_step_px: args.scroll_step_px,
                    },
                },
            )?;
            if args.json {
                print_json(&serde_json::json!({"ok": true, "result": result}))?;
            } else if result.action == "screenshot" {
                println!("screenshot: {}", result.output);
            } else {
                println!("{}: {}", result.action, result.value.unwrap_or_default());
            }
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
        assert!(
            run(&[
                "session".to_owned(),
                "discover".to_owned(),
                "--browser".to_owned(),
                "firefox".to_owned()
            ])
            .is_err()
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
    fn apply_runtime_overrides_updates_fields() {
        let mut cfg = default_config();
        let overrides = RuntimeOverrides {
            image: Some("my-image".to_owned()),
            container_name: Some("my-container".to_owned()),
            network: Some("bridge".to_owned()),
            profile: Some("my-profile".to_owned()),
            profile_dir: Some("/tmp/profile".to_owned()),
            host_bind: Some("0.0.0.0:8000".to_owned()),
            host_mcp_port: Some(1),
            host_novnc_port: Some(2),
            mcp_port: Some(3),
            novnc_port: Some(4),
            vnc_password: Some("pass".to_owned()),
            mcp_version: Some("1".to_owned()),
            browser_channel: Some("stable".to_owned()),
            allowed_hosts: Some("localhost".to_owned()),
        };
        let profile_dir_passed = apply_runtime_overrides(&mut cfg, &overrides);
        assert!(profile_dir_passed);
        assert_eq!(cfg.image_name, "my-image");
        assert_eq!(cfg.container_name, "my-container");
        assert_eq!(cfg.network, "bridge");
        assert_eq!(cfg.profile_name, "my-profile");
        assert_eq!(cfg.profile_dir, "/tmp/profile");
        assert_eq!(cfg.host_bind, "0.0.0.0:8000");
        assert_eq!(cfg.host_mcp_port, 1);
        assert_eq!(cfg.host_novnc_port, 2);
        assert_eq!(cfg.mcp_port, 3);
        assert_eq!(cfg.novnc_port, 4);
        assert_eq!(cfg.vnc_password, "pass");
        assert_eq!(cfg.mcp_version, "1");
        assert_eq!(cfg.browser_channel, "stable");
        assert_eq!(cfg.allowed_hosts, "localhost");
    }
}
