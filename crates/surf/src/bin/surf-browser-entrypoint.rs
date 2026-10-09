use std::fs::{self, File};
use std::io::{self, Write};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use surf::browser::generate_secure_secret;

const DEFAULT_DISPLAY_NUM: &str = "99";
const DEFAULT_XVFB_WHD: &str = "1920x1080x24";
const DEFAULT_MCP_PORT: &str = "8931";
const DEFAULT_VNC_PORT: &str = "5900";
const DEFAULT_NOVNC_PORT: &str = "6080";
const DEFAULT_VNC_PASSWORD: &str = "";
const DEFAULT_PROFILE_DIR: &str = "/home/pwuser/.playwright-mcp-profile";
const DEFAULT_ALLOWED_HOSTS: &str = "*";
const DEFAULT_BROWSER_CHANNEL: &str = "chromium";
const DEFAULT_AGENTS: &str = "default";
const DEFAULT_PROFILE_NAME: &str = "default";
const STOCK_NOVNC_ROOT: &str = "/usr/share/novnc";
const SURF_NOVNC_ROOT: &str = "/tmp/surf-novnc";

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
    if !is_legacy_single(&config.agents) {
        for agent in &config.agents {
            let dir = config.profile_dir.join("agents").join(agent);
            fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        }
    }
    ensure_fluxbox_workspaces(&config.profile_name, &config.agents)?;
    clear_stale_x_display(&config.display_num)?;
    for agent in &config.agents {
        let dir = agent_profile_dir(&config, agent);
        clear_stale_browser_lock(&dir)?;
        clear_crashed_exit_state(&dir);
    }
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

    let mut xvfb = spawn_logged_process(
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
    wait_for_display(&mut xvfb, &socket_path, 50, Duration::from_millis(100))?;

    let fluxbox = spawn_logged_process(
        "fluxbox",
        std::iter::empty::<&str>(),
        Path::new("/tmp/fluxbox.log"),
        &[("DISPLAY", display.as_str())],
    )?;

    let x11vnc = Command::new("x11vnc")
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

    let websockify_target = format!("localhost:{}", config.vnc_port);
    let novnc_root = novnc_root.to_string_lossy().to_string();
    let websockify = spawn_logged_process(
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

    let mut children = vec![
        SupervisedChild::fatal("Xvfb", xvfb),
        SupervisedChild::fatal("fluxbox", fluxbox),
        SupervisedChild::fatal("x11vnc", x11vnc),
        SupervisedChild::fatal("websockify", websockify),
    ];
    for (slot, agent) in config.agents.iter().enumerate() {
        let child = spawn_agent_mcp(&config, &display, slot, agent)?;
        children.push(SupervisedChild::agent_mcp(agent, slot, child));
    }

    supervise(children, &config, &display)
}

fn is_legacy_single(agents: &[String]) -> bool {
    agents.len() == 1 && agents[0] == "default"
}

fn agent_profile_dir(config: &EntryPointConfig, agent: &str) -> PathBuf {
    if is_legacy_single(&config.agents) {
        config.profile_dir.clone()
    } else {
        config.profile_dir.join("agents").join(agent)
    }
}

fn mcp_args(config: &EntryPointConfig, slot: usize, agent: &str) -> Vec<String> {
    let mut args = vec![
        "--host".to_owned(),
        "0.0.0.0".to_owned(),
        "--allowed-hosts".to_owned(),
        config.allowed_hosts.clone(),
        "--port".to_owned(),
        (config.mcp_port as usize + slot).to_string(),
        "--user-data-dir".to_owned(),
        agent_profile_dir(config, agent)
            .to_string_lossy()
            .to_string(),
        // One browser per agent MCP; the agent's own sessions share it.
        "--shared-browser-context".to_owned(),
    ];
    // Playwright MCP defaults to the branded Chrome channel when --browser is
    // omitted, which is not installed here; always pass the configured
    // browser explicitly ("chromium" selects the bundled build installed at
    // image build time).
    let channel = config.browser_channel.trim().to_lowercase();
    let channel = if channel.is_empty() {
        "chromium".to_owned()
    } else {
        channel
    };
    args.push("--browser".to_owned());
    args.push(channel);
    args
}

fn spawn_agent_mcp(
    config: &EntryPointConfig,
    display: &str,
    slot: usize,
    agent: &str,
) -> Result<Child> {
    let log_path = PathBuf::from(format!("/tmp/mcp-agent-{agent}.log"));
    eprintln!(
        "starting MCP for agent '{agent}' on port {}",
        config.mcp_port as usize + slot
    );
    spawn_logged_process(
        "playwright-mcp",
        mcp_args(config, slot, agent),
        &log_path,
        &[("DISPLAY", display)],
    )
}

struct SupervisedChild {
    name: String,
    child: Child,
    /// Fatal children take the container down; agent MCPs are restarted.
    fatal: bool,
    agent_slot: Option<(String, usize)>,
}

impl SupervisedChild {
    fn fatal(name: &str, child: Child) -> Self {
        Self {
            name: name.to_owned(),
            child,
            fatal: true,
            agent_slot: None,
        }
    }

    fn agent_mcp(agent: &str, slot: usize, child: Child) -> Self {
        Self {
            name: format!("mcp-agent-{agent}"),
            child,
            fatal: false,
            agent_slot: Some((agent.to_owned(), slot)),
        }
    }
}

fn supervise(
    mut children: Vec<SupervisedChild>,
    config: &EntryPointConfig,
    display: &str,
) -> Result<()> {
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, OnceLock};

    static TERMINATED: OnceLock<Arc<AtomicBool>> = OnceLock::new();
    let terminated = TERMINATED
        .get_or_init(|| Arc::new(AtomicBool::new(false)))
        .clone();
    for signal in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        signal_hook::flag::register(signal, Arc::clone(&terminated))
            .context("register signal handler")?;
    }

    let mut tick: u64 = 0;
    loop {
        if terminated.load(Ordering::Relaxed) {
            eprintln!("surf-browser-entrypoint: shutting down");
            shutdown_children(&mut children);
            return Ok(());
        }
        let mut fatal_exit: Option<(String, std::process::ExitStatus)> = None;
        let mut restarts: Vec<(String, usize)> = Vec::new();
        for entry in children.iter_mut() {
            match entry.child.try_wait().context("poll child")? {
                None => {}
                Some(status) => {
                    if entry.fatal {
                        fatal_exit = Some((entry.name.clone(), status));
                        break;
                    }
                    if let Some((agent, slot)) = entry.agent_slot.clone() {
                        restarts.push((agent, slot));
                    }
                }
            }
        }
        if let Some((name, status)) = fatal_exit {
            eprintln!("surf-browser-entrypoint: {name} exited ({status}); stopping");
            shutdown_children(&mut children);
            bail!("{name} exited ({status})");
        }
        for (agent, slot) in restarts {
            eprintln!("surf-browser-entrypoint: mcp-agent-{agent} exited; restarting in 3s");
            thread::sleep(Duration::from_secs(3));
            match spawn_agent_mcp(config, display, slot, &agent) {
                Ok(child) => {
                    if let Some(entry) = children
                        .iter_mut()
                        .find(|e| e.agent_slot.as_ref().is_some_and(|(a, _)| a == &agent))
                    {
                        entry.child = child;
                    }
                }
                Err(error) => eprintln!("surf-browser-entrypoint: restart failed: {error:#}"),
            }
        }
        tick += 1;
        if tick.is_multiple_of(2) {
            place_agent_windows(display, &config.profile_dir, &config.agents);
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn shutdown_children(children: &mut [SupervisedChild]) {
    for entry in children.iter() {
        // Graceful first so Chrome flushes profiles; SIGKILL follows below.
        let _ = Command::new("kill")
            .args(["-TERM", &entry.child.id().to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    for entry in children.iter_mut() {
        loop {
            match entry.child.try_wait() {
                Ok(None) if std::time::Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(200));
                }
                _ => break,
            }
        }
        let _ = entry.child.kill();
        let _ = entry.child.wait();
    }
}

struct PlacedWindow {
    id: String,
    desktop: i32,
    pid: u32,
    class: String,
}

/// Parse `wmctrl -l -p -x` output. Columns: id, desktop, pid, class, host,
/// title... Only the first four are used.
fn parse_wmctrl_line(line: &str) -> Option<PlacedWindow> {
    let mut parts = line.split_whitespace();
    let id = parts.next()?.to_owned();
    let desktop: i32 = parts.next()?.parse().ok()?;
    let pid: u32 = parts.next()?.parse().ok()?;
    let class = parts.next()?.to_owned();
    Some(PlacedWindow {
        id,
        desktop,
        pid,
        class,
    })
}

fn is_browser_window(class: &str) -> bool {
    let class = class.to_lowercase();
    ["chrom", "firefox", "navigator", "minibrowser"]
        .iter()
        .any(|needle| class.contains(needle))
}

/// Find which agent owns a window PID by walking process ancestry to the
/// `playwright-mcp` whose `--user-data-dir` matches the agent's profile.
fn owning_agent(pid: u32, profile_dir: &Path, agents: &[String]) -> Option<String> {
    let mut current = pid;
    for _ in 0..32 {
        if current <= 1 {
            return None;
        }
        if let Some(agent) = mcp_agent_for_pid(current, profile_dir, agents) {
            return Some(agent);
        }
        current = parent_pid(current)?;
    }
    None
}

fn mcp_agent_for_pid(pid: u32, profile_dir: &Path, agents: &[String]) -> Option<String> {
    let cmdline = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let args: Vec<String> = cmdline
        .split(|b| *b == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect();
    if !args.iter().any(|arg| arg.contains("playwright-mcp")) {
        return None;
    }
    let mut user_data_dir = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--user-data-dir" {
            user_data_dir = iter.next().cloned();
            break;
        }
        if let Some(value) = arg.strip_prefix("--user-data-dir=") {
            user_data_dir = Some(value.to_owned());
            break;
        }
    }
    let dir = PathBuf::from(user_data_dir?);
    if dir == profile_dir && agents.len() == 1 {
        return Some(agents[0].clone());
    }
    if dir.starts_with(profile_dir.join("agents"))
        && let Some(name) = dir.file_name().and_then(|n| n.to_str())
        && agents.iter().any(|a| a == name)
    {
        return Some(name.to_owned());
    }
    None
}

fn parent_pid(pid: u32) -> Option<u32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // comm may contain spaces/parens; ppid is the 2nd field after the final ')'.
    let after_comm = stat.rsplit(')').next()?;
    after_comm.split_whitespace().nth(1)?.parse().ok()
}

/// Move every agent browser window to its agent's workspace slot.
fn place_agent_windows(display: &str, profile_dir: &Path, agents: &[String]) {
    let output = Command::new("wmctrl")
        .args(["-l", "-p", "-x"])
        .env("DISPLAY", display)
        .output();
    let Ok(output) = output else {
        return;
    };
    if !output.status.success() {
        return;
    }
    let listing = String::from_utf8_lossy(&output.stdout);
    for line in listing.lines() {
        let Some(window) = parse_wmctrl_line(line) else {
            continue;
        };
        if window.desktop < 0 || !is_browser_window(&window.class) {
            continue;
        }
        let Some(agent) = owning_agent(window.pid, profile_dir, agents) else {
            continue;
        };
        let Some(slot) = agents.iter().position(|a| a == &agent) else {
            continue;
        };
        if window.desktop as usize == slot {
            continue;
        }
        let status = Command::new("wmctrl")
            .args(["-i", "-r", &window.id, "-t", &slot.to_string()])
            .env("DISPLAY", display)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if matches!(status, Ok(s) if s.success()) {
            eprintln!("moved {agent} window {} to workspace {slot}", window.id);
        }
    }
}

/// Spawn a long-lived child, returning its handle.
///
/// The handle must be kept: dropping a `Child` does not wait, so an exited child
/// stays a zombie until this process reaps it (see `reap_finished_children`).
fn spawn_logged_process<I, S>(
    program: &str,
    args: I,
    log_path: &Path,
    envs: &[(&str, &str)],
) -> Result<Child>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let stdout =
        File::create(log_path).with_context(|| format!("create {}", log_path.display()))?;
    let stderr = stdout
        .try_clone()
        .with_context(|| format!("clone {}", log_path.display()))?;
    let child = Command::new(program)
        .args(args.into_iter().map(|value| value.as_ref().to_owned()))
        .envs(envs.iter().copied())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .with_context(|| format!("spawn {program}"))?;
    Ok(child)
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
    replace_symlink(&root.join("vnc.html"), &root.join("index.html"))?;
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

/// Remove X lock/socket left behind by a previous run.
///
/// A container filesystem survives `docker restart`, so an unclean shutdown
/// leaves `/tmp/.X<n>-lock` and `/tmp/.X11-unix/X<n>` in place. Xvfb then refuses
/// to start ("Server is already active for display <n>") and, because the stale
/// socket satisfies a file-existence readiness check, the failure goes unnoticed:
/// fluxbox and x11vnc follow it down and the container serves noVNC with no X
/// behind it. Clearing them first is what `startx` and friends do.
fn clear_stale_x_display(display_num: &str) -> Result<()> {
    for path in [
        PathBuf::from(format!("/tmp/.X{display_num}-lock")),
        PathBuf::from(format!("/tmp/.X11-unix/X{display_num}")),
    ] {
        match fs::remove_file(&path) {
            Ok(()) => eprintln!("removed stale {}", path.display()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("remove stale {}", path.display()));
            }
        }
    }
    Ok(())
}

