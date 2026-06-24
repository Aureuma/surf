# Repo Rules
## Version Source Of Truth
- Keep one repo-wide version for `surf`.
- The canonical hard-coded version source is the root `Cargo.toml` under `[workspace.package].version`.
- Member crates must inherit that version with `version.workspace = true` instead of carrying their own hard-coded copies.
