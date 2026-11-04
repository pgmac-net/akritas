# ADR 0001: Rename to `akritas`, with one-way auto-migration

- **Status:** Accepted
- **Date:** 2026-11-04
- **Issues:** [#172](https://github.com/pgmac-net/akritas/issues/172) (name), [#173](https://github.com/pgmac-net/akritas/issues/173) (implementation)

## Context

`docker-registry-walk` no longer described the app. It browses OCI artifacts and Helm charts as well as Docker images, it manages registries (copy, retag, delete, export) rather than just walking them, and HTTP Helm repositories (classic `index.yaml`, which is not OCI) are committed future work.

The old name was baked into user state. The config file lived at `~/.config/docker-registry-walk/config.toml`, and credentials sat in the keychain under `docker-registry-walk/<registry>`. Changing the name without carrying that state across would make the upgrade look like data loss: an empty config and a credential prompt for every registry.

## Decision

1. **Name:** `akritas` (ἀκρίτας, the Byzantine frontier guard who patrolled the empire's borders). It is format-neutral: no "docker" and no "oci", because HTTP Helm is not OCI. It is 7 characters, and its crate and GitHub names were unclaimed. The shortlist is in #172.
2. **One string everywhere:** the repo, binary, config directory, keychain service prefix, User-Agent and title bar all derive from `config::APP_NAME` (`CARGO_PKG_NAME`).
3. **Config migration:** copy the old `config.toml` to the new path only when the new file is missing. This runs before `Config::load`, which writes example content to a missing path. A one-line notice goes to stderr before the TUI starts. The old file is kept.
4. **Keychain migration:**
   - **Get:** read the current service first, then fall back to the legacy one, and copy a legacy hit forward. A failed copy still returns the secret, silently, because the TUI owns the terminal.
   - **Set:** write the current service only.
   - **Delete:** clear both services. It succeeds if either delete does.
5. **Lifetime:** both fallbacks stay for v0.11.x–v0.12.x and are removed in v0.13.0.
6. **Repo rename:** done in terraform-github by rekeying `config/repos.yaml` plus a `moved` block, so the plan is an in-place rename with zero destroys. A bare rekey would plan a destroy/create, and `archive_on_destroy` would archive the real repo.
7. **Version:** v0.11.0, `feat!`, following the pre-1.0 convention where breaking changes bump the minor version.

## Consequences

- Users upgrading keep their config and credentials without doing anything, and a downgrade still finds the old data, because nothing is deleted.
- A credential deleted after the upgrade stays deleted, because delete clears both names. This is the reason delete is not symmetrical with set.
- Old keychain entries and the old config directory linger until the user removes them; the app never cleans them up.
- The old binary name is gone; there is no shim or alias.
- Historical `specs/` artefacts keep the old name on purpose.