/// Remove Chrome singleton locks left behind by a previous container.
///
/// Profiles persist on a bind mount while the browser does not survive
/// container recreation, so after an unclean shutdown the stale
/// `SingletonLock`/`SingletonSocket`/`SingletonCookie` make the new browser
/// refuse to start ("already in use"). At entrypoint time no Chrome can be
/// running yet in this container, so removing them is safe.
fn clear_stale_browser_lock(profile_dir: &Path) -> Result<()> {
    for name in ["SingletonLock", "SingletonSocket", "SingletonCookie"] {
        let path = profile_dir.join(name);
        match fs::remove_file(&path) {
            Ok(()) => eprintln!("removed stale {}", path.display()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("remove stale {}", path.display()));
            }
        }
    }
    Ok(())
}

/// Mark a persisted profile as cleanly exited so Chrome does not show the
/// "Restore pages?" bubble after a container restart killed it mid-run.
///
/// Best-effort: profiles that are missing or unparseable are left untouched.
fn clear_crashed_exit_state(profile_dir: &Path) {
    let prefs_path = profile_dir.join("Default").join("Preferences");
    let Ok(text) = fs::read_to_string(&prefs_path) else {
        return;
    };
    let Ok(mut prefs) = serde_json::from_str::<serde_json::Value>(&text) else {
        return;
    };
    let Some(profile) = prefs.get_mut("profile") else {
        return;
    };
    let mut changed = false;
    if profile.get("exit_type").and_then(|v| v.as_str()) != Some("Normal") {
        profile["exit_type"] = serde_json::Value::String("Normal".to_owned());
        changed = true;
    }
    if profile.get("exited_cleanly").and_then(|v| v.as_bool()) != Some(true) {
        profile["exited_cleanly"] = serde_json::Value::Bool(true);
        changed = true;
    }
    if !changed {
        return;
    }
    let Ok(text) = serde_json::to_string(&prefs) else {
        return;
    };
    if fs::write(&prefs_path, text).is_ok() {
        eprintln!("cleared crashed exit state in {}", prefs_path.display());
    }
}

