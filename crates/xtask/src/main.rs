use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use tar::Builder;

#[derive(Parser)]
#[command(name = "xtask")]
struct Cli {
    #[command(subcommand)]
    command: CommandKind,
}

#[derive(Subcommand)]
enum CommandKind {
    ValidateReleaseVersion(ValidateReleaseVersionArgs),
    BuildReleaseAsset(BuildReleaseAssetArgs),
    BuildReleaseAssets(BuildReleaseAssetsArgs),
    WriteChecksums(WriteChecksumsArgs),
    DispatchCi(DispatchCiArgs),
}

#[derive(Args)]
struct ValidateReleaseVersionArgs {
    #[arg(long)]
    tag: String,
}

#[derive(Args)]
struct BuildReleaseAssetArgs {
    #[arg(long)]
    version: String,
    #[arg(long)]
    target: String,
    #[arg(long = "archive-suffix")]
    archive_suffix: String,
    #[arg(long = "out-dir")]
    out_dir: PathBuf,
}

#[derive(Args)]
struct BuildReleaseAssetsArgs {
    #[arg(long)]
    version: String,
    #[arg(long = "out-dir")]
    out_dir: PathBuf,
}

#[derive(Args)]
struct WriteChecksumsArgs {
    #[arg(long = "dir")]
    dir: PathBuf,
}

#[derive(Args)]
struct DispatchCiArgs {
    #[arg(long, default_value = "Aureuma/surf")]
    repo: String,
    #[arg(long)]
    r#ref: Option<String>,
    #[arg(long, default_value = "ci.yml")]
    workflow: String,
    #[arg(long = "workflow-input")]
    workflow_input: Vec<String>,
    #[arg(long = "no-wait")]
    no_wait: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        CommandKind::ValidateReleaseVersion(args) => validate_release_version(&args.tag),
        CommandKind::BuildReleaseAsset(args) => build_release_asset(
            &args.version,
            &args.target,
            &args.archive_suffix,
            &args.out_dir,
        ),
        CommandKind::BuildReleaseAssets(args) => build_release_assets(&args.version, &args.out_dir),
        CommandKind::WriteChecksums(args) => write_checksums(&args.dir),
        CommandKind::DispatchCi(args) => dispatch_ci(args),
    }
}

fn validate_release_version(tag: &str) -> Result<()> {
    if surf::constants::SURF_VERSION != tag.trim() {
        bail!(
            "version mismatch: crates/surf/Cargo.toml={}, tag={}",
            surf::constants::SURF_VERSION,
            tag.trim()
        );
    }
    println!(
        "release tag and crates/surf/Cargo.toml are aligned ({})",
        tag.trim()
    );
    Ok(())
}

fn build_release_asset(
    version: &str,
    target: &str,
    archive_suffix: &str,
    out_dir: &Path,
) -> Result<()> {
    let repo_root = repo_root()?;
    fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    let version_nov = version.trim().trim_start_matches('v');
    let stem = format!("surf_{}_{}", version_nov, archive_suffix);

    let status = Command::new("cargo")
        .current_dir(&repo_root)
        .args([
            "build",
            "--release",
            "--locked",
            "-p",
            "surf",
            "--bin",
            "surf",
            "--target",
            target,
        ])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("spawn cargo build for release asset")?;
    if !status.success() {
        bail!("cargo build failed for target {target}");
    }

    let stage_root = repo_root.join("target").join("xtask-stage").join(&stem);
    if stage_root.exists() {
        fs::remove_dir_all(&stage_root)
            .with_context(|| format!("remove {}", stage_root.display()))?;
    }
    fs::create_dir_all(&stage_root).with_context(|| format!("create {}", stage_root.display()))?;

    let binary_src = repo_root
        .join("target")
        .join(target)
        .join("release")
        .join("surf");
    let binary_dst = stage_root.join("surf");
    fs::copy(&binary_src, &binary_dst)
        .with_context(|| format!("copy {} -> {}", binary_src.display(), binary_dst.display()))?;

    for file in ["README.md", "LICENSE"] {
        let src = repo_root.join(file);
        if src.exists() {
            let dst = stage_root.join(file);
            fs::copy(&src, &dst)
                .with_context(|| format!("copy {} -> {}", src.display(), dst.display()))?;
        }
    }

    let archive_path = out_dir.join(format!("{stem}.tar.gz"));
    let archive_file = fs::File::create(&archive_path)
        .with_context(|| format!("create {}", archive_path.display()))?;
    let encoder = GzEncoder::new(archive_file, Compression::default());
    let mut tar = Builder::new(encoder);
    tar.append_dir_all(&stem, &stage_root)
        .with_context(|| format!("archive {}", stage_root.display()))?;
    tar.finish().context("finish tar archive")?;

    println!("built {}", archive_path.display());
    Ok(())
}

