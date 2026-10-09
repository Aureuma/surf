use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::path::Path;

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

#[derive(Debug, Clone, Default)]
pub struct TunnelStartOptions<'a> {
    pub name: &'a str,
    pub target_url: Option<&'a str>,
    pub mode: Option<&'a str>,
    pub token: Option<&'a str>,
    pub fort_key: Option<&'a str>,
    pub fort_env_file: Option<&'a str>,
    pub image: Option<&'a str>,
}

pub fn start_tunnel(
    cfg: &BrowserConfig,
    opts: &TunnelStartOptions<'_>,
) -> Result<TunnelStatusPayload> {
    must_have_command("docker")?;
    let name = opts.name.trim();
    let settings = load_surf_settings_or_default();
    let tunnel_cfg = settings.tunnel;
    let target = opts
        .target_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            let value = tunnel_cfg.target_url.trim();
            (!value.is_empty()).then(|| value.to_owned())
        })
        .unwrap_or_else(|| novnc_url(cfg));
    let mode = opts
        .mode
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
    let image = opts
        .image
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| tunnel_cfg.image);
    let fort_key = opts
        .fort_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| tunnel_cfg.fort_key.trim().to_owned());
    let fort_env_file = opts
        .fort_env_file
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| crate::paths::env_trimmed("SURF_TUNNEL_FORT_ENV_FILE"))
        .unwrap_or_else(|| tunnel_cfg.fort_env_file.trim().to_owned());

    remove_docker_container(name)?;

    let mut run_args = vec![
        "run".to_owned(),
        "-d".to_owned(),
        "--name".to_owned(),
        name.to_owned(),
        "--restart".to_owned(),
        "unless-stopped".to_owned(),
        "--add-host".to_owned(),
        "host.docker.internal:host-gateway".to_owned(),
        image,
        "tunnel".to_owned(),
        "--no-autoupdate".to_owned(),
    ];
    if mode == "quick" {
        run_args.push("--url".to_owned());
        run_args.push(tunnel_origin_url(&target));
    } else {
        let token = resolve_token(opts.token, &fort_key, &fort_env_file)?;
        if token.trim().is_empty() {
            bail!(
                "tunnel token mode requires token value (use --token, SURF_CLOUDFLARE_TUNNEL_TOKEN, or Fort-backed --fort-key/--fort-env-file)"
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
        container_name: name.to_owned(),
        running: true,
        url,
        mode,
        error: String::new(),
    })
}

/// Rewrite loopback origins so the cloudflared container can reach host
/// services: inside the container `127.0.0.1` is the container itself, while
/// `host.docker.internal` is mapped to the host gateway (see `--add-host`).
pub fn tunnel_origin_url(target: &str) -> String {
    let Ok(mut url) = url::Url::parse(target.trim()) else {
        return target.trim().to_owned();
    };
    if matches!(
        url.host_str().map(str::to_lowercase).as_deref(),
        Some("127.0.0.1" | "localhost" | "::1" | "0.0.0.0")
    ) && url.set_host(Some("host.docker.internal")).is_ok()
    {
        return url.to_string();
    }
    target.trim().to_owned()
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
    fort_env_file: &str,
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
    let (fort_repo, fort_env) = infer_repo_env_from_path(fort_env_file)?;
    fort_get(fort_repo, fort_env, fort_key)
}

fn infer_repo_env_from_path(path: &str) -> Result<(&str, &str)> {
    let path = path.trim();
    if path.is_empty() {
        bail!(
            "Fort-backed tunnel token requires SURF_TUNNEL_FORT_ENV_FILE or --fort-env-file pointing at /path/to/<repo>/.env.dev or .env.prod"
        );
    }
    let path = Path::new(path);
    let env = match path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        ".env.dev" | ".env.development" => "dev",
        ".env.prod" | ".env.production" => "prod",
        _ => {
            bail!(
                "Fort-backed tunnel token env file must point at /path/to/<repo>/.env.dev or .env.prod"
            )
        }
    };
    let repo = path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .trim();
    if repo.is_empty() {
        bail!(
            "Fort-backed tunnel token env file must point at /path/to/<repo>/.env.dev or .env.prod"
        );
    }
    Ok((repo, env))
}

pub fn fort_get(repo: &str, env: &str, key: &str) -> Result<String> {
    crate::runtime::must_have_command("fort")?;
    let output = Command::new("fort")
        .args([
            "get",
            "--repo",
            repo.trim(),
            "--env",
            env.trim(),
            "--key",
            key.trim(),
        ])
        .output()
        .with_context(|| format!("spawn fort get {key}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let message = if stderr.is_empty() {
            "fort get failed".to_owned()
        } else {
            stderr
        };
        bail!("fort get {key} failed: {message}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::tunnel_origin_url;

    #[test]
    fn tunnel_origin_url_rewrites_loopback_for_container() {
        assert_eq!(
            tunnel_origin_url("http://127.0.0.1:6080"),
            "http://host.docker.internal:6080/"
        );
        assert_eq!(
            tunnel_origin_url("http://localhost:8931/vnc.html"),
            "http://host.docker.internal:8931/vnc.html"
        );
        assert_eq!(
            tunnel_origin_url("http://192.168.1.20:6080"),
            "http://192.168.1.20:6080"
        );
        assert_eq!(tunnel_origin_url("not-a-url"), "not-a-url");
    }
}
