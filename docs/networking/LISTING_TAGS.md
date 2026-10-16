# Listing tags: a listing's categories

Status: **decided by McKale 2026-10-01 and 2026-10-08.** The Worker half is
built and tested (`infrastructure/cloudflare/api`); it is not deployed. Tags are
a listing's only categories: up to eight free-form tags (#zombies #tycoon
#coop). Authors enter them as chips in Studio's Publish dialog, the MCP publish
tool sends them, the site edits them, and the site shows them as clickable chips
that filter the gallery. An author may leave them empty, and the review's judge
then chooses them (Guardian Code v2.3 Part III).

This page is the contract. The Worker enforces it, and the Publish dialog, the
MCP tool and the site build against it.

## The value

`tags` is an array of strings on a listing.

- **At most 8.** Counted after duplicates are dropped.
- **Each tag is normalized**, in this order: Unicode NFKC, trim whitespace,
  lower-case, remove one leading `#`.
- **What is left must match** `^[a-z0-9]+(?:-[a-z0-9]+)*$` and be **2 to 24
  characters** long. So: letters a to z, digits, and single hyphens between
  them. No space, no emoji, no underscore, no letter outside a to z, no hyphen
  at either end, no double hyphen. A `#` does not count toward the length.
- **A tag is never personal information.** The check is narrow and is not a
  word list:
  - a **phone number**: 7 or more digits in a row, where single hyphens between
    digit groups do not break the row (`5551234`, `555-1234`, `call-555-1234`).
    A year range of two 4-digit groups (`1939-1945`) is not a phone number;
  - an **email address**: an `@` with text on both sides and a dot after it, or
    the same spelled in words (`ann-at-mail-com`, `ann-at-mail-dot-com`);
  - a **handle**: text that starts with `@`.
  Slurs, insults and other personal information are a reviewer's call (Jev's
  `tag_content`, below).
- **Duplicates are dropped** after normalization. Order is kept, so `["#Zombies",
  "coop", "ZOMBIES"]` is stored as `["zombies", "coop"]`.
- **An empty array is valid.** It asks the review to choose the tags.
- **A tag is never repaired or dropped to make a list fit.** One bad tag
  refuses the whole request.

Examples: `Zombies`, `#Zombies` and the full-width form of `zombies` all become
`zombies`. `co-op`, `3d` and `2026` are valid. `sci_fi`, `sci fi`, `co--op`,
`-coop`, `a`, `caf` plus an accented e, a rocket emoji, `555-1234` and
`@ann` are refused. `##coop` is refused: only one `#` is removed, and what
remains still starts with `#`.

## Who chose them: `tags_source`

Every listing carries `tags_source`, `"author"` or `"judge"`.

- **`"author"`**: the author sent these tags, or the listing has none yet.
- **`"judge"`**: the author gave none, and the review's judge chose them.

When the author gives no tags (`[]`, or a listing that has none), the judge
chooses **up to 5** when the listing is reviewed (`suggested_tags`, Guardian
Code Part VII). The Worker normalizes each one by the rule above and drops what
is not a valid tag (personal information included). For a fiction Space they
carry `fiction` first and a genre tag of the judge's (`science-fiction`,
`fantasy`, or any tag ending in `-fiction` or `-fi`), so the Code's fiction label
is met; the label is checked on the judge's tags after they are assigned. They
are written to the listing when the case is decided, for every outcome except a
quarantine, a harm (Group A) rejection and any other legal-lane outcome. A
listing whose tags are the judge's has them chosen again at its next review.

The author is told in one line, **"Tags chosen for your Space: a, b, c"**, in
the review email and in the case history (`GET /api/account/cases`,
`feedback.tags_chosen`, with the list in `feedback.assigned_tags`). The author
view of the review (`GET /api/simulations/{id}/moderation`) carries the same
`tags_chosen` and `assigned_tags`.

**After the judge.** The author may send the tags back or change them:

- **The same set**, in any order or spelling, keeps `"judge"`, is stored in the
  order sent, and is **not reviewed again**.
- **Any other set** is the author's (`"author"`) and sends the listing back to
  review, as a changed name does.
- **`[]`** asks the review to choose again, and is reviewed.

## The genre a listing had before tags

Listings from before tags carry a `genre`. It is read as a tag, lazily:

- Normalized like a tag. `"all"` (the old default, which named no category) and
  a genre that is not a valid tag (`Sci Fi`) are skipped.
- Added after the listing's own tags when it is not already one of them and the
  list has room under 8.
