# Guest play: the Gallery without an account

**Status:** design, answered by McKale on 2026-09-30, Worker build starting. Lead: Website session. Reviewed once by an independent advisor (2026-09-30), whose findings are folded in below.
**McKale's decision (2026-09-28):** people can play verified simulations from the Gallery for free, without signing up: 3 simulations a day and 60 minutes in total. When either runs out, the page asks them to register. Play launches in the browser, after a choice of "Male or Female". The desktop Player requires an account.

## 1. What can and cannot be enforced

This decides the shape of everything else.

| Kind of play | Where it runs | What a server can enforce |
|---|---|---|
| Browser, a world that runs on the visitor's machine (version 1) | The visitor's browser | How many worlds a guest is allowed to start, and a time lease the server measures with its own clock. Not what a modified browser does with content it already downloaded. |
| A world hosted by another user's Studio | The host's machine | Nothing today. `GET /api/simulations/{id}/live` returns the join link, with its key and pin, to anyone for an approved listing, and the host checks nothing. A guest needs no ticket to join. |

So the guest allowance is a **funnel and capacity limit, not content protection**. Published worlds are public by design: the desktop Player downloads them anonymously. Version 1 therefore covers the first row only, and says so to McKale. Hosted guest play waits until the join link is gated and hosts check who joins (section 8).

## 2. Who is a guest

**McKale's rule: one guest allowance per network address per day.** So the guest is the address, and there is nothing for the visitor to store.

- The Worker derives the guest id from the caller's address: `gid = HMAC(day key, address)`, with IPv6 reduced to its /64 and IPv4-mapped IPv6 read as IPv4. The day key is `HMAC(JWT_SECRET, "eustress-guest-day:" + UTC date)`, so the id changes every UTC day, cannot be turned back into the address, and cannot be matched from one day to the next.
- No cookie, no `localStorage` token, no header. Clearing storage changes nothing, since the address is the identity.
- Everyone behind one address shares one allowance: a household, a school, an office, a phone carrier. The first visitor of the day may play as a guest, and the next one sees the register prompt. McKale accepted this. A VPN gets a new address and so a new allowance, which he also accepted as known.
- Nothing about the visitor is recorded except the chosen character, `M` or `F`, inside the ticket. The page may remember that choice in the tab (`sessionStorage`) to pre-fill the avatar if the visitor registers.

## 3. The allowance

One Durable Object per guest id (which already contains the day), holding:

- `sims`: the distinct simulation ids started that day, at most **3**.
- `seconds`: seconds of play used that day, at most **3600**.
- `lease`: the one active lease, if any (section 4).

It deletes itself by alarm 25 hours after its day began. Starting a simulation already counted that day does not count again.

## 4. Leases: time measured by the server

A **ticket** is permission to hold a lease on one simulation. It is not a countdown the browser reports.

1. `POST /api/guest/play {sim_id, character}`: the Worker checks that the listing is approved and rated `all_ages` (section 6), that the guest has a sim slot and seconds left, and that this guest holds no other lease. It answers `{ticket, expires_in, remaining_seconds, sims_left}`. The ticket is a signed `{typ:"lease", tid, gid, sim, character, day, iat}`, valid for a **full 60-minute lease or the seconds left, whichever is less**, and debited against its issue day, so a guest is never cut off at midnight UTC.
2. The browser Player sends `POST /api/guest/beat {ticket}` every 30 s. Each beat debits the time the Worker measured since the previous beat, **capped at 1.5 times the interval**, and answers `{remaining}` or `{expired}`. No beat for 90 s ends the lease.
3. A second lease on the same guest ends the first: a ticket pasted into a chat serves one player.
4. There is **no refund and no host-reported leave**. A client that stops beating only hurts itself. This closes the colluding-host and the sock-puppet attacks on the previous draft's reserve-and-refund model.

The browser Player stops play when a beat answers `expired`. A modified client can keep running content it already holds, which section 1 accepts.

## 5. Abuse limits

