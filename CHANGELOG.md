# Changelog

All notable changes to this project will be documented in this file.

## [v0.1.1] - 2026-02-27
### Added
- Added persistent Surf settings at `~/.si/surf/settings.toml` with automatic directory/file bootstrap.
- Added `surf config` subcommands: `show|get|set|path|init`.
- Added coverage for settings load/write behavior and default resolution in `cmd/surf/settings_test.go`.

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
