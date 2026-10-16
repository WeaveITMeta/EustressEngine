# Listing cover: the box art a Gallery card shows

Status: **Worker side built 2026-10-01; deployed on the orchestrator's
schedule.** A listing may carry a cover the creator uploads: portrait box art
or 16:9. The Gallery shows the cover when the listing has one and the
thumbnail otherwise. Studio's Publish dialog and the MCP publish tool set it
through the routes below; the Gallery reads it from the listing views.

Code: `infrastructure/cloudflare/api/src/cover.mjs`. Tests:
`tests/cover.test.mjs`.

## The routes

All three are on `/api/simulations/{id}/cover`.

| Route | Who | What |
|---|---|---|
| `PUT` | the author | The request body is the image bytes. Stores the cover and replaces any earlier one. |
| `DELETE` | the author | Removes the cover. Idempotent. |
| `GET` | anyone, within the rules under "Who sees it" | The image. |

`PUT` and `DELETE` take the author's session (`Authorization: Bearer ...`),
the same as the thumbnail upload, and are reversible, so they are session
routes in `route_policy.mjs`.

### PUT

Send the bytes as the body. The body is read only as far as the 2 MB cap, so a
body with no declared length (chunked) or one that never ends is cut off and
refused with `413` as soon as more than 2 MB has arrived. The `Content-Type`
header is **never read**: the
type comes from the bytes themselves, so a PNG labelled `text/html` is a PNG
and an HTML page labelled `image/png` is refused. (Send the right label
anyway.)

`200`:

```json
{
  "ok": true,
  "review": "submit",
  "cover_url": "https://api.eustress.dev/api/simulations/{id}/cover?v=1767225600000",
  "cover": { "width": 600, "height": 800 }
}
```

`review: "submit"` says a review is owed: see "Review" below.

Refusals. Every one is JSON `{ "error": "...", "code": "..." }`, and every
`error` for a bad image states the rules, so a dialog can show it as it is.

| Status | `code` | When |
|---|---|---|
| 400 | `cover_empty` | The body is empty. |
| 401 | none | No session. |
| 404 | none | The listing does not exist **or is not yours**: one answer for both. |
| 409 | `in_review` | A hold is on the listing, or the author's publishing is frozen. The same answer and the same wording as every other write to a listing; it never says why. |
| 413 | `cover_too_large` | More than 2 MB (declared or actual). |
| 415 | `cover_bad_type` | Not a PNG, JPEG or WebP whose size can be read from its header, or larger than the platform accepts for any image, or a file that cannot be read to its end. SVG is never accepted. |
| 422 | `cover_too_small` | The short side is under 320 px. |
| 422 | `cover_bad_ratio` | The width to height ratio is outside 1:2 to 2:1. |

### DELETE

`200` `{ "ok": true, "removed": true, "review": "submit" }`. When there was no
cover, nothing changed and nothing needs a review:
`{ "ok": true, "removed": false, "review": null }`. The same `401`, `404`
and `409` as `PUT`.

### GET

`200` with the image, or `404` `{ "error": "No cover" }`. See "Who sees it".

## The rules

| Rule | Value |
|---|---|
| Type | PNG, JPEG or WebP, taken from the file's own bytes. Never SVG. |
| Weight | At most 2 MB (2,097,152 bytes). |
| Short side | At least 320 px. |
| Shape | Width to height between 1:2 and 2:1, both ends allowed (320 x 640 and 640 x 320 pass; 320 x 641 and 641 x 320 do not). |
| Largest | The platform's bound for every user image: 8192 px on a side and 36 megapixels (`src/user_images.mjs`). |

The size is read from the image headers (PNG IHDR, JPEG start-of-frame, WebP
VP8, VP8L, VP8X and each animation frame); the largest size the file states
anywhere counts. No pixel is decoded. The rules are one sentence in
`COVER_RULES`: "A cover is a PNG, JPEG or WebP image of at most 2 MB, at
least 320 px on its short side, with a width to height ratio between 1:2 and
2:1." The Publish dialog can check the same rules before it uploads and show
that sentence.

## What is stored

- **Only what draws the picture is stored.** The file is rebuilt from an
  allowlist of its segments: EXIF (with GPS), XMP, text and comment chunks,
  vendor segments, a second picture appended after the first and any bytes
  after the end marker are all dropped. The pixels are never re-encoded. A
  file with no end marker, a second header, or a critical PNG chunk the
  platform does not know is refused (`cover_bad_type`).
- R2 key `covers/{id}/cover.{png|jpg|webp}`. One file per listing: a PNG
  replaced by a JPEG leaves only the JPEG.
- A SHA-256 of the stored bytes goes in with them, and R2 verifies it. The
  approval binds the object by that digest.
- The listing record carries
  `cover: { ext, width, height, bytes, updated_at }`. `bytes` is the size of
  what is stored (after the metadata is dropped).

## Review

A cover is the creator's own art, so it is reviewed like the thumbnail.

