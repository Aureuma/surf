# Release Runbook

This repo uses Git tags + GitHub Releases. Follow this order to avoid broken or partial releases.

## Preconditions

- Local worktree is clean: `git status`
- CI is green on `main`
- You have GitHub permissions to push tags and create releases
- `gh` CLI is authenticated for `Aureuma/surf`

## 1. Decide Version

- Pick next semver tag, e.g. `vX.Y.Z`.
- Keep `v0.x.y` consistent with prior tags in this repo.

## 2. Update Changelog + Version

1. Edit `CHANGELOG.md`.
1. Add a new top section for the version/date, e.g.:
   - `## [vX.Y.Z] - YYYY-MM-DD`
1. Add user-facing bullets grouped by area.
1. Update `crates/surf/Cargo.toml`:
   - `version = "X.Y.Z"`

## 3. Validate Release Inputs (CI-only tests)

1. Validate tag/version alignment:
   - `cargo run --locked -p xtask -- validate-release-version --tag vX.Y.Z`
1. Run tests on GitHub Actions only:
   - `cargo run --locked -p xtask -- dispatch-ci --repo Aureuma/surf --ref main --workflow ci.yml`
1. Build a local native preflight asset on a matching host (optional packaging sanity check, not test execution):
   - `cargo run --locked -p xtask -- build-release-asset --version vX.Y.Z --target <native-target> --archive-suffix <archive-suffix> --out-dir .artifacts/release-preflight`

## 4. Commit

1. Commit release prep changes:
   - `git add CHANGELOG.md Cargo.lock crates/surf/Cargo.toml docs/RELEASE.md docs/RELEASE_RUNBOOK.md docs/RELEASING.md`
   - `git commit -m "release: vX.Y.Z"`

## 5. Tag

1. Create an annotated tag:
   - `git tag -a vX.Y.Z -m "vX.Y.Z"`

## 6. Push

1. Push commit(s):
   - `git push origin main`
1. Push tag:
   - `git push origin vX.Y.Z`

## 7. Publish GitHub Release

1. Prepare release notes from the matching `CHANGELOG.md` section.
1. Publish release for the existing tag:

```bash
gh release create vX.Y.Z \
  --repo Aureuma/surf \
  --title "vX.Y.Z - <short title>" \
  --notes-file release-notes.md \
  --verify-tag
```

When the release is published, workflow `.github/workflows/cli-release-assets.yml` auto-runs and:
- validates tag/version alignment
- builds linux/macos amd64 + arm64 CLI archives on matching native runners
- generates `checksums.txt`
- uploads release assets to GitHub Release
- publishes browser image to `ghcr.io/aureuma/surf-browser`
- verifies required assets are present

Manual dispatch fallback:

```bash
gh workflow run "CLI Release Assets" -R Aureuma/surf -f tag=vX.Y.Z
```

## 8. Post-release Checks

- Workflow status:
  - `gh run list --workflow "CLI Release Assets" --repo Aureuma/surf --limit 1`
- CI workflow status:
  - `gh run list --workflow "CI" --repo Aureuma/surf --limit 1`
- Release assets:
  - `gh release view vX.Y.Z --repo Aureuma/surf --json assets --jq '.assets[].name'`
  - Confirm:
    - `surf_<version>_linux_amd64.tar.gz`
    - `surf_<version>_linux_arm64.tar.gz`
    - `surf_<version>_darwin_amd64.tar.gz`
    - `surf_<version>_darwin_arm64.tar.gz`
    - `checksums.txt`