- It is the author's tag (`tags_source: "author"`).
- It is **written into `tags` on the listing's next write**, whatever that write
  is (a commit, an upload, a thumbnail, a cover, an archive, a review decision).
  From then on `tags` is the whole answer, so removing that tag later is final.

There is no one-off script: a listing that is never written again is still read
the same way.

**`genre` on a listing a route returns** is a **deprecated mirror for one
release**: the migrated genre (normalized) when the listing had one that is a
valid tag and not `"all"`, and `null` otherwise. Read `tags`; `genre` will be
removed.

## Where tags travel

| Request | Field | Not sent |
|---|---|---|
| Create: `POST /api/simulations/publish` | `tags` in the body | see below |
| Whole-Universe commit: `POST /api/simulations/{id}/world/commit` | `listing.tags` | the tags stay as they were |
| The site's edit: `PATCH /api/simulations/{id}/tags` | `tags` in the body | refused (`400 bad_tags`) |

"Not sent" means the key is absent or `null`. A commit with no `listing` at all
leaves the tags alone, so an older Studio never clears them by accident.

**An incoming `genre`** (an older Studio still sends one, on create and in a
commit's `listing.genre`) is **accepted and ignored**, with one exception: when
no `tags` are sent and the listing has no tags, a genre that is a valid tag and
not `"all"` becomes the listing's one tag, the author's. An empty `genre` (`""`
or whitespace) is the same as none. On create, a listing whose `tags` were sent
(an empty list included) stores `genre: null`.

All three routes check the tags **before anything is written**. A refused create
makes no listing and does not count toward the author's publish total. A
refused commit or edit leaves the listing exactly as it was: not even the name
or the player count in the same commit is stored. A Space-only publish carries
no listing, so it never touches tags.

### The site's edit

`PATCH /api/simulations/{id}/tags`, signed in as the author, body
`{ "tags": ["harbour", "tides"] }`. Only the author (anyone else gets `404`, as
for a listing that does not exist), and never while the listing is held (`409
in_review`). Its session is checked fresh (`src/route_policy.mjs`). Answers
`200`:

```json
{ "ok": true, "id": "...", "tags": ["harbour", "tides"], "tags_source": "author", "genre": null, "changed": true, "status": "in_review" }
```

`changed` is false when the set is the one the listing has. `status` is the
author-facing review status (`listed` when nothing changed on a listed listing,
`in_review` after a change). A changed set sends the listing to `pending`, and
the review runs as it does for a commit's changed text (Studio's `/submit`, or
the sweep).

### The refusal

HTTP 400, `Content-Type: application/json`:

```json
{
  "error": "Tag \"sci_fi\" is not a valid tag. A tag is 2 to 24 characters: lowercase letters a to z and digits, with single hyphens between them, and an optional leading #. A tag is never a phone number, an email address or a handle. A listing holds at most 8 tags.",
  "code": "bad_tags"
}
```

- The message names the **first** bad tag and ends with the rules, so a client
  can show it as it is.
- Personal information is **named by its kind and never repeated**: `Entry 2 of
  tags looks like a phone number, and a tag never carries personal information.
  ...` (or `an email address`, `a handle`).
- The named tag is **escaped and cut at 40 characters** (an ellipsis follows a
  longer one). Quotes, angle brackets, the ampersand, the backslash, the
  backtick, control and invisible characters and direction overrides appear as
  a six-character escape (a backslash, a `u` and four hex digits). Letters and
  emoji stay readable. A message is safe to put in a page, and a 10 KB tag costs
  a short answer.
- More than 8 after duplicates: `A listing holds at most 8 tags, and this
  request names 9. ...` with the rules after it.
- Not a list: `tags must be a list of strings. ...` An entry that is not a
  string: `Entry 2 of tags must be a string, not a number. ...`
- A list longer than 256 entries is refused as too many without being read.
- Every tag is checked before the count, so a list with a bad tag at the end
  names that tag even when it also has too many.

## What a reader gets

Every route that returns a listing carries the same three fields:

| Field | Shape |
|---|---|
| `tags` | array of strings, always present, `[]` for none; the migrated genre included |
| `tags_source` | `"author"` or `"judge"` |
| `genre` | string or `null`: the deprecated mirror, for one release |

- **Gallery card** (`galleryView`): `GET /api/simulations`, `/api/gallery`,
  `/api/gallery/featured` and `GET /api/simulations/{id}`. An unlisted listing
  read by its author or an admin carries them too.
- **The live strip** (`GET /api/simulations/live`): each session.
- **The author's projects** (`GET /api/projects`): each project.
- **Create** (`201`), **commit** (`200`) and **the tags edit** (`200`): the
  listing's tags as stored.
- `GET /api/makers` returns people, not listings, and carries no category field.

