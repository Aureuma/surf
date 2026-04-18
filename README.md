# surf

`surf` is a Rust browser runtime for SI.

It provides:
- Dockerized headed Playwright MCP runtime
- persisted browser profiles (container + host)
- noVNC access for visual browser sessions
- existing browser-session attach and actions (Chrome CDP)
- optional Cloudflare tunnel exposure for noVNC
- optional hosted token tunnel mode with secret retrieval through `si fort`
- local headed host browser control (Chromium-based) for macOS/Linux
- MCP compatibility proxy (`/mcp` -> `/sse` GET rewrite)
- persistent surf settings in `~/.si/surf/settings.toml`

## Install

`surf` is intended to run through `si surf`.

For local `surf` development in this repo:

```bash
cargo build -p surf
SURF_STANDALONE_UNSAFE=1 cargo run -p surf -- version
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
si surf session act --session <name> --action elements
si surf session act --session <name> --action scroll --delta-y 480 --steps 4
si surf session act --session <name> --action type --selector "#q" --text "hello"
si surf session act --session <name> --action paste --text " world"
si surf session act --session <name> --action screenshot --out ./shot.png
si surf session detach --session <name>
```

`read_only` mode is default. Write actions (`click`, `type`, `paste`, `scroll`) require `--mode interactive` during attach.

Available actions:
- Read-safe: `title`, `url`, `text`, `elements`, `copy`, `screenshot`, `eval`
- Interactive only: `click`, `type`, `paste`, `scroll`

Humanized interaction options are available per action call:

```bash
si surf session act --session <name> --action click --selector "button[type=submit]" \
  --human=true --min-delay-ms 40 --max-delay-ms 180 --mouse-steps 12
```

Policy controls (allowlist/blocklist) are enforced from settings:
- `existing_session.allowed_domains` (default `["*"]`)
- `existing_session.blocked_domains`

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
si surf config set --key tunnel.fort_key --value SURF_CLOUDFLARE_TUNNEL_TOKEN
si surf config set --key tunnel.fort_repo --value surf
si surf config set --key tunnel.fort_env --value dev
```

## Public noVNC exposure over Cloudflare

For shared dev HTTPS viewing, prefer routing Surf through Viva's existing dev tunnel and keep the MCP endpoint private by default. Use Surf's native tunnel commands for local operator workflows or quick temporary exposure.

Quick ephemeral tunnel:

```bash
si surf tunnel start --mode quick
si surf tunnel status
si surf tunnel logs
```

Named/managed token mode (token from env or `si fort`):

```bash
si surf tunnel start --mode token \
  --fort-key SURF_CLOUDFLARE_TUNNEL_TOKEN \
  --fort-repo surf \
  --fort-env dev
```

Token resolution order:
1. `--token`
2. `SURF_CLOUDFLARE_TUNNEL_TOKEN`
3. `si fort get --repo <repo> --env <env> --key <fort-key>` when Fort settings are provided

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

`si surf` should be paired with `si fort` for runtime secret access when a Surf flow needs secret material.
It can also manage wrapper defaults in `~/.si/settings.toml` via:

```bash
si surf config show --json
si surf config set --repo /path/to/surf --bin /path/to/surf/bin/surf --build true
```

## Release and versioning

`surf` follows the single release guide in `docs/RELEASING.md`:

- version source of truth: `crates/surf/Cargo.toml` (`version`, surfaced as `SURF_VERSION`)
- tag must match the minor release version (`cargo run --locked -p xtask -- validate-release-version --tag vX.Y.0`)
- multi-arch archives + checksums:
  - `surf_<version>_linux_amd64.tar.gz`
  - `surf_<version>_linux_arm64.tar.gz`
  - `surf_<version>_darwin_amd64.tar.gz`
  - `surf_<version>_darwin_arm64.tar.gz`
- workflow: `.github/workflows/cli-release-assets.yml`
- release helper crate: `cargo run --locked -p xtask -- <command>`
- browser image publish to GHCR: `ghcr.io/aureuma/surf-browser`
