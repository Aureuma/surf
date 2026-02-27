# Releasing and Changelog Guide

This project follows Semantic Versioning and keeps a human-focused changelog.

## Versioning Rules

- Use SemVer: MAJOR.MINOR.PATCH (tag format: `vX.Y.Z`).
- Breaking changes:
  - While the project is on the `v0.x` line: bump MINOR.
  - After the project graduates off `v0.x`: bump MAJOR.
- Features: bump MINOR.
- Fixes/docs-only releases: bump PATCH.

## Changelog Format

Use this structure for each release entry:

```md
## [vX.Y.Z] - YYYY-MM-DD
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

### 1) Determine version and release title

- Decide `vX.Y.Z` using the SemVer rules.
- Choose short release title.
- GitHub title format: `vX.Y.Z - Suggested Name`.

### 2) Draft changelog entry

1. Add new top entry to `CHANGELOG.md`.
2. Summarize user-facing changes.

### 3) Bump code version

- Update `cmd/surf/version.go` (`surfVersion`).

### 4) Validate and commit release prep

```bash
go test ./...
tools/release/validate-release-version.sh --tag vX.Y.Z
tools/release/build-cli-release-assets.sh --version vX.Y.Z --out-dir .artifacts/release-preflight
git add CHANGELOG.md cmd/surf/version.go docs/RELEASE.md docs/RELEASE_RUNBOOK.md docs/RELEASING.md
git commit -m "release: vX.Y.Z"
```

### 5) Tag release commit

```bash
git tag -a vX.Y.Z -m "vX.Y.Z"
```

### 6) Push commit and tag

```bash
git push origin main
git push origin vX.Y.Z
```

### 7) Publish GitHub release

```bash
gh release create vX.Y.Z \
  --repo Aureuma/surf \
  --title "vX.Y.Z - Suggested Name" \
  --notes-file release-notes.md \
  --verify-tag
```

### 8) Verify published release

```bash
gh release view vX.Y.Z --repo Aureuma/surf --web
```

### 9) Verify release workflow and assets

```bash
gh run list --workflow "CLI Release Assets" --repo Aureuma/surf --limit 1
gh release view vX.Y.Z --repo Aureuma/surf --json assets --jq '.assets[].name'
```

Expected assets:
- `surf_<version>_linux_amd64.tar.gz`
- `surf_<version>_linux_arm64.tar.gz`
- `surf_<version>_darwin_amd64.tar.gz`
- `surf_<version>_darwin_arm64.tar.gz`
- `checksums.txt`
