# surf

`surf` is a Go-first browser runtime for SI and standalone automation.

It provides:
- Dockerized headed Playwright MCP runtime
- noVNC access for visual browser sessions
- optional Cloudflare quick tunnel exposure for noVNC
- lightweight Chrome extension scaffold/install flow
- MCP compatibility proxy (`/mcp` -> `/sse` GET rewrite)

## Install

```bash
go install github.com/Aureuma/surf/cmd/surf@latest
```

## Quick start

```bash
surf build
surf start
surf status
surf logs
```

Default endpoints:
- MCP: `http://127.0.0.1:8932/mcp`
- noVNC: `http://127.0.0.1:6080/vnc.html?autoconnect=1&resize=scale`

## Expose noVNC over internet (Cloudflare)

```bash
surf tunnel start
surf tunnel status
surf tunnel logs
```

This uses `cloudflare/cloudflared` in Docker to create a quick tunnel to the local noVNC endpoint.

## Chrome extension scaffold

```bash
surf extension install
surf extension path
surf extension doctor
```

## SI integration

`si` exposes this as:

```bash
si surf <...>
```

`si` is a thin interface; runtime implementation lives in this repo.

## Release

- CI: `.github/workflows/ci.yml`
- Release binaries + Docker image: `.github/workflows/release.yml`
- Target binaries: linux/macos, amd64/arm64
