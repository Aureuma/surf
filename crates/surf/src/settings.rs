use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::constants::{
    DEFAULT_CLOUDFLARED_IMAGE, DEFAULT_CONTAINER, DEFAULT_HOST_BIND, DEFAULT_HOST_CDP_PORT,
    DEFAULT_HOST_MCP_PORT, DEFAULT_HOST_NOVNC_PORT, DEFAULT_IMAGE, DEFAULT_MCP_PORT,
    DEFAULT_MCP_VERSION, DEFAULT_NETWORK, DEFAULT_NOVNC_PORT, DEFAULT_PROFILE_NAME,
    DEFAULT_SURF_CONFIG_FILE, DEFAULT_SURF_CONFIG_ROOT, DEFAULT_SURF_STATE_DIR_PATH,
    DEFAULT_TUNNEL_NAME, SESSION_MODE_INTERACTIVE, SESSION_MODE_READ_ONLY,
    SURF_SETTINGS_SCHEMA_VERSION,
};
use crate::paths::{default_state_path, env_trimmed, sanitize_profile_name, surf_settings_path};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SurfSettings {
    pub schema_version: i32,
    pub paths: SurfSettingsPaths,
    pub browser: SurfBrowserSettings,
    pub existing_session: SurfExistingSessionSettings,
    pub tunnel: SurfTunnelSettings,
    pub metadata: SurfSettingsMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct SurfSettingsMetadata {
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SurfSettingsPaths {
    pub root: String,
    pub settings_file: String,
    pub state_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SurfBrowserSettings {
    pub image_name: String,
    pub container_name: String,
    pub network: String,
    pub profile_name: String,
    pub profile_dir: String,
    pub host_bind: String,
    pub host_mcp_port: i32,
    pub host_novnc_port: i32,
    pub mcp_port: i32,
    pub novnc_port: i32,
    pub vnc_password: String,
    pub mcp_version: String,
    pub browser_channel: String,
    pub allowed_hosts: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SurfTunnelSettings {
    pub container_name: String,
    pub target_url: String,
    pub mode: String,
    pub image: String,
    #[serde(alias = "vault_key")]
    pub fort_key: String,
    pub fort_repo: String,
    pub fort_env: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SurfExistingSessionSettings {
    pub enabled: bool,
    pub default_browser: String,
    pub mode: String,
    pub chrome_host: String,
    pub chrome_cdp_port: i32,
    pub attach_timeout_seconds: i32,
    pub action_timeout_seconds: i32,
    pub humanize: bool,
    pub human_min_delay_ms: i32,
    pub human_max_delay_ms: i32,
    pub human_mouse_steps: i32,
    pub human_type_min_delay_ms: i32,
    pub human_type_max_delay_ms: i32,
    pub human_scroll_step_px: i32,
    pub allowed_domains: Vec<String>,
    pub blocked_domains: Vec<String>,
}

impl Default for SurfSettings {
    fn default() -> Self {
        Self {
            schema_version: SURF_SETTINGS_SCHEMA_VERSION,
            paths: SurfSettingsPaths::default(),
            browser: SurfBrowserSettings::default(),
            existing_session: SurfExistingSessionSettings::default(),
            tunnel: SurfTunnelSettings::default(),
            metadata: SurfSettingsMetadata::default(),
        }
    }
}

impl Default for SurfSettingsPaths {
    fn default() -> Self {
        Self {
            root: DEFAULT_SURF_CONFIG_ROOT.to_owned(),
            settings_file: DEFAULT_SURF_CONFIG_FILE.to_owned(),
            state_dir: DEFAULT_SURF_STATE_DIR_PATH.to_owned(),
        }
    }
}

impl Default for SurfBrowserSettings {
    fn default() -> Self {
        Self {
            image_name: DEFAULT_IMAGE.to_owned(),
            container_name: DEFAULT_CONTAINER.to_owned(),
            network: DEFAULT_NETWORK.to_owned(),
            profile_name: DEFAULT_PROFILE_NAME.to_owned(),
            profile_dir: String::new(),
            host_bind: DEFAULT_HOST_BIND.to_owned(),
            host_mcp_port: DEFAULT_HOST_MCP_PORT,
            host_novnc_port: DEFAULT_HOST_NOVNC_PORT,
            mcp_port: DEFAULT_MCP_PORT,
            novnc_port: DEFAULT_NOVNC_PORT,
            vnc_password: "surf".to_owned(),
            mcp_version: DEFAULT_MCP_VERSION.to_owned(),
            browser_channel: "chromium".to_owned(),
            allowed_hosts: "*".to_owned(),
        }
    }
}

impl Default for SurfTunnelSettings {
    fn default() -> Self {
        Self {
            container_name: DEFAULT_TUNNEL_NAME.to_owned(),
            target_url: String::new(),
            mode: "quick".to_owned(),
            image: DEFAULT_CLOUDFLARED_IMAGE.to_owned(),
            fort_key: String::new(),
            fort_repo: String::new(),
            fort_env: String::new(),
        }
    }
}

impl Default for SurfExistingSessionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            default_browser: "chrome".to_owned(),
            mode: SESSION_MODE_READ_ONLY.to_owned(),
            chrome_host: "127.0.0.1".to_owned(),
            chrome_cdp_port: DEFAULT_HOST_CDP_PORT,
            attach_timeout_seconds: 8,
            action_timeout_seconds: 15,
            humanize: true,
            human_min_delay_ms: 40,
            human_max_delay_ms: 180,
            human_mouse_steps: 12,
            human_type_min_delay_ms: 35,
            human_type_max_delay_ms: 130,
            human_scroll_step_px: 280,
            allowed_domains: vec!["*".to_owned()],
            blocked_domains: vec![],
        }
    }
}

pub fn default_surf_settings() -> SurfSettings {
    SurfSettings::default()
}

pub fn apply_surf_settings_defaults(settings: &mut SurfSettings) {
    if settings.schema_version <= 0 {
        settings.schema_version = SURF_SETTINGS_SCHEMA_VERSION;
    }

    if settings.paths.root.trim().is_empty() {
        settings.paths.root = DEFAULT_SURF_CONFIG_ROOT.to_owned();
    }
    if settings.paths.settings_file.trim().is_empty() {
        settings.paths.settings_file = DEFAULT_SURF_CONFIG_FILE.to_owned();
    }
    if settings.paths.state_dir.trim().is_empty() {
        settings.paths.state_dir = DEFAULT_SURF_STATE_DIR_PATH.to_owned();
    }

    if settings.browser.image_name.trim().is_empty() {
        settings.browser.image_name = DEFAULT_IMAGE.to_owned();
    }
    if settings.browser.container_name.trim().is_empty() {
        settings.browser.container_name = DEFAULT_CONTAINER.to_owned();
    }
    if settings.browser.network.trim().is_empty() {
        settings.browser.network = DEFAULT_NETWORK.to_owned();
    }
    if settings.browser.profile_name.trim().is_empty() {
        settings.browser.profile_name = DEFAULT_PROFILE_NAME.to_owned();
    }
    settings.browser.profile_name = sanitize_profile_name(&settings.browser.profile_name);
    if settings.browser.host_bind.trim().is_empty() {
        settings.browser.host_bind = DEFAULT_HOST_BIND.to_owned();
    }
    if settings.browser.host_mcp_port <= 0 {
        settings.browser.host_mcp_port = DEFAULT_HOST_MCP_PORT;
    }
    if settings.browser.host_novnc_port <= 0 {
        settings.browser.host_novnc_port = DEFAULT_HOST_NOVNC_PORT;
    }
    if settings.browser.mcp_port <= 0 {
        settings.browser.mcp_port = DEFAULT_MCP_PORT;
    }
    if settings.browser.novnc_port <= 0 {
        settings.browser.novnc_port = DEFAULT_NOVNC_PORT;
    }
    if settings.browser.vnc_password.trim().is_empty() {
        settings.browser.vnc_password = "surf".to_owned();
    }
    if settings.browser.mcp_version.trim().is_empty() {
        settings.browser.mcp_version = DEFAULT_MCP_VERSION.to_owned();
    }
    if settings.browser.browser_channel.trim().is_empty() {
        settings.browser.browser_channel = "chromium".to_owned();
    }
    if settings.browser.allowed_hosts.trim().is_empty() {
        settings.browser.allowed_hosts = "*".to_owned();
    }

    let browser = settings
        .existing_session
        .default_browser
        .trim()
        .to_lowercase();
    settings.existing_session.default_browser = match browser.as_str() {
        "chrome" | "safari" => browser,
        _ => "chrome".to_owned(),
    };

    let mode = settings.existing_session.mode.trim().to_lowercase();
    settings.existing_session.mode = match mode.as_str() {
        SESSION_MODE_READ_ONLY | SESSION_MODE_INTERACTIVE => mode,
        _ => SESSION_MODE_READ_ONLY.to_owned(),
    };

    if settings.existing_session.chrome_host.trim().is_empty() {
        settings.existing_session.chrome_host = "127.0.0.1".to_owned();
    }
    if settings.existing_session.chrome_cdp_port <= 0 {
        settings.existing_session.chrome_cdp_port = DEFAULT_HOST_CDP_PORT;
    }
    if settings.existing_session.attach_timeout_seconds <= 0 {
        settings.existing_session.attach_timeout_seconds = 8;
    }
    if settings.existing_session.action_timeout_seconds <= 0 {
        settings.existing_session.action_timeout_seconds = 15;
    }
    if settings.existing_session.human_min_delay_ms <= 0 {
        settings.existing_session.human_min_delay_ms = 40;
    }
    if settings.existing_session.human_max_delay_ms <= 0 {
        settings.existing_session.human_max_delay_ms = 180;
    }
    if settings.existing_session.human_max_delay_ms < settings.existing_session.human_min_delay_ms {
        settings.existing_session.human_max_delay_ms = settings.existing_session.human_min_delay_ms;
    }
    if settings.existing_session.human_mouse_steps <= 0 {
        settings.existing_session.human_mouse_steps = 12;
    }
    if settings.existing_session.human_type_min_delay_ms <= 0 {
        settings.existing_session.human_type_min_delay_ms = 35;
    }
    if settings.existing_session.human_type_max_delay_ms <= 0 {
        settings.existing_session.human_type_max_delay_ms = 130;
    }
    if settings.existing_session.human_type_max_delay_ms
        < settings.existing_session.human_type_min_delay_ms
    {
        settings.existing_session.human_type_max_delay_ms =
            settings.existing_session.human_type_min_delay_ms;
    }
    if settings.existing_session.human_scroll_step_px <= 0 {
        settings.existing_session.human_scroll_step_px = 280;
    }
    if settings.existing_session.allowed_domains.is_empty() {
        settings.existing_session.allowed_domains = vec!["*".to_owned()];
    }
    settings.existing_session.allowed_domains =
        normalize_domain_list(&settings.existing_session.allowed_domains);
    settings.existing_session.blocked_domains =
        normalize_domain_list(&settings.existing_session.blocked_domains);

    if settings.tunnel.container_name.trim().is_empty() {
        settings.tunnel.container_name = DEFAULT_TUNNEL_NAME.to_owned();
    }
    let tunnel_mode = settings.tunnel.mode.trim().to_lowercase();
    settings.tunnel.mode = match tunnel_mode.as_str() {
        "quick" | "token" => tunnel_mode,
        _ => "quick".to_owned(),
    };
    if settings.tunnel.image.trim().is_empty() {
        settings.tunnel.image = DEFAULT_CLOUDFLARED_IMAGE.to_owned();
    }
    if settings.tunnel.fort_key.trim().is_empty() {
        settings.tunnel.fort_key.clear();
    }
    if settings.tunnel.fort_repo.trim().is_empty() {
        settings.tunnel.fort_repo.clear();
    }
    if settings.tunnel.fort_env.trim().is_empty() {
        settings.tunnel.fort_env.clear();
    }
}

pub fn load_surf_settings() -> Result<SurfSettings> {
    let path = surf_settings_path();
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("settings path has no parent: {}", path.display()))?;
    fs::create_dir_all(dir)
        .with_context(|| format!("create settings directory {}", dir.display()))?;

    match fs::read_to_string(&path) {
        Ok(data) => {
            let mut settings: SurfSettings = toml::from_str(&data)
                .with_context(|| format!("parse surf settings {}", path.display()))?;
            apply_surf_settings_defaults(&mut settings);
            save_surf_settings(&settings)?;
            Ok(settings)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let settings = default_surf_settings();
            save_surf_settings(&settings)?;
            Ok(settings)
        }
        Err(err) => Err(err).with_context(|| format!("read surf settings {}", path.display())),
    }
}

pub fn load_surf_settings_or_default() -> SurfSettings {
    load_surf_settings().unwrap_or_else(|_| {
        let mut fallback = default_surf_settings();
        apply_surf_settings_defaults(&mut fallback);
        fallback
    })
}

pub fn save_surf_settings(settings: &SurfSettings) -> Result<()> {
    let path = surf_settings_path();
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("settings path has no parent: {}", path.display()))?;
    fs::create_dir_all(dir)
        .with_context(|| format!("create settings directory {}", dir.display()))?;

    let mut rendered = settings.clone();
    apply_surf_settings_defaults(&mut rendered);
    rendered.paths.root = DEFAULT_SURF_CONFIG_ROOT.to_owned();
    rendered.paths.settings_file = DEFAULT_SURF_CONFIG_FILE.to_owned();
    rendered.metadata.updated_at = iso_timestamp();

    let data = toml::to_string_pretty(&rendered).context("serialize surf settings")?;
    let mut temp = NamedTempFile::new_in(dir)
        .with_context(|| format!("create temp settings file in {}", dir.display()))?;
    temp.as_file_mut()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .context("set temporary settings permissions")?;
    temp.write_all(data.as_bytes())
        .context("write temporary settings file")?;
    temp.as_file_mut()
        .sync_all()
        .context("sync temporary settings file")?;
    let temp_path = temp.into_temp_path();
    fs::rename(&temp_path, &path).with_context(|| {
        format!(
            "replace settings file {} from {}",
            path.display(),
            temp_path.display()
        )
    })?;
    Ok(())
}

