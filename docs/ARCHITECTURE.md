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
- optional Cloudflare tunnel for internet noVNC observation with generated operator-visible viewer passwords when none are configured
- quick mode and Fort-backed token mode
- shared HTTPS browser viewing typically routed through Viva's dev Cloudflare tunnel as `surf-browser.aureuma.ai`, with optional MCP at `surf-browser-mcp.aureuma.ai`, and ideally gated by Cloudflare Access

4. Integration layer
- internal surf runtime binary
- SI bridge (`si surf ...`) as the public interface
- Fort-oriented secret plumbing through `si fort`
5. Existing-session action layer
- Chrome CDP attach for real user tabs
- text-first DOM understanding (`elements`, `text`, `copy`)
- interactive actions over CDP input events (`click`, `type`, `paste`, `scroll`)
- humanized timing/mouse movement controls
- domain allowlist/blocklist policy gating

6. Release and operational tooling
- Cargo workspace rooted at repo root
- `crates/surf` for the runtime and CLI surface
- `crates/xtask` for release validation, packaging, checksums, and CI helpers
- Rust-native browser container entrypoint for Xvfb, VNC, noVNC, and Playwright MCP bootstrap

Design goals:
- Rust-first implementation
- deterministic runtime behavior
- explicit profile persistence paths
- clear operator errors when dependencies are missing
- transparent controls for consent/policy oriented automation
