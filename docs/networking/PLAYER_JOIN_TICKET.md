# A Player with no sign-in joins a listed session

**Status:** contract written 2026-10-08 for McKale's "Fix it" (P1, Security's interim ruling on J3). Security
approved both halves. The Play page landed 2026-10-08; the networking, common and engine half landed 2026-10-14
for build 32; the Player's half follows on the same wave. Not yet run with a real browser and Player. The real fix,
a sign-in in the Player itself, follows below.

## The problem

A gallery-listed session admits only players whose `Hello` carries an identity ticket. The gallery admits the
ticket once, and the ticket is bound to the host's certificate pin (`docs/networking/HOSTED_ADMISSION.md`). The
Player can mint a ticket only from Studio's saved sign-in on the same machine. A machine with just the Player is
refused with "A ticket is required to join", and its hint, sign in, is something the Player cannot do.

## P1: the Play page hands the Player a ticket

The signed-in Play page mints the ticket at the click. It passes the ticket to the Player as part of the
`eustress-player://` link that Windows hands the Player as its launch argument. The ticket never travels in an
`http(s)` URL.

```text
eustress-player://join/<host>:<port>?key=<key>&pin=<64 hex>&ticket=<ticket>&api=<name>
eustress-player://play/<simulation id>?api=<name>
```

- `ticket`: the answer of `POST /api/identity/ticket { "audience": "<the link's pin>" }`, made with the
  viewer's own session.
  - Shape: 1 to 2,048 characters of `A-Z a-z 0-9 . _ -`; the Player refuses any other.
  - Lifetime: 10 minutes.
  - Binding: the host's pin.
  - Use: the gallery admits it once and refuses it after that ("This ticket was already used").
- `api`: which Eustress API the link belongs to, as a name: `production` or `staging`.
  - These are the only APIs a Player knows. Any other name refuses the link and says so.
  - It must equal the API the Player is set up for. For one run that is `--api-url` (which wins) or `--env staging` or `--env production`. Otherwise it is `EUSTRESS_API_URL` when that is set, else the API compiled in from `EUSTRESS_DEFAULT_API_URL` (a staging-channel build), else production. On Windows the registered command carries `--api-url` when that API is not production.
    A link for the other refuses with "This link is for the staging server; this Player is set up for
    production.", and nothing is fetched, minted or connected. So no web page can point a production Player
    at staging, whose listings production moderation never saw. A ticket from the other API would be useless
    anyway.
  - A link with no `api` takes the Player's own.
  - A link that names `production` opens on production, whatever the Player is otherwise set up for (its
    `--api-url`, `--env`, `EUSTRESS_API_URL` or the API compiled into a staging-channel build). Production is the
    moderated server, so this cannot show a visitor anything production moderation did not pass. Without it, the
    command a staging run registers (`--api-url` staging) sent every production gallery link to staging, which
    answered "This simulation was not found" (2026-11-11). A link that names `staging` still never moves a Player:
    a Player set up for production refuses it.
  - A link that names `ticket` or `api` twice is refused.

### The website (Play page)

- Mint only for a signed-in viewer, only at the click, and only when `POST /api/simulations/{id}/play` answers
  `status: "live"` with a link that carries a pin. Never mint on page load, hover or prefetch. The audience is
  that link's pin.
- Append `&ticket=<ticket>&api=<name>` to the link and navigate to it directly, with no `http(s)` redirect hop
  in between, so no Referer or server log carries it. The site's own host names the API: the staging site
  (`staging.eustress-cbb.pages.dev`) says `staging`, any other says `production`.
- The ticket never goes into an `http(s)` URL, query or fragment. It also never goes into history state,
  storage, analytics, a console log, or an error report. It never appears in the link's display text or in a
  "copy link" button.
- The click's user activation lasts about 5 seconds, and `/play` already spends part of it. So minting gets
  2.5 seconds. On a timeout or any failure, the page opens the link without a ticket, and the Player then uses
  a sign-in of its own, if it has one.
- `web/src/api/live.rs`'s link check caps the Worker's own link at 512 characters before the ticket is
  appended. It must accept the full link, up to 512 + 2,048 + 32 characters, with `_` in values.
