use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::constants::DEFAULT_HOST_CDP_PORT;
use crate::paths::{env_trimmed, host_profile_dir, sanitize_profile_name};
use crate::runtime::{find_command, probe_http_status};
use crate::settings::surf_state_dir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostProcessState {
    pub profile: String,
    pub pid: i32,
    pub browser_path: String,
    pub cdp_port: i32,
    pub profile_dir: String,
    pub started_at: String,
    pub log_file: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostStatus {
    pub ok: bool,
    pub profile: String,
    pub pid: i32,
    pub alive: bool,
    pub cdp_port: i32,
    pub cdp_status: i32,
    pub cdp_url: String,
    pub state: HostProcessState,
}

pub fn host_runtime_dir() -> PathBuf {
    surf_state_dir().join("browser").join("host")
}

pub fn host_state_path(profile: &str) -> PathBuf {
    host_runtime_dir().join(format!("{}.json", sanitize_profile_name(profile)))
}

pub fn host_log_path(profile: &str) -> PathBuf {
    host_runtime_dir().join(format!("{}.log", sanitize_profile_name(profile)))
}

pub fn read_host_pid(profile: &str) -> Result<i32> {
    Ok(read_host_state(profile)?.pid)
}

pub fn read_host_state(profile: &str) -> Result<HostProcessState> {
    let path = host_state_path(profile);
    let data =
        fs::read_to_string(&path).with_context(|| format!("read host state {}", path.display()))?;
    serde_json::from_str(&data).context("parse host state")
}

pub fn write_host_state(state: &HostProcessState) -> Result<()> {
    let path = host_state_path(&state.profile);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create host runtime directory {}", parent.display()))?;
    }
    let data = serde_json::to_string_pretty(state).context("serialize host state")?;
    fs::write(&path, data).with_context(|| format!("write host state {}", path.display()))?;
    Ok(())
}

pub fn process_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

pub fn host_launch_args(
    cdp_port: i32,
    profile_dir: &str,
    is_root: bool,
    display: &str,
) -> Vec<String> {
    let mut args = vec![
        format!("--remote-debugging-port={cdp_port}"),
        format!("--user-data-dir={profile_dir}"),
        "--no-first-run".to_owned(),
        "--no-default-browser-check".to_owned(),
    ];
    if cfg!(target_os = "linux") && is_root {
        args.push("--no-sandbox".to_owned());
        args.push("--disable-setuid-sandbox".to_owned());
    }
    if display.trim().is_empty() {
        args.push("--headless=new".to_owned());
    }
    args.push("about:blank".to_owned());
    args
}

pub fn default_host_profile_name() -> String {
    if let Some(profile) = env_trimmed("SURF_HOST_PROFILE") {
        return sanitize_profile_name(&profile);
    }
    let settings = crate::settings::load_surf_settings_or_default();
    if !settings.browser.profile_name.trim().is_empty() {
        return sanitize_profile_name(&settings.browser.profile_name);
    }
    "default".to_owned()
}

pub fn detect_host_browser_binary() -> Result<String> {
    if let Some(path) = env_trimmed("SURF_HOST_BROWSER_PATH") {
        return Ok(path);
    }
    if cfg!(target_os = "macos") {
        let candidates = [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ];
        for candidate in candidates {
            if Path::new(candidate).exists() {
                return Ok(candidate.to_owned());
            }
        }
        if let Some(home) = env_trimmed("HOME") {
            let candidate = PathBuf::from(home)
                .join("Applications")
                .join("Google Chrome.app")
                .join("Contents")
                .join("MacOS")
                .join("Google Chrome");
            if candidate.exists() {
                return Ok(candidate.display().to_string());
            }
        }
    }
    for binary in [
        "google-chrome",
        "google-chrome-stable",
        "brave-browser",
        "microsoft-edge",
        "chromium",
        "chromium-browser",
    ] {
        if let Some(path) = find_command(binary) {
            return Ok(path.display().to_string());
        }
    }
    bail!("no supported Chromium-based browser found; set --browser-path or SURF_HOST_BROWSER_PATH")
}

