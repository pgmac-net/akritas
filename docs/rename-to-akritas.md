# Rename to akritas (issue #173)

Work record for [#173](https://github.com/pgmac-net/akritas/issues/173). The name was chosen in [#172](https://github.com/pgmac-net/akritas/issues/172). Decision record: [ADR 0001](adr/0001-rename-to-akritas.md). User-facing migration notes: the README section "Migrating from docker-registry-walk".

## What changed

- **Identity:** the package and binary are now `akritas`. `config::APP_NAME` (`CARGO_PKG_NAME`) drives the config directory, keychain service prefix, GHCR User-Agent and TUI title bar, so the name lives in one place.
- **Config migration:** `Config::migrate_legacy`, called in `main` before `Config::load`, copies `docker-registry-walk/config.toml` to `akritas/config.toml` when only the old file exists. It prints `migrated config: <old> -> <new> (old copy left in place)` to stderr before the TUI starts.
- **Keychain migration:** `KeyringStore` now holds both service names.
  - `get_password` falls back to the legacy name and copies a hit forward. A failed copy still returns the secret.
  - `set_password` writes the new name only.
  - `delete_password` clears both names.
  - The OS calls (the `keyring` crate, then the `secret-tool` fallback) moved behind a private `KeyringBackend` trait, so the logic is unit-tested against an in-memory keychain.
- **Release:** artefacts are renamed to `akritas-<os>-<arch>`.
- **Docs:** README (new tagline, "Formerly" note, migration section), CLAUDE.md (new invariant), and every `docs/` page and repo URL. `specs/` is left as written; the constitution title is updated.

## Process

The ticket went through the pickup-ticket flow: pickup, then a five-question grilling session, a plan posted to the ticket, approval, and implementation on Opus.

The grilling settled:
1. the repo rename goes through terraform-github with a `moved` block;
2. delete clears both keychain names;
3. the version is v0.11.0 (`feat!`);
4. the docs scope;
5. a stderr notice, with the fallbacks removed in v0.13.0.

## Verification

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`: 364 passed, 12 of them new:
  - config migration: copy, no-op when the new file exists, no-op when neither exists, and paths;
  - keychain: current first; legacy fallback and copy-forward; failed copy; miss; set new only; delete clears both; delete with a single entry; delete with none.
- Live, in tmux with a temporary `XDG_CONFIG_HOME`:
  - The legacy config was copied byte-identically, the notice was printed, the title bar showed `akritas v0.10.0`, and the second run printed nothing.
  - A throwaway `docker-registry-walk/kr-smoke` secret-tool entry was copied forward to `akritas/kr-smoke` on connect, with the legacy entry kept. Both entries were removed afterwards.

## Deviations from plan

- The `migrate_legacy_config` helper returns `bool`, and `Config::migrate_legacy` returns the `(old, new)` paths; the plan had a single function returning the paths.
- Pushing hit a GitHub incident (Git Operations degraded, HTTP 500s), so the branch was pushed by a retry loop over HTTPS.

## Remaining steps

1. Merge this PR.
2. terraform-github PR: rekey `config/repos.yaml` to `akritas`, add the `moved` block, update the description and topics, and confirm the plan shows 0 to destroy. Apply locally.
3. Bump to v0.11.0 and tag, after the repo rename.
4. Rename the local clone and Notion parent page.
5. Follow-up issue: remove the fallbacks in v0.13.0.
