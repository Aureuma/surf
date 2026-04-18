use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::browser::{BrowserConfig, novnc_url};
use crate::runtime::{
    extract_try_cloudflare_url, fetch_container_logs, is_container_running, must_have_command,
    remove_docker_container, run_docker_output,
};
use crate::settings::load_surf_settings_or_default;

#[derive(Debug, Clone, Serialize)]
pub struct TunnelStatusPayload {
    pub ok: bool,
    pub container_name: String,
    pub running: bool,
    pub url: String,
    pub mode: String,
    pub error: String,
}

pub fn start_tunnel(
    cfg: &BrowserConfig,
    name: &str,
    target_url: Option<&str>,
    mode: Option<&str>,
    token: Option<&str>,
    fort_key: Option<&str>,
    fort_repo: Option<&str>,
    fort_env: Option<&str>,
    image: Option<&str>,
) -> Result<TunnelStatusPayload> {
    must_have_command("docker")?;
    let settings = load_surf_settings_or_default();
    let tunnel_cfg = settings.tunnel;
    let target = target_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            let value = tunnel_cfg.target_url.trim();
            (!value.is_empty()).then(|| value.to_owned())
        })
        .unwrap_or_else(|| novnc_url(cfg));
    let mode = mode
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase)
        .or_else(|| {
            let value = tunnel_cfg.mode.trim();
            (!value.is_empty()).then(|| value.to_lowercase())
        })
        .unwrap_or_else(|| "quick".to_owned());
    if mode != "quick" && mode != "token" {
        bail!("invalid --mode {mode:?} (expected quick|token)");
    }
    let image = image
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| tunnel_cfg.image);
    let fort_key = fort_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| tunnel_cfg.fort_key.trim().to_owned());
    let fort_repo = fort_repo
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| crate::paths::env_trimmed("SURF_TUNNEL_FORT_REPO"))
        .unwrap_or_else(|| tunnel_cfg.fort_repo.trim().to_owned());
    let fort_env = fort_env
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| crate::paths::env_trimmed("SURF_TUNNEL_FORT_ENV"))
        .unwrap_or_else(|| tunnel_cfg.fort_env.trim().to_owned());

    remove_docker_container(name)?;

    let mut run_args = vec![
        "run".to_owned(),
        "-d".to_owned(),
        "--name".to_owned(),
        name.trim().to_owned(),
        "--restart".to_owned(),
        "unless-stopped".to_owned(),
        image,
        "tunnel".to_owned(),
        "--no-autoupdate".to_owned(),
    ];
    if mode == "quick" {
        run_args.push("--url".to_owned());
        run_args.push(target);
    } else {
        let token = resolve_token(token, &fort_key, &fort_repo, &fort_env)?;
        if token.trim().is_empty() {
            bail!(
                "tunnel token mode requires token value (use --token, SURF_CLOUDFLARE_TUNNEL_TOKEN, or Fort-backed --fort-key/--fort-repo/--fort-env)"
            );
        }
        run_args.push("run".to_owned());
        run_args.push("--token".to_owned());
        run_args.push(token);
    }

    run_docker_output(&run_args)?;
    let url = extract_try_cloudflare_url(&fetch_container_logs(name, 200));
    Ok(TunnelStatusPayload {
        ok: true,
        container_name: name.trim().to_owned(),
        running: true,
        url,
        mode,
        error: String::new(),
    })
}

pub fn stop_tunnel(name: &str) -> Result<()> {
    must_have_command("docker")?;
    remove_docker_container(name)
}

pub fn tunnel_status(name: &str) -> Result<TunnelStatusPayload> {
    must_have_command("docker")?;
    let (running, _) = is_container_running(name)?;
    let logs = fetch_container_logs(name, 200);
    let url = extract_try_cloudflare_url(&logs);
    Ok(TunnelStatusPayload {
        ok: running,
        container_name: name.trim().to_owned(),
        running,
        url,
        mode: String::new(),
        error: if running {
            String::new()
        } else {
            "tunnel container not running".to_owned()
        },
    })
}

pub fn tunnel_logs(name: &str, tail: i32, follow: bool) -> Result<()> {
    must_have_command("docker")?;
    let mut args = vec!["logs".to_owned(), "--tail".to_owned(), tail.to_string()];
    if follow {
        args.push("-f".to_owned());
    }
    args.push(name.trim().to_owned());
    let status = Command::new("docker")
        .args(&args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("spawn docker logs for tunnel")?;
    if !status.success() {
        bail!("docker logs failed");
    }
    Ok(())
}

pub fn resolve_token(
    explicit: Option<&str>,
    fort_key: &str,
    fort_repo: &str,
    fort_env: &str,
) -> Result<String> {
    if let Some(token) = explicit.map(str::trim).filter(|value| !value.is_empty()) {
        return Ok(token.to_owned());
    }
    if let Some(token) = crate::paths::env_trimmed("SURF_CLOUDFLARE_TUNNEL_TOKEN") {
        return Ok(token);
    }
    let fort_key = fort_key.trim();
    if fort_key.is_empty() {
        return Ok(String::new());
    }
    if fort_repo.trim().is_empty() || fort_env.trim().is_empty() {
        bail!(
            "Fort-backed tunnel token requires both repo and env (use --fort-repo/--fort-env or SURF_TUNNEL_FORT_REPO/SURF_TUNNEL_FORT_ENV)"
        );
    }
    fort_get(fort_repo, fort_env, fort_key)
}

pub fn fort_get(repo: &str, env: &str, key: &str) -> Result<String> {
    crate::runtime::must_have_command("si")?;
    let output = Command::new("si")
        .args([
            "fort",
            "get",
            "--repo",
            repo.trim(),
            "--env",
            env.trim(),
            "--key",
            key.trim(),
        ])
        .output()
        .with_context(|| format!("spawn si fort get {key}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let message = if stderr.is_empty() {
            "si fort get failed".to_owned()
        } else {
            stderr
        };
        bail!("si fort get {key} failed: {message}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
