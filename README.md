# surf

`surf` is a Playwright MCP server: one container running a headed Chromium
per agent, each with its own profile and MCP endpoint, plus a browser-based
viewer with one named workspace per agent so the operator watches every
agent's tabs.

```text
agent A ──https──> cloudflared ──http──> surf proxy ──http──> MCP:8931 ──> Chromium ──> ws "profile/A"
agent B ──╯  /agent/<name>/mcp + per-agent token   (8931)      MCP:8932 ──> Chromium ──> ws "profile/B"
operator ──https──> cloudflared ──http──> noVNC (VNC password) ──> same X display (Fluxbox workspaces)
```

Each agent gets an isolated browser context (separate profile, cookies,
tabs) on the shared X display. Workspaces are organizational, not security
boundaries: all agents share one X server.

## Install

`surf` is a standalone CLI. For local development:

```bash
cargo build -p surf
cargo run -p surf -- version
```

## Quick start

```bash
surf build                                # browser image (playwright + MCP baked in)
surf start --profile agents --agents alice,bob
                                          # container: MCPs 127.0.0.1:8932+, viewer 0.0.0.0:6080
surf proxy                                # auth gateway 0.0.0.0:8931 (generates tokens on first run)
surf tunnel start --mode quick --name surf-mcp-cloudflared --target-url http://127.0.0.1:8931
surf tunnel start --mode quick --name surf-novnc-cloudflared   # viewer (default target)
```

Default endpoints:

- MCP (agent): `https://<mcp-tunnel>/agent/<name>/mcp` with that agent's token
- MCP (slot 0 alias): `https://<mcp-tunnel>/mcp` with the main proxy token
- noVNC (operator): `https://<novnc-tunnel>/vnc.html` with the viewer password
- Local MCP (no tunnel): `http://127.0.0.1:8932/mcp` (slot 0), `:8933` (slot 1), ...
- Local viewer: `http://127.0.0.1:6080/vnc.html`

With no `--agents` flag, `surf start` runs the legacy single `default`
agent (flat profile layout, `/mcp` only). The agent set is fixed at
container start; changing it recreates the container.

## Ports and binds

| Service  | Host port  | Bind      | Auth          | Expose via        |
| -------- | ---------- | --------- | ------------- | ----------------- |
| MCP raw  | 8932+slot  | 127.0.0.1 | none upstream | never directly    |
| Proxy    | 8931       | 0.0.0.0   | bearer token  | tunnel for agents |
| noVNC    | 6080       | 0.0.0.0   | VNC password  | tunnel for viewer |

Raw MCP has no authentication, so it never leaves the box: remote agents
always go through the proxy. The proxy and the viewer bind all interfaces so
tunnel containers can reach them; both are secret-gated. Up to 8 agent
slots; slot N listens on host port `8932+N` (loopback) and container port
`8931+N`.

## Agents

```bash
surf start --agents alice,bob,carol       # one browser context per agent
surf agent list                           # slots, workspaces, ports, MCP URLs
surf agent view bob                       # switch the viewer to bob's workspace
```

Each agent gets a Fluxbox workspace named `<profile>/<agent>` (clickable in
the viewer toolbar), a Chrome window auto-placed there (popups and relaunched
browsers follow), and a profile under
`~/.surf/browser/profiles/container/<profile>/agents/<name>/`.

## Agent client config

Streamable HTTP with a bearer token:

```json
{
  "mcpServers": {
    "playwright": {
      "url": "https://<mcp-tunnel>/agent/alice/mcp",
      "headers": { "Authorization": "Bearer <alice-token>" }
    }
  }
}
```

Clients without custom header support can use the query fallback instead:

```text
https://<mcp-tunnel>/agent/alice/mcp?token=<alice-token>
```

Read tokens back with `surf config show` (`proxy.tokens.<name>`).
Rotate one with `surf config set --key proxy.tokens.alice --value <new>`
then restart the proxy. The main `proxy.auth_token` gates the `/mcp`
slot-0 alias only.

## Stable public URLs (token mode)

Quick tunnels get a random `*.trycloudflare.com` hostname on every restart.
For stable agent URLs, use a named tunnel:

1. Create a tunnel in Cloudflare (`cloudflared tunnel create surf`) and map
   two hostnames to it, e.g. `surf-mcp.example.com` and
   `surf-view.example.com`.
2. Ingress rules (dashboard or `config.yml` on the tunnel):
   `surf-mcp.example.com -> http://host.docker.internal:8931`,
   `surf-view.example.com -> http://host.docker.internal:6080`.
   (`host.docker.internal` is pre-mapped to the host gateway.)
3. Store the tunnel token with `fort`, then:

```bash
surf tunnel start --mode token \
  --fort-key SURF_CLOUDFLARE_TUNNEL_TOKEN \
  --fort-env-file /path/to/safe/surf/.env.dev
```

Token resolution order: `--token`, `SURF_CLOUDFLARE_TUNNEL_TOKEN`,
then `fort get` using the repo/env inferred from `--fort-env-file`.

## Firewall

Tunnel containers reach host services through the Docker bridge. With `ufw`
blocking container-to-host traffic, allow the two origins (adjust the subnet
to your Docker bridge):

```bash
sudo ufw allow in from 172.17.0.0/16 to any port 8931 comment 'docker bridge -> surf mcp proxy'
sudo ufw allow in from 172.17.0.0/16 to any port 6080 comment 'docker bridge -> surf novnc'
```

## Proxy persistence

`surf proxy` runs in the foreground. For a persistent server, install the
systemd user unit (edit the binary path first):

```bash
mkdir -p ~/.config/systemd/user
cp deploy/surf-proxy.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now surf-proxy
```

The browser container and tunnel containers already restart on their own
(`restart: unless-stopped`).

## Profile persistence

Browser state lives on the host and survives container rebuilds:

- state root: `~/.surf` (override with `SURF_STATE_DIR`)
- container profile: `~/.surf/browser/profiles/container/<profile>`
- per-agent profiles: `.../<profile>/agents/<name>/`

Use `--profile` across `build`/`start`/`status` to switch profile context.
(Stale Chrome singleton locks and crashed exit flags are cleared at
container boot so restarts always come up clean.)

## Settings

`~/.si/surf/settings.toml` (auto-created, mode `0600`):

```bash
surf config show            # whole file (includes secrets)
surf config set --key proxy.auth_token --value <token>
surf config set --key proxy.tokens.alice --value <token>
surf config set --key browser.agents --value alice,bob
surf config set --key browser.vnc_password --value <password>
surf config set --key browser.host_novnc_bind --value 127.0.0.1
surf config set --key tunnel.mode --value token
```

When `browser.vnc_password` is empty, `surf start` generates a strong
per-start viewer password and prints it.

## Release and versioning

`surf` follows `docs/RELEASING.md`:

- version source of truth: root `Cargo.toml` `[workspace.package].version`
  (surfaced as `SURF_VERSION`)
- tag must match the minor release version
- browser image publish to GHCR: `ghcr.io/aureuma/surf-browser`
