use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use crate::constants::{SURF_VERSION, USAGE_TEXT};
use crate::settings::{
    default_surf_settings, load_surf_settings_or_default, save_surf_settings, set_surf_config_value,
};

#[derive(Debug, Parser)]
#[command(disable_help_flag = true, disable_version_flag = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Build,
    Start,
    Stop,
    Status,
    Logs,
    Session,
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Proxy,
    Host,
    Tunnel,
    Extension,
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
        Command::Build => not_yet("build"),
        Command::Start => not_yet("start"),
        Command::Stop => not_yet("stop"),
        Command::Status => not_yet("status"),
        Command::Logs => not_yet("logs"),
        Command::Session => not_yet("session"),
        Command::Proxy => not_yet("proxy"),
        Command::Host => not_yet("host"),
        Command::Tunnel => not_yet("tunnel"),
        Command::Extension => not_yet("extension"),
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

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn not_yet(command: &str) -> Result<i32> {
    bail!("{command} is not implemented in Rust yet")
}
