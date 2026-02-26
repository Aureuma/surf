# Architecture

`surf` is split into four layers:

1. Runtime orchestration
- Docker lifecycle for Playwright headed MCP container
- status/health probing for MCP and noVNC endpoints
- persisted container profile directories

2. Host browser orchestration (macOS/Linux)
- launches local Chromium-based browser in headed mode
- isolated host profile directories and process state
- CDP status probing

3. Access layer
- local reverse proxy mode for MCP path compatibility
- optional Cloudflare tunnel for internet noVNC observation
- quick mode and token mode

4. Integration layer
- standalone CLI (`surf ...`)
- SI bridge (`si surf ...`) as interface
- vault-oriented secret plumbing through `si vault`

Design goals:
- Go-first implementation
- deterministic runtime behavior
- explicit profile persistence paths
- clear operator errors when dependencies are missing
