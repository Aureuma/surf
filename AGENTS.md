# Repo Rules

- Follow the shared workspace rules in `/home/shawn/Development/AGENTS.md`.

## Secrets And Credentials

- `si fort` is the canonical interface for secret and credential management for this repo. Use raw `si vault` only for explicit Fort/SI Vault maintenance or required local encryption work under the shared workspace rules.

## Version Source Of Truth

- Keep one repo-wide version for `surf`.
- The canonical hard-coded version source is the root `Cargo.toml` under `[workspace.package].version`.
- Member crates must inherit that version with `version.workspace = true` instead of carrying their own hard-coded copies.
- Every commit that changes tracked content in this repo must bump the patch version in the root workspace version in the same commit.
