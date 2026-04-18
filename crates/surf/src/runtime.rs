use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;

use crate::browser::{
    BrowserConfig, apply_container_profile_default, mcp_url, novnc_url, resolve_profile_mount,
    viewer_password_warnings,
};

#[derive(Debug, Clone, Serialize)]
pub struct StatusPayload {
    pub ok: bool,
    pub container_name: String,
    pub container_running: bool,
    pub container_status_line: String,
    pub mcp_url: String,
    pub novnc_url: String,
    pub mcp_host_code: i32,
    pub mcp_container_code: i32,
    pub novnc_host_code: i32,
    pub novnc_container_code: i32,
    pub mcp_ready: bool,
    pub novnc_ready: bool,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildResult {
    pub image: String,
    pub dockerfile: String,
    pub context: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StartResult {
    pub config: BrowserConfig,
    pub status: StatusPayload,
    pub mcp_url: String,
    pub novnc_url: String,
    pub container_name: String,
    pub viewer_password: Option<String>,
    pub viewer_password_generated: bool,
    pub warnings: Vec<String>,
}

pub fn build_runtime(
    image_name: &str,
    repo: Option<&str>,
    context_dir: Option<&str>,
    dockerfile: Option<&str>,
) -> Result<BuildResult> {
    must_have_command("docker")?;

    let (resolved_context, resolved_dockerfile) =
        resolve_build_inputs(repo, context_dir, dockerfile)?;

    let status = Command::new("docker")
        .args([
            "build",
            "-t",
            image_name,
            "-f",
            &resolved_dockerfile,
            &resolved_context,
        ])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("spawn docker build")?;
    if !status.success() {
        bail!("docker build failed");
    }

    Ok(BuildResult {
        image: image_name.to_owned(),
        dockerfile: resolved_dockerfile,
        context: resolved_context,
    })
}

pub fn start_runtime(
    cfg: &BrowserConfig,
    repo: Option<&str>,
    skip_build: bool,
    profile_dir_flag_passed: bool,
) -> Result<StartResult> {
    must_have_command("docker")?;
    let mut cfg = cfg.clone();
    apply_container_profile_default(profile_dir_flag_passed, &mut cfg);
    let profile_mount = resolve_profile_mount(&cfg.profile_dir);

    if let Some(host_path) = profile_mount.host_path.as_ref() {
        fs::create_dir_all(host_path)
            .with_context(|| format!("create profile directory {host_path}"))?;
    }
    if !skip_build {
        build_runtime(&cfg.image_name, repo, None, None)?;
    }
    remove_docker_container(&cfg.container_name)?;
    ensure_docker_network(&cfg.network)?;

    let run_args = vec![
        "run".to_owned(),
        "-d".to_owned(),
        "--name".to_owned(),
        cfg.container_name.clone(),
        "--restart".to_owned(),
        "unless-stopped".to_owned(),
        "--init".to_owned(),
        "--ipc=host".to_owned(),
        "--network".to_owned(),
        cfg.network.clone(),
        "--user".to_owned(),
        "pwuser".to_owned(),
        "-e".to_owned(),
        format!("VNC_PASSWORD={}", cfg.vnc_password),
        "-e".to_owned(),
        format!("MCP_VERSION={}", cfg.mcp_version),
        "-e".to_owned(),
        format!("BROWSER_CHANNEL={}", cfg.browser_channel),
        "-e".to_owned(),
        format!("ALLOWED_HOSTS={}", cfg.allowed_hosts),
        "-e".to_owned(),
        format!("MCP_PORT={}", cfg.mcp_port),
        "-e".to_owned(),
        format!("NOVNC_PORT={}", cfg.novnc_port),
        "-p".to_owned(),
        format!("{}:{}:{}", cfg.host_bind, cfg.host_mcp_port, cfg.mcp_port),
        "-p".to_owned(),
        format!(
            "{}:{}:{}",
            cfg.host_bind, cfg.host_novnc_port, cfg.novnc_port
        ),
        "-v".to_owned(),
        profile_mount.mount_arg,
        cfg.image_name.clone(),
    ];
    run_docker_output(&run_args).context("docker run failed")?;

    let status = wait_for_status(&cfg, 15, Duration::from_secs(1))?;
    let warnings = viewer_password_warnings(&cfg.vnc_password, cfg.vnc_password_generated);
    let viewer_password = cfg.vnc_password_generated.then(|| cfg.vnc_password.clone());
    Ok(StartResult {
        mcp_url: mcp_url(&cfg),
        novnc_url: novnc_url(&cfg),
        container_name: cfg.container_name.clone(),
        viewer_password,
        viewer_password_generated: cfg.vnc_password_generated,
        warnings,
        status,
        config: cfg,
    })
}

pub fn stop_runtime(container_name: &str) -> Result<()> {
    must_have_command("docker")?;
    remove_docker_container(container_name)
}

pub fn status_runtime(cfg: &BrowserConfig, profile_dir_flag_passed: bool) -> Result<StatusPayload> {
    must_have_command("docker")?;
    let mut cfg = cfg.clone();
    apply_container_profile_default(profile_dir_flag_passed, &mut cfg);
    evaluate_status(&cfg)
}

pub fn stream_container_logs(container_name: &str, tail: i32, follow: bool) -> Result<()> {
    must_have_command("docker")?;
    let mut args = vec!["logs".to_owned(), "--tail".to_owned(), tail.to_string()];
    if follow {
        args.push("-f".to_owned());
    }
    args.push(container_name.trim().to_owned());
    let status = Command::new("docker")
        .args(&args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("spawn docker logs")?;
    if !status.success() {
        bail!("docker logs failed");
    }
    Ok(())
}

pub fn evaluate_status(cfg: &BrowserConfig) -> Result<StatusPayload> {
    let mut status = StatusPayload {
        ok: false,
        container_name: cfg.container_name.clone(),
        container_running: false,
        container_status_line: String::new(),
        mcp_url: mcp_url(cfg),
        novnc_url: novnc_url(cfg),
        mcp_host_code: 0,
        mcp_container_code: 0,
        novnc_host_code: 0,
        novnc_container_code: 0,
        mcp_ready: false,
        novnc_ready: false,
        error: String::new(),
    };

    let ps_out = run_docker_output(&[
        "ps".to_owned(),
        "--filter".to_owned(),
        format!("name=^/{}$", cfg.container_name),
        "--format".to_owned(),
        "{{.Names}}\t{{.Status}}\t{{.Ports}}".to_owned(),
    ])?;
    status.container_status_line = ps_out.trim().to_owned();
    status.container_running = !status.container_status_line.is_empty();
    if !status.container_running {
        status.error = "container not running".to_owned();
        return Ok(status);
    }

    status.mcp_host_code = probe_http_status(&status.mcp_url);
    if matches!(status.mcp_host_code, 200 | 400) {
        status.mcp_ready = true;
    }
    status.novnc_host_code = probe_http_status(&status.novnc_url);
    if (200..400).contains(&status.novnc_host_code) {
        status.novnc_ready = true;
    }

    if !status.mcp_ready {
        status.mcp_container_code = probe_container_http_status(
            &cfg.container_name,
            &format!("http://127.0.0.1:{}/mcp", cfg.mcp_port),
        );
        if matches!(status.mcp_container_code, 200 | 400) {
            status.mcp_ready = true;
        }
    }
    if !status.novnc_ready {
        status.novnc_container_code = probe_container_http_status(
            &cfg.container_name,
            &format!("http://127.0.0.1:{}/vnc.html", cfg.novnc_port),
        );
        if (200..400).contains(&status.novnc_container_code) {
            status.novnc_ready = true;
        }
    }

    status.ok = status.container_running && status.mcp_ready && status.novnc_ready;
    if !status.ok {
        status.error = "endpoint checks failed".to_owned();
    }
    Ok(status)
}

pub fn wait_for_status(
    cfg: &BrowserConfig,
    retries: usize,
    delay: Duration,
) -> Result<StatusPayload> {
    let mut last = evaluate_status(cfg)?;
    for _ in 0..retries {
        let status = evaluate_status(cfg)?;
        if status.ok {
            return Ok(status);
        }
        last = status;
        thread::sleep(delay);
    }
    if last.error.trim().is_empty() {
        last.error = "browser health checks did not pass in time".to_owned();
    }
    Err(anyhow!(last.error.clone()))
}

pub fn resolve_assets_root(repo: Option<&str>) -> Result<PathBuf> {
    let root = resolve_repo_root(repo)?;
    resolve_assets_root_from_root(&root)
}

pub fn resolve_build_inputs(
    repo: Option<&str>,
    context_dir: Option<&str>,
    dockerfile: Option<&str>,
) -> Result<(String, String)> {
    let root = resolve_repo_root(repo)?;
    let assets_root = resolve_assets_root_from_root(&root)?;
    let resolved_context = context_dir
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| root.display().to_string());
    let resolved_dockerfile = dockerfile
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| assets_root.join("Dockerfile").display().to_string());
    Ok((resolved_context, resolved_dockerfile))
}

fn resolve_assets_root_from_root(root: &Path) -> Result<PathBuf> {
    let assets = root.join("runtime").join("browser");
    if !assets.join("Dockerfile").exists() {
        bail!("browser assets not found at {}", assets.display());
    }
    Ok(assets)
}

pub fn resolve_repo_root(repo: Option<&str>) -> Result<PathBuf> {
    if let Some(repo) = repo.map(str::trim).filter(|value| !value.is_empty()) {
        let resolved: PathBuf = crate::paths::expand_tilde(repo).components().collect();
        if resolved.exists() {
            return Ok(resolved);
        }
        bail!("repository path not found: {}", resolved.display());
    }
    if let Some(repo) = crate::paths::env_trimmed("SURF_REPO") {
        let resolved: PathBuf = crate::paths::expand_tilde(&repo).components().collect();
        if resolved.exists() {
            return Ok(resolved);
        }
    }
    if let Ok(cwd) = std::env::current_dir()
        && has_surf_layout(&cwd)
    {
        return Ok(cwd);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent().and_then(|path| path.parent())
    {
        let candidate = parent.to_path_buf();
        if has_surf_layout(&candidate) {
            return Ok(candidate);
        }
    }
    bail!("unable to locate surf repository root; pass --repo")
}

pub fn has_surf_layout(root: &Path) -> bool {
    root.join("runtime")
        .join("browser")
        .join("Dockerfile")
        .exists()
        && (root.join("cmd").join("surf").exists() || root.join("crates").join("surf").exists())
}

pub fn run_docker_output(args: &[String]) -> Result<String> {
    let output = Command::new("docker")
        .args(args)
        .output()
        .with_context(|| format!("spawn docker {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let message = if stderr.is_empty() {
            if stdout.is_empty() {
                "docker command failed".to_owned()
            } else {
                stdout
            }
        } else {
            stderr
        };
        bail!("{message}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn ensure_docker_network(name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        bail!("network name is required");
    }
    let inspect = vec!["network".to_owned(), "inspect".to_owned(), name.to_owned()];
    if run_docker_output(&inspect).is_ok() {
        return Ok(());
    }
    let create = vec!["network".to_owned(), "create".to_owned(), name.to_owned()];
    run_docker_output(&create).with_context(|| format!("ensure docker network {name}"))?;
    Ok(())
}

pub fn remove_docker_container(name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(());
    }
    let args = vec!["rm".to_owned(), "-f".to_owned(), name.to_owned()];
    if let Err(error) = run_docker_output(&args) {
        let message = error.to_string().to_lowercase();
        if message.contains("no such container") {
            return Ok(());
        }
        return Err(error);
    }
    Ok(())
}

pub fn probe_http_status(raw_url: &str) -> i32 {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build();
    let Ok(client) = client else {
        return 0;
    };
    client
        .get(raw_url)
        .send()
        .map(|response| i32::from(response.status().as_u16()))
        .unwrap_or(0)
}

pub fn probe_container_http_status(container_name: &str, raw_url: &str) -> i32 {
    let args = vec![
        "exec".to_owned(),
        container_name.to_owned(),
        "sh".to_owned(),
        "-lc".to_owned(),
        format!(
            "curl -sS -o /dev/null -w '%{{http_code}}' {}",
            single_quote(raw_url)
        ),
    ];
    run_docker_output(&args)
        .ok()
        .and_then(|value| value.trim().parse::<i32>().ok())
        .unwrap_or(0)
}

pub fn is_container_running(name: &str) -> Result<(bool, String)> {
    let ps_out = run_docker_output(&[
        "ps".to_owned(),
        "--filter".to_owned(),
        format!("name=^/{}$", name.trim()),
        "--format".to_owned(),
        "{{.Names}}\t{{.Status}}".to_owned(),
    ])?;
    let line = ps_out.trim().to_owned();
    Ok((!line.is_empty(), line))
}

pub fn fetch_container_logs(name: &str, tail: i32) -> String {
    let output = Command::new("docker")
        .args(["logs", "--tail", &tail.to_string(), name.trim()])
        .output();
    match output {
        Ok(output) => {
            let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
            combined.push_str(&String::from_utf8_lossy(&output.stderr));
            combined.trim().to_owned()
        }
        Err(_) => String::new(),
    }
}

pub fn extract_try_cloudflare_url(log_text: &str) -> String {
    let mut last = String::new();
    for token in log_text.split_whitespace() {
        let candidate = token.trim_matches(|ch: char| matches!(ch, '|' | '"' | '\'' | ')' | '('));
        if candidate.starts_with("https://") && candidate.contains(".trycloudflare.com") {
            last = candidate.to_owned();
        }
    }
    last
}

pub fn must_have_command(cmd: &str) -> Result<()> {
    if find_command(cmd).is_some() {
        return Ok(());
    }
    bail!("missing required command: {cmd}")
}

pub fn find_command(command: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for entry in std::env::split_paths(&path) {
        let candidate = entry.join(command);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn copy_dir(source: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest).with_context(|| format!("create directory {}", dest.display()))?;
    for entry in fs::read_dir(source).with_context(|| format!("read {}", source.display()))? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&src_path, &dst_path)?;
        } else {
            copy_file(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

pub fn copy_file(source: &Path, dest: &Path) -> Result<()> {
    let mut input =
        fs::File::open(source).with_context(|| format!("open source file {}", source.display()))?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create parent directory {}", parent.display()))?;
    }
    let mut output =
        fs::File::create(dest).with_context(|| format!("create destination {}", dest.display()))?;
    io::copy(&mut input, &mut output)
        .with_context(|| format!("copy {} -> {}", source.display(), dest.display()))?;
    output.flush().ok();
    Ok(())
}

pub fn single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::{extract_try_cloudflare_url, resolve_build_inputs};
    use std::fs;

    #[test]
    fn extract_cloudflare_url_returns_last_match() {
        let logs =
            "random\nhttps://alpha.trycloudflare.com\nother\nhttps://beta.trycloudflare.com\n";
        assert_eq!(
            extract_try_cloudflare_url(logs),
            "https://beta.trycloudflare.com"
        );
    }

    #[test]
    fn extract_cloudflare_banner_url() {
        let logs = r#"
2026-02-28T05:11:41Z INF |  https://alliance-naval-licenses-childrens.trycloudflare.com |
"#;
        assert_eq!(
            extract_try_cloudflare_url(logs),
            "https://alliance-naval-licenses-childrens.trycloudflare.com"
        );
    }

    #[test]
    fn resolve_build_inputs_defaults_to_repo_root_context() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("runtime/browser")).unwrap();
        fs::create_dir_all(dir.path().join("crates/surf")).unwrap();
        fs::write(
            dir.path().join("runtime/browser/Dockerfile"),
            "FROM scratch
",
        )
        .unwrap();
        let (context, dockerfile) =
            resolve_build_inputs(Some(dir.path().to_str().unwrap()), None, None).unwrap();
        assert_eq!(context, dir.path().display().to_string());
        assert_eq!(
            dockerfile,
            dir.path()
                .join("runtime/browser/Dockerfile")
                .display()
                .to_string()
        );
    }
}
