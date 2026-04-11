# Releasing and Changelog Guide

This document is the single source for release policy and the release checklist. It follows Semantic Versioning and keeps a human-focused changelog.

## Versioning Rules

- Use the SemVer shape `MAJOR.MINOR.PATCH`, but apply it operationally in this repo.
- Every commit must bump PATCH in the same commit.
- A published release bumps MINOR, resets PATCH to `0`, and uses a release tag of `vX.Y.0`.
- Only minor release versions are tagged and published to GitHub Releases or other distribution channels.
- MAJOR changes remain exceptional and must be called out explicitly when they happen.

## Changelog Format

Use this structure for each release entry:

```md
## [vX.Y.0] - YYYY-MM-DD
### Added
- ...
### Changed
- ...
### Fixed
- ...
### Removed
- ...
### Security
- ...
```

Guidelines:
- Newest first.
- Use only sections that apply.
- Keep bullets short, user-facing, and past tense.
- Dates are UTC in `YYYY-MM-DD`.

## Release Process

### 0) Pre-flight checks

```bash
git status -sb
git fetch --tags origin
git switch main
git pull --ff-only
```
- Ensure CI is green on `main`.
- Ensure `gh` is authenticated for `Aureuma/surf`.

### 1) Determine version and release title

- Decide the next release version `vX.Y.0`.
- Choose short release title.
- GitHub title format: `vX.Y.0 - Suggested Name`.

### 2) Draft changelog entry

1. Add new top entry to `CHANGELOG.md`.
2. Summarize user-facing changes.
3. Cover every patch-bump commit since the previous minor release.

### 3) Bump code version

- Update `crates/surf/Cargo.toml` to `X.Y.0`.

### 4) Validate on GitHub Actions and commit release prep

```bash
cargo run --locked -p xtask -- validate-release-version --tag vX.Y.0
cargo run --locked -p xtask -- dispatch-ci --repo Aureuma/surf --ref main --workflow ci.yml
cargo run --locked -p xtask -- dispatch-ci --repo Aureuma/surf --ref vX.Y.0 --workflow cli-release-assets --workflow-input tag=vX.Y.0
cargo run --locked -p xtask -- build-release-asset --version vX.Y.0 --target <native-target> --archive-suffix <archive-suffix> --out-dir .artifacts/release-preflight
git add CHANGELOG.md Cargo.lock crates/surf/Cargo.toml docs/RELEASE.md docs/RELEASING.md
git commit -m "release: vX.Y.0"
```

### 5) Tag release commit

```bash
git tag -a vX.Y.0 -m "vX.Y.0"
```

### 6) Push commit and tag

```bash
git push origin main
git push origin vX.Y.0
```

### 7) Publish GitHub release

```bash
gh release create vX.Y.0 \
  --repo Aureuma/surf \
  --title "vX.Y.0 - Suggested Name" \
  --notes-file release-notes.md \
  --verify-tag
```

### 8) Verify published release

```bash
gh release view vX.Y.0 --repo Aureuma/surf --web
```

### 9) Verify release workflow and assets

```bash
gh run list --workflow "CLI Release Assets" --repo Aureuma/surf --limit 1
gh release view vX.Y.0 --repo Aureuma/surf --json assets --jq '.assets[].name'
```

Expected assets:
- `surf_<version>_linux_amd64.tar.gz`
- `surf_<version>_linux_arm64.tar.gz`
- `surf_<version>_darwin_amd64.tar.gz`
- `surf_<version>_darwin_arm64.tar.gz`
- `checksums.txt`