pub fn set_surf_config_value(settings: &mut SurfSettings, key: &str, value: &str) -> Result<()> {
    let resolved_key = key.trim().to_lowercase();
    let resolved_value = value.trim();
    match resolved_key.as_str() {
        "paths.state_dir" => settings.paths.state_dir = resolved_value.to_owned(),
        "browser.image_name" => settings.browser.image_name = resolved_value.to_owned(),
        "browser.container_name" => settings.browser.container_name = resolved_value.to_owned(),
        "browser.network" => settings.browser.network = resolved_value.to_owned(),
        "browser.profile_name" => {
            settings.browser.profile_name = sanitize_profile_name(resolved_value)
        }
        "browser.profile_dir" => settings.browser.profile_dir = resolved_value.to_owned(),
        "browser.host_bind" => settings.browser.host_bind = resolved_value.to_owned(),
        "browser.host_mcp_port" => {
            settings.browser.host_mcp_port = parse_i32(&resolved_key, resolved_value)?;
        }
        "browser.host_novnc_port" => {
            settings.browser.host_novnc_port = parse_i32(&resolved_key, resolved_value)?;
        }
        "browser.mcp_port" => {
            settings.browser.mcp_port = parse_i32(&resolved_key, resolved_value)?;
        }
        "browser.novnc_port" => {
            settings.browser.novnc_port = parse_i32(&resolved_key, resolved_value)?;
        }
        "browser.vnc_password" => settings.browser.vnc_password = resolved_value.to_owned(),
        "browser.mcp_version" => settings.browser.mcp_version = resolved_value.to_owned(),
        "browser.browser_channel" => settings.browser.browser_channel = resolved_value.to_owned(),
        "browser.allowed_hosts" => settings.browser.allowed_hosts = resolved_value.to_owned(),
        "tunnel.container_name" => settings.tunnel.container_name = resolved_value.to_owned(),
        "tunnel.target_url" => settings.tunnel.target_url = resolved_value.to_owned(),
        "tunnel.mode" => {
            let mode = resolved_value.to_lowercase();
            if mode != "quick" && mode != "token" {
                bail!("invalid mode {resolved_value:?} (expected quick|token)");
            }
            settings.tunnel.mode = mode;
        }
        "tunnel.image" => settings.tunnel.image = resolved_value.to_owned(),
        "tunnel.fort_key" | "tunnel.vault_key" => {
            settings.tunnel.fort_key = resolved_value.to_owned()
        }
        "tunnel.fort_repo" => settings.tunnel.fort_repo = resolved_value.to_owned(),
        "tunnel.fort_env" => settings.tunnel.fort_env = resolved_value.to_owned(),
        "existing_session.enabled" => {
            settings.existing_session.enabled = parse_bool(&resolved_key, resolved_value)?;
        }
        "existing_session.default_browser" => {
            let browser = resolved_value.to_lowercase();
            if browser != "chrome" && browser != "safari" {
                bail!("invalid browser {resolved_value:?} (expected chrome|safari)");
            }
            settings.existing_session.default_browser = browser;
        }
        "existing_session.mode" => {
            let mode = resolved_value.to_lowercase();
            if mode != SESSION_MODE_READ_ONLY && mode != SESSION_MODE_INTERACTIVE {
                bail!("invalid mode {resolved_value:?} (expected read_only|interactive)");
            }
            settings.existing_session.mode = mode;
        }
        "existing_session.chrome_host" => {
            settings.existing_session.chrome_host = resolved_value.to_owned()
        }
        "existing_session.chrome_cdp_port" => {
            settings.existing_session.chrome_cdp_port = parse_i32(&resolved_key, resolved_value)?;
        }
        "existing_session.attach_timeout_seconds" => {
            settings.existing_session.attach_timeout_seconds =
                parse_i32(&resolved_key, resolved_value)?;
        }
        "existing_session.action_timeout_seconds" => {
            settings.existing_session.action_timeout_seconds =
                parse_i32(&resolved_key, resolved_value)?;
        }
        "existing_session.humanize" => {
            settings.existing_session.humanize = parse_bool(&resolved_key, resolved_value)?;
        }
        "existing_session.human_min_delay_ms" => {
            settings.existing_session.human_min_delay_ms =
                parse_i32(&resolved_key, resolved_value)?;
        }
        "existing_session.human_max_delay_ms" => {
            settings.existing_session.human_max_delay_ms =
                parse_i32(&resolved_key, resolved_value)?;
        }
        "existing_session.human_mouse_steps" => {
            settings.existing_session.human_mouse_steps = parse_i32(&resolved_key, resolved_value)?;
        }
        "existing_session.human_type_min_delay_ms" => {
            settings.existing_session.human_type_min_delay_ms =
                parse_i32(&resolved_key, resolved_value)?;
        }
        "existing_session.human_type_max_delay_ms" => {
            settings.existing_session.human_type_max_delay_ms =
                parse_i32(&resolved_key, resolved_value)?;
        }
        "existing_session.human_scroll_step_px" => {
            settings.existing_session.human_scroll_step_px =
                parse_i32(&resolved_key, resolved_value)?;
        }
        "existing_session.allowed_domains" => {
            settings.existing_session.allowed_domains = split_csv_list(resolved_value);
        }
        "existing_session.blocked_domains" => {
            settings.existing_session.blocked_domains = split_csv_list(resolved_value);
        }
        _ => bail!("unsupported key: {key}"),
    }
    apply_surf_settings_defaults(settings);
    Ok(())
}

