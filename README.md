# surf

`surf` is a Go-first browser runtime for SI.

It provides:
- Dockerized headed Playwright MCP runtime
- persisted browser profiles (container + host)
- noVNC access for visual browser sessions
- existing browser-session attach and actions (Chrome CDP)
- optional Cloudflare tunnel exposure for noVNC
- optional hosted token tunnel mode with secret retrieval from `si vault`
- local headed host browser control (Chromium-based) for macOS/Linux
- MCP compatibility proxy (`/mcp` -> `/sse` GET rewrite)
- persistent surf settings in `~/.si/surf/settings.toml`

## Install

```bash
go install github.com/Aureuma/si/tools/si@latest
```

## Quick start

```bash
si surf build
si surf start --profile default
si surf status
si surf logs
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
si surf host start --profile work
si surf host status --profile work
si surf host stop --profile work
```

## Existing Session Attach (Chrome)

Attach to an already-open Chrome/Chromium tab that has CDP enabled:

```bash
si surf session discover
si surf session attach --id <target-id>
si surf session act --session <name> --action title
si surf session act --session <name> --action screenshot --out ./shot.png
si surf session detach --session <name>
```

`read_only` mode is default. Write actions (`click`, `type`) require `--mode interactive` during attach.

## Settings

`surf` manages its own settings file at:

- `~/.si/surf/settings.toml`

The directory and file are auto-created on first config/runtime usage.

Common commands:

```bash
si surf config path
si surf config init
si surf config show --json
si surf config set --key tunnel.mode --value token
si surf config set --key tunnel.vault_key --value SURF_CLOUDFLARE_TUNNEL_TOKEN
```

## Public noVNC exposure over Cloudflare

Quick ephemeral tunnel:

```bash
si surf tunnel start --mode quick
si surf tunnel status
si surf tunnel logs
```

Named/managed token mode (token from env or si vault):

```bash
si surf tunnel start --mode token --vault-key SURF_CLOUDFLARE_TUNNEL_TOKEN
```

Token resolution order:
1. `--token`
2. `SURF_CLOUDFLARE_TUNNEL_TOKEN`
3. `si vault get <vault-key>` when `--vault-key` is provided

## Chrome extension scaffold

```bash
si surf extension install
si surf extension path
si surf extension doctor
```

## SI integration

`si` exposes this as a thin interface:

```bash
si surf <...>
```

`surf` is an internal runtime binary and should be invoked through `si surf`.

`si surf` can hydrate selected `SURF_*` secrets from `si vault` when present.
It can also manage wrapper defaults in `~/.si/settings.toml` via:

```bash
si surf config show --json
si surf config set --repo /path/to/surf --bin /path/to/surf/bin/surf --build true
```

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
