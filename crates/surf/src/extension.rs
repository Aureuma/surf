use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::runtime::{copy_dir, resolve_repo_root};
use crate::settings::surf_state_dir;

#[derive(Debug, Clone, Serialize)]
pub struct ExtensionDoctor {
    pub ok: bool,
    pub path: String,
    pub manifest: String,
}

pub fn default_extension_install_path() -> PathBuf {
    surf_state_dir().join("extensions").join("chrome-relay")
}

pub fn install_extension(repo: Option<&str>, dest: Option<&str>) -> Result<PathBuf> {
    let source_root = resolve_repo_root(repo)?;
    let source_dir = source_root.join("extensions").join("chrome");
    if !source_dir.join("manifest.json").exists() {
        bail!("extension template missing at {}", source_dir.display());
    }
    let destination = dest
        .map(crate::paths::expand_tilde)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(default_extension_install_path);
    if destination.exists() {
        fs::remove_dir_all(&destination)
            .with_context(|| format!("remove existing {}", destination.display()))?;
    }
    copy_dir(&source_dir, &destination)?;
    Ok(destination)
}

pub fn extension_path(repo: Option<&str>, source: bool) -> Result<PathBuf> {
    if source {
        return Ok(resolve_repo_root(repo)?.join("extensions").join("chrome"));
    }
    Ok(default_extension_install_path())
}

pub fn extension_doctor(path: Option<&str>) -> Result<ExtensionDoctor> {
    let path = path
        .map(crate::paths::expand_tilde)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(default_extension_install_path);
    let manifest = path.join("manifest.json");
    Ok(ExtensionDoctor {
        ok: manifest.exists(),
        path: path.display().to_string(),
        manifest: manifest.display().to_string(),
    })
}