- A solo link names `?api=production` or `?api=staging` right after the id, then at most one `ticket` (the solo
  play ticket, "Solo play pass" below) and at most one `min` (`X.Y.Z`), in either order, and nothing else. The page
  adds `?api=` to every solo link, production included: every released Player reads it (the first release,
  staging-channel v0.1.0 of 2026-10-20, already does). A Player from before P1 would take `<id>?api=..` as the
  id, refuse it, and close without a word; none was released. Join links may carry `ticket`, `api` and `min`
  at once, because an older Player ignores parameters it does not know.
- `min`: the oldest Player the link opens in, from the Player manifest's optional top-level `min_version`
  (`player/latest.json`, `staging/player/latest.json` on the staging site). The page appends `&min=X.Y.Z` to every
  desktop Player link (three parts of one to five digits; any other value is left off) and none to the browser
  Player's, which is the site's own build.

### The Player

- It reads `ticket` and `api` only from an `eustress-player://` (or `eustress://`) launch argument. It never
  reads them from `--connect`, an `https://` link, or an environment variable.
- It takes the ticket out first (`split_link_extras`), and parses only what is left. So no launch error,
  usage text, panic or notice can contain the ticket. A test proves it: a malformed join link that carries a
  ticket fails with a message without the ticket text.
- The ticket lives in a type that cannot be copied and whose `Debug` prints no ticket text
  (`JoinTicket(..)`). It is never in `JoinLink`, never in `JoinLink::to_link()`, and never in a log line, a
  notice, a panic message, or a file. No error message quotes it.
- It checks the link's `api` against its own (`LinkExtras::check_api`) before anything is fetched, minted
  or connected, and refuses a link for the other API.
- It sends a link's ticket in exactly one `Hello`. A join that fails is never retried with that ticket; the
  notice says to join again from the gallery.
- With no ticket in the link, the Player mints its own from the sign-in it has, as today.
- The join notice shows the host's refusal in plain words. The host passes on the gallery's own text. Since
  2026-10-08 that includes the Adult content gate on the ticket's account: `adult_content_off` (the setting is
  off) and `age_21_required` (a 21+ listing, verified age under 21). An unlisted or quarantined listing still
  answers "This session is not available right now." to anyone but its author.
- A `play/<id>` link may end in `?api=<name>`. The Player reads the id without it.
- A link of up to 512 + 2,048 + 32 characters is a normal Windows launch argument; the Player sets no lower cap.

### The host and the Worker

No change. The host passes the `Hello`'s ticket to `/live/admit`. The Worker checks the pin and the expiry,
and records the ticket's id, so a second use is refused.

### What stays exposed

While the Player runs, its command line, with the ticket, can be read by other processes of the same Windows
user (`Get-CimInstance Win32_Process`). Three limits bound this: the ticket is single-use, bound to one hosting
session's pin, and gone after 10 minutes. Windows process-creation auditing and endpoint security tools also
record command lines, so a spent ticket can sit in those logs after it expires. That is acceptable for the same
reasons: single use, the pin binding, and the expiry. Security reviewed and approved this for P1 on 2026-10-08.

## Solo play pass

**Status:** written 2026-11-13 (McKale: "Studio is not the source of truth. I am signed in on staging website.").
API, Player and Website halves in the tree, not deployed; Security reviews before any deploy.

A solo link plays a listing on this computer. Until now the Player sent Studio's saved sign-in with the download,
so a teen-rated listing (Box Head) answered `403 sign_in_required` to a person signed in on the website but not in
Studio, or signed in to the other service. Now the Play page hands the Player the website's sign-in for that one
listing.

### The link

```text
eustress-player://play/<simulation id>?api=<name>&ticket=<ticket>
eustress-player://play/<simulation id>?api=<name>&ticket=<ticket>&min=X.Y.Z
```

The keys come in any order. `ticket`, `api` and `min` follow the rules above for join links: one of each at most,
the same ticket shape and length, and the same refusals. A solo link with no `ticket` plays as before.

### The website (Play page)

- A signed-in viewer's solo Play in the desktop Player mints at the click: `POST /api/simulations/{id}/play-ticket`
  with the site session (`play_chooser.rs ticket_request`). Never on page load, hover or prefetch; never for the
  browser Player, which plays with the page's own session (a single-use ticket it would never redeem is not minted).
