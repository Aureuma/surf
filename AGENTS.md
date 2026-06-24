# Repo Rules
This repository follows the global instructions in `/home/shawn/Development/AGENTS.md`; local entries below only add repository-specific overrides.
## Version Source Of Truth
- The canonical hard-coded version source is the root `Cargo.toml` under `[workspace.package].version`.
- Member crates must inherit that version with `version.workspace = true` instead of carrying their own hard-coded copies.
