use std::path::PathBuf;

use crate::constants::{
    DEFAULT_CONTAINER, DEFAULT_HOST_BIND, DEFAULT_HOST_MCP_PORT, DEFAULT_HOST_NOVNC_PORT,
    DEFAULT_IMAGE, DEFAULT_MCP_PORT, DEFAULT_MCP_VERSION, DEFAULT_NETWORK, DEFAULT_NOVNC_PORT,
    DEFAULT_PROFILE_NAME, PROFILE_VOLUME_PREFIX,
};
use crate::paths::{container_profile_dir, env_trimmed, expand_tilde, sanitize_profile_name};
use crate::settings::{SurfSettings, load_surf_settings_or_default, surf_state_dir};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserConfig {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileMount {
    pub mount_arg: String,
    pub host_path: Option<String>,
    pub bind_mount: bool,
}

pub fn default_config() -> BrowserConfig {
    let settings = load_surf_settings_or_default();
    from_settings(&settings)
}

pub fn from_settings(settings: &SurfSettings) -> BrowserConfig {
    let profile = env_or(
        "SURF_PROFILE",
        first_non_empty([settings.browser.profile_name.trim(), DEFAULT_PROFILE_NAME]),
    );
    let profile = if profile.trim().is_empty() {
        DEFAULT_PROFILE_NAME.to_owned()
    } else {
        profile
    };

    let vnc_password = env_trimmed("SURF_VNC_PASSWORD")
        .unwrap_or_else(|| settings.browser.vnc_password.trim().to_owned());
    let vnc_password = if vnc_password.trim().is_empty() {
        "surf".to_owned()
    } else {
        vnc_password
    };

    let profile_dir = env_or("SURF_PROFILE_DIR", settings.browser.profile_dir.trim());
    let profile_dir = if profile_dir.trim().is_empty() {
        container_profile_dir(&profile, &surf_state_dir())
            .display()
            .to_string()
    } else {
        profile_dir
    };

    BrowserConfig {
        image_name: env_or(
            "SURF_IMAGE",
            first_non_empty([settings.browser.image_name.trim(), DEFAULT_IMAGE]),
        ),
        container_name: env_or(
            "SURF_CONTAINER",
            first_non_empty([settings.browser.container_name.trim(), DEFAULT_CONTAINER]),
        ),
        network: env_or(
            "SURF_NETWORK",
            first_non_empty([settings.browser.network.trim(), DEFAULT_NETWORK]),
        ),
        profile_name: profile,
        profile_dir,
        host_bind: env_or(
            "SURF_HOST_BIND",
            first_non_empty([settings.browser.host_bind.trim(), DEFAULT_HOST_BIND]),
        ),
        host_mcp_port: env_or_int(
            "SURF_HOST_MCP_PORT",
            int_or_fallback(settings.browser.host_mcp_port, DEFAULT_HOST_MCP_PORT),
        ),
        host_novnc_port: env_or_int(
            "SURF_HOST_NOVNC_PORT",
            int_or_fallback(settings.browser.host_novnc_port, DEFAULT_HOST_NOVNC_PORT),
        ),
        mcp_port: env_or_int(
            "SURF_MCP_PORT",
            int_or_fallback(settings.browser.mcp_port, DEFAULT_MCP_PORT),
        ),
        novnc_port: env_or_int(
            "SURF_NOVNC_PORT",
            int_or_fallback(settings.browser.novnc_port, DEFAULT_NOVNC_PORT),
        ),
        vnc_password,
        mcp_version: env_or(
            "SURF_MCP_VERSION",
            first_non_empty([settings.browser.mcp_version.trim(), DEFAULT_MCP_VERSION]),
        ),
        browser_channel: env_or(
            "SURF_BROWSER_CHANNEL",
            first_non_empty([settings.browser.browser_channel.trim(), "chromium"]),
        ),
        allowed_hosts: env_or(
            "SURF_ALLOWED_HOSTS",
            first_non_empty([settings.browser.allowed_hosts.trim(), "*"]),
        ),
    }
}

pub fn host_connect(bind: &str) -> String {
    let bind = bind.trim();
    if bind.is_empty() || bind == "0.0.0.0" {
        "127.0.0.1".to_owned()
    } else {
        bind.to_owned()
    }
}

pub fn mcp_url(cfg: &BrowserConfig) -> String {
    format!(
        "http://{}:{}/mcp",
        host_connect(&cfg.host_bind),
        cfg.host_mcp_port
    )
}

pub fn novnc_url(cfg: &BrowserConfig) -> String {
    format!(
        "http://{}:{}/vnc.html?autoconnect=1&resize=scale",
        host_connect(&cfg.host_bind),
        cfg.host_novnc_port
    )
}

pub fn resolve_profile_mount(profile_dir: &str) -> ProfileMount {
    let resolved = profile_dir.trim();
    if resolved.to_lowercase().starts_with(PROFILE_VOLUME_PREFIX) {
        let volume = sanitize_profile_name(resolved[PROFILE_VOLUME_PREFIX.len()..].trim());
        return ProfileMount {
            mount_arg: format!("surf-profile-{volume}:/home/pwuser/.playwright-mcp-profile"),
            host_path: None,
            bind_mount: false,
        };
    }

    let cleaned: PathBuf = expand_tilde(resolved).components().collect();
    ProfileMount {
        mount_arg: format!("{}:/home/pwuser/.playwright-mcp-profile", cleaned.display()),
        host_path: Some(cleaned.display().to_string()),
        bind_mount: true,
    }
}

pub fn apply_container_profile_default(profile_dir_flag_passed: bool, cfg: &mut BrowserConfig) {
    cfg.profile_name = sanitize_profile_name(&cfg.profile_name);
    if !profile_dir_flag_passed && cfg.profile_dir.trim().is_empty() {
        cfg.profile_dir = container_profile_dir(&cfg.profile_name, &surf_state_dir())
            .display()
            .to_string();
    }
}

pub fn default_host_profile_name() -> String {
    if let Some(profile) = env_trimmed("SURF_HOST_PROFILE") {
        return sanitize_profile_name(&profile);
    }
    let settings = load_surf_settings_or_default();
    if !settings.browser.profile_name.trim().is_empty() {
        return sanitize_profile_name(&settings.browser.profile_name);
    }
    DEFAULT_PROFILE_NAME.to_owned()
}

fn env_or(key: &str, fallback: impl AsRef<str>) -> String {
    env_trimmed(key).unwrap_or_else(|| fallback.as_ref().trim().to_owned())
}

fn env_or_int(key: &str, fallback: i32) -> i32 {
    env_trimmed(key)
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

fn int_or_fallback(value: i32, fallback: i32) -> i32 {
    if value <= 0 { fallback } else { value }
}

fn first_non_empty<'a>(values: impl IntoIterator<Item = &'a str>) -> String {
    values
        .into_iter()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_owned()
}
