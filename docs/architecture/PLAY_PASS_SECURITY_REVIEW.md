# Solo Play Pass: Security Review

**Reviewer:** independent adversarial review (Opus), standing in for the Security session, 2026-11-13.
**Scope:** the solo play pass as it sits in the working tree, not deployed.

- API: `infrastructure/cloudflare/api/src/play_pass.mjs` (new), `src/live.mjs` (`spend_play_ticket`), `src/world.mjs`
  (`servableListing`), `src/index.js` (routes, `handleGetSimulation`, `handleDownloadPak`), `src/route_policy.mjs`,
  `tests/play_pass.test.mjs`.
- Player: `eustress/crates/client/src/systems/space_fetch.rs`, `credential.rs`, `join_notice.rs`, `net_play.rs`,
  `main.rs`, and `eustress/crates/common/eustress-networking/src/join_link.rs`.
- Doc: `docs/networking/PLAYER_JOIN_TICKET.md`, "Solo play pass".

**Method:** read every file above and every helper they call. Copied the API to a scratch folder, ran the builder's 13
tests (13 pass), and wrote 11 probes against the real Worker with in-memory storage and the real `LiveHost` Durable
Object. Each SAFE probe was shown to fail under a deliberate mutation. The Rust side was reviewed by reading only
(no cargo), plus the reqwest 0.12.28 source for redirect behavior. Nothing in `src/`, `tests/` or `crates/` was edited.

**Probe folder:** `C:\Users\miksu\AppData\Local\Temp\claude\f4e4f9229cad81e7\4f7eb5ac-1847-4466-9ea5-53fc20c31261\scratchpad\play_pass_probe`
(the probes are in `tests\play_pass_probe.test.mjs`; run `node --test tests/play_pass_probe.test.mjs` there; M1 takes
about 40 seconds).

## Verdict

**Staging: yes, with conditions. Production: not until M1 and M2 are fixed.**

No blocker and no high. The core design holds: the pass is narrow (one listing, reads only), fail closed, keyed
separately from every other use of `JWT_SECRET`, judged against the account's current state on every request, and
never reaches a `Bearer` path. Two medium findings: one account can lock every other account out of a listing's
signed-in solo play, and the ticket's ten-minute life is longer than its job needs for something that rides in a URL.

Conditions for staging:

1. The Website half does not exist yet. Today the site never mints a play ticket, and `play_chooser.rs:334`
   (`is_solo_link`) refuses any solo link that carries one, so this deploy is inert for real users until the Website
   session lands its half. That half must:
   - mint the ticket at the click, never on page load;
   - navigate to the link immediately;
   - never render the ticketed link in an `href`, in copyable text, in the page URL, in analytics or in a log.
2. M1 and M2 are fixed before production. They may ship to staging as they are, because staging has no public traffic
   to deny.
3. No change to `identity.mjs` `isAudience` that widens it beyond 64 hex characters. Ticket separation rests on it
   (see SAFE 3).

## Findings

### M1 (medium): one account locks every other account out of a listing's signed-in solo play

- **Where:** `src/live.mjs:703-722` (`spend_play_ticket`, global per-listing cap), `src/live.mjs:317`
  (`MAX_PLAY_TICKETS = 5000`), `src/play_pass.mjs:188-205` (mint with no limit), `src/index.js:7518`
  (`UNAUTHENTICATED_AUTH_PATHS`; neither route is limited, and the mint never asks `AUTH_RATE_LIMITER`).
- **Scenario:**
  1. Mallory, one verified account, mints and redeems 5,000 play tickets for a rival's listing. That is about 17
     requests a second for 10 minutes; nothing limits her.
  2. The listing's spent-ticket table is full. Every other account's redeem answers `503 busy`, and the Player shows
     "Eustress could not check this Play link right now". A signed-in solo launch has no signed-out fallback, so even
     a G-rated listing will not open for anyone signed in on the website.
  3. Her tickets expire after 10 minutes, and she repeats.
  4. Each redeem also lists up to 5,001 keys in the Durable Object, so the attack costs the platform storage
     operations as well.
- **Probe (fails today):** `FINDING M1`. After one account's 5,000 redeems, a second account's redeem gets
  `503 {"code":"busy"}`. `FINDING M1b`: with a limiter bound that allows 3, twenty mints from one account all return
  200, and the limiter is asked 0 times.
