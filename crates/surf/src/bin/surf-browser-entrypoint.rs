use std::fs::{self, File};
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use surf::browser::generate_secure_vnc_password;

const DEFAULT_DISPLAY_NUM: &str = "99";
const DEFAULT_XVFB_WHD: &str = "1920x1080x24";
const DEFAULT_MCP_PORT: &str = "8931";
const DEFAULT_VNC_PORT: &str = "5900";
const DEFAULT_NOVNC_PORT: &str = "6080";
const DEFAULT_VNC_PASSWORD: &str = "";
const DEFAULT_MCP_VERSION: &str = "0.0.64";
const DEFAULT_PROFILE_DIR: &str = "/home/pwuser/.playwright-mcp-profile";
const DEFAULT_ALLOWED_HOSTS: &str = "*";
const DEFAULT_BROWSER_CHANNEL: &str = "chromium";
const DEFAULT_FLUXBOX_WORKSPACES: &str = "1";

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let config = EntryPointConfig::from_env();
    let display = format!(":{}", config.display_num);

    fs::create_dir_all("/home/pwuser/.vnc").context("create VNC directory")?;
    fs::create_dir_all(&config.profile_dir)
        .with_context(|| format!("create {}", config.profile_dir.display()))?;
    ensure_fluxbox_single_workspace()?;

    let status = Command::new("x11vnc")
        .args([
            "-storepasswd",
            &config.vnc_password,
            "/home/pwuser/.vnc/passwd",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("spawn x11vnc -storepasswd")?;
    if !status.success() {
        bail!("x11vnc -storepasswd failed");
    }

    spawn_logged_process(
        "Xvfb",
        [
            display.as_str(),
            "-screen",
            "0",
            &config.xvfb_whd,
            "-ac",
            "+extension",
            "RANDR",
        ],
        Path::new("/tmp/xvfb.log"),
        &[],
    )?;

    let socket_path = PathBuf::from(format!("/tmp/.X11-unix/X{}", config.display_num));
    wait_for_path(&socket_path, 50, Duration::from_millis(100))?;

    spawn_logged_process(
        "fluxbox",
        std::iter::empty::<&str>(),
        Path::new("/tmp/fluxbox.log"),
        &[("DISPLAY", display.as_str())],
    )?;

    let x11vnc_status = Command::new("x11vnc")
        .args([
            "-display",
            display.as_str(),
            "-rfbport",
            &config.vnc_port,
            "-rfbauth",
            "/home/pwuser/.vnc/passwd",
            "-forever",
            "-shared",
            "-noxdamage",
            "-o",
            "/tmp/x11vnc.log",
        ])
        .env("DISPLAY", &display)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("spawn x11vnc")?;
    let _ = x11vnc_status;

    let websockify_target = format!("localhost:{}", config.vnc_port);
    spawn_logged_process(
        "websockify",
        [
            "--web",
            "/usr/share/novnc/",
            &config.novnc_port,
            websockify_target.as_str(),
        ],
        Path::new("/tmp/websockify.log"),
        &[],
    )?;

    let error = Command::new("npx")
        .args([
            "-y",
            &format!("@playwright/mcp@{}", config.mcp_version),
            "--host",
            "0.0.0.0",
            "--allowed-hosts",
            &config.allowed_hosts,
            "--browser",
            &config.browser_channel,
            "--port",
            &config.mcp_port,
            "--user-data-dir",
            config.profile_dir.to_string_lossy().as_ref(),
        ])
        .env("DISPLAY", &display)
        .exec();
    bail!("exec npx failed: {error}");
}

fn spawn_logged_process<I, S>(
    program: &str,
    args: I,
    log_path: &Path,
    envs: &[(&str, &str)],
) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let stdout =
        File::create(log_path).with_context(|| format!("create {}", log_path.display()))?;
    let stderr = stdout
        .try_clone()
        .with_context(|| format!("clone {}", log_path.display()))?;
    Command::new(program)
        .args(args.into_iter().map(|value| value.as_ref().to_owned()))
        .envs(envs.iter().copied())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .with_context(|| format!("spawn {program}"))?;
    Ok(())
}

