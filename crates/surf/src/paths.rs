use std::env;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::{Mutex, OnceLock};

use crate::constants::{
    DEFAULT_PROFILE_NAME, DEFAULT_SURF_CONFIG_ROOT, DEFAULT_SURF_STATE_DIR_PATH,
};

pub fn expand_tilde(raw: &str) -> PathBuf {
    let value = raw.trim();
    if value.is_empty() {
        return PathBuf::new();
    }
    if value == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from("~"));
    }
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(value)
}

pub fn sanitize_profile_name(raw: &str) -> String {
    let trimmed = raw.trim().to_lowercase();
    if trimmed.is_empty() {
        return DEFAULT_PROFILE_NAME.to_owned();
    }

    let mut out = String::with_capacity(trimmed.len());
    let mut last_dash = false;
    for ch in trimmed.chars() {
        let keep = ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-';
        if keep {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        DEFAULT_PROFILE_NAME.to_owned()
    } else {
        trimmed.to_owned()
    }
}

pub fn surf_settings_root_dir() -> PathBuf {
    if let Some(explicit) = env_trimmed("SURF_SETTINGS_DIR") {
        return clean_path(explicit);
    }
    if let Some(home_override) = env_trimmed("SURF_SETTINGS_HOME") {
        return clean_path(home_override).join(".si").join("surf");
    }
    if let Some(home) = home_dir() {
        return home.join(".si").join("surf");
    }
    clean_path(DEFAULT_SURF_CONFIG_ROOT)
}

pub fn surf_settings_path() -> PathBuf {
    if let Some(explicit) = env_trimmed("SURF_SETTINGS_FILE") {
        return clean_path(explicit);
    }
    surf_settings_root_dir().join("settings.toml")
}

pub fn fallback_surf_state_dir() -> PathBuf {
    if let Some(home) = home_dir() {
        return home.join(".surf");
    }
    PathBuf::from("/tmp/.surf")
}

pub fn default_state_path() -> PathBuf {
    clean_path(DEFAULT_SURF_STATE_DIR_PATH)
}

pub fn container_profile_dir(profile: &str, state_dir: &Path) -> PathBuf {
    state_dir
        .join("browser")
        .join("profiles")
        .join("container")
        .join(sanitize_profile_name(profile))
}

pub fn host_profile_dir(profile: &str, state_dir: &Path) -> PathBuf {
    state_dir
        .join("browser")
        .join("profiles")
        .join("host")
        .join(sanitize_profile_name(profile))
}

pub fn env_trimmed(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

fn home_dir() -> Option<PathBuf> {
    env_trimmed("HOME").map(PathBuf::from)
}

fn clean_path(input: impl AsRef<str>) -> PathBuf {
    let expanded = expand_tilde(input.as_ref());
    if expanded.as_os_str().is_empty() {
        PathBuf::new()
    } else {
        expanded.components().collect()
    }
}

#[cfg(test)]
static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(test)]
pub(crate) fn env_lock() -> &'static Mutex<()> {
    ENV_LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
pub(crate) fn set_env(key: &str, value: Option<&str>) {
    match value {
        Some(value) => unsafe { env::set_var(key, value) },
        None => unsafe { env::remove_var(key) },
    }
}
