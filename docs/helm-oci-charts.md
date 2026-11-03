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
refuses `/v2/_catalog` to anonymous users, so name the charts you want with
`repositories` ([#161](https://github.com/pgmac-net/docker-registry-walk/issues/161)).
They are listed straight away, marked `★`, and the refused catalog is a status
message instead of a prompt:

```toml
[[registry]]
name = "8gears"
url = "https://8gears.container-registry.com"
repositories = ["library/n8n"]
```

`oci://ghcr.io/actions/actions-runner-controller-charts/` — GHCR. Anonymously
the packages API lists nothing, so seed the charts under that prefix:

```toml
[[registry]]
name = "ghcr-actions"
url = "https://ghcr.io"
type = "ghcr"
owner = "actions"
repositories = [
  "actions/actions-runner-controller-charts/gha-runner-scale-set-controller",
  "actions/actions-runner-controller-charts/gha-runner-scale-set",
]
```

Without `repositories`, the browse-by-name prompt still opens when discovery
fails; type the full chart path there. Either way, `o` opens any other chart by
name.

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

Verified against a local `registry:2` (delete enabled) with charts pushed by
`helm push` (issue [#159](https://github.com/pgmac-net/docker-registry-walk/issues/159)).
These operations are generic manifest and blob operations and nothing in them
assumes an image config, so charts behave like images:

| Op | On a chart |
|----|-----------|
| Copy-to `C` | Works. Manifest digest is preserved, and `helm pull` of the copy is byte-identical to the original |
| Export `e` | Works. The OCI-layout tar round-trips: `skopeo copy oci-archive:… docker://…` back into the registry, then `helm pull`, gives the same manifest digest and tarball |
| Retag `r` | Works, but the new tag is just a tag: it need not match the chart's `Chart.yaml` `version` |
| Diff `D` | Works: shows the chart layer added/removed |
| Delete `d` | Works. It deletes **by digest**, so every tag pointing at that manifest goes with it; since [#163](https://github.com/pgmac-net/docker-registry-walk/issues/163) the confirm dialog names those tags first, the delete is refused if the tag moved meanwhile, and the list is reloaded afterwards (see "Deleting tags" in the README) |
| Prune `P` | Finds nothing. It matches tags literally named `sha256:<hex>`, which cannot exist on a spec-compliant registry (`:` is not a legal tag character) |

Export writes to `<dest>.partial` and renames it into place only when the
archive is complete, so a file at the destination is always a whole export.

Copy and export used to crash the TUI on their first progress tick
(`Sender::blocking_send` called from a tokio worker). That affected container
images as well as charts and is fixed in #159.

## Tag order

Chart versions sort by SemVer precedence, newest first, so `0.15.1` is at the top
and `0.10.0` follows `0.9.3` (issue
[#160](https://github.com/pgmac-net/docker-registry-walk/issues/160)). `s`
flips the order. Helm's `_` for `+` build metadata is understood. See "Tag sort
order" in the README.

## Not covered (follow-ups)

- Delete removing sibling tags silently, prune never matching, and retag vs
  `Chart.yaml` `version` (filed as separate issues from #159).