- **Smallest fix:**
  1. Limit the mint per account: `env.AUTH_RATE_LIMITER.limit({ key: \`play-ticket:${accountId}\` })` in
     `mintPlayTicket`, answering 429 (for example, 10 a minute).
  2. Cap each account's share of the table: store `ptk:<id>` as `{ e, a }` and refuse `busy` only for an account that
     already holds, say, 20 live ids on this listing. The global cap stays as a backstop.
  3. Also limit `POST .../play-pass` per address through `authRateLimited`. It costs one HMAC per forged request, so
     this is defense in depth.

The cap itself is sound: it never forgets an id early (SAFE 2). The cap is soft by design: an entry beyond the first
5,001 keys is not counted, so the table can briefly hold a little over 5,000. That allows no double redeem, because
the spent check is a direct `get`.

### M2 (medium): a play ticket lives 10 minutes in a URL anyone can redeem

- **Where:** `src/play_pass.mjs:203` uses `makeIdentityTicket`, whose life is fixed at `TICKET_TTL_S = 600`
  (`src/identity.mjs:33,77`). The ticket travels in `eustress-player://play/<id>?...&ticket=<t>`.
- **Scenario:** the ticket is a bearer credential. Nothing ties it to the Player that should redeem it, and it cannot
  be tied: the site's only channel to the Player is the link. In the time a ticket lives, anyone who sees the link can
  redeem it as the minting account. Ways to see it:
  - the process command line (any same-user process, process-creation auditing, EDR telemetry);
  - the browser's "Open Eustress Player?" prompt;
  - a Player that is not installed or fails to start, leaving the ticket unspent;
  - a person who copies the link to share it.

  For an account with Adult content on, a shared link to an `adult_18` listing hands the recipient that listing for an
  hour. The real Player redeems within seconds of launch, so 10 minutes only helps someone other than the Player.
- **Probe (fails today; a policy probe):** `FINDING M2` asserts that a play ticket lives at most 120 seconds. Today it
  lives 600.
- **Smallest fix:** give `makeIdentityTicket` an optional `ttl`, and mint play tickets with `ttl: 120`. The redeem
  computes `issued = seen.expires - TICKET_TTL_S` (`play_pass.mjs:228`), so it must use the same value; otherwise the
  floor check is wrong by 480 seconds in the lenient direction. Consider a shorter pass life too: the Player needs the
  pass only while it downloads.

A residual risk stays even after the fix: an adult who deliberately hands a fresh link or a pass to a minor. That risk
is no larger than handing over the downloaded files or the session token, and the pass is far narrower than the token.
It is accepted.

### L1 (low): a Durable Object exception's message reaches the public redeem route

- **Where:** `src/live.mjs:882` returns `{ status: 500, error: String(e?.message || e) }`, and
  `src/play_pass.mjs:231` relays `spent.error` as it is.
- **Scenario:** a storage fault in the spend sends its internal message (storage or host details) to an
  unauthenticated caller.
- **Probe (fails today):** `FINDING L1` answers `500 {"error":"internal storage detail: shard 7 at do-host-42"}`.
- **Smallest fix:** in `redeemPlayTicket`, pass through only the known codes (`used`, `expired`, `busy`). For anything
  else, answer the fixed `503 play_pass_unavailable` copy.

### L2 (low): the Player replays the ticket to wherever a 307 or 308 points

- **Where:** `eustress/crates/client/src/systems/space_fetch.rs:225-228` builds the client with reqwest's default
  redirect policy (up to 10), and `:284-288` POSTs the ticket in the body. In reqwest 0.12.28, `redirect.rs:349-351`
  (`clone_body`) re-sends the body on a 307 or 308 to any http or https URL. Only headers are stripped across hosts
  (`remove_sensitive_headers`), and https-to-http is allowed (no `https_only`).
- **Scenario:** a 307 from the API origin, whether from a misconfigured edge rule, a hijacked route, or a local proxy,
  sends the ticket body to another origin, even over plain http. The PlayPass header itself is safe: reqwest drops
  `Authorization` across hosts and ports (`redirect.rs:239-251`).
- **Probe:** static only; no cargo was run. A Rust test would stand up a local server that answers 307 to a second
  local port and assert that the second port never receives the body.
- **Smallest fix:** `.redirect(reqwest::redirect::Policy::none())` on the fetch client (the API never redirects these
  routes), and `.https_only(true)` unless the base is a localhost test server.

### L3 (low, parity): sign-out and ban reach a pass through the KV mirror

