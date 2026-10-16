# Release notices: "Get notified"

Status: **Worker side built 2026-10-02; deployed on the orchestrator's schedule.** There is no Player release yet
(`downloads.eustress.dev/player/latest.json` answers 404), so the download page says "Public Alpha: first release coming
soon" and offers a **Get notified** button. A signed-in account with a verified email can ask to be emailed once when the
Player, or Studio, is released. The same routes serve both products.

Code: `infrastructure/cloudflare/api/src/release_notify.mjs`. Tests: `tests/release_notify.test.mjs`.

## Products

Exactly two: `player` (Eustress Player) and `studio` (Eustress Studio). Any other name answers `404`
`{ "error": "Unknown product", "code": "unknown_product" }`.

## The account's routes

All three need a signed-in account (`Authorization: Bearer ...`) and are session routes in `route_policy.mjs`: a mail
preference, easy to undo.

| Route | What | Answer |
|---|---|---|
| `GET /api/release-notify/{product}` | Where the account stands | `{ "subscribed": bool, "email_verified": bool }` |
| `PUT /api/release-notify/{product}` | Subscribe. Idempotent. | `{ "ok": true, "subscribed": true, "email_verified": true }` |
| `DELETE /api/release-notify/{product}` | Unsubscribe. Idempotent. | `{ "ok": true, "subscribed": false }` |

- `PUT` is refused `409` `{ "code": "email_not_verified" }` unless the account has a **verified** email. The
  predicate is the one the email routes use: the account object's `email_status` (`verified`), see
  `docs/architecture/IDENTITY_KEY_ROTATION_AND_RECOVERY.md`, "Verified email". An account with an address on
  file that was never confirmed cannot subscribe; it confirms the address first (`POST /api/account/email/start`).
- **No address is accepted from the request.** `PUT` reads nothing from its body. The address is the account's own,
  read when the notice is sent. No response carries an address.
- `DELETE` works for any signed-in account, verified or not, and answers the same whether or not there was a
  subscription.
- `PUT` and `DELETE` are rate limited per account (`release-notify:{accountId}` on the same limiter as the other
  account writes, `AUTH_RATE_LIMITER`): `429` `rate_limited` with `Retry-After: 60`. `GET` is not. The routes skip the
  limiter when the binding is missing, so `tests/deploy_bindings.test.mjs` requires it in both environments.
- A deployment with no account objects answers `501` `not_configured`; an account object that cannot be reached, `503`
  `account_unavailable`; a signed-in token whose account record is gone, `404` `no_account`.

The page can show: not signed in, sign in; signed in with `email_verified: false`, a link to verify the email first;
signed in and verified, the **Get notified** button (`PUT`), then "You will be emailed once" with a way to cancel
(`DELETE`). When the release exists, `latest.json` answers and the button gives way to the download.

## What is stored

One key per subscription, `relnotify:{product}:{accountId}` in the account KV (`USERS`), holding `{ "at": "<ISO time>" }`
and nothing else: no copy of the email, no expiry. A second `PUT` leaves the first untouched.

## The administrator's send

Nothing runs this by itself. An administrator calls:

```
POST /api/admin/release-notify/{product}/send
{ "version": "0.1.0", "url": "https://downloads.eustress.dev/player/0.1.0/...", "dry_run": false, "cursor": "...", "limit": 50 }
```

- **Admin only** (the one `isAdmin` predicate: the `admin:{id}` flag or the `admin` role); anyone else gets
  `403` and nothing is sent. A `fresh` route in `route_policy.mjs`: it cannot be taken back.
- `version`: major.minor.patch with an optional short pre-release tag (`0.1.0`, `1.0.0-alpha.1`).
- `url`: `https`, on `downloads.eustress.dev` or `eustress.dev` only, no name or password in it, no port other than the
  default, no space or control character, at most 300 characters, and **no query and no fragment**. On
  `downloads.eustress.dev` any path; on `eustress.dev` only a download page: `/download` or a path under `/download/` or
  `/downloads/`. (`https://eustress.dev/redirect?to=...` is a redirect somebody else can aim; it is refused.) The email
  carries the address as the address parser reads it.
