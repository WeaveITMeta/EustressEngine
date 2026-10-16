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
  - It must equal the API the Player is set up for (its link registration's `--api-url`, else production).
    A link for the other refuses with "This link is for the staging server; this Player is set up for
    production.", and nothing is fetched, minted or connected. So no web page can point a production Player
    at staging, whose listings production moderation never saw. A ticket from the other API would be useless
    anyway.
  - A link with no `api` takes the Player's own.
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
- A solo link accepts exactly `?api=production` or `?api=staging` after the id, and nothing else. **Order:**
  the page adds `?api=` to solo links only once a Player that reads it has shipped. A Player from before P1
  takes `<id>?api=..` as the id, refuses it, and closes without a word. Join links may carry `ticket` and `api`
  at once, because an older Player ignores parameters it does not know.

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