- **Where:** `src/play_pass.mjs:138-142` (`aboveFloor`).
- **Behavior:** it reads `authfloor:` from KV, as `verifyAuth` does for session-class reads. A sign-out or ban
  recorded in the account object reaches passes up to about a minute later.
  `if (!env?.USERS) return true` fails open when the binding is missing, also as `verifyAuth` does. Every deployment
  binds `USERS` (`tests/deploy_bindings.test.mjs`).
- **Action:** none for staging. If passes ever gate anything beyond reads, check them fresh against the account
  object.

### I1 (informational): the Player still sends Studio's sign-in to the avatar service on a ticket launch

- **Where:** `net_play.rs:192` calls `credential::studio_token_for(...)` for the avatar whatever the launch carried.
  `space_fetch.rs:36-37` says "Studio's sign-in is not sent at all on such a launch."
- **Effect:**
  - Not a leak: the token is origin-bound, and goes to the origin that issued it.
  - The claim is wrong, though. A ticket launch plays the world as the website's account but shows Studio's
    account's avatar.
- **Fix:** skip the Studio token (or fetch the avatar with the pass, if the avatar route ever accepts one) when
  `PublishedTarget.ticket` was set, or correct the doc comment.

### I2 (informational): a pass does not open a private source world

`world.mjs` `mayReadSource` uses `deps.verifyAuth` (`Bearer` only). A pass therefore never reads a source world
unless the author shares it (`open_source`), not even the author's own pass. This fails closed. It is noted only
because the brief listed "source" among the routes the pass opens.

## What is SAFE, with evidence

All SAFE probes pass on the reviewed code, and each one fails under the mutation named.

1. **Replay and races.** Twenty concurrent redeems of one ticket give exactly one pass; the other 19 get `used`
   (probe "twenty concurrent redeems"; it fails when the `ptk:` check in `spend_play_ticket` is removed).
   - On Cloudflare the spend op awaits only storage calls, so the Durable Object's input gate makes get-then-put
     atomic.
   - An id is dropped only once its ticket is past expiry. `until < nowS` in the op, and `(e+1)*1000` in the alarm,
     match the redeem's `expires < now` refusal, so no window exists where a ticket is valid and its id is forgotten.
   - The `play:<id>` object name cannot collide with a hosting session: session names come from `[a-f0-9-]+` paths,
     which cannot contain `:`.
2. **The cap never lets a ticket through twice.** A full table answers `busy` and keeps every id (see M1).
3. **Key separation and token confusion** (probe "key separation"; it fails when `playPassOf` also reads a
   `Bearer epp1.` header or when `verifyAuth` decodes a pass). Each of these reads exactly as signed out on the world
   manifest:
   - a session JWT sent as `PlayPass`;
   - a play ticket sent as `PlayPass`;
   - a pass sent as `Bearer`;
   - `playpass` in lower case;
   - a pass re-signed with the raw `JWT_SECRET`, the identity-ticket key, or the image-URL key;
   - a header holding both schemes.

   Also:
   - A pass offered to redeem is `invalid`.
   - A play ticket at `/api/identity/verify` is refused with 400 (the audience is not 64 hex), and a pass there is
     refused with 401.
   - The derived key `HMAC(JWT_SECRET, "eustress-play-pass-v1")` cannot be obtained from any other raw-secret HMAC the
     Worker computes. Those messages all carry fixed prefixes (`unsub:`, `eustress-address:`, `eustress-visit-v2:`,
     guest purposes), and every JWT signing input contains a `.`, which the label does not.
4. **No escalation through Bearer-only paths** (probe "Bearer-only routes"; it fails under the `verifyAuth` mutation).
   On 20 routes a pass is exactly the same as no header, and none answers 200. The routes: me, logout, identity
   ticket, play-ticket, play count, favorite, commerce purchase, ledger spend, Tickets checkout, adult-content toggle,
   profile, inventory, purchases, admin cron-health, world begin, tags, server-credential, the deletion read, the
   delete request, and the live heartbeat.

   The reasons, by helper:
   - `verifyAuth`, `bannedBearer`, `authMethod`, `requireAdmin`, `freshGate`, `verifyFreshAuth`, `bearerOf` and
     `sessionDigest` all require a `Bearer ` prefix.
   - `kycGate`'s `actingAccount` and `commerce.mjs:1681` require `/^Bearer\s+/i`.
   - `withPlayPass` is used only by `handleGetSimulation` and `handleDownloadPak`, and `accountFromPlayPass` only by
     `servableListing`. That closure feeds only `refuseAdult`, `canServe` and the visit count.
