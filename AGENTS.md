# Repo Rules

- Follow the shared workspace rules in `/home/shawn/Development/AGENTS.md`.
- Keep every `tickets/` directory and all files beneath it ignored by Git. Do not add, force-add, or commit ticket files to version control.

## Secrets And Credentials

- `si fort` is the canonical interface for secret and credential management for this repo. Use raw `si vault` only for explicit Fort/SI Vault maintenance or required local encryption work under the shared workspace rules.

## Version Source Of Truth

- Keep one repo-wide version for `surf`.
- The canonical hard-coded version source is the root `Cargo.toml` under `[workspace.package].version`.
- Member crates must inherit that version with `version.workspace = true` instead of carrying their own hard-coded copies.
- Every commit that changes tracked content in this repo must bump the patch version in the root workspace version in the same commit.

### File Names And Paths

- Name new files with stable, descriptive kebab-case unless the repo already has a stricter local convention; keep Viva deploy surfaces predictable with `deploy/viva.<env>.yaml`, `deploy/viva.<component>.<env>.yaml`, and `docker-compose.viva.<env>.yml`.
- Keep file paths predictable and reviewable: avoid unrelated renames, generated churn, and mixed concerns; when moving or renaming files, update all references in the same change.


## Node Package Manager
- For Node-based workspaces in this repository, the preferred package manager is `pnpm` (use `corepack pnpm ...` by default).

## Message Readability
- Emojify reports/messages where it improves readability, using relevant emojis only.