pub fn start_host_browser(
    profile: &str,
    profile_dir: Option<&str>,
    browser_path: Option<&str>,
    cdp_port: Option<i32>,
) -> Result<HostProcessState> {
    if !cfg!(target_os = "linux") && !cfg!(target_os = "macos") {
        bail!("host browser mode is only supported on linux and darwin");
    }

    let profile = sanitize_profile_name(profile);
    let profile_dir = profile_dir
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            host_profile_dir(&profile, &surf_state_dir())
                .display()
                .to_string()
        });
    fs::create_dir_all(&profile_dir)
        .with_context(|| format!("create profile directory {profile_dir}"))?;
    if let Ok(pid) = read_host_pid(&profile)
        && pid > 0
        && process_alive(pid)
    {
        bail!("host browser profile {profile} already running (pid={pid})");
    }

    let browser_path = browser_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or(detect_host_browser_binary()?);
    if !Path::new(&browser_path).exists() {
        bail!("browser binary not found: {browser_path}");
    }

    let log_path = host_log_path(&profile);
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create host log directory {}", parent.display()))?;
    }
    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("open host log {}", log_path.display()))?;
    let log_file_err = log_file.try_clone().context("clone host log handle")?;

    let cdp_port = cdp_port.unwrap_or_else(|| {
        env_trimmed("SURF_HOST_CDP_PORT")
            .and_then(|value| value.parse::<i32>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_HOST_CDP_PORT)
    });

    let launch_args = host_launch_args(
        cdp_port,
        &profile_dir,
        unsafe { libc::geteuid() == 0 },
        &std::env::var("DISPLAY").unwrap_or_default(),
    );
    let mut command = Command::new(&browser_path);
    command
        .args(&launch_args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err));
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command.spawn().context("start host browser")?;
    let pid = i32::try_from(child.id()).unwrap_or(0);

    let state = HostProcessState {
        profile: profile.clone(),
        pid,
        browser_path,
        cdp_port,
        profile_dir,
        started_at: iso_timestamp(),
        log_file: log_path.display().to_string(),
    };
    write_host_state(&state)?;
    Ok(state)
}

pub fn stop_host_browser(profile: &str) -> Result<HostProcessState> {
    let profile = sanitize_profile_name(profile);
    let state = read_host_state(&profile)?;
    if state.pid <= 0 {
        bail!("invalid pid for profile {profile}");
    }
    unsafe {
        libc::kill(state.pid, libc::SIGTERM);
    }
    for _ in 0..20 {
        if !process_alive(state.pid) {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    if process_alive(state.pid) {
        unsafe {
            libc::kill(state.pid, libc::SIGKILL);
        }
    }
    let _ = fs::remove_file(host_state_path(&profile));
    Ok(state)
}

pub fn host_status(profile: &str) -> Result<HostStatus> {
    let profile = sanitize_profile_name(profile);
    let state = read_host_state(&profile)?;
    let alive = process_alive(state.pid);
    let cdp_url = format!("http://127.0.0.1:{}/json/version", state.cdp_port);
    let cdp_status = probe_http_status(&cdp_url);
    let ok = alive && cdp_status == 200;
    Ok(HostStatus {
        ok,
        profile,
        pid: state.pid,
        alive,
        cdp_port: state.cdp_port,
        cdp_status,
        cdp_url,
        state,
    })
}

pub fn host_logs(profile: &str) -> Result<String> {
    let state = read_host_state(profile)?;
    fs::read_to_string(&state.log_file).with_context(|| format!("read host log {}", state.log_file))
}

fn iso_timestamp() -> String {
    let output = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output();
    match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }
        _ => "1970-01-01T00:00:00Z".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serial_test::serial;

    use super::{default_host_profile_name, host_launch_args};
    use crate::paths::{env_lock, set_env};
    use crate::settings::{default_surf_settings, save_surf_settings};

    #[test]
    fn host_launch_args_base() {
        let args = host_launch_args(18800, "/tmp/profile", false, ":99");
        let joined = args.join(" ");
        assert!(joined.contains("--remote-debugging-port=18800"));
        assert!(joined.contains("--user-data-dir=/tmp/profile"));
        assert!(!joined.contains("--headless=new"));
        assert_eq!(args.last().unwrap(), "about:blank");
    }

    #[test]
    fn host_launch_args_headless_when_no_display() {
        let args = host_launch_args(18800, "/tmp/profile", true, "");
        let joined = args.join(" ");
        if cfg!(target_os = "linux") {
            assert!(joined.contains("--no-sandbox"));
            assert!(joined.contains("--disable-setuid-sandbox"));
        }
        assert!(joined.contains("--headless=new"));
    }

    #[test]
    #[serial]
    fn default_host_profile_name_prefers_settings_then_env() {
        let _guard = env_lock().lock().unwrap();
        let home = tempfile::tempdir().unwrap();
        set_env(
            "SURF_SETTINGS_HOME",
            Some(home.path().to_string_lossy().as_ref()),
        );
        set_env("SURF_SETTINGS_FILE", None);
        set_env("SURF_HOST_PROFILE", None);

        let mut settings = default_surf_settings();
        settings.browser.profile_name = "lingospeak".to_owned();
        save_surf_settings(&settings).unwrap();
        assert_eq!(default_host_profile_name(), "lingospeak");

        set_env("SURF_HOST_PROFILE", Some("prod-main"));
        assert_eq!(default_host_profile_name(), "prod-main");

        set_env(
            "SURF_SETTINGS_FILE",
            Some(
                PathBuf::from(home.path())
                    .join("missing/settings.toml")
                    .display()
                    .to_string()
                    .as_str(),
            ),
        );
        set_env("SURF_HOST_PROFILE", None);
        assert_eq!(default_host_profile_name(), "default");
    }
}
