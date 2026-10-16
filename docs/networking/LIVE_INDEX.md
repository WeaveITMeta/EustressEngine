# The live index: sessions hosted right now

Status: **Worker side built 2026-10-01; deployed on the orchestrator's
schedule, after the `v7-live-index` migration is applied in the environment.**
The Gallery's "live now" strip reads one public route that names every session
being hosted, with the player counts and the country of each host, without a
join link.

Code: `infrastructure/cloudflare/api/src/live_index.mjs` (the object and the
route), `src/live.mjs` (the heartbeat that feeds it), `src/body_cache.mjs` (the
10-second cache). Tests: `tests/live_index.test.mjs`.

## The read

`GET /api/simulations/live`, public. Optional `rating_max=all_ages|teen_13|mature_17|adult_18`
asks for the viewer's age slice, as the list route does.

```json
{
  "sessions": [
    {
      "id": "listing id",
      "name": "Box Head",
      "author_name": "host",
      "thumbnail_url": "https://api.eustress.dev/api/simulations/{id}/thumbnail",
      "cover_url": "https://api.eustress.dev/api/simulations/{id}/cover?v=1767225600000",
      "players": 3,
      "max_players": 8,
      "region": { "country": "DE" },
      "since": "2026-10-01T12:00:00.000Z"
    }
  ],
  "players_total": 3,
  "sessions_total": 1
}
```

- **Newest first** by `since` (when the host started answering at its
  current link), at most **24** sessions.
- `players_total` and `sessions_total` cover **every** listed live session,
  even when the list is capped at 24.
- `players` is what the host's heartbeat claims, **capped at the players the
  Worker admitted to that hosting session plus the host itself** (see
  "Players"). `players_total` adds up these numbers.
- `thumbnail_url` and `cover_url` are the listing card's own (the cover
  contract is `docs/networking/LISTING_COVER.md`); each is `null` when the
  listing has none. `region` is `{ "country": "DE" }`, or `null` when the
  platform did not say.
- `since` is an ISO time, as the single-listing live read gives it.
- `Cache-Control: public, max-age=10`, `Vary: Origin`.
- `503` `{ "error": ..., "code": "live_unavailable" }` with `Cache-Control:
  no-store` when the deployment has no live index or it cannot be reached. A
  strip reads that as "nothing to show".

### Which sessions

Only sessions of **listed** listings (approved and public). Each listing is
read through the same legal-lock overlay as the list route, so an unlisted,
archived, pending, rejected, quarantined or locked listing never appears and
adds nothing to the totals. A read looks at the 500 newest index entries at
most; the totals count what it read.

### The cache

A Worker's `Cache-Control: public` caches nothing, so the computed strip is
kept in the platform's cache for **10 seconds**: a URL that is hammered costs
one scan of the index (and one read per live listing) per window per data
centre. The key is the request's own origin plus the age slice, one of five
values; nothing else in the query string changes the answer or adds an
entry. The response is built again for each request, so its CORS headers name
the origin that asked. A scan that fails is not kept. The strip can therefore
trail the index by up to 10 seconds, on top of the host's 30-second heartbeat.

`GET /api/makers` keeps its scan the same way (10 seconds per age slice; `q`
and `limit` are applied to the kept result).

## What feeds it

A **heartbeat** (`POST /api/simulations/{id}/live`, the author's, every 30 s
while Studio hosts) tells the index
`{ sim_id, author, players, max_players, region, since }`, **only while the
listing is listed**. An author's unlisted listings can host (the author checks
their host before approval) but never reach the index, so they cannot fill it.
A host's **stop** (`DELETE .../live`) removes the entry at once. An entry
**expires 90 seconds after its last beat**: the index object sets its own
alarm, so a host that vanishes disappears by itself, and a read treats an entry
past 90 seconds as gone even before the alarm has cleared it. The 90 seconds is
the same number as the host's own heartbeat deadline (`HEARTBEAT_TTL_MS`); a
test holds the two together.

**An author holds at most 5 entries.** A sixth session drops the author's
oldest (by `since`); a session already held is renewed and never costs a place.
The cap is kept by the author's account id, which the index holds only for this
and never returns.

The index is told **after** the host's own record is written. A missing
binding, an index that throws, answers an error or nonsense, or takes longer
than 1.5 seconds **never fails the heartbeat or the stop**: the host is live
all the same, and the entry catches up on the next beat. Heartbeat bodies,
limits and answer are those of the host route: it answers
`{ "ok": true, "expires_at": ... }`.

One Durable Object, class `LiveIndex`, named `all`, bound as `LIVE_INDEX`,
created by migration `v7-live-index` in **both** the production and the
`[env.staging]` blocks of `wrangler.toml`. `tests/deploy_bindings.test.mjs`
requires the binding, the migration and the `LiveIndex` export in both.

## Players

The heartbeat's `players` is a claim from the host. It is shown capped at
`admitted + 1`: the players the Worker admitted to the current hosting session
(each is a ticket the host's `POST .../live/admit` let through, counted once;
a used ticket is refused and adds none) plus the host's own character. A host
that claims 10,000 players and has admitted nobody shows 1; a host that is not
playing and has admitted nobody shows 0. A new hosting session (a restart on a
new link) and a stop start the count again. The count never falls as players
leave, so it is a ceiling and never an undercount.

The same cap applies everywhere a number of players is shown, so a Space's
page never shows a bigger number than the strip: the strip, the single-listing
read (`GET /api/simulations/{id}/live`, which the listing page prints as
"Playing N" and "N of M players") and the `live.players` the play route
returns. The strip's number is capped when the host's heartbeat arrives; the
single read and the play route cap at the moment they are read, so a player
admitted a moment ago shows there at once. `max_players` is what the host sent.

## Region: the country only

A heartbeat's region comes from the platform (`request.cf`):

- `country`: two upper-case letters (`^[A-Z]{2}$`); `XX`, the platform's word
  for "unknown", is no country. Anything else is dropped and never stored,
  on the host or in the index. If it is not valid, `region` is `null`.
- The data centre (`colo`) is **never read, stored or returned.**

Why: a data centre code is city-level (PHX, SEA). A home host's heartbeat would
publish the metro it lives in. The country is the coarsest place that still
tells a visitor where a session runs. This is reversible: adding the data
centre back is a change to `cleanRegion` and `regionOf` alone.

The host's record stores it, and `GET /api/simulations/{id}/live` (the
single-listing read) returns it for a live host as `"region": { "country": "DE" }`
or `null`. A record that carries more than a country shows the country only. A
host that moves has its new country on its next beat; a beat the platform could
not place clears it, so a stale place is never kept.

## Privacy

- **Never an address.** Country only. No IP, city, data centre, coordinates,
  postal code or network number is read, stored or returned.
- **Never the join link, key or pin.** The index holds `sim_id`, the author's
  account id (for the cap, never returned), the two counts, the country,
  `since` and the time of the last beat. The single-listing read keeps its own
  rule for the link: signed-in accounts and live guest leases only.
- No account id and no email is in the answer; `author_name` is what the
  listing already shows publicly.

## Operations of the object

`beat`, `stop` and `list`, called by `live.mjs` and `live_index.mjs` over the
object's own fetch. `beat` refuses a listing id that is not hex and dashes, an
author that is not an account id, counts that are not whole numbers (`players`
0 to 10,000, `max_players` 1 to 10,000) and times that are not numbers; it
cleans the region itself and applies the per-author cap. The alarm clears what
has expired, and the authors' lists with it, and wakes again for the earliest
remaining expiry.
