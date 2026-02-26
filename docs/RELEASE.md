# Release Runbook

## Versioning

1. Update `cmd/surf/version.go` (`surfVersion`) to the new tag value.
2. Tag format: `vX.Y.Z`.

## Validate locally

```bash
tools/release/validate-release-version.sh --tag v0.1.0
tools/release/build-cli-release-assets.sh --version v0.1.0 --out-dir dist
```

## Publish

```bash
git tag v0.1.0
git push origin v0.1.0
```

The GitHub Actions workflow `.github/workflows/cli-release-assets.yml` will:
- validate tag/version match
- build linux/macos amd64/arm64 archives
- generate `checksums.txt`
- upload assets to the release
- publish browser Docker image to GHCR

## Verify

- `surf version`
- `surf start --profile default && surf status`
- release page contains all archives + checksums