/// Wait for the X socket, failing fast if the server died instead.
///
/// The socket alone is not proof of life (see `clear_stale_x_display`), so the
/// child is polled too: if Xvfb exits, report that rather than waiting out the
/// timeout and letting dependents fail with a confusing "couldn't connect".
fn wait_for_display(
    child: &mut Child,
    socket_path: &Path,
    attempts: usize,
    interval: Duration,
) -> Result<()> {
    for _ in 0..attempts {
        if let Some(status) = child.try_wait().context("poll Xvfb")? {
            bail!("Xvfb exited before the display was ready ({status}); see /tmp/xvfb.log");
        }
        if socket_path.exists() {
            return Ok(());
        }
        thread::sleep(interval);
    }
    bail!("timed out waiting for {}", socket_path.display());
}

fn parse_agents(csv: &str) -> Vec<String> {
    let agents: Vec<String> = csv
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect();
    if agents.is_empty() {
        vec!["default".to_owned()]
    } else {
        agents
    }
}

/// Workspace label for an agent: `<profile>/<agent>`.
fn workspace_name(profile: &str, agent: &str) -> String {
    format!("{profile}/{agent}")
}

fn ensure_fluxbox_workspaces(profile: &str, agents: &[String]) -> Result<()> {
    let fluxbox_dir = Path::new("/home/pwuser/.fluxbox");
    fs::create_dir_all(fluxbox_dir).context("create Fluxbox directory")?;
    let init_path = fluxbox_dir.join("init");
    let existing = fs::read_to_string(&init_path).unwrap_or_default();
    let normalized = normalize_fluxbox_init(&existing, profile, agents);
    if normalized == existing {
        return Ok(());
    }
    let mut file =
        File::create(&init_path).with_context(|| format!("create {}", init_path.display()))?;
    file.write_all(normalized.as_bytes())
        .with_context(|| format!("write {}", init_path.display()))?;
    Ok(())
}

