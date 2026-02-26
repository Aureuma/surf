# surf

`surf` is a Go-first browser runtime for SI and standalone automation.

It provides:
- Dockerized headed Playwright MCP runtime
- persisted browser profiles (container + host)
- noVNC access for visual browser sessions
- optional Cloudflare tunnel exposure for noVNC
- optional hosted token tunnel mode with secret retrieval from `si vault`
- local headed host browser control (Chromium-based) for macOS/Linux
- MCP compatibility proxy (`/mcp` -> `/sse` GET rewrite)

## Install

```bash
go install github.com/Aureuma/surf/cmd/surf@latest
```

## Quick start

```bash
surf build
surf start --profile default
surf status
surf logs
```

Default endpoints:
- MCP: `http://127.0.0.1:8932/mcp`
- noVNC: `http://127.0.0.1:6080/vnc.html?autoconnect=1&resize=scale`

## Profile persistence

Inspired by OpenClaw style profile isolation, `surf` stores runtime state under:

- state root: `~/.surf` (override with `SURF_STATE_DIR`)
- container profile: `~/.surf/browser/profiles/container/<profile>`
- host profile: `~/.surf/browser/profiles/host/<profile>`

Use `--profile` across runtime commands to switch profile context.

## Host browser mode (macOS/Linux)

`surf` can launch a headed local Chromium-based browser with isolated profile + CDP:

```bash
surf host start --profile work
surf host status --profile work
surf host stop --profile work
```

## Public noVNC exposure over Cloudflare

Quick ephemeral tunnel:

```bash
surf tunnel start --mode quick
surf tunnel status
surf tunnel logs
```

Named/managed token mode (token from env or si vault):

```bash
surf tunnel start --mode token --vault-key SURF_CLOUDFLARE_TUNNEL_TOKEN
```

Token resolution order:
1. `--token`
2. `SURF_CLOUDFLARE_TUNNEL_TOKEN`
3. `si vault get <vault-key>` when `--vault-key` is provided

## Chrome extension scaffold

```bash
surf extension install
surf extension path
surf extension doctor
```

## SI integration

`si` exposes this as a thin interface:

```bash
si surf <...>
```

`si surf` can hydrate selected `SURF_*` secrets from `si vault` when present.

## Release and versioning

`surf` follows a `si`-style release runbook:

- version source of truth: `cmd/surf/version.go` (`surfVersion`)
- tag must match version (`tools/release/validate-release-version.sh`)
- multi-arch archives + checksums:
  - `surf_<version>_linux_amd64.tar.gz`
  - `surf_<version>_linux_arm64.tar.gz`
  - `surf_<version>_darwin_amd64.tar.gz`
  - `surf_<version>_darwin_arm64.tar.gz`
- workflow: `.github/workflows/cli-release-assets.yml`
- browser image publish to GHCR: `ghcr.io/aureuma/surf-browser`