- `limit`: subscribers handled in this request, 1 to 60, 50 if left out. `cursor`: the `next_cursor` of the earlier
  answer.
- `dry_run: true` returns the counts and sends, deletes and claims nothing.
- A refused request is `400` with `bad_version`, `bad_url`, `bad_limit`, `bad_cursor` or `bad_request`, before a
  subscriber is touched. With no mail binding, `501` `mail_not_configured` (a dry run still works); with no bucket for the
  claims or no secret to key them under, `501` `not_configured`.

### What one request may spend

One request can only make so many calls: every KV, R2, Durable Object and mail call counts against the platform's
subrequest limit for a request (1000). One subscriber costs at most **11** calls (the account's user record 1, its
object's state 1 and `email_status` 1, the claim 1, the daily allowance 4, the mail 1, the claim rewritten 1, the
subscription deleted 1) and **14** when the account's object is set up in that run; a request itself spends about 12
(admin check, session gate, list, audit). So a page of 60 is at most 60 x 14 + 12 = 852 calls, under the limit; 70 would be
992, with nothing to spare. That is why the page is 60 and the send works a page at a time
(`SEND_PAGE_MAX`, `SUBREQUESTS_PER_SUBSCRIBER`, `SUBREQUESTS_PER_REQUEST` in the code; a test counts the calls).

The answer counts and never names anyone:

```json
{ "ok": true, "product": "player", "version": "0.1.0", "sent": 48, "skipped_unverified": 2, "failed": 0,
  "already_sent": 0, "unconfirmed": 0, "orphans_removed": 0, "would_send": 0, "would_remove_orphans": 0,
  "dry_run": false, "done": false, "next_cursor": "relnotify:player:..." }
```

Run it again with `cursor` set to `next_cursor` until `done` is true. Subscribers that were mailed are gone, so a run from
the start only meets those that were skipped, failed or unconfirmed.

### Each subscriber

1. The account must still exist. Its user record is read first; if it is **not there** the account is definitely gone (a
   deleted account) and the subscription is deleted: `orphans_removed`. A lookup that fails any other way (a KV or object
   error, a transient failure) is `failed` and the subscription is kept: only a definite not-found removes anything.
2. The email must **still be verified**, checked now. If not, `skipped_unverified`: the subscription is kept, and a
   later run reaches it once the email is verified again.
3. The address is **claimed before anything is sent**, ONE email goes out in the shared shell through the account mail
   path and its limits (`sendNotice`, `takeMailSlot` in `src/mail_limits.mjs`, the per-account `notice` allowance), the
   claim is rewritten as sent, and the subscription is deleted: `sent`.

### At most once: the claim

The claim is an R2 object, `locks/release-notify/{product}/{version}/{keyed hash of the address}.json`, created with
the precondition `onlyIf: { etagDoesNotMatch: '*' }`, which R2 enforces atomically. It is the primitive moderation's
lease uses (`acquireLease` in `src/moderation.mjs`); KV is eventually consistent and cannot do it. Two sends that overlap,
a retry after a timeout and a double click all race for the claim and exactly one wins. A claim holds
`{ state, at, run }` and no address.

- **A claim someone else holds** is never sent over. If its state is `sent`, the address is `already_sent` and the
  subscription ends. If it is `claiming` and younger than `CLAIM_STALE_MS` (5 minutes), another run is sending it right now:
  `already_sent`, and the subscription is kept (that run ends it, or, if its send fails, gives the claim back).
- **A send that fails**, or that the daily allowance refuses, **deletes its claim**: `failed`, the subscription is kept,
  and the next run tries again.
- **A claim still `claiming` after 5 minutes**, or one that cannot be read, is **`unconfirmed`**: the Worker died between
  the claim and the mail, or could not write the confirmation. It is not treated as sent and it is not sent again: an
  email is at most once, and a silent skip is not an answer, so the count is in the response for a person to look at.
  Whether the mail went is in the mail provider's log; to send again, delete the claim object.
