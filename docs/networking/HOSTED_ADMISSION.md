# Who may join a hosted gallery session

Status: **plan, not built; approved 2026-09-30.** It gates guests on hosted
sessions (`docs/architecture/GUEST_PLAY.md` section 8). Owners: Website writes
the Worker half (`live.mjs`, and the ticket id in `identity.mjs`, coordinated
with Commerce); Multiplayer writes the host half (networking, Studio).

## The gap

- `GET /api/simulations/{id}/live` (Worker `live.mjs`) returns the whole join
  link (address, port, join key, certificate pin) to anyone, for an approved
  listing.
- The host admits any player whose `Hello` arrives on the right key. An
  identity ticket is optional: a join without one plays as a guest
  (`remote_players.rs`, `guest_user_id`).
- An identity ticket (`identity.mjs`, `eit1.<payload>.<hmac>`) is bound to the
  host's certificate pin and lives 10 minutes, but holds no id and is never
  recorded. A captured ticket joins the same host again until it expires.

So a link copied out of `/live` or pasted between people joins without an
account, as many times as anyone likes.

## The rule

A gallery-hosted session admits only a player the Worker vouches for, once:

1. **The link goes only to someone who may join.** `GET /live` returns the
   link to a caller with a session token or a live guest lease, and to the
   author and admins as now. Everyone else still sees Live or Offline, the
   player count and the start time.
2. **Every join is admitted by the Worker, once.** The Player's `Hello`
   carries a ticket: an identity ticket for an account, or a guest ticket
   minted from a guest lease. The host sends it to the simulation's LiveHost
   object, which checks it and records its id for this hosting session. A
   ticket used once is refused after that, and so is a join with no ticket.
3. **Guests are marked.** The admission tells the host `guest: true`, and the
   host applies the guest rules: user id 2^52 + peer (as now), no purchases,
   no chat.

Hosting without a listing (F9 on a LAN, a pasted link between friends) stays
open as today: the rule binds only sessions that announce themselves to the
gallery.

With admission, the link stops being a capability: an account holder who
copies it and shares it admits nobody, because the host asks every `Hello`
for a ticket of its own. A reconnect after a dropped connection mints a fresh
ticket; the `used` refusal says so.

## The pieces

### Worker (Website)

- `identity.mjs`: tickets gain a random id (`j`, 16 bytes). Guest tickets
  share the format with `g: 1` and the lease id, minted by `guest.mjs` against
  a live lease and the host's pin.
- `live.mjs` GET: the link only for a session or a live lease (as above).
- `live.mjs`: `POST /api/simulations/{id}/live/admit` `{ticket, audience}`,
  called with the author's token, the one the heartbeat uses, so Studio needs
  no new credential; anyone else gets 403. The LiveHost object checks the
  ticket (signature, expiry, audience equal to the pin of the link it holds),
  refuses an id it has seen this session, and answers
  `{account_id?, username, guest, lease?}` or 403 with a code (`no_ticket`,
  `used`, `invalid`, `expired`, `wrong_host`, `busy`).
  - The record of seen ids is bounded: at most 5,000, each dropped at its
    expiry by the object's alarm. A full record answers `busy`; it never
    forgets an id early. It clears when the host stops or a new pin starts a
    new session.
  - For a guest ticket it keeps the lease with the id, so heartbeats can debit
    the right lease.
  - The object is single-threaded, so of two admissions of one ticket at once,
    exactly one wins.
- Heartbeats gain `admitted: [ticket id]` for the players still connected
  (at most `max_players` ids of 32 hex characters, else 400), so the object
  debits the guest leases in use (section 4 of GUEST_PLAY.md).
- Two guest tickets, kept apart: the LEASE ticket (`guest.mjs`) permits
  heartbeats and names no host; a guest JOIN ticket is an `eit1` with `g: 1`,
  the lease id and the audience of the host it joins, minted from a live lease
  by `POST /api/guest/join-ticket {lease, sim}`, as `/api/identity/ticket`
  mints account tickets. One verifier serves both kinds of join ticket.
- The listing page shows LIVE when `/live` answers live without a link, and
  routes Play to sign-in (or, later, the guest dialog).

### Host (Multiplayer)

- Networking (`session.rs`): a new peer stage between `Hello` and `Welcome`,
  `AwaitingAdmission`. When the host requires admission, a `Hello` surfaces
  as `NetNotice::Admit { peer, ticket }` and nothing of the world is sent.
  The shell answers with `HostSession::admit(peer, guest)` (the Welcome
  follows) or `refuse(peer, reason)` (`ToPlayer::Refused`, then close).
  Unanswered after 15 s, the host refuses: "the gallery could not confirm
  this join". Chat from a guest peer is dropped, never relayed.
- `HostConfig.admission`: `Open` (today) or `Gallery`. Studio sets `Gallery`
  when it hosts a listing (the session has a `sim_id` and heartbeats), and
  `Open` otherwise.
- Studio (`play_datamodel/remote_players.rs`): admission calls `/live/admit`
  (in place of `/api/identity/verify` for gallery sessions) on its own
  thread, and records `guest`. The commerce host path refuses purchase prompts
  for a guest (CLI Commerce's file).
- The Player: before joining a gallery session, it mints an identity ticket
  (signed in) or a guest ticket (guest lease, browser Player), as it already
  mints identity tickets for any pinned join.

### Tests

- Worker: a ticket admits once; a second use, a ticket for another pin, an
  expired one and no ticket are refused with their codes; `/live` hides the
  link from an anonymous caller and shows it to a session and a lease.
- Networking: with `Gallery` admission, a `Hello` sends no Welcome until
  `admit`; `refuse` sends `Refused` and closes; the 15 s timeout refuses;
  a guest's chat is dropped. With `Open`, today's behaviour holds.
- A live two-machine test through the tunnel: a pasted link without a ticket
  is refused; a signed-in Player joins; the same ticket twice is refused.

## Order

1. Worker: ticket ids, `/live/admit`, the GET gate. Accounts work without
   guest leases.
2. Host: the admission stage, `HostConfig.admission`, Studio's admit call.
3. Guests, once `guest.mjs` and the browser Player exist: guest tickets,
   `guest: true`, the guest rules, lease debiting through heartbeats.

Steps 1 and 2 close the gap for accounts on their own: no join without an
account and no ticket twice.
