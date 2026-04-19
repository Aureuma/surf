# Changelog

All notable changes to this project will be documented in this file.

## [v0.1.7] - 2026-04-19
### Fixed
- Regenerated Surf's workspace lockfile so `cargo build --locked` succeeds again for the browser runtime Docker build.

### Verified
- Confirmed the locked workspace check passes after the lockfile refresh.

## [v0.1.5] - 2026-04-19
### Fixed
- Forced the Surf Fluxbox runtime to use a single workspace by default so the noVNC viewer no longer starts with four desktops.

## [v0.1.4] - 2026-04-18
### Changed
- Updated Surf's Viva-managed public hostnames to `surf-browser.aureuma.ai` for noVNC and `surf-browser-mcp.aureuma.ai` for MCP.

### Verified
- Confirmed that the public `surf-browser.aureuma.ai` viewer serves `vnc.html`, and `surf-browser-mcp.aureuma.ai/mcp` reaches the Surf MCP service through the Viva dev tunnel.

## [v0.1.3] - 2026-04-18
### Changed
- Replaced Surf's fixed default noVNC password with generated per-start viewer passwords unless an explicit password is configured.
- Surf start output now surfaces generated viewer passwords and warns when an explicit password is weak or still using the legacy `surf` placeholder.

### Verified
- Confirmed that the public `surf-browser.aureuma.ai` viewer serves `vnc.html` and accepts websocket connections on `/websockify` through the Viva dev tunnel.

## [v0.1.2] - 2026-04-18
### Changed
- Updated Surf tunnel secret resolution and documentation to use `si fort` rather than direct `si vault` access.
- Clarified that shared dev HTTPS viewing should normally flow through Viva's existing dev tunnel while leaving MCP private by default.

### Fixed
- Fixed the default browser image build path so `si surf build` uses the repo root as Docker build context.

## [v0.1.1] - 2026-02-27
### Added
- Added persistent Surf settings at `~/.si/surf/settings.toml` with automatic directory/file bootstrap.
- Added `surf config` subcommands: `show|get|set|path|init`.
- Added coverage for settings load/write behavior and default resolution in the Surf test suite.

### Changed
- Updated tunnel startup defaults to read from Surf settings while still honoring `SURF_*` environment overrides.
- Updated browser/runtime defaults to read from Surf settings and state directory configuration.
- Expanded release documentation by adopting SI-style runbook and release policy docs.

### Fixed
- Improved startup behavior by best-effort settings bootstrap on bare `surf` invocation.
- Improved host runtime reliability when running as root (profile and data-dir handling).

## [v0.1.0] - 2026-02-26
### Added
- Initial public Surf CLI release with browser runtime management, host mode, tunnel support, and release asset workflows.
