# Publish change log

Status: **requested by McKale 2026-10-02 for build 21.** Every publish that
changes something writes a change log, `CHANGELOG.md`, stores it with that
version of the listing, and Studio can download it afterwards. It is the
author's record; the Gallery does not show it (public text would need
moderation, a separate decision). Studio's half is staged for build 21;
the Worker's half is in the tree for staging.

## What it says

Built by Studio (`echk_publish`) at the end of a publish, from what it
already knows. A publish that changed nothing writes none.

```markdown
# Box Head Zombie Defense Tycoon: version 7

Published Oct 2, 2026 at 6:26 PM (author's local time) to staging (api-staging.eustress.dev). Listing 07d084d8-....

## Listing

- Max players: 10 → 4
- Tags: zombies, tycoon → zombies, tycoon, co-op
- Cover: none → added (600 × 800)
- Rating: Teen 13+ before this version; the review rates this version.

## World

- Space "Main": 3 chunks changed, 1 chunk added (12 records, 340 KB). Folders touched: Workspace/Map, Workspace/Zombies.
- Space "Lobby": added (4 chunks, 88 records, 1.2 MB).
- Space "Old Test": removed (2 chunks, 300 KB).
- Shared assets: 1 chunk added (2.4 MB), 2 chunks removed.

## Totals

- Players download 18.6 MB in 42 chunks. This publish uploaded 6 chunks (3.9 MB).
```

- **Listing:** each field that changed, old → new: name, description (its
  length and the first line, not the whole text), max players, tags,
  cover (added, replaced, removed). Old values come from
  `GET /api/simulations/{id}` just before the cover and the commit. Tags
  compare as the API stores them, in any order. Only a whole publish
  sends the listing's text, so a Space-only publish lists no text fields,
  and a cover-only update lists only the cover. Rating is not the
  author's to change: the line says what it was and, when the commit
  answered `in_review`, that the review rates this version; otherwise
  that it is unchanged. A new listing says so; a listing the API did not
  answer for says its changes could not be listed.
- **World:** per Space, by comparing the previously published manifest
  with the new one, chunk by grid position: Spaces added or removed,
  chunks added, changed or removed, with record counts and sizes, and the
  top-level folders (one or two Space-relative segments) the added or
  changed chunks touch, read from those chunks' record paths, never their
  contents; a path that is not a safe record path names nothing. Studio
  reads at most 256 MB of chunks again for folder names, and the totals
  say so when it stopped. Shared assets are named by their content, so
  they are added or removed, never changed. A first `.echk` publish lists
  every Space as added; a published world the API did not answer for
  gets a line saying so.
- **Totals:** what a player downloads, and what this publish uploaded.
- Times are 12-hour, the author's local time. Every line is one line:
  control characters become spaces (the Worker refuses them). At most
  64 KB; a longer log stops at a whole line and ends with one saying the
  rest was left out.

## Where it is kept (Worker)

Author-only routes, the same session and ownership check as the world
routes (`ownedListing`):

| Route | What |
|---|---|
| `POST /api/simulations/{id}/changelogs` | Body `{ "version": n, "publish_hash": "...", "markdown": "..." }`. Stores it as the next log for that listing. `markdown` is text, at most 64 KB. Returns `{ "id": "..." }`. |
| `GET /api/simulations/{id}/changelogs` | The listing's logs, newest first, at most 50: `[{ "id", "version", "publish_hash", "created_at", "bytes" }]`. |
| `GET /api/simulations/{id}/changelogs/{log_id}` | The log, `text/markdown; charset=utf-8`, with `Content-Disposition: attachment; filename="CHANGELOG-v7.md"`. |

- Anyone but the author (and an admin) gets `404`, the same as a missing
  log.
- Stored in R2 under the listing; deleted with the listing.
- Studio posts the log right after a successful publish (after the
  commits, or after a cover-only update). The body is exactly
  `{ version, publish_hash, markdown }`: the version the commit answered
  (a cover-only update: the listing's own, which it keeps) and the hash
  the commit sent, which the Worker checks against the listing
  (`409 changelog_mismatch` otherwise). A failed post, or a publish the
  API gave no version for, does not fail the publish: the Output says the
  log was not stored, and Studio keeps the text for this session.

## Studio

- `echk_publish::Published` carries `changelog: Option<String>` (the
  markdown), `version: Option<u32>` and `changelog_stored: bool`.
- After a successful publish the Publish dialog shows **Download change log
  (.md)**, which opens a save dialog (`CHANGELOG-v7.md` by default). It
  saves the text Studio just built, so it works even if storing failed.
- Earlier versions (`multiplayer.rs`): a `RefreshMyChangeLogs { listing_id }`
  message fills the `MyChangeLogs` resource (`state`, `listing`, `items`
  of `ChangeLogSummary { id, version, publish_hash, created_at, bytes }`,
  newest first) off the frame; asking about another listing drops the
  answer still owed for the last one. `DownloadChangeLog { listing_id,
  log_id, save_to }` saves one log through a sibling temporary file, at
  most 64 KB, and `MyChangeLogs.downloads[log_id]` says where it went or
  why not. Only ids made of letters, digits, `-` and `_` go into a URL.

## Owners

- Multiplayer: building the log, posting it, `Published.changelog`
  (`engine/src/space/echk_publish.rs`), and the earlier-logs fetch
  (`engine/src/multiplayer.rs`).
- Jev: the three Worker routes (with the world routes), and the publish
  handler passing `Published.changelog` to the dialog.
- UI: the button, the save dialog and the earlier-logs list.
- Security: a review; a log holds folder and Space names.