- The ticket is appended after the API name (`?api=<name>&ticket=<ticket>`, then `&min=` when there is one) and handled
  exactly as a join ticket: the same 2.5-second budget, the same fallback to the link without it on a timeout or any
  answer but 200 (a rating 403 included), never in an `http(s)` URL, history, storage, a log or the page.
- "Eustress Player didn't open" (the page still visible and focused 3 seconds after the hand-over) offers this
  computer's build from the Player manifest, no button for a platform the release has no build for, and "Try again",
  which runs Play again so a signed-in viewer gets a fresh ticket.

### The endpoints

- `POST /api/simulations/{id}/play-ticket`: session (the website's `Authorization: Bearer`).
  - Answer: `{ticket, expires}`.
  - The ticket is an identity ticket (`eit1.`, `identity.mjs`) whose audience is `play:<id>`, with no username.
  - It lasts 2 minutes (`PLAY_TICKET_TTL_S`; join tickets keep their 10). The Player redeems it within seconds of
    launch, so a longer life would only help someone who copied the link (Security review M2).
  - Per account: the mint is rate limited through `AUTH_RATE_LIMITER` (key `play-ticket:<account>`), and one account
    holds at most 20 unexpired play tickets per listing, spent or not (`MAX_PLAY_TICKETS_PER_ACCOUNT`, recorded as
    `ptm:<account>:<id>` in the listing's play object). Over either: 429 `rate_limited` or `too_many_play_tickets`.
    So no one account can fill the listing's spent table and lock everyone else out (Security review M1).
  - A listing that does not exist, or that `GET /api/simulations/{id}` would not show this viewer, gets that GET's own
    answer: `404 Simulation not found`, or the rating gate's 403. The mint tells nobody more than the GET does.
  - Route class: `session`, like `/api/identity/ticket`.
- `POST /api/simulations/{id}/play-pass { "ticket": "<ticket>" }`: public; the ticket is the proof.
  - Answer: `{pass, expires}`.
  - Refused unless the ticket is genuine, unexpired, made for exactly `play:<id>`, minted after the account's session
    floor (a sign-out or a ban since then refuses it), never redeemed before, and claiming no longer life than a play
    ticket's. Codes: `no_ticket` (400), `invalid` and `expired` (401), `used` (403), `rate_limited` (429, per
    address), `busy` and `play_pass_unavailable` (503). Only these codes, with fixed words, ever leave the route: a
    storage fault in the play object answers `503 busy` and never its own message (Security review L1).
  - Its id is spent in the listing's own `LiveHost` Durable Object, named `play:<id>` (the store that spends join
    tickets at `/live/admit`, in an object no hosting session uses, so a new hosting session never forgets a spent
      play ticket). An id is kept until its ticket expires. A full table (5,000 ids per listing, a backstop behind the
    per-account cap) refuses (`busy`) and never forgets an id early.
  - A join ticket is never a play ticket, and a play ticket is never a join ticket: a host's pin is 64 hex
    characters and never `play:...`.

### The pass

- Shape: `epp1.<base64url payload>.<base64url HMAC-SHA256>`. Payload: `{a: account, s: listing id, i: when the ticket
  was minted, e: expiry}`, in unix seconds.
- Key: `HMAC(JWT_SECRET, "eustress-play-pass-v1")`, its own label, so a pass, an identity ticket, a signed image URL
  and a session token can never pass for one another.
- Lifetime: 1 hour, for this one listing.
- Header: `Authorization: PlayPass <pass>`. `verifyAuth` reads only `Bearer `, so a pass is never a session: it
  cannot mint another ticket, act as the account anywhere else, or reach any route but these reads.
- Read by: `GET /api/simulations/{id}`, `/world/manifest`, `/world/chunks/{hash}` and the older `/download`. There it
  is the account's viewer exactly as a session would be (`adult_content.mjs viewerFor`), so the rating gate judges
  the real account's setting and age, and the author still plays their own unlisted listing.
- Fail closed: a pass that is missing, malformed, forged, expired, made for another listing, or issued before the
  account's session floor is no credential at all. That viewer is signed out: since McKale's 2026-11-19 decision a
  signed-out viewer plays solo up to `mature_17`, so a teen-rated listing is served, and an adult one answers
  `adult_content_off`.

### The Player

- `split_link_extras` takes the ticket out of a `play/<id>` link before anything parses it, as for a join link.
  The launch keeps it in `PlayTicket` (`Debug`: `PlayTicket(..)`) for the one fetch that redeems it.