fn build_release_assets(version: &str, out_dir: &Path) -> Result<()> {
    fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    let targets = native_release_targets()?;
    for (target, archive_suffix) in targets {
        build_release_asset(version, target, archive_suffix, out_dir)?;
    }

    write_checksums(out_dir)?;
    println!(
        "built native release asset set in {}. Full multi-architecture release assembly happens in GitHub Actions.",
        out_dir.display()
    );
    Ok(())
}

fn write_checksums(dir: &Path) -> Result<()> {
    let mut archives = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("gz"))
        .collect::<Vec<_>>();
    archives.sort();

    let mut checksums = String::new();
    for archive in archives {
        let digest = sha256_file(&archive)?;
        let file_name = archive
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow::anyhow!("invalid archive name"))?;
        checksums.push_str(&format!("{digest}  {file_name}\n"));
    }
    fs::write(dir.join("checksums.txt"), checksums).context("write checksums")?;
    println!("wrote {}", dir.join("checksums.txt").display());
    Ok(())
}

fn native_release_targets() -> Result<Vec<(&'static str, &'static str)>> {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => Ok(vec![("x86_64-unknown-linux-gnu", "linux_amd64")]),
        ("linux", "aarch64") => Ok(vec![("aarch64-unknown-linux-gnu", "linux_arm64")]),
        ("macos", "x86_64") => Ok(vec![("x86_64-apple-darwin", "darwin_amd64")]),
        ("macos", "aarch64") => Ok(vec![("aarch64-apple-darwin", "darwin_arm64")]),
        (os, arch) => bail!("unsupported release host {os}/{arch}"),
    }
}

fn dispatch_ci(args: DispatchCiArgs) -> Result<()> {
    require_cmd("gh")?;
    require_cmd("git")?;

    let reference = match args.r#ref {
        Some(reference) => reference,
        None => command_output("git", &["rev-parse", "--abbrev-ref", "HEAD"])?,
    };
    let sha = command_output("git", &["rev-parse", &format!("{reference}^{{commit}}")])?;

    let mut workflow_run_args = vec![
        "workflow".to_owned(),
        "run".to_owned(),
        args.workflow.clone(),
        "--repo".to_owned(),
        args.repo.clone(),
        "--ref".to_owned(),
        reference.clone(),
    ];
    for field in parse_workflow_inputs(&args.workflow_input)? {
        workflow_run_args.push("--field".to_owned());
        workflow_run_args.push(format!("{}={}", field.0, field.1));
    }

    println!(
        "Dispatching workflow={} repo={} ref={} sha={}",
        args.workflow, args.repo, reference, sha
    );
    run_command("gh", workflow_run_args.as_slice())?;

    if args.no_wait {
        println!("Dispatched. Not waiting for completion because --no-wait was set.");
        return Ok(());
    }

    let run_id = command_output(
        "gh",
        &[
            "run",
            "list",
            "--repo",
            &args.repo,
            "--workflow",
            &args.workflow,
            "--json",
            "databaseId,headSha,event,createdAt",
            "--limit",
            "50",
            "--jq",
            &format!(
                "map(select(.event == \"workflow_dispatch\" and .headSha == \"{}\")) | sort_by(.createdAt) | last | .databaseId // \"\"",
                sha
            ),
        ],
    )?;
    if run_id.trim().is_empty() {
        bail!("could not find dispatched workflow run for sha={sha}");
    }

    println!("Watching CI run id={}", run_id.trim());
    run_command("gh", &["run", "watch", run_id.trim(), "--repo", &args.repo])?;
    let conclusion = command_output(
        "gh",
        &[
            "run",
            "view",
            run_id.trim(),
            "--repo",
            &args.repo,
            "--json",
            "conclusion",
            "--jq",
            ".conclusion",
        ],
    )?;
    if conclusion.trim() != "success" {
        bail!(
            "CI run {} did not succeed (conclusion={})",
            run_id.trim(),
            conclusion.trim()
        );
    }
    println!("CI run {} succeeded.", run_id.trim());
    Ok(())
}