fn wait_for_path(path: &Path, attempts: usize, interval: Duration) -> Result<()> {
    for _ in 0..attempts {
        if path.exists() {
            return Ok(());
        }
        thread::sleep(interval);
    }
    bail!("timed out waiting for {}", path.display());
}

fn ensure_fluxbox_single_workspace() -> Result<()> {
    let fluxbox_dir = Path::new("/home/pwuser/.fluxbox");
    fs::create_dir_all(fluxbox_dir).context("create Fluxbox directory")?;
    let init_path = fluxbox_dir.join("init");
    let existing = fs::read_to_string(&init_path).unwrap_or_default();
    let normalized = normalize_fluxbox_init(&existing);
    if normalized == existing {
        return Ok(());
    }
    let mut file =
        File::create(&init_path).with_context(|| format!("create {}", init_path.display()))?;
    file.write_all(normalized.as_bytes())
        .with_context(|| format!("write {}", init_path.display()))?;
    Ok(())
}

fn normalize_fluxbox_init(existing: &str) -> String {
    let workspace_key = "session.screen0.workspaces:";
    let mut lines = Vec::new();
    let mut saw_workspace_key = false;
    for line in existing.lines() {
        if line.trim_start().starts_with(workspace_key) {
            if !saw_workspace_key {
                lines.push(format!("{workspace_key}	{DEFAULT_FLUXBOX_WORKSPACES}"));
                saw_workspace_key = true;
            }
            continue;
        }
        lines.push(line.to_owned());
    }
    if !saw_workspace_key {
        lines.push(format!("{workspace_key}	{DEFAULT_FLUXBOX_WORKSPACES}"));
    }
    let mut normalized = lines.join("\n");
    if !normalized.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

struct EntryPointConfig {
    display_num: String,
    xvfb_whd: String,
    mcp_port: String,
    vnc_port: String,
    novnc_port: String,
    vnc_password: String,
    mcp_version: String,
    profile_dir: PathBuf,
    allowed_hosts: String,
    browser_channel: String,
}

impl EntryPointConfig {
    fn from_env() -> Self {
        let raw_vnc_password = env_or("VNC_PASSWORD", DEFAULT_VNC_PASSWORD);
        let vnc_password = if raw_vnc_password.trim().is_empty() {
            let generated = generate_secure_vnc_password(24);
            eprintln!(
                "surf-browser-entrypoint: generated a random VNC password because none was provided"
            );
            generated
        } else {
            raw_vnc_password
        };
        Self {
            display_num: env_or("DISPLAY_NUM", DEFAULT_DISPLAY_NUM),
            xvfb_whd: env_or("XVFB_WHD", DEFAULT_XVFB_WHD),
            mcp_port: env_or("MCP_PORT", DEFAULT_MCP_PORT),
            vnc_port: env_or("VNC_PORT", DEFAULT_VNC_PORT),
            novnc_port: env_or("NOVNC_PORT", DEFAULT_NOVNC_PORT),
            vnc_password,
            mcp_version: env_or("MCP_VERSION", DEFAULT_MCP_VERSION),
            profile_dir: PathBuf::from(env_or("PROFILE_DIR", DEFAULT_PROFILE_DIR)),
            allowed_hosts: env_or("ALLOWED_HOSTS", DEFAULT_ALLOWED_HOSTS),
            browser_channel: env_or("BROWSER_CHANNEL", DEFAULT_BROWSER_CHANNEL),
        }
    }
}

fn env_or(name: &str, default_value: &str) -> String {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default_value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::normalize_fluxbox_init;

    #[test]
    fn normalize_fluxbox_init_adds_single_workspace_when_missing() {
        let got = normalize_fluxbox_init(
            "session.menuFile:	~/.fluxbox/menu
session.styleFile:	default
",
        );
        assert!(got.contains(
            "session.screen0.workspaces:	1
"
        ));
    }

    #[test]
    fn normalize_fluxbox_init_replaces_existing_workspace_count() {
        let got = normalize_fluxbox_init(
            "session.menuFile:	~/.fluxbox/menu
session.screen0.workspaces:	4
",
        );
        assert!(got.contains(
            "session.screen0.workspaces:	1
"
        ));
        assert!(!got.contains(
            "session.screen0.workspaces:	4
"
        ));
    }
}