fn normalize_fluxbox_init(existing: &str, profile: &str, agents: &[String]) -> String {
    let workspace_key = "session.screen0.workspaces:";
    let names_key = "session.screen0.workspaceNames:";
    let names = agents
        .iter()
        .map(|agent| workspace_name(profile, agent))
        .collect::<Vec<_>>()
        .join(",");
    let mut lines = Vec::new();
    let mut saw_workspace_key = false;
    let mut saw_names_key = false;
    for line in existing.lines() {
        if line.trim_start().starts_with(workspace_key) {
            if !saw_workspace_key {
                lines.push(format!("{workspace_key}	{}", agents.len()));
                saw_workspace_key = true;
            }
            continue;
        }
        if line.trim_start().starts_with(names_key) {
            if !saw_names_key {
                lines.push(format!("{names_key}	{names}"));
                saw_names_key = true;
            }
            continue;
        }
        lines.push(line.to_owned());
    }
    if !saw_workspace_key {
        lines.push(format!("{workspace_key}	{}", agents.len()));
    }
    if !saw_names_key {
        lines.push(format!("{names_key}	{names}"));
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
    mcp_port: u16,
    vnc_port: String,
    novnc_port: String,
    vnc_password: String,
    profile_dir: PathBuf,
    profile_name: String,
    allowed_hosts: String,
    browser_channel: String,
    agents: Vec<String>,
}

impl EntryPointConfig {
    fn from_env() -> Self {
        let raw_vnc_password = env_or("VNC_PASSWORD", DEFAULT_VNC_PASSWORD);
        let vnc_password = if raw_vnc_password.trim().is_empty() {
            let generated = generate_secure_secret(24);
            eprintln!(
                "surf-browser-entrypoint: generated a random VNC password because none was provided"
            );
            generated
        } else {
            raw_vnc_password
        };
        let agents = parse_agents(&env_or("AGENTS", DEFAULT_AGENTS));
        Self {
            display_num: env_or("DISPLAY_NUM", DEFAULT_DISPLAY_NUM),
            xvfb_whd: env_or("XVFB_WHD", DEFAULT_XVFB_WHD),
            mcp_port: env_or("MCP_PORT", DEFAULT_MCP_PORT).parse().unwrap_or(8931),
            vnc_port: env_or("VNC_PORT", DEFAULT_VNC_PORT),
            novnc_port: env_or("NOVNC_PORT", DEFAULT_NOVNC_PORT),
            vnc_password,
            profile_dir: PathBuf::from(env_or("PROFILE_DIR", DEFAULT_PROFILE_DIR)),
            profile_name: env_or("PROFILE_NAME", DEFAULT_PROFILE_NAME),
            allowed_hosts: env_or("ALLOWED_HOSTS", DEFAULT_ALLOWED_HOSTS),
            browser_channel: env_or("BROWSER_CHANNEL", DEFAULT_BROWSER_CHANNEL),
            agents,
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
    use super::{is_browser_window, normalize_fluxbox_init, parse_agents, parse_wmctrl_line};

    #[test]
    fn normalize_fluxbox_init_sets_count_and_names() {
        let agents = vec!["alice".to_owned(), "bob".to_owned()];
        let got = normalize_fluxbox_init(
            "session.menuFile:	~/.fluxbox/menu
session.screen0.workspaces:	4
",
            "work",
            &agents,
        );
        assert!(got.contains("session.screen0.workspaces:	2\n"));
        assert!(got.contains("session.screen0.workspaceNames:	work/alice,work/bob\n"));
        assert!(!got.contains("session.screen0.workspaces:	4\n"));
    }

    #[test]
    fn parse_agents_defaults_to_default() {
        assert_eq!(parse_agents(""), vec!["default".to_owned()]);
        assert_eq!(
            parse_agents("alice, bob,,"),
            vec!["alice".to_owned(), "bob".to_owned()]
        );
    }

    #[test]
    fn parse_wmctrl_line_reads_id_desktop_pid_class() {
        let window =
            parse_wmctrl_line("0x01200003  1 4321 chromium.Chromium  host  New Tab - Chromium")
                .unwrap();
        assert_eq!(window.id, "0x01200003");
        assert_eq!(window.desktop, 1);
        assert_eq!(window.pid, 4321);
        assert_eq!(window.class, "chromium.Chromium");
        assert!(parse_wmctrl_line("garbage").is_none());
    }

    #[test]
    fn is_browser_window_matches_browser_classes() {
        assert!(is_browser_window("chromium.Chromium"));
        assert!(is_browser_window("google-chrome.Google-chrome"));
        assert!(is_browser_window("Navigator.Firefox"));
        assert!(!is_browser_window("fluxbox.Toolbar"));
    }
}
