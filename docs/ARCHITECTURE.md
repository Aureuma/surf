# Architecture

`surf` is a Playwright MCP server with three layers:

1. Browser runtime (container `surf-playwright-mcp-headed`)
- Xvfb + Fluxbox + x11vnc + noVNC (stock viewer at `/vnc.html`)
- one `@playwright/mcp` (baked into the image) per agent, serving
  Streamable HTTP at container ports `8931+slot`
- one Fluxbox workspace per agent named `<profile>/<agent>`; a supervisor
  loop maps each Chrome window to its agent via PID ancestry and keeps it
  on the agent's workspace
- persisted profiles bind-mounted from
  `~/.surf/browser/profiles/container/<profile>[/agents/<name>]`
- raw MCP published on loopback only (`127.0.0.1:8932+slot`); noVNC on all
  interfaces (`0.0.0.0:6080`) behind the VNC password
- stale Chrome locks and crashed exit flags cleared at boot

2. Auth gateway (`surf proxy`, host process on `0.0.0.0:8931`)
- routes `/mcp` to slot 0 and `/agent/<name>/mcp` to that agent's slot
- requires the main token (`proxy.auth_token`) or the agent token
  (`proxy.tokens.<name>`) via `Authorization: Bearer` or `?token=`
- tokens persisted in settings, generated on first run

3. Public access (`surf tunnel`, cloudflared containers)
- quick mode: ephemeral `*.trycloudflare.com` URL for one origin
- token mode: named tunnel with dashboard-managed ingress (stable hostnames)
- loopback origins are rewritten to `host.docker.internal` so the tunnel
  container can reach host services

Design goals:
- Rust-first orchestration, stock upstream browser tooling
- raw unauthenticated MCP never leaves the box
- every public surface is secret-gated (proxy token, VNC password)
- explicit profile persistence paths
- clear operator errors when dependencies are missing