5. **A pass is never an administrator** (probe "admin pass is no admin"). An administrator's pass for a held listing
   gets 404 on the listing and 403 on the manifest, because `requireAdmin` reads `Bearer` only and `servableListing`
   sets `admin = null` whenever a pass is in use. The author's own pass reads its held listing, as the author's
   session does.
6. **KYC.** An account that must still verify cannot mint: `kycGate` answers `403 kyc_required`, because the mint is a
   mutating session-class route.
7. **Cross-listing** (probe "cross-listing"; it fails when the `payload.s !== simId` check is removed). A pass for
   TEEN equals no header on TEEN_B's listing, manifest, chunk, source and `.pak`. TEEN_B's chunk hash asked for under
   TEEN's path is 404, since chunks live under `universes/<id>/chunks/` and must be named by that listing's manifest.
   Previews, thumbnails, covers, `/live` and join tickets never read a pass.
8. **Rating and state are judged on every request** (probe "judged per request"; it fails when `servableListing`
   skips `simForServing`). With a live pass:
   - turning Adult content off refuses the next request;
   - re-rating teen to `adult_18` refuses it;
   - a legal lock (`serve_block` and `content_lock`) refuses the manifest, the chunk and the listing;
   - unlisting refuses it;
   - the deletion R2 mark refuses it (and the listing GET answers 404);
   - a moved session floor (sign-out or ban) refuses it.

   A revoked approval binding reads as pending through `simForServing`, and only the author is served. Mint and
   redeem are both POSTs under `/api/simulations/{id}`, so `spaceDeleteWriteGate` refuses both on a deleted listing.
9. **Fail closed.** `accountFromPlayPass` catches everything and answers null. `inspectPlayPass` checks:
   - the exact shape (`epp1.`, a payload of at most 1,024 characters, a 43-character signature), before any work;
   - a constant-time HMAC comparison;
   - integer `e` and `i`;
   - expiry;
   - a life no longer than one hour;
   - the exact listing.

   The builder's 13 tests also cover forged, expired, misnamed and malformed passes.
10. **No leakage from the API** (probe "nothing the redeem or mint answers"). Error bodies never echo a ticket or a
    pass, and both routes answer `Cache-Control: private, no-store`. Nothing in `play_pass.mjs` logs, and the
    `LiveHost` logger prints the exception but not the arguments.
11. **No leakage from the Player.**
    - `Credential`, `PlayTicket`, `LinkExtras` and `FetchSpace` print no secret in `Debug`.
    - The header value is `set_sensitive`.
    - `Launch::from_args` removes the ticket before parsing, so no error or usage text can quote it.
    - Redeem errors quote only the status and a lower-case code (`pass_from_answer`).
    - Fetch errors use `without_url()`, and URLs never carry a secret.
    - Panic reports log no argv.

    One residue remains: the ticket in the process command line (see M2).
12. **The Player's fallback.** `fallback_api` moves only from staging to production, only for a link that named no
    server, and only on a 404 manifest. The pass is bound to the origin of the redeem URL (`Credential::issued_by`),
    and `credential::request` attaches it only to that exact origin, so a staging pass never travels to production.
    A link naming staging is refused by a production Player (`check_api`), so a staging ticket never reaches
    production's redeem route. A link naming production forces production. `Authorization` is dropped on any
    cross-host or cross-port redirect.

## Probe results (scratch copy of the tree as reviewed)

```text
tests/play_pass.test.mjs (builder)         13 pass, 0 fail
tests/play_pass_probe.test.mjs
  FINDING M1   fail  another account is locked out: 503 {"code":"busy"} (39 s)
  FINDING M1b  fail  20 mints from one account: all 200; limiter asked 0 times
  FINDING M2   fail  a play ticket lives 600 s
  FINDING L1   fail  relayed: 500 {"error":"internal storage detail: shard 7 at do-host-42"}
  SAFE x7      pass
Mutations (the SAFE probe each one breaks):
  playPassOf also reads "Bearer epp1."        -> key separation fails
  verifyAuth decodes a PlayPass payload       -> Bearer-only routes and key separation fail
  spend_play_ticket skips the ptk: check      -> concurrent redeems fail (20 passes)
  servableListing skips simForServing         -> judged per request fails (serve_block served 200)
  inspectPlayPass skips the listing check     -> cross-listing fails
```
