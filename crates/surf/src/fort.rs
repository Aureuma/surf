use std::process::Command;

use anyhow::{Context, Result, bail};

const FORT_REF_PREFIX: &str = "fort:";
const FORT_REPO: &str = "surf";

pub fn is_fort_ref(value: &str) -> bool {
    value.trim().starts_with(FORT_REF_PREFIX)
}

pub fn fort_key(value: &str) -> Option<String> {
    let key = value.trim().strip_prefix(FORT_REF_PREFIX)?.trim();
    (!key.is_empty()).then(|| key.to_string())
}

pub fn fort_env_from(explicit: Option<String>) -> String {
    explicit
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "dev".to_string())
}

pub fn fort_env() -> String {
    fort_env_from(std::env::var("SURF_FORT_ENV").ok())
}

fn fort_bin() -> String {
    std::env::var("SURF_FORT_BIN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "fort".to_string())
}

pub fn resolve_secret_value(value: &str) -> Result<String> {
    resolve_secret_value_with_bin(value, &fort_bin(), &fort_env())
}

fn resolve_secret_value_with_bin(value: &str, bin: &str, env: &str) -> Result<String> {
    let Some(key) = fort_key(value) else {
        return Ok(value.to_string());
    };
    let output = Command::new(bin)
        .args([
            "--json", "get", "--repo", FORT_REPO, "--env", env, "--key", &key,
        ])
        .output()
        .with_context(|| format!("run fort get for {key}"))?;
    if !output.status.success() {
        bail!(
            "fort get for {key} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("parse fort get output for {key}"))?;
    parsed
        .get("value")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .ok_or_else(|| anyhow::anyhow!("fort get for {key} returned no value"))
}

#[cfg(test)]
mod tests {
    use super::resolve_secret_value_with_bin;
    use super::{fort_env_from, fort_key, is_fort_ref, resolve_secret_value};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn fort_ref_detection_matches_prefix_rule() {
        assert!(is_fort_ref("fort:SURF_VNC_PASSWORD"));
        assert!(is_fort_ref("  fort:SURF_VNC_PASSWORD  "));
        assert!(!is_fort_ref("topsecret"));
        assert!(!is_fort_ref(""));
        assert_eq!(
            fort_key("fort:SURF_VNC_PASSWORD"),
            Some("SURF_VNC_PASSWORD".to_string())
        );
        assert_eq!(fort_key("fort:  "), None);
        assert_eq!(fort_key("topsecret"), None);
    }

    #[test]
    fn fort_env_selection_prefers_explicit_value() {
        assert_eq!(fort_env_from(None), "dev");
        assert_eq!(fort_env_from(Some("  ".to_string())), "dev");
        assert_eq!(fort_env_from(Some("prod".to_string())), "prod");
    }

    #[test]
    fn literal_values_pass_through_unresolved() {
        assert_eq!(
            resolve_secret_value("topsecret").expect("literal"),
            "topsecret"
        );
        assert_eq!(resolve_secret_value("").expect("empty"), "");
    }

    #[test]
    fn fort_ref_resolves_through_stub_binary() {
        let dir = tempfile::tempdir().expect("tempdir");
        let stub = dir.path().join("fort");
        fs::write(
            &stub,
            "#!/usr/bin/env bash\necho '{\"value\":\"stub-secret\"}'\n",
        )
        .expect("write stub");
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).expect("chmod stub");
        assert_eq!(
            resolve_secret_value_with_bin(
                "fort:SURF_VNC_PASSWORD",
                &stub.display().to_string(),
                "dev"
            )
            .expect("resolve"),
            "stub-secret"
        );
    }
}