- With a ticket, the fetch redeems it once at `{api}/api/simulations/{id}/play-pass` and then sends
  `Authorization: PlayPass <pass>` on every request for that listing. It sends no Studio sign-in with the world, not
  even on the production fallback below. (The avatar is still fetched with Studio's sign-in, origin checked, so the
  body shown is Studio's account's.) The redeem follows no redirect (reqwest re-sends a body on a 307 or 308) and
  speaks https only, except to a test server on this machine (Security review L2). The
  pass is a `Credential` bound to the API origin that issued it (`systems/credential.rs`), so it never goes to
  another server.
- A redeem that fails is never retried with the same ticket. The notice says why ("This Play link was already
  used.", "This Play link has expired.") and "Join again from the gallery."
- Without a ticket, the Player sends Studio's sign-in, and only to the API origin that issued it.
- A link that names no `api`, opened by a Player set up for staging, whose world manifest staging answers 404, is
  tried once on production, with Studio's production login only (`credential::studio_login_for`), and with none
  after a ticket launch. Production is the moderated server, so opening it is always allowed. Never from
  production to staging, and never for a link that named its server. The log says which API was used and why
  ("link names production", "link names staging", "link names none, using staging"), never the link.

### What never gets logged

The ticket and the pass never appear in a log line, a notice, a panic, an error message, a URL or `Debug` output, on
either side. The Worker never echoes either; the Player quotes only the redeem's HTTP status and its `code`, and only
when that code is a plain lower-case word. Tests prove a failure message carries no ticket text
(`space_fetch.rs a_play_pass_answer_never_quotes_a_ticket_or_a_pass`, `main.rs
a_solo_link_keeps_its_ticket_in_any_order_and_never_prints_it`). What stays exposed is the same as for a join link:
the ticket in the Player's command line, single use, bound to one listing, gone after 2 minutes.

## Booked next: a link must not decide whose identity the Player sends

This predates P1, and P1 makes it more visible. With no ticket in a link, the Player mints its own from its
saved sign-in, for whatever pin the link names. So any web page can launch a join link to its own host and
receive the person's identity ticket: their account id, username and IP address.

The fix:
- Play-page join links carry the listing id.
- Before minting its own ticket, the Player checks that `GET /api/simulations/{id}/live` returns the same pin.
- With no id, or a different pin, it joins without an identity, or refuses with the gallery hint.

Owners: Client (the check in `join_ticket`), Website (the id in the link), Multiplayer (the id in
`LinkExtras`).

## Code

| Piece | Where | Owner |
|---|---|---|
| `LinkExtras` and `JoinTicket`: parse `ticket` and `api`, refuse bad ones, never print the ticket; `JoinLink`'s `Debug` never prints the join key | `eustress-networking/src/join_link.rs` | Multiplayer |
| `Hello`'s `Debug` never prints the ticket; `PlayerSession::hello` takes it into the one `Hello`; the host holds a joiner's ticket as `session::Ticket` (`Debug`: `Ticket(..)`) and takes it at `WorldReady` | `eustress-networking/src/wire.rs`, `session.rs`; engine `multiplayer.rs`, `remote_players.rs` read it with `secret()` | Multiplayer |
| The known API names (`production`, `staging`) | `eustress-common/src/api_base.rs` | Multiplayer |
| Launch: take the extras from the link argument, apply `api` first, put the ticket in `JoinTarget` for one `Hello` | `client/src/main.rs`, `systems/net_play.rs`, `systems/commerce.rs` | Client |
| Mint the ticket at the click, add `ticket` and `api` | the Play chooser, `web/src/api/live.rs` | Website |
| Review | | Security |

## The real fix: a sign-in in the Player

A passkey stays a website sign-in (`docs/architecture/PASSKEYS.md`): Studio and the Player keep the identity
file. So the Player gets Studio's identity-file sign-in:
- **Where:** the join notice's "Sign in" opens a file dialog for the identity file.
- **Saving:** the login is saved where Studio saves it, the same login file per API, written private.
- **After sign-in:** the Player mints its own ticket for every join, as it already does from Studio's saved
  sign-in.

Then P1's ticket in the link is needed only by a Player that has never signed in. Booked after P1, with its
own Security review, because it handles the identity key.
