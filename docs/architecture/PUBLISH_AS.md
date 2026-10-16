# Publish As

Status: **build 18 (McKale, 2026-10-01).** Publish into a listing other than
this Universe's own, as Roblox's "Publish to Roblox As...", so a second copy
of a game can overwrite the published one instead of creating a second
listing.

## What the author sees

- File > **Publish As...** opens the Publish dialog with its **Publish to**
  picker focused, on "New listing". A plain Publish starts on this
  Universe's own listing.
- The picker lists "New listing", then the author's own listings on the API
  in use, newest first, each with its state and when it last changed.
- Picking another listing asks once more, inline: "This replaces the
  published version of <title>. Its players, visits and review history
  stay; the new content is reviewed again."
- Publish As is for the whole Universe; a Space-only publish keeps the
  Universe's own listing.

## How it works

- **Target:** `eustress_common::project_manifest::PublishTarget`:
  `Remembered` (this Universe's own listing on the API in use, a new one
  the first time; the default, and what MCP sends), `New` (a new listing
  whatever it had), `Existing(id)` (one of the author's own listings).
  `PublishRequest.target` carries it from the dialog to the publish.
- **The list:** `crate::multiplayer::MyListings` (state, items, `own`),
  refreshed when the dialog writes `RefreshMyListings`: `GET /api/projects`
  on the API in use with the signed-in token, off the frame. The route
  already exists and lists only the caller's own listings.
- **The publish:** `echk_publish::publish_to(plan, token, listing,
  space_only, &target, progress)`; `publish` is `publish_to` with
  `Remembered`.
  - A chosen listing (`Existing`) becomes this Universe's listing on the
    API in use (its `sync.toml`, for that API only) **only after** the API
    accepts the upload into it (`world/begin`). The API accepts only for
    the listing's author, and never while the listing is locked (a legal
    hold, a quarantine, or a publish-frozen author).
  - A refusal of a chosen listing never falls back to creating a new one:
    a 404, or a 403 without a code, reads "That listing isn't one of yours
    on <server>, so it can't be replaced. Nothing was published."; a 409
    reads "That listing is being reviewed and can't be replaced right now."
  - `New` creates a listing and makes it this Universe's listing, as a first
    publish does.
- **The Worker** needs no change: `world.mjs` `ownedListing` already
  refuses a listing the caller does not author (403) and a locked listing
  (409 `in_review`) on begin, every chunk and commit.

## Owners

- Multiplayer: `project_manifest.rs` (`PublishTarget`), `multiplayer.rs`
  (`MyListings`), `echk_publish.rs` (`publish_to`).
- UI: the picker, the confirm, File > Publish As..., and
  `PublishRequest.target` in `file_dialogs.rs` with both literals that list
  every field (`slint_ui.rs` on_publish, `engine_bridge/ui_surface.rs`).
- Jev: `file_event_handler.rs` calls `publish_to` with `request.target`.
- MCP (build 19): the publish tool's `listing_id` ("new" or an id).

## Tests

- `echk_publish::tests::publish_as_starts_from_the_chosen_listing`: the
  starting listing per target, and the refusal sentences (someone else's,
  missing, locked, and another 403 keeping its own reason).
- `echk_publish::tests::publish_as_rewrites_this_apis_listing_only`: the
  chosen listing replaces this API's entry in `sync.toml` and leaves other
  APIs' entries.
- `multiplayer::tests::publish_as_lists_the_authors_listings`: parsing
  `GET /api/projects` newest first, and the Universe's own listing.