## Filter and search

`GET /api/simulations` and its alias `GET /api/gallery` (and
`/api/gallery/featured`) read these parameters, with `rating_max`:

| Parameter | Meaning |
|---|---|
| `tag=zombies` | Only listings that carry the tag. **Repeat it** (`tag=zombies&tag=coop`) and a listing must carry **all** of them. |
| `genre=racing` | **Deprecated alias of `tag=`** for one release: read exactly as a `tag=` value, and combined with the `tag=` values (all of them). `genre=all` and an empty `genre=` ask nothing. |
| `q=text` | Free-text search: a case-insensitive substring of the name, the description, the author's name or any tag. A leading `#` in `q` is ignored for tags, so `q=%23zomb` finds `zombies`. |

- A `tag` value is **normalized like a stored tag** (`%23ZOMBIES` finds
  `zombies`).
- A `tag` value that is **not a valid tag matches nothing**: the answer is `200`
  with `{ "simulations": [], "total": 0 }`, never an error. One bad value among
  good ones also matches nothing, because every tag must match.
- An **empty** `tag=` (or only whitespace) asks for nothing.
- Only **listed** listings are searched: a listing in review, held, rejected or
  private is never found by its tag or by `q`.
- `tag`, `genre`, `q` and `rating_max` combine; each narrows the result.
- The Worker does not read `sort`, `limit`, `page` or `category` on these
  routes; the site applies them to what it receives.

### The popular tags

`GET /api/simulations/tags` (public, no sign-in), the source of the gallery's
tag chips:

```json
{ "tags": [ { "tag": "tycoon", "count": 2 }, { "tag": "zombies", "count": 2 }, { "tag": "coop", "count": 1 } ] }
```

- The **top 30** tags on **listed** listings, by `count` descending and then by
  `tag` ascending. A tag counts once per listing. A migrated genre and a judge's
  tag count like any other.
- It counts the listings the list route serves, so every tag it names has
  listings behind it, and a tag only an unlisted listing carries never appears.
- It follows `rating_max` like the list does: `?rating_max=teen_13` leaves out
  tags that only mature or adult listings carry, so an all-ages page does not
  name them. Other list parameters (`tag`, `genre`, `q`) are ignored here.
- `Cache-Control: public, max-age=300` with `Vary: Origin`. The Worker itself
  does not cache: each uncached call is one KV list and up to 100 reads. A
  listing that just left the gallery can still show in the counts for up to five
  minutes.
- The list and the popular tags share one scan, which reads the first 100
  listings the store returns.

## Review

Tags are text a reviewer reads, so they follow the rule for a name or a
description, not the rule for the player count:

- A commit or edit that **changes the set of tags** sends an approved listing
  back to `pending`, the same way a changed name does, so it is judged again.
  Adding, removing, swapping and clearing all count.
- A commit or edit that sends **the same tags again** is no change: spelled
  differently (`#ZOMBIES`), or in another order. A different order is stored as
  sent and does not go back to review. A judge's tags sent back stay the
  judge's.
- A listing with no tags that gets `tags: []` is no change.
- Create always starts a listing at `pending`.

**What moderation reads.** `src/moderation.mjs` shows the tags to the models
next to the name and the description. Tags are the only category: no genre is
shown.

- The classifier's state: `listing.tags` and `listing.tags_by_author` in
  `buildJevState`.
- The judge's case summary: `listing.tags` and `listing.author_gave_tags` in
  `caseSummaryForModel`. With `author_gave_tags` false the judge fills
  `suggested_tags`; otherwise it leaves it empty.

The list is the stored one with the migrated genre, capped at 8 tags of 24
characters, so a record written some other way cannot swell the text.

**What tags decide.** Tags are for finding a Space. They never set its rating.
Beyond the Code's articles that name them (B1's horror words and B4's meme
material in the tags, as before) and the fiction label, they decide one thing
more, and only for tags the author chose:

- Jev's `tag_fit`: do the tags describe what the Space contains? A confident
  "no" (0.85) is a B9 wording fault.
- Jev's `tag_content`: is any tag a slur, an insult aimed at a person or group,
  or personal information about a real person? A confident "yes" (0.85) is a B9
  wording fault.

Either is **not yet**, `not_yet_tags`: "Not yet: fix your tags (Code B9)", with
the change "Fix your tags so each one describes what the Space contains...". It
follows the Code's rule 2 for every B9 wording fault: not yet when every test is
2 or above, held for a person when a test is `null`, and rejected under B9 when
a test is below 2.

## Who builds what