- **Nothing starts a review by itself.** After a `PUT` or a `DELETE` that
  changed something, the client posts `POST /api/simulations/{id}/submit`
  (the response says `review: "submit"`). A cover change alone is reviewed
  through the same art-only path as a thumbnail change.
- The listing **stays listed** while that review runs. A failed review takes
  the listing down, as any failed review does.
- The judge sees the cover with the other images, and the approval binds
  which cover was approved.
- Until a review has bound the cover that is stored, the public `GET` answers
  `404` and the Gallery shows the thumbnail. There is **no placeholder
  panel** for a cover.
- Where an approval binding is required (production sets
  `REQUIRE_APPROVAL_BINDING`), a cover on a listing with no binding is held
  back from the public the same way.

For the Publish dialog and the MCP tool: upload the cover (`PUT`) before the
`submit` the publish already posts, and one review covers both. Show `error`
from a refusal as it is.

## Who sees it

`GET /api/simulations/{id}/cover`:

- **The public** gets the image when the listing is listed (approved and
  public) **and** a review has bound this cover. `Cache-Control: public,
  max-age=300`.
- **The author and an admin** also get an unreviewed cover, so the dialog can
  show what is waiting. `Cache-Control: private, no-store`. A quarantined
  listing is served to an admin only.
- **Everyone else** gets `404` `{ "error": "No cover" }`: the same answer for
  no cover, a listing that does not exist, a private or archived one, a held
  one, and a cover not yet reviewed. A hold is never distinguishable from "no
  cover".
- The legal lock is read before anything is decided, so a locked listing
  serves nothing to the public whatever its record says.
- **The object that is sent is the object that was judged.** After the check
  that the cover is approved, the Worker fetches the object and compares the
  identity of what the fetch returned (its SHA-256, or its etag and version)
  with the approval before it streams the body. A file put between the check
  and the read is not served, and the answer is the same `404`. The author and
  an admin, who may see an unreviewed cover, are not held to this.

Every image response carries `X-Content-Type-Options: nosniff` and
`Content-Security-Policy: default-src 'none'; sandbox`, and no cookie, so a
mislabelled body cannot run as a page on this origin.

## What the listing views say

`cover_url` and `cover` are in the listing card of the list
(`/api/simulations`, `/api/gallery`, `/api/gallery/featured`), the single read
(`/api/simulations/{id}`) and the author's projects (`/api/projects`):

```json
{ "cover_url": "https://api.eustress.dev/api/simulations/{id}/cover?v=1767225600000",
  "cover": { "width": 600, "height": 800 } }
```

or `{ "cover_url": null, "cover": null }` when the listing has none.

- `cover_url` is absolute, on the origin of the request that asked, so
  staging names staging. `v` is the time the cover was set, in milliseconds:
  a replaced cover is a new URL, so a browser never shows the old one.
- `cover_url` is present whenever the listing has a cover, **including a
  cover still waiting for review**. The card requests it and falls back to
  `thumbnail_url` when the request answers 404 (an image `onerror`). The list
  makes no storage call to check this.
- `cover` says whether it is portrait or landscape, so the card can reserve
  the right shape before the image loads.

## Previews: the review captures of an approved listing

Alongside the cover, the Gallery can show the captures the judge saw.

- `GET /api/simulations/{id}/previews` returns
  `{ "previews": [{ "n": 0, "url": "https://.../api/simulations/{id}/preview/0" }] }`,
  sorted by `n`, URLs absolute on the asking origin.
  `Cache-Control: public, max-age=60`.
- `GET /api/simulations/{id}/preview/{n}` (`n` from 0 to 7) returns one
  capture. The type follows the file's extension; `Cache-Control: public,
  max-age=300`. Anything else is `404` `{ "error": "No preview" }`.

A capture is offered only while the listing is approved and public, its
approval binding holds and no legal lock is on it. The Worker asks the
moderation module (`approvedCaptureKeys`) on **every request** and serves
only the object it returns for that number; it remembers nothing between
requests. `approvedCaptureKeys` returns a capture only when the stored object
still matches what the approval bound, and the object fetched to be sent is
compared with the bound capture for that number once more before it is
streamed. A listing with nothing to show, and one
that is held or does not exist, all answer `{ "previews": [] }`.

Plainly:

- **Covers have metadata stripped; previews are served as uploaded.** A
  capture is evidence the judge saw as it was.
- Captures and covers are images the author's Studio or tools upload. The
  judge saw them and the binding fixes **which** images are shown, but the
  Worker cannot verify them against the content until the server-side render
  job exists.

The list route makes no preview call; the site asks `/previews` on demand.

## Moderation and the Worker's other writes

- `src/moderation.mjs` owns the binding (`coverApproved`,
  `approvedCaptureKeys`, `snapshotReviewed`); `src/cover.mjs` only calls
  them. The route handler's `deps` can replace `approvedCaptureKeys` (tests do).
- The record is read again just before the cover is written, so a review
  decision that landed while the image was stored is not undone.
- The cover routes read the listing through the legal lock
  (`simWithLegalLock`) before they ask whether it is listed or servable;
  `tests/serving_reads.test.mjs` fails any function that does not.