- The mail is handed over after the claim exists, so no failure of the records that come after it (the confirmation, the
  subscription delete, both of them, a KV outage, a 429 burst) can make a second email possible: the claim stands, and
  the next runs find it `claiming` or, past 5 minutes, `unconfirmed`.

The claim's name is a **keyed hash**: HMAC-SHA-256 of the lower-cased address under a key derived from a secret for this
one purpose, truncated to 128 bits. The secret is the one the account object's audit already uses to name an address
(`AUDIT_PRINT_KEY`, falling back to the Worker's `JWT_SECRET`, see `addressPrint` in `src/email_ops.mjs`); the key is derived the
way `src/visits.mjs` derives its address key, so the secret itself signs nothing here. Whoever can read the claims
cannot test whether an address received a notice.

### The audit entry and the logs

One audit entry is written per request (`RELEASE_NOTIFY_SEND`, with the product as its target): who, the version, the
link, whether it was a dry run, and the counts. An address is never in it. If the audit store fails the send is not
undone. **No address reaches a log**: a mail binding's error can quote the recipient, so `sendNotice` logs the kind of error and the
message with every address masked (`scrubAddresses` in `src/account_notices.mjs`: bare, quoted, in angle brackets, any case,
percent-encoded), and the send's own logs do the same.

### Resolving an `unconfirmed` claim: by hand, by the owner, rarely

There is no route for this, on purpose (Security, 2026-10-02): deleting a claim IS permission to email that address again,
so it is a deliberate act after looking at the mail provider's log, done by the owner in the Cloudflare dashboard (R2,
the `SCENES` bucket) or with wrangler. If it is ever made a route it must be FRESH, admin-only, one claim at a time, refuse a
claim younger than 5 minutes or already `sent`, and audit the admin id and the claim name.

1. A send answers `unconfirmed: N` (a dry run answers it too, without sending anything).
2. In the R2 object browser open the prefix `locks/release-notify/{product}/{version}/`. Each object is one address's claim,
   named by a keyed hash, so the name does not say whose it is. Read each one: `{ "state": "claiming", "at": ..., "run": ... }`.
   The unconfirmed ones are `claiming` with an `at` more than 5 minutes old.
3. Check whether the mail left, in the mail provider's log for that version, around each `at`. The log is the only place that
   knows; the Worker died between the claim and the mail, or could not write the confirmation.
4. If the mail LEFT, leave the claim: that subscriber has their one email, and the subscription is still there until a run
   meets the claim again and ends it as `already_sent`.
5. If the mail did NOT leave, delete that one claim object. The next send (from the start, no cursor) emails that address once.
6. Never delete a claim whose state is `sent`, or a `claiming` claim younger than 5 minutes: a send may be running.

### The first production send

The Player and Studio must exist at the link before it is sent. Then, in this order: a **dry run** (`dry_run: true`) and read the
counts; one page with `limit: 1` to a test account the owner controls, and read the email; only then the full run, page by
page with the `cursor` the answer returns, until `done`. Production needs the owner's explicit OK.

## The email

Built in the shell (`src/email_shell.mjs`), `plain` footer: the one link is the button (and the same address spelled out
beside it); the footer carries none. No tracking pixel, no tracking parameter, one image (the header gear), a plain-text
twin with the same link. It says what it is (a release notice for the product), who it is for (the verified address of
the account that asked) and that it is sent once. The subject is "Eustress Player 0.1.0 is available". The sender is
`RELEASE_MAIL_FROM`, else `NOTICE_MAIL_FROM`, else `identity@eustress.dev`, shown as **Eustress**, not "Eustress
Security": a security notice carries no link, and a message that does must not look like one. A sender name is
plain words (letters, digits, spaces, periods, hyphens, at most 60), so a name cannot restructure the From header.

## What it does not do

- It does not run on a schedule, on a deploy or on a published release. A person runs it.
- It does not read, keep or answer an address.
- It does not unsubscribe by link (the email ends the subscription itself). The page has the `DELETE` for anyone who
  wants out before then.
