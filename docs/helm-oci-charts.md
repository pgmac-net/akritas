# Helm chart OCI registries

How `docker-registry-walk` browses Helm charts stored in OCI registries
(`helm push` / `oci://…`). Issue
[#157](https://github.com/pgmac-net/docker-registry-walk/issues/157).

## Short answer

A Helm chart in an OCI registry is an ordinary OCI artifact: a repository, tags
(chart versions), and a manifest whose **config blob** is `Chart.yaml` rather
than an image config. So charts need no special registry type and no special
config — add the registry like any other and open the chart's repository.
Charts are recognised **per tag**, from the manifest's config media type
`application/vnd.cncf.helm.config.v1+json`, and shown accordingly.

One registry can serve images and charts side by side (ghcr.io and Harbor both
do), which is why this is detected per tag and not configured per profile.

## Configuration

`oci://8gears.container-registry.com/library/n8n` — a Harbor registry. Harbor
refuses `/v2/_catalog` to anonymous users, so the repo list is empty and the
TUI offers *browse by name* straight away:

```toml
[[registry]]
name = "8gears"
url = "https://8gears.container-registry.com"
```

Then type `library/n8n` at the prompt.

`oci://ghcr.io/actions/actions-runner-controller-charts/` — GHCR. Anonymously,
type the full chart path at the browse-by-name prompt:

```toml
[[registry]]
name = "ghcr-actions"
url = "https://ghcr.io"
type = "ghcr"
owner = "actions"
```

```
actions/actions-runner-controller-charts/gha-runner-scale-set-controller
actions/actions-runner-controller-charts/gha-runner-scale-set
```

With a personal access token that has `read:packages` (`--token` or `$CR_PAT`)
and `owner = "actions"`, the GHCR picker lists the owner's packages including
charts, and you can filter by name. This path needs a token with that scope and
was not exercised against a live token while writing this.

An `oci://` URL names a *repository* (host + path), or sometimes just a
namespace prefix. The registry URL in config is only ever the host. A prefix
such as `…/actions-runner-controller-charts/` is not itself a repository and
cannot be listed anonymously — name a chart under it.

## What the TUI shows for a chart

Selecting a chart tag shows, in the Details pane:

| Line | Source |
|------|--------|
| `Helm pull` | `helm pull oci://<host>/<repo> --version <tag>` |
| `Digest`, `Type` | manifest |
| `Kind` | `Helm chart` |
| `Chart`, `Version`, `App version`, `Description` | the chart config blob (`Chart.yaml`); each line omitted if absent |
| `Created` | the `org.opencontainers.image.created` manifest annotation, if present |
| `Total size`, `Layers` | manifest |

`OS/Arch` and image `Labels` are not shown — they do not exist for charts.
`c` copies the `helm pull … --version …` command. The `--version` form is used
because it is the unambiguous one; tag-in-reference support varies by Helm
version. `i` (Inspect) shows the raw manifest and the chart config as JSON.

## Changes made for charts

Three general behaviours were fixed, and each helps non-Helm registries too:

1. **Anonymous catalog refusal is no longer a dead end.** A profile with no
   credential source that gets a 401 on `/v2/_catalog` now opens the
   browse-by-name prompt (`should_offer_browse` in `tui/event.rs`). Previously
   only an error was shown. The silent credential re-read and credential
   prompts are unchanged for profiles that have a credential source.
2. **Details follow the browsed repository.** The Details fetch is keyed on
   `App::current_repo` (`detail_repo`), not the Repos-pane selection. After
   browse-by-name — or a Docker Hub search pick — the pane is empty but the
   tags are loaded, and Details never appeared. When the catalog works, the two
   are equal, so nothing changes there.
3. **Anonymous profiles answer a Bearer challenge.** A profile with no
   credentials used a bare client, so on a token-auth registry (Harbor, Quay,
   …) every tags/manifest request got 401: the anonymous token exchange that an
   unauthenticated `docker pull` performs never happened. No-credential
   profiles now use `GhcrCredentials::anonymous`, which sends nothing up front
   and answers a **same-origin** Bearer challenge anonymously. Registries that
   issue no challenge see no change.

## Other operations on charts

Copy-to (`C`), retag (`r`), diff (`D`), export (`e`), prune (`P`) and delete
(`d`) are generic manifest and blob operations and do not look at the media
type, so they are expected to work on charts. This was **not verified** against
a writable chart registry. Both sample registries are read-only to us.

## Not covered (follow-ups)

- Verify write operations on charts against a local `registry:2` with
  `helm push`.
- Semver-aware tag sort. Tags sort lexically (`0.10.0` before `0.2.0`).
- A `repositories = [...]` seed list for registries with no browsable catalog,
  so a known set of charts is one keystroke away.
