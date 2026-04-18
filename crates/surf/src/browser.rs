use std::path::PathBuf;

use rand::Rng;
use serde::Serialize;

use crate::constants::{
    DEFAULT_CONTAINER, DEFAULT_HOST_BIND, DEFAULT_HOST_MCP_PORT, DEFAULT_HOST_NOVNC_PORT,
    DEFAULT_IMAGE, DEFAULT_MCP_PORT, DEFAULT_MCP_VERSION, DEFAULT_NETWORK, DEFAULT_NOVNC_PORT,
    DEFAULT_PROFILE_NAME, PROFILE_VOLUME_PREFIX,
};
use crate::paths::{container_profile_dir, env_trimmed, expand_tilde, sanitize_profile_name};
use crate::settings::{SurfSettings, load_surf_settings_or_default, surf_state_dir};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
    #[serde(skip_serializing)]
    pub vnc_password: String,
    pub vnc_password_generated: bool,
    pub mcp_version: String,
    pub browser_channel: String,
    pub allowed_hosts: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

    let (vnc_password, vnc_password_generated) = resolve_vnc_password(
        env_trimmed("SURF_VNC_PASSWORD"),
        settings.browser.vnc_password.trim(),
    );

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
        vnc_password_generated,
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

pub fn resolve_vnc_password(
    explicit_password: Option<String>,
    configured_password: &str,
) -> (String, bool) {
    if let Some(password) = explicit_password
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        return (password, false);
    }
    let configured_password = configured_password.trim();
    if configured_password.is_empty() || is_legacy_placeholder_password(configured_password) {
        return (generate_secure_vnc_password(24), true);
    }
    (configured_password.to_owned(), false)
}

pub fn generate_secure_vnc_password(len: usize) -> String {
    let mut rng = rand::rng();
    (&mut rng)
        .sample_iter(rand::distr::Alphanumeric)
        .take(len.max(16))
        .map(char::from)
        .collect()
}

pub fn is_legacy_placeholder_password(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case("surf")
}

pub fn viewer_password_warnings(password: &str, generated: bool) -> Vec<String> {
    let mut warnings = Vec::new();
    if generated {
        warnings.push(
            "viewer password was generated at startup because Surf had no explicit VNC password configured"
                .to_owned(),
        );
        return warnings;
    }
    if is_legacy_placeholder_password(password) {
        warnings.push(
            "viewer password is using the legacy insecure `surf` value; set a strong password before public sharing"
                .to_owned(),
        );
    } else if password.chars().count() < 12 {
        warnings.push(
            "viewer password is shorter than 12 characters; use a longer secret for shared/public sessions"
                .to_owned(),
        );
    }
    warnings
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

#[cfg(test)]
mod tests {
    use super::{
        generate_secure_vnc_password, is_legacy_placeholder_password, resolve_vnc_password,
        viewer_password_warnings,
    };

    #[test]
    fn resolve_vnc_password_generates_for_empty_or_legacy_placeholder() {
        let (empty_password, empty_generated) = resolve_vnc_password(None, "");
        assert!(empty_generated);
        assert!(empty_password.len() >= 16);
        assert_ne!(empty_password, "surf");

        let (legacy_password, legacy_generated) = resolve_vnc_password(None, "surf");
        assert!(legacy_generated);
        assert!(legacy_password.len() >= 16);
        assert_ne!(legacy_password, "surf");
    }

    #[test]
    fn resolve_vnc_password_keeps_explicit_passwords() {
        let (configured_password, configured_generated) =
            resolve_vnc_password(None, "topsecretvalue");
        assert!(!configured_generated);
        assert_eq!(configured_password, "topsecretvalue");

        let (explicit_password, explicit_generated) =
            resolve_vnc_password(Some("surf".to_owned()), "ignored");
        assert!(!explicit_generated);
        assert_eq!(explicit_password, "surf");
    }

    #[test]
    fn viewer_password_warnings_flag_weak_or_generated_passwords() {
        assert_eq!(viewer_password_warnings("ignored", true).len(), 1);
        assert_eq!(viewer_password_warnings("surf", false).len(), 1);
        assert_eq!(viewer_password_warnings("shortpass", false).len(), 1);
        assert!(viewer_password_warnings("avery-strong-password", false).is_empty());
        assert!(is_legacy_placeholder_password("surf"));
        assert!(generate_secure_vnc_password(24).len() >= 16);
    }
}
