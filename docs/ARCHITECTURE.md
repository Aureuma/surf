# Architecture

`surf` is split into three layers:

1. Runtime orchestration
- Docker lifecycle for Playwright headed MCP container
- status/health probing for MCP and noVNC endpoints

2. Access layer
- local reverse proxy mode for MCP path compatibility
- optional public tunnel (cloudflared) for noVNC observation

3. Integration layer
- standalone CLI (`surf ...`)
- SI bridge (`si surf ...`) shelling into `surf`

Design goals:
- Go-first implementation
- deterministic runtime behavior
- clear operator errors when dependencies are missing