fn parse_workflow_inputs(inputs: &[String]) -> Result<Vec<(String, String)>> {
    inputs
        .iter()
        .map(|input| {
            input
                .split_once('=')
                .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
                .filter(|(key, _)| !key.is_empty())
                .ok_or_else(|| anyhow::anyhow!("workflow input must be KEY=VALUE, got: {input}"))
        })
        .collect::<Result<Vec<_>>>()
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn repo_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow::anyhow!("unable to resolve repo root"))
}

fn require_cmd(command: &str) -> Result<()> {
    command_output("which", &[command]).map(|_| ())
}

fn command_output<S>(program: &str, args: &[S]) -> Result<String>
where
    S: AsRef<str>,
{
    let output = Command::new(program)
        .args(args.iter().map(AsRef::as_ref))
        .output()
        .with_context(|| format!("spawn {}", program))?;
    if !output.status.success() {
        bail!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim().to_owned()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn run_command<S>(program: &str, args: &[S]) -> Result<()>
where
    S: AsRef<str>,
{
    let status = Command::new(program)
        .args(args.iter().map(AsRef::as_ref))
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("spawn {}", program))?;
    if !status.success() {
        bail!("{program} command failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_workflow_inputs_accepts_key_value_pairs() {
        let parsed = parse_workflow_inputs(&["tag=v0.1.1".to_string(), "release=true".to_string()])
            .expect("workflow input parse");
        assert_eq!(parsed[0], ("tag".to_string(), "v0.1.1".to_string()));
        assert_eq!(parsed[1], ("release".to_string(), "true".to_string()));
    }

    #[test]
    fn parse_workflow_inputs_rejects_invalid_entry() {
        let error = parse_workflow_inputs(&["invalid".to_string()]).expect_err("invalid input");
        assert!(error.to_string().contains("KEY=VALUE"));
    }

    #[test]
    fn native_release_targets_for_current_host_is_supported() {
        let targets = native_release_targets().expect("supported host");
        let (target, suffix) = targets[0];
        match (env::consts::OS, env::consts::ARCH) {
            ("linux", "x86_64") => {
                assert_eq!(target, "x86_64-unknown-linux-gnu");
                assert_eq!(suffix, "linux_amd64");
            }
            ("linux", "aarch64") => {
                assert_eq!(target, "aarch64-unknown-linux-gnu");
                assert_eq!(suffix, "linux_arm64");
            }
            ("macos", "x86_64") => {
                assert_eq!(target, "x86_64-apple-darwin");
                assert_eq!(suffix, "darwin_amd64");
            }
            ("macos", "aarch64") => {
                assert_eq!(target, "aarch64-apple-darwin");
                assert_eq!(suffix, "darwin_arm64");
            }
            (os, arch) => panic!("unsupported host for this test: {os}/{arch}"),
        }
    }

    #[test]
    fn validate_release_version_matches_current_constant() {
        assert!(validate_release_version(surf::constants::SURF_VERSION).is_ok());
        assert!(validate_release_version("v0.0.0").is_err());
    }

    #[test]
    fn write_checksums_generates_sorted_sha_entries() {
        let mut work_dir = std::env::temp_dir();
        let suffix = format!("surf-xtask-checksums-{}", std::process::id());
        work_dir.push(suffix);
        let _ = std::fs::remove_dir_all(&work_dir);
        std::fs::create_dir_all(&work_dir).expect("temp dir");
        let first = work_dir.join("surf_0.1.1_darwin_amd64.tar.gz");
        let second = work_dir.join("surf_0.1.1_darwin_arm64.tar.gz");

        std::fs::write(&first, b"darwin amd64").expect("write first test archive");
        std::fs::write(&second, b"darwin arm64").expect("write second test archive");

        write_checksums(work_dir.as_path()).expect("write checksums");

        let checksums =
            std::fs::read_to_string(work_dir.join("checksums.txt")).expect("read checksums");
        let lines: Vec<_> = checksums.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("surf_0.1.1_darwin_amd64.tar.gz"));
        assert!(lines[1].ends_with("surf_0.1.1_darwin_arm64.tar.gz"));
        let _ = std::fs::remove_dir_all(&work_dir);
    }
}