pub fn split_csv_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub fn normalize_domain_list(domains: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(domains.len());
    for domain in domains {
        let item = domain.trim().to_lowercase();
        if item.is_empty() || out.contains(&item) {
            continue;
        }
        out.push(item);
    }
    out
}

pub fn surf_state_dir() -> PathBuf {
    if let Some(value) = env_trimmed("SURF_STATE_DIR") {
        return crate::paths::expand_tilde(&value)
            .components()
            .collect::<PathBuf>();
    }
    let settings = load_surf_settings_or_default();
    if !settings.paths.state_dir.trim().is_empty() {
        return crate::paths::expand_tilde(&settings.paths.state_dir)
            .components()
            .collect::<PathBuf>();
    }
    default_state_path()
}

fn parse_i32(key: &str, value: &str) -> Result<i32> {
    value
        .parse::<i32>()
        .with_context(|| format!("invalid int for {key}"))
}

fn parse_bool(key: &str, value: &str) -> Result<bool> {
    value
        .parse::<bool>()
        .with_context(|| format!("invalid bool for {key}"))
}

fn iso_timestamp() -> String {
    let output = std::process::Command::new("date")
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
    use std::fs;
    use std::path::{Path, PathBuf};

    use serial_test::serial;

    use super::{
        default_surf_settings, load_surf_settings, save_surf_settings, set_surf_config_value,
        surf_state_dir,
    };
    use crate::browser::{
        apply_container_profile_default, default_config, host_connect, mcp_url,
        resolve_profile_mount,
    };
    use crate::paths::{
        container_profile_dir, env_lock, host_profile_dir, set_env, surf_settings_path,
    };

    fn reset_env(home: &Path) {
        set_env("SURF_SETTINGS_HOME", Some(home.to_string_lossy().as_ref()));
        set_env("SURF_SETTINGS_FILE", None);
        set_env("SURF_SETTINGS_DIR", None);
        set_env("SURF_STATE_DIR", None);
        set_env("SURF_IMAGE", None);
        set_env("SURF_CONTAINER", None);
        set_env("SURF_NETWORK", None);
        set_env("SURF_PROFILE", None);
        set_env("SURF_PROFILE_DIR", None);
        set_env("SURF_HOST_BIND", None);
        set_env("SURF_HOST_MCP_PORT", None);
        set_env("SURF_HOST_NOVNC_PORT", None);
        set_env("SURF_MCP_PORT", None);
        set_env("SURF_NOVNC_PORT", None);
        set_env("SURF_VNC_PASSWORD", None);
        set_env("SURF_MCP_VERSION", None);
        set_env("SURF_BROWSER_CHANNEL", None);
        set_env("SURF_ALLOWED_HOSTS", None);
        set_env("SURF_HOST_PROFILE", None);
    }

    #[test]
    #[serial]
    fn load_surf_settings_creates_default_file() {
        let _guard = env_lock().lock().unwrap_or_else(|error| error.into_inner());
        let home = tempfile::tempdir().unwrap();
        reset_env(home.path());

        let settings = load_surf_settings().unwrap();
        assert_eq!(settings.schema_version, 1);

        let path = home.path().join(".si").join("surf").join("settings.toml");
        assert!(
            path.exists(),
            "expected settings file at {}",
            path.display()
        );
    }

    #[test]
    #[serial]
    fn set_surf_config_value_updates_supported_keys() {
        let _guard = env_lock().lock().unwrap_or_else(|error| error.into_inner());
        let mut settings = default_surf_settings();
        set_surf_config_value(&mut settings, "tunnel.mode", "token").unwrap();
        set_surf_config_value(&mut settings, "browser.host_mcp_port", "9999").unwrap();
        set_surf_config_value(&mut settings, "existing_session.mode", "interactive").unwrap();
        set_surf_config_value(&mut settings, "existing_session.default_browser", "safari").unwrap();
        set_surf_config_value(&mut settings, "existing_session.chrome_cdp_port", "17777").unwrap();
        set_surf_config_value(&mut settings, "existing_session.humanize", "true").unwrap();
        set_surf_config_value(&mut settings, "existing_session.human_mouse_steps", "16").unwrap();
        set_surf_config_value(
            &mut settings,
            "existing_session.allowed_domains",
            "example.com,*.example.org",
        )
        .unwrap();
        set_surf_config_value(
            &mut settings,
            "existing_session.blocked_domains",
            "admin.example.com",
        )
        .unwrap();

        assert_eq!(settings.tunnel.mode, "token");
        assert_eq!(settings.browser.host_mcp_port, 9999);
        assert_eq!(settings.existing_session.mode, "interactive");
        assert_eq!(settings.existing_session.default_browser, "safari");
        assert_eq!(settings.existing_session.chrome_cdp_port, 17777);
        assert!(settings.existing_session.humanize);
        assert_eq!(settings.existing_session.human_mouse_steps, 16);
        assert_eq!(settings.existing_session.allowed_domains.len(), 2);
        assert_eq!(settings.existing_session.blocked_domains.len(), 1);
        assert!(set_surf_config_value(&mut settings, "tunnel.mode", "bad").is_err());
    }

    #[test]
    #[serial]
    fn default_config_uses_surf_settings() {
        let _guard = env_lock().lock().unwrap_or_else(|error| error.into_inner());
        let home = tempfile::tempdir().unwrap();
        reset_env(home.path());

        let mut settings = default_surf_settings();
        settings.browser.image_name = "test/surf:1".to_owned();
        settings.browser.container_name = "surf-test".to_owned();
        settings.browser.network = "test-net".to_owned();
        settings.browser.profile_name = "work".to_owned();
        settings.browser.host_mcp_port = 9999;
        settings.browser.host_novnc_port = 6090;
        settings.browser.mcp_port = 9900;
        settings.browser.novnc_port = 6091;
        settings.browser.vnc_password = "topsecret".to_owned();
        settings.browser.mcp_version = "9.9.9".to_owned();
        settings.browser.browser_channel = "chrome".to_owned();
        settings.browser.allowed_hosts = "example.com".to_owned();
        save_surf_settings(&settings).unwrap();

        let cfg = default_config();
        assert_eq!(cfg.image_name, "test/surf:1");
        assert_eq!(cfg.container_name, "surf-test");
        assert_eq!(cfg.network, "test-net");
        assert_eq!(cfg.profile_name, "work");
        assert_eq!(cfg.host_mcp_port, 9999);
        assert_eq!(cfg.host_novnc_port, 6090);
        assert_eq!(cfg.vnc_password, "topsecret");
        assert_eq!(cfg.mcp_version, "9.9.9");
    }

    #[test]
    #[serial]
    fn surf_state_dir_uses_settings() {
        let _guard = env_lock().lock().unwrap_or_else(|error| error.into_inner());
        let home = tempfile::tempdir().unwrap();
        reset_env(home.path());

        let mut settings = default_surf_settings();
        settings.paths.state_dir = home.path().join("custom-surf-state").display().to_string();
        save_surf_settings(&settings).unwrap();

        assert_eq!(surf_state_dir(), home.path().join("custom-surf-state"));
    }

    #[test]
    #[serial]
    fn tunnel_and_existing_session_round_trip() {
        let _guard = env_lock().lock().unwrap_or_else(|error| error.into_inner());
        let home = tempfile::tempdir().unwrap();
        reset_env(home.path());

        let mut want = default_surf_settings();
        want.tunnel.container_name = "surf-cloudflared-test".to_owned();
        want.tunnel.target_url =
            "http://127.0.0.1:6081/vnc.html?autoconnect=1&resize=scale".to_owned();
        want.tunnel.mode = "token".to_owned();
        want.tunnel.image = "cloudflare/cloudflared:2026.2.0".to_owned();
        want.tunnel.fort_key = "SURF_CLOUDFLARE_TUNNEL_TOKEN".to_owned();
        want.tunnel.fort_repo = "surf".to_owned();
        want.tunnel.fort_env = "dev".to_owned();
        want.existing_session.mode = "interactive".to_owned();
        want.existing_session.chrome_cdp_port = 19922;
        want.existing_session.allowed_domains =
            vec!["example.com".to_owned(), "*.example.org".to_owned()];
        want.existing_session.blocked_domains = vec!["admin.example.com".to_owned()];
        save_surf_settings(&want).unwrap();

        let got = load_surf_settings().unwrap();
        assert_eq!(got.tunnel.container_name, want.tunnel.container_name);
        assert_eq!(got.tunnel.target_url, want.tunnel.target_url);
        assert_eq!(got.tunnel.mode, want.tunnel.mode);
        assert_eq!(got.tunnel.image, want.tunnel.image);
        assert_eq!(got.tunnel.fort_key, want.tunnel.fort_key);
        assert_eq!(got.tunnel.fort_repo, want.tunnel.fort_repo);
        assert_eq!(got.tunnel.fort_env, want.tunnel.fort_env);
        assert_eq!(got.existing_session.mode, "interactive");
        assert_eq!(got.existing_session.chrome_cdp_port, 19922);
        assert_eq!(got.existing_session.allowed_domains.len(), 2);
        assert_eq!(got.existing_session.blocked_domains.len(), 1);
    }

    #[test]
    #[serial]
    fn helper_paths_match_go_behavior() {
        let _guard = env_lock().lock().unwrap_or_else(|error| error.into_inner());
        let home = tempfile::tempdir().unwrap();
        reset_env(home.path());
        set_env("SURF_STATE_DIR", Some("/tmp/surf-state"));

        assert_eq!(
            container_profile_dir("Work", &surf_state_dir()),
            PathBuf::from("/tmp/surf-state/browser/profiles/container/work")
        );
        assert_eq!(
            host_profile_dir("work", &surf_state_dir()),
            PathBuf::from("/tmp/surf-state/browser/profiles/host/work")
        );
        assert_eq!(host_connect(""), "127.0.0.1");
        assert_eq!(host_connect("0.0.0.0"), "127.0.0.1");
        assert_eq!(host_connect("192.168.1.20"), "192.168.1.20");
        assert_eq!(
            mcp_url(&default_config()),
            "http://127.0.0.1:8932/mcp".to_owned()
        );
    }

    #[test]
    #[serial]
    fn resolve_profile_mount_and_apply_default_match_go_behavior() {
        let _guard = env_lock().lock().unwrap_or_else(|error| error.into_inner());
        let home = tempfile::tempdir().unwrap();
        reset_env(home.path());
        set_env("HOME", Some(home.path().to_string_lossy().as_ref()));
        set_env("SURF_STATE_DIR", Some("/tmp/surf-state"));

        let bind = resolve_profile_mount("~/state/profile");
        assert!(bind.bind_mount);
        assert!(
            bind.host_path
                .as_deref()
                .unwrap()
                .ends_with("/state/profile")
        );
        assert!(
            bind.mount_arg
                .contains(":/home/pwuser/.playwright-mcp-profile")
        );

        let volume = resolve_profile_mount("volume:ls");
        assert!(!volume.bind_mount);
        assert!(volume.host_path.is_none());
        assert_eq!(
            volume.mount_arg,
            "surf-profile-ls:/home/pwuser/.playwright-mcp-profile"
        );

        let mut cfg = default_config();
        cfg.profile_name = "ls".to_owned();
        cfg.profile_dir = "volume:ls".to_owned();
        apply_container_profile_default(false, &mut cfg);
        assert_eq!(cfg.profile_dir, "volume:ls");

        let mut cfg = default_config();
        cfg.profile_name = "work".to_owned();
        cfg.profile_dir.clear();
        apply_container_profile_default(false, &mut cfg);
        assert_eq!(
            cfg.profile_dir,
            "/tmp/surf-state/browser/profiles/container/work".to_owned()
        );

        fs::metadata(surf_settings_path()).ok();
    }
}
