use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::symlink;
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
const STOCK_NOVNC_ROOT: &str = "/usr/share/novnc";
const SURF_NOVNC_ROOT: &str = "/tmp/surf-novnc";
const SURF_VIEWER_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Surf Browser</title>
  <style>
    html,
    body {
      height: 100%;
      margin: 0;
      overflow: hidden;
      background: #111827;
      color: #f9fafb;
      font: 13px system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }

    #toolbar {
      align-items: center;
      background: #111827;
      border-bottom: 1px solid #374151;
      display: flex;
      gap: 8px;
      height: 36px;
      padding: 0 10px;
    }

    #status {
      color: #d1d5db;
      flex: 1;
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    button,
    a {
      background: #1f2937;
      border: 1px solid #4b5563;
      border-radius: 6px;
      color: #f9fafb;
      cursor: pointer;
      font: inherit;
      padding: 5px 9px;
      text-decoration: none;
    }

    button:hover,
    a:hover {
      background: #374151;
    }

    #screen {
      height: calc(100% - 37px);
      outline: none;
      overflow: hidden;
      width: 100%;
    }

    #screen:focus-within {
      box-shadow: inset 0 0 0 2px #22c55e;
    }
  </style>
</head>
<body>
  <div id="toolbar">
    <div id="status">Connecting</div>
    <button id="focus_button" type="button">Focus</button>
    <a href="/vnc.html?autoconnect=1&resize=scale">Stock noVNC</a>
  </div>
  <div id="screen" tabindex="-1" aria-label="Surf browser screen"></div>
  <script type="module">
    import RFB from "./core/rfb.js";

    const screen = document.getElementById("screen");
    const status = document.getElementById("status");
    const focusButton = document.getElementById("focus_button");
    let rfb;

    function queryValue(name, fallback) {
      const params = new URLSearchParams(window.location.search);
      const hashParams = new URLSearchParams(window.location.hash.replace(/^#/, ""));
      return params.get(name) ?? hashParams.get(name) ?? fallback;
    }

    function setStatus(message) {
      status.textContent = message;
    }

    function websocketUrl() {
      const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
      const host = queryValue("host", window.location.hostname);
      const port = queryValue("port", window.location.port);
      const path = queryValue("path", "websockify").replace(/^\/+/, "");
      const authority = port ? `${host}:${port}` : host;
      return `${protocol}//${authority}/${path}`;
    }

    function focusRemote() {
      if (!rfb) return;
      requestAnimationFrame(() => {
        try {
          rfb.focus();
        } catch (_error) {
          screen.focus();
        }
      });
    }

    function credentialsAreRequired() {
      const password = window.prompt("VNC password");
      rfb.sendCredentials({ password: password ?? "" });
      focusRemote();
    }

    function connect() {
      const password = queryValue("password", undefined);
      const options = password === undefined ? {} : { credentials: { password } };
      rfb = new RFB(screen, websocketUrl(), options);
      rfb.viewOnly = queryValue("view_only", "false") === "true";
      rfb.scaleViewport = queryValue("scale", "true") !== "false";
      rfb.clipViewport = false;
      rfb.addEventListener("connect", () => {
        setStatus("Connected");
        focusRemote();
      });
      rfb.addEventListener("disconnect", (event) => {
        setStatus(event.detail.clean ? "Disconnected" : "Connection closed");
      });
      rfb.addEventListener("credentialsrequired", credentialsAreRequired);
      rfb.addEventListener("desktopname", (event) => {
        if (event.detail.name) setStatus(`Connected to ${event.detail.name}`);
      });
    }

    for (const eventName of ["pointerdown", "mousedown", "touchstart", "click"]) {
      screen.addEventListener(eventName, focusRemote, { capture: true, passive: true });
    }
    window.addEventListener("focus", focusRemote);
    document.addEventListener("visibilitychange", () => {
      if (!document.hidden) focusRemote();
    });
    document.addEventListener("keydown", focusRemote, { capture: true });
    focusButton.addEventListener("click", focusRemote);

    connect();
  </script>
</body>
</html>
"#;
const SURF_INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta http-equiv="refresh" content="0; url=/surf.html">
  <title>Surf Browser</title>
</head>
<body>
  <a href="/surf.html">Surf Browser</a>
</body>
</html>
"#;

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
    let novnc_root = prepare_novnc_web_root()?;

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
    let novnc_root = novnc_root.to_string_lossy().to_string();
    spawn_logged_process(
        "websockify",
        [
            "--web",
            novnc_root.as_str(),
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

fn prepare_novnc_web_root() -> Result<PathBuf> {
    let root = PathBuf::from(SURF_NOVNC_ROOT);
    fs::create_dir_all(&root).with_context(|| format!("create {}", root.display()))?;
    for name in [
        "app",
        "core",
        "include",
        "vendor",
        "utils",
        "vnc.html",
        "vnc_lite.html",
    ] {
        let source = Path::new(STOCK_NOVNC_ROOT).join(name);
        let target = root.join(name);
        replace_symlink(&source, &target)?;
    }
    write_text(root.join("surf.html"), SURF_VIEWER_HTML)?;
    write_text(root.join("index.html"), SURF_INDEX_HTML)?;
    Ok(root)
}

fn replace_symlink(source: &Path, target: &Path) -> Result<()> {
    if target.symlink_metadata().is_ok() {
        fs::remove_file(target)
            .or_else(|_| fs::remove_dir_all(target))
            .with_context(|| format!("replace existing noVNC web entry {}", target.display()))?;
    }
    symlink(source, target)
        .with_context(|| format!("link {} -> {}", target.display(), source.display()))?;
    Ok(())
}

fn write_text(path: PathBuf, body: &str) -> Result<()> {
    let mut file = File::create(&path).with_context(|| format!("create {}", path.display()))?;
    file.write_all(body.as_bytes())
        .with_context(|| format!("write {}", path.display()))?;
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
    use super::{SURF_INDEX_HTML, SURF_VIEWER_HTML, normalize_fluxbox_init};

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

    #[test]
    fn surf_viewer_focuses_remote_screen() {
        assert!(SURF_VIEWER_HTML.contains("rfb.focus()"));
        assert!(SURF_VIEWER_HTML.contains("pointerdown"));
        assert!(SURF_VIEWER_HTML.contains("keydown"));
        assert!(SURF_VIEWER_HTML.contains("new RFB(screen, websocketUrl(), options)"));
    }

    #[test]
    fn surf_index_redirects_to_hardened_viewer() {
        assert!(SURF_INDEX_HTML.contains("url=/surf.html"));
        assert!(SURF_INDEX_HTML.contains("href=\"/surf.html\""));
    }
}
