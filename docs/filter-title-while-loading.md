# Filter query stays visible while a list loads

Issue [#146](https://github.com/pgmac-net/docker-registry-walk/issues/146).

## Problem

Pressing `/` in the Repos or Tags pane shows the query in the pane's border
title (` Repos / nginx `). While a remote page fetch was in flight the title
became ` Repositories ⠹ `, so the query vanished until loading finished. On
registries with many pages, `on_*_page` flips the state to `Idle` and the event
loop flips it back to `Loading` for the next page, so the query also flickered.

Cause: `draw_repos`/`draw_tags` matched on `LoadState` first and only checked
`filter_mode` inside the `Idle` arm, treating the load marker and the query as
mutually exclusive.

## Fix

One pure helper, `pane_title` in `src/tui/ui.rs`, shared by both panes. The load
marker (spinner or `✗`) comes first so a long query clips rather than the marker.

| State   | Typing filter         | Kept filter                   | No filter           |
|---------|-----------------------|-------------------------------|---------------------|
| Loading | ` Repos ⠹ / nginx `   | ` Repositories ⠹ (filtered) ` | ` Repositories ⠹ `  |
| Error   | ` Repos ✗ / nginx `   | ` Repositories ✗ `            | ` Repositories ✗ `  |
| Idle    | ` Repos / nginx `     | unchanged                     | unchanged           |

Tags is identical with `Tags` as both names.

## Decisions

- Title, not a dedicated input row: no layout shift, no list-height change.
  A very long query still clips at pane width, as before.
- Also covers typing in the Error state and a kept filter during loading, since
  they shared the root cause.
- Modal pickers (Docker Hub search, Artifactory, GHCR) were not affected; their
  input lives in the box body.

## Tests

`pane_title_matrix` covers the table; two render tests assert the typed query is
on screen during `Loading` for Repos and Tags.
