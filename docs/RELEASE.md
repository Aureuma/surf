# Release Runbook

1. Tag release
```bash
git tag v0.1.0
git push origin v0.1.0
```

2. GitHub Actions `release` workflow will:
- run tests
- build binaries for linux/macos amd64/arm64
- upload release artifacts
- build and push Docker image to GHCR

3. Validate
- `surf version`
- `surf start && surf status`