- **One allowance per address per day** (section 2) is the main limit. There is no human check (McKale decided against Turnstile) and no mint cap: there is nothing to mint, since the id is the address.
- The address is never stored. The Durable Object is named by the day-keyed hash and deletes itself by alarm within 25 hours.
- `POST /api/guest/play` also goes through the per-address rate limiter used for the auth routes (`AUTH_RATE_LIMITER`, 30 a minute).
- Known and accepted: a VPN or a changing mobile address gets another allowance. The content is public, so there is little to gain.
- A listing may cap its guest sessions later (a share of `max_players`), so account holders always get in. That belongs to hosted play.

## 6. Which simulations

A simulation is guest-playable when `isListable` holds (approved and public) **and** its rating is `all_ages`, **and** it runs in the browser (section 9). Guests have no age check and may be children, so anything rated above `all_ages` asks for an account. Guest sessions have no text or voice chat, a fixed display name "Guest", and persist nothing.

## 7. The pages

1. **Play** on a listing, signed out: a dialog titled "Choose your character" with two buttons, "Male" and "Female". The listing's own page already shows OFFLINE or LIVE and the rating.
2. The choice calls `/api/guest/play`. A refusal carries `{code: "guest_limit", reason: "sims" | "minutes", resets_at}` and the page shows what ran out and when it resets (midnight UTC, shown in the visitor's local time), with a Register button.
3. After registering, the page returns to the listing and offers Play with the account: the account's launch needs no guest ticket, and accounts have no guest allowance. The `M` or `F` choice, remembered in the tab, becomes the new account's starting avatar (`PUT /api/avatar`).
4. The desktop Player requires an account to play. The installer is a static file, so gating the download page stops nothing; the download page asks signed-out visitors to sign in first as a courtesy, and the Player itself enforces the rule.

## 8. Not in version 1: guests on hosted sessions

Needs three things first, none of which the Website session owns:

1. `GET /live` returns the link only to a caller holding an account token or a live guest lease.
2. The host registers each join with the LiveHost Durable Object (an online check), so a pasted ticket cannot join twice.
3. Hosts receive `guest = true` and apply the same rules (guest user ids 2^52 + peer, no purchases, no chat).

Then the host beats carry the active ticket ids, and the Worker debits those leases the same way as in section 4.

## 9. Blocked on other work

- **The browser (WASM) Player does not exist.** Until it does, guest play is a design. Which listings run in a browser (size, no Luau virtual machine, no features the port lacks) must be known to the Gallery: a `browser_play` flag on the listing, set at publish from the manifest. Owner: Eustress WASM and Multiplayer. Without it, the Play button would offer guests worlds the browser cannot run.
- **Privacy policy** gets the section in section 12, which McKale reviews before it goes on the site.

## 10. Build order, once the open questions are answered

1. Worker: `guest.mjs` (address-derived guest id, lease ticket, allowance Durable Object), routes, binding and migration `v4-guest`, tests with fake storage (quota, lease, second lease, the address key and its IPv6 and IPv4-mapped forms, rating gate, ticket replay, day rollover).
2. Site: the character dialog, the limit prompt, registration conversion, the privacy page.
3. Browser Player: heartbeat and stop on `expired` (Eustress WASM).
4. Later: hosted guests (section 8).

## 11. Decided by McKale (2026-09-30)

1. Version 1 is browser-only, with a soft limit against a modified client; hosted guests come after the `/live` link is gated (section 8 and `docs/networking/HOSTED_ADMISSION.md`).
2. Guests play only `all_ages` simulations.
3. The daily reset is midnight UTC, shown in local time, and a lease is never cut at midnight.
4. One guest allowance per address per day; no Turnstile; VPN bypass accepted.
5. The dialog says "Choose your character", with the buttons "Male" and "Female".

## 12. Privacy policy text, for McKale's review

> **Playing as a guest.** You can play approved, all-ages simulations from the Gallery without an account. When you do, we keep a one-way fingerprint of your network address, made with a key that changes every day, so it cannot be turned back into your address or matched from one day to the next. We use it to count how many simulations you start and how long you play in a day. We also keep those counts, the simulation you chose, and the character you picked (Male or Female). We delete all of it within 25 hours. We do not ask for your name or email, and we do not use cookies for guest play. The page may remember your character choice in your browser tab until you close it, so it can pre-fill your avatar if you register. We do not link guest play to an account you create later. Cloudflare, which delivers eustress.dev, sees your network address in order to deliver the site; see its privacy policy.