- **Studio, Publish dialog.** A **Tags** chips field. Type a tag and press
  Enter, comma or space to make a chip; Backspace removes the last. Normalize the
  way the Worker does (the steps above) so a chip shows what will be stored, and
  refuse a chip that is not valid with the rule in plain words. Show at most 8.
  Pre-fill the listing's tags (the commit answers with `tags` and `tags_source`)
  and mark the judge's as chosen by the review. Send `tags` on create and in
  `listing.tags` of every whole-Universe commit; send `[]` when the author leaves
  them to the review. Stop sending `genre`. When the Worker answers `bad_tags`,
  show its `error` text.
- **Studio, MCP publish tool.** An optional `tags` argument (array of strings).
  Given, it is sent as above; absent, the listing's tags stand. Return the
  Worker's `error` text on `bad_tags`.
- **Site.** Show each card's `tags` as chips; a click goes to the gallery with
  `tag=` set, and several chips narrow it (all of them). Offer
  `GET /api/simulations/tags` as the chip row, with the viewer's `rating_max`.
  Send the search box as `q`. Encode `#` as `%23` in a query. Replace any genre
  filter with `tag=` (`genre=` works for one release). Let the author edit the
  tags with `PATCH /api/simulations/{id}/tags`, and show "Tags chosen for your
  Space" where the review says so.
- **Worker.** `src/tags.mjs` holds the rules and the migration, `src/index.js`
  create, reads, list, filter, the popular tags and the tags edit,
  `src/world.mjs` the commit, `src/moderation.mjs` the judge's tags and Jev's
  tag questions, `src/route_policy.mjs` names the popular-tags route as a public
  read and the tags edit as fresh.

## Rules in code

```js
// Worker: src/tags.mjs
normalizeTag(raw)        // string | null (personal information is null)
checkTags(value)         // { ok: true, tags } | { ok: false, error }
personalInfoIn(raw)      // 'phone' | 'email' | 'handle' | null
listingTags(sim)         // the tags, with a genre from before tags
listingTagFields(sim)    // { tags, tags_source, genre }
judgeTags(suggested)     // the judge's, normalized, at most 5
MAX_TAGS = 8, JUDGE_TAGS_MAX = 5, TAG_MIN = 2, TAG_MAX = 24
```

A client without NFKC can send the author's text as typed: the Worker
normalizes and answers with the stored list.

## Tests

`infrastructure/cloudflare/api/tests/listing_tags.test.mjs`:

- The rule: case, leading `#`, full-width letters, whitespace, hyphen rules,
  lengths 1, 2, 24 and 25, digits, accented and Cyrillic letters, emoji,
  underscore and double hyphen refused, a lone `#`, duplicates and order, the
  limit of 8 and 9.
- Refusals name the bad tag and the rules with `400 bad_tags` and write nothing
  (the listing record is byte for byte as it was; no listing is counted).
- A hostile tag (markup, quotes, direction overrides, 10 KB) is refused and
  never echoed raw or beyond 40 characters.
- Create stores `[]` when none is sent; a commit keeps the tags when none are
  sent and clears them on `[]`.
- Changed tags send an approved listing back to review; the same tags, spelled
  differently or reordered, do not.
- `tags` on the card, on one listing, on the author's projects.
- `tag=` single, repeated (all of them), normalized, invalid (matches nothing,
  no error), empty; `q` over tags, name, description and author.
- The popular tags: counts, order, ties, the top 30, listed only, once per
  listing, cache header, `rating_max`.
- Moderation: the classifier's state and the judge's summary carry the tags.

`infrastructure/cloudflare/api/tests/listing_tags_judge.test.mjs`:

- Personal information: phone numbers with and without hyphens, a year range
  allowed, an email address and a handle, named and never repeated.
- `tags`, `tags_source` and the `genre` mirror on every route that returns a
  listing: the list, the gallery, featured, one listing, the author's projects,
  the live strip; create, commit and the tags edit.
- The lazy migration: a genre becomes a tag, `"all"` and an invalid genre are
  skipped, the cap holds, and the next write writes it back.
- `?genre=` aliases `?tag=`.
- `tags: []` gets the judge's tags, written with `"judge"`; suggestions are
  normalized and capped at 5, invalid ones dropped; fiction gets `fiction` and a
  genre tag, and the label rule passes; none on a quarantine or a harm
  rejection; the line in the email and the case history.
- The same set sent back keeps `"judge"` with no review; a changed set is the
  author's and is reviewed (commit and the tags edit).
- An older Studio's genre is ignored, and an empty dossier genre is absent.
- `tag_fit` and `tag_content` give "not yet, fix your tags"; tags never change
  the rating; the judge's schema carries `suggested_tags`.
