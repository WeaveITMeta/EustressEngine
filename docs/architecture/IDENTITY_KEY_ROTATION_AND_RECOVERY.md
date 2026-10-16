# Replacing an identity key, and recovering a lost one

**Status:** design, revision 7 (2026-10-01). Replace-my-key (b) and the account Object are as Security approved them and are
the part to build first; **Phase 1 can proceed now**. Recovery of a lost key (c) is now a **build-it-ourselves** design after
McKale's decisions of the same day (own live capture and face match; only the extra checks for accounts that hold money; recovery
photos and video kept 14 months). Security approved revision 5 as a design and its launch gates, reviewed revision 6 and **approved spikes S0, S1 and S2** (not
the build of S3 onward and not a launch) until five items were in the spike criteria, the pass bar and the gates; **revision 7
folds all five in**, and appends Security's statement of residual risk. **Revision 8 records Phase 1 as built** (next section); recovery
(c) is not built. Owner: Website (Worker and site). Staging first; production only on McKale's go.

## Phase 1 as built (2026-10-01, staging only)

Worker (`infrastructure/cloudflare/api`), each part with tests that fail when the rule is removed:

- **The account object** (`src/account_auth.mjs`): one per account; current key, retired keys, session floor, value freeze, payout pin,
  hash-chained audit, outbox of KV mirror writes. One operation at a time, the retry alarm included; every mirror write has a 5 second
  limit and every call to the object an 8 second limit, so a stuck write cannot hold sign-in; the alarm is armed before anything is
  written; a floor write only moves the floor up. Sign-in asks the object and fails closed.
- **Replacing a key** (`src/identity_routes.mjs`): challenge, two signatures, one step; the credentials list with revoke; the notice to
  every address; at most 3 replacements per account per day, counted where both signatures are good. The user record is not rewritten:
  the key and the epoch are read from the object, Discord's unlink is a fact in the object, and account deletion removes the current
  key's index entry.
- **Verified email** (`src/email_ops.mjs`, `src/email_routes.mjs`): the first link goes only to the address on file; a change takes the
  current key's signature, waits seven days, warns the existing addresses with a cancel link (which also ends every sign-in), and a
  replacement of the key drops a change the old key asked for. The account's address is a key of its own (`email:{id}`), never a
  rewrite of the user record; the audit holds an HMAC print of an address, never the address. Account mail has a daily allowance per
  recipient (`src/mail_limits.mjs`: 5 verification links, 20 notices) with the overflow counted.
- **The gate and the table** (`src/route_policy.mjs`, `src/fresh_auth.mjs`): every POST, PUT, PATCH and DELETE is classed public, session,
  fresh or value. Fresh and value routes ask the object; value routes refuse a frozen account (423 `account_frozen`). Only the strict
  classes use a prefix; a path in the source that is not named fails the test.
- **What the freeze and the pin hold back:** value routes (Bliss spend, a purchase and its refund, Connect onboarding) for 72 hours, or
  7 days when the account held value; Connect onboarding, which creates the payout account and opens the page that edits it, for 30
  days (423 `payout_pinned`); the nightly payout run moves a frozen account's share into a hold (an unreadable freeze counts as frozen) and
  pays it once the freeze has ended, exactly once, to the destination the day was planned with (the pin keeps that the owner's own).
  The object keeps `valuefreeze:{id}` in KV for code with no session.
- **Seen in `/health`:** whether the account objects and the value sources are bound; a test reads `wrangler.toml` for both
  environments.

Site (`eustress/crates/web`): Settings, Security has "Email address" and "Identity key"; the one-time prompt asks to confirm the
address first, then to replace the key; `/verify-email` and `/cancel-email-change` act only when a person presses the button, take the
token out of the address bar and send no referrer; a banner shows a pending change of address across the signed-in site.

- **A key replacement is not a thing that can half happen** (`tests/key_change_durability.test.mjs`): the notice is recorded in the
  same write as the key; the route that made the change sends it and acknowledges it (`ack_notice`, per address); the object's alarm
  sends what is not acknowledged after a minute, retries a failed send with growing waits and gives up after six; a change answers
  its caller after at most 3 seconds spent on KV (the caller waits 8), so a stuck KV cannot make a caller give up on a change that
  was already made; and a rotation whose answer was lost says it may have gone through and to try the new file.
- **An administrator's replacement** (`POST /api/admin/accounts/replace-key/begin` and `/replace-key`, admin only, switched on by
  `ADMIN_REPLACE_KEY_ENABLED`, which production leaves unset): for an owner who lost the key. A person has checked who the owner is;
  the owner made a new key on `/replacement-key` after support opened a case, and sent only its public half and a signature.
  - **The administrator signs with their own identity key**, through the same challenge flow as a rotation, over
    `eustress-admin-replace-v1 / origin / admin id / account id / new key / case / challenge`; the administrator's own object checks it,
    spends the challenge, and counts at most **5 replacements a day** per administrator (counted where a signature is good, so a
    stolen session without the key uses none). A stolen administrator session alone replaces nothing. The target is never the
    administrator or another administrator.
  - **The new key signs too**, over `eustress-admin-replace-key-v1 / origin / username / case / new key`: a mistyped, truncated or
    invented key cannot retire the old one. Both messages have shared vectors (`docs/architecture/admin-replace-vectors.json`) read by the
    Worker's tests, the support script (`scripts/admin-replace-key.mjs`) and the site.
  - **The owner can undo it.** The notice to every address the account had carries a one-time link, valid for the length of the freeze and at
    least 7 days, that puts the earlier key back, retires whatever is current as `disputed` (the holder of the new key may have replaced it
    again), ends every session, keeps value frozen and the payout pinned, flags the case in `admin-review:{id}` (listed by `GET
    /api/admin/accounts/review-flags`, closed by `POST /api/admin/accounts/review-flags/resolve` with a note) and tells every address. A
    support person who was deceived cannot lock the owner out for good. **While the link is open, no second replacement of the account is
    allowed** (409 `replacement_in_window`): a second link would put back the key the first replacement installed. The notice and the page
    say in plain words that someone who DID ask support for the replacement must not use the link: it puts the OLD key back.
  - **A second channel, first:** after every check has passed (a dry run) and BEFORE anything changes, the security mailbox
    (`SECURITY_MAILBOX`) and the other administrators flagged `admin:{id}` are mailed the case and the reason (no key, no link); if the
    security mailbox cannot be told, nothing changes (503 `second_channel_unavailable`). One predicate (`src/admin_check.mjs`) decides who
    is an administrator, for the caller and for the target.
  - **Durable:** the case, the reason and the administrator are in the account's audit chain and in the admin log (written before anyone is
    mailed); everything that acted as the account (API keys, webhook endpoints, the Discord link) is owed a revocation in the same write as
    the key, tried at once and by the object's alarm with growing waits (eight tries), then flagged for an administrator; the answer says
    `revoked_complete: false` until it is done.
  - Every session ends, value is frozen, the payout destination is pinned, and every address is told. The administrator gets no token. It does
    not use up the owner's 3 replacements a day.

**Support procedure for a lost key:** the owner writes to support from the address on the account; support checks identity by the
policy in force (compare with the sign-up ID; a call) and opens a case; the owner opens `/replacement-key`, types the username and the
case number, and sends the public key and the signature it shows; an administrator runs `scripts/admin-replace-key.mjs` with their own
identity file; the owner signs in with the new file. If the owner did not ask for it, the link in the notice undoes it.

**Before `ADMIN_REPLACE_KEY_ENABLED` goes on in production** (the route is off there until every line is true, Security has cleared it,
and McKale says go):

1. `EMAIL` is bound, and `/health` shows `integrations.admin_replace.email_bound: true`.
2. `SECURITY_MAILBOX` is set to a mailbox that people other than the administrators read and that a named person is on the hook to watch
   (administrators known only by role are not mailed: the security mailbox carries the second channel), and `/health` shows
   `security_mailbox_set: true`.
3. One dry run on staging has actually delivered the summary to that mailbox (staging with the same kind of mailbox), and someone read it.
4. The people who may replace a key hold identity files of their own and have run `scripts/admin-replace-key.mjs` against staging.
5. The review flags (`GET /api/admin/accounts/review-flags`) have an owner who looks at them, and know the `/resolve` route.

**Not built, and so not claimed:** the Studio and Player half (Studio's Sign Out is local, `AuthState::sign_out_everywhere` exists); recovery (c); the
passphrase lock; a second administrator's co-signature for replacing another administrator's key (such a request is refused); credential
revocation as part of a rotation's own durable write (it follows the change and is reported; an administrator's replacement owes it durably).

McKale's decisions of 2026-10-01 that this implements:

- **(a)** Sign-up keeps the identity `.toml` download and the email, as today. The file now carries only the account,
  the keys and `verified`; a passphrase lock is optional, with plain download plus email as the default.
- **(b)** "Replace my key": a signed-in user generates a new key pair in the browser; the Worker swaps the account's
  public key and revokes the old one, and every token issued under it ends. Every existing account gets a one-time
  prompt to do it.
- **(c)** Recovery of a lost key, in his words, as redesigned: **a selfie check at launch, through a live stream that
  deepfakes cannot get through; no handwritten-code interim; a recovery is approved or denied, and near-instant is the
  goal, on the assumption that people will try to impersonate account holders; the ID stays on file for KYC
  compliance; the recovery photo or footage is kept, so that if an impersonator gets through the real holder can
  revoke the recovery and the fake becomes evidence; liveness through a live stream plus an AI agent that spots
  fakes and tells the real holder from someone else, compared against the stored ID and sign-up photos.** Low
  confidence means deny, not hold. Biometric law needs a lawyer's view before launch; this design flags it and does
  not wait for it.

## What the outside review changed, plainly

An independent review of the liveness idea (with Cloudflare's current documentation checked) reached conclusions
McKale should have before he chooses:

- **A live stream is not what stops a deepfake.** A virtual camera feeds a WebRTC stream exactly as a real webcam
  does. A capture counts as live only when the **server issues a challenge the client could not predict and checks the
  response on its own side**, and when the capture is protected against **injection attacks** (a swapped video fed in
  below the browser). Stopping those at scale takes certified injection-attack detection and device-integrity checks,
  and a web page can give neither.
- **Cloudflare cannot do the live analysis by itself today.** Realtime's SFU terminates WebRTC, but its WebSocket
  adapter (beta) forwards audio plus JPEG video at about **one frame a second**, and a higher rate needs an
  Enterprise contract; one frame a second cannot measure a screen-flash reflection. Its recording path writes a
  recording to our storage for analysis **after** the session. Containers have no GPU and inbound UDP is not
  confirmed. Workers AI has vision models but no certified face matcher or liveness model. The owner's workstation
  is out: biometric data on a home machine is a legal risk and a single point of failure.
- **Two language models are not two independent checks.** They see the same frames and share blind spots. A model's
  confidence is not calibrated and drifts between versions, so it must not be the face-match gate.
- **The ID portrait is a weak baseline.** Anyone with stolen ID photos has the portrait, and a deepfake is tuned to
  match it. A face captured **while the real holder still held their key** is a much better reference.

McKale decided to **build the capture ourselves** (not Persona or Stripe Identity), knowing the above. So the design keeps his shape
(instant approve or deny, a live capture, the AI decides, the footage kept, the real holder can revoke) and builds the live
capture, the face match and the fake detection in house behind a swappable `LivenessProvider` interface; the decision, the audit,
the revoke path and the retention are ours as before. The section "The capture we build" says what that means, including what it
cannot promise, and "The spike plan" says how each part is proved. The three safeguards he did not choose (a recovery key, an
opt-in face baseline, a freeze after recovery) are written up under "Options kept for later".

## What exists today

- An account is an Ed25519 key pair made in the browser. `user.public_key` holds the key; the index
  `pubkey:<hex>` maps it to the account. Sign-in signs a one-time challenge (login v1); tokens last 72 hours.
- Each account has a **session floor** (`authfloor:{id}`): a token issued before it is refused. Sign-out, a ban
  and a deletion move it.
- KYC: one model call reads the ID and stores name, date of birth, sex, jurisdiction and its analysis in
  `kyc-{session}-front`; registration copies it to `kyc-{userId}-front`. **Nothing reads the ID number from the
  document.** `id_hash` comes from the registrant's request body, is optional, and is used unvalidated as
  `idhash:{hash}`: **one ID, one account has never been enforced.** **There is no selfie, no liveness check and no
  face baseline for any account**, and no email is verified.
- Registration takes a public key without proof that the registrant holds the private key.
- **Credentials that never read the session floor:** commerce API keys (`apikey:{hash}` to the account) and
  webhook endpoints authenticate as the account. The Discord link and the Stripe Connect connection also
  belong to the account.
- Seven places rewrite the whole `user:` record from a read (register, ban, warn, review, risk override,
  rescreen, and one in the commerce path), so any of them can write a stale `public_key` back.
- **The stored ID images are now the master key to every account.** If recovery trusts a document match, whoever
  holds the ID images is a recovery attempt away from the account. They need the protection of the footage
  (below), not less.

## Principles

1. **One authority per account.** The current key, the session floor, the key history, the freeze, the recovery
   key and any recovery or dispute live in one Durable Object per account (`AccountAuth`), which handles one
   request at a time. KV keeps a mirror for reads that may lag by about a minute. Every auth and email path reads the
   key from the Object; after migration `public_key` is dropped from the user record.
2. **Evidence in, code decides.** The ID image and the footage are attacker-controlled input. Models and the
   provider fill a fixed schema of scores and fields; the Worker applies thresholds in code; anything missing or
   below threshold is a **denial**. No default approval, ever.
3. **The real holder can always say no.** After any key change the owner is told on every channel they have, and
   one click pauses value and key re-enabling without a sign-in (a pause destroys nothing, and lapses after 48 hours unless corroborated). That channel must be one the owner
   demonstrably controls: a verified email, a live session, or a retired key.
4. **Every key change is announced** (to every address in the notice rule, under "Verified email"), recorded in the
   hash-chained audit with old and new key fingerprints, and alerts the admin when it follows a dispute.
5. **Value waits, and so does everything that acts as the account.** After a key change, payouts, withdrawals and
   transfers are frozen, the payout destination is pinned, and API keys, webhooks and links are dealt with
   (below). The freeze holds **everywhere value moves, not only on user routes**: the scheduled payout run in
   `scheduled()`, and the internal Wallet, Treasury and CommerceHub paths, skip a frozen account, and a test proves
   it. **A recovery has no value freeze by default (McKale's decision); the extra checks for accounts that hold money, the one-click pause and the circuit breaker are what limit a fake that gets through, and a freeze after recovery is an option kept for later.** The freeze after a rotation by the old key (b) and the pause in a dispute stay.
6. **Biometric data is treated as the most dangerous data we hold.** Separate encrypted storage, logged access,
   consent before capture, a published retention schedule, deletion by schedule, a legal hold for disputes.

## The identity file

- **Contents:** `user_id`, `username`, `public_key`, `private_key`, `issued_at` under `[identity]`, and `verified` under
  `[verification]`, with the two header comments. No birthday, no ID type, no ID hash. `generate_identity_toml` (web
  `login.rs`) and `validIdentityFile` (Worker) change together; the validator accepts the old shape and the new one
  for files already issued. *Built and on staging.*
- **Encrypted form (optional lock):** the user may choose a passphrase at sign-up and in "Replace my key"; without
  one the file is the plain one and is downloaded and emailed as today. The lock is a versioned envelope that records
  the key-derivation function and its parameters (Argon2id through the `argon2` crate in WebAssembly; PBKDF2-HMAC-SHA256
  with at least 600,000 iterations only if WebCrypto is all that is available), a salt, and AES-GCM. The Worker and the
  mail provider then never hold a raw key. `validIdentityFile` accepts the envelope by shape and length only.
- **How a locked file marks itself (proposal, for Security and Multiplayer):** a table named `[lock]`. A file with a
  `[lock]` table is locked; Studio checks for it before reading any key. The public parts stay readable, so the Worker
  can bind the file to the account and Studio can say which account it is; only the private key is inside the lock:

  ```toml
  [identity]
  version = "1.0"
  user_id = "..."
  username = "..."
  public_key = "..."          # readable; no private_key here
  issued_at = "..."

  [lock]
  version = 1
  kdf = "argon2id"            # or "pbkdf2-sha256" with iterations >= 600000
  memory_kib = 65536
  iterations = 3
  parallelism = 1
  salt = "<base64, 16 bytes>"
  nonce = "<base64, 12 bytes>"
  ciphertext = "<base64>"     # AES-256-GCM over the 32 private-key bytes, tag included; the public key is the associated data
  ```

  `validIdentityFile` accepts the `[lock]` keys by name, base64 shape and length. A locked file carries no
  `private_key`, so the older readers (which need one) fail with "no private key" rather than a wrong-key error.
- **Order:** Studio's identity parser reads two 64-hex fields, so a locked file fails there. So: **Phase 1** issues
  replacement keys as the plain file, downloaded and emailed as at sign-up (the default McKale chose); **Phase 1b** adds
  passphrase support to Studio (Multiplayer), and only then do sign-up and "Replace my key" show the lock option, so
  nobody locks themselves out of Studio. A Studio handed a locked file before then says "this file is encrypted" and
  does not try it.
- **Phishing:** the one-time prompt exists only inside the signed-in site. No email carries a link to it, and the
  prompt says so in words.

## The `AccountAuth` Durable Object

One per account id, SQLite-backed (migration `v6-account-auth`, after `v1` to `v5`). State:

| Field | Meaning |
|---|---|
| `public_key` | The only key that may sign in. Lower-case 64 hex. |
| `retired` | Keys that were replaced: key, date, how (`rotation` or `recovery`). Never reusable by any account. |
| `floor` | The session floor (seconds). |
| `value_frozen_until` | Before this time payouts, withdrawals, peer transfers and purchases of other users' listings are refused. |
| `payout_pinned_until` | Before this time the connected payout account cannot change without an admin. |
| `email_verified` | The verified address, if any, and when it became verified. |
| `dispute` | The open dispute, if any (below). |
| `attempts` | Recovery attempts: time, outcome code. Only those that passed the document and number gates count against the budget. |
| `challenges` | Open one-time challenges: digest, purpose, expiry. |
| `audit_head` | The hash of the newest audit record (the audit chain, below). |
| `mirror_pending` | The outbox of KV writes not yet confirmed. |
| `first_payout_hold` | Set by a recovery approval while the first-payout delay switch is on: the first payout after it waits 24 hours, then clears. |

Operations (each atomic): `begin_rotation`, `rotate`, `begin_recovery`, `record_attempt`,
`apply_recovery`, `open_dispute`, `resolve_dispute` (admin), `verify_email`, `state` (read), `bump_floor`,
`repair_mirror`.

**Mirrors.** After a change the Object writes the KV copies (`user:{id}.public_key` while it exists,
`pubkey:<new>`, a deleted `pubkey:<old>`, `retired-key:<old>`, `authfloor:{id}`) **through an outbox**: each write
is recorded in `mirror_pending` first and an alarm retries it until it is confirmed, so a failure part-way cannot
leave a new key unresolvable (a lock-out). An admin route repairs a stuck mirror.

**Availability.** Sign-in asks the Object and **fails closed** if it is unavailable. Existing accounts are
**seeded lazily** from KV on first touch (a test covers an account with no Object state yet).

**What asks the Object.** Ordinary routes keep the cheap KV floor check. **Deny by default:** every non-GET route
asks the Object (`requireFreshAuth`) unless a table marks it `cheap`, and a test fails if any mutating route is not
in the table, so a route added later cannot silently take the cheap path. Always fresh: the identity routes, the
admin routes, payouts and Connect changes, Delta and Bliss withdrawals, wallet transfers, publishing, hosting a
listing, API key and webhook create or revoke, account deletion, and anything that changes the account email.
Value routes also refuse while `value_frozen_until` is in the future.

**Known lag:** the new key's `pubkey:` index can take about a minute to appear at another location, so sign-in
with the new file on a second device can answer "not registered" for that long. The client message says "try
again in a minute". The dangerous direction (a retired key still resolving) is closed, because sign-in asks the
Object. The rotation reply carries a fresh token so the device that rotated carries on at once.

## Verified email

The owner's "no" must travel by a channel the thief does not hold. If the first person to sign in could claim the
address, the verified email would be the thief's. So the address is not whoever types first:

1. **An account with an email on file** gets its first verification link **only at that address**, never at a typed
   one. `POST /api/account/email/start` with no body sends the link there; the link opens a page on the site that
   posts the token (`.../confirm`), and the Object records `email_verified`. The link expires in 24 hours and works
   once, and is requested from the signed-in site only.
2. **Adding or changing the verified address** needs the **current key's signature** (the same two-step
   challenge as rotation), a notice to the old address with a **cancel link**, and a **7 day wait** before the
   change takes effect.
3. **A verified address counts as verified only once it is 30 days old.** A younger one is treated as unverified:
   it receives notices but is not accepted as a recovery step-up factor.
4. **An account with no email on file** may add a first address under rule 2's signature and wait, and that
   address is also "young" for its first 30 days.

The one-time prompt asks to verify the address on file (rule 1) before offering the key replacement; an account with
none is asked to add one (rule 4) and is told that recovery needs one.

**The notice rule, one rule for everything.** Every key notice (rotation, recovery attempted, completed, disputed, and
any email change) goes to **all** of: the verified address, any young or pending address, the address on file,
every live session (site, Studio, Player), and, for 30 days after any change, **the address that was replaced**.
Never "or": once a thief's address has become verified it must not displace the owner's as the recipient. The Object
stores when an address **became verified** (not when it was typed), which is what the 30 day rule counts. The site's
sign-in banner shows a pending email change, because an account with no email on file has no old address to warn.

## Registration changes

1. **Proof of possession:** registration signs a challenge with the key
   (`eustress-register-v1 / <origin> / <public key> / <username> / <challenge>`), so nobody registers a key they do
   not hold, and a retired key can never register.
2. **No client `id_hash`.** The Worker computes `v1:` + `HMAC(pepper, type : number-read-from-the-image :
   date-of-birth-read-from-the-image)` itself, rejects a client-supplied hash, and compares the number the user
   typed with the one read, for consistency. The version prefix lets `ID_HASH_PEPPER` be rotated; an **offline
   copy of the pepper is kept**, because losing it breaks every recovery. **The raw number never stays:** the model's
   analysis (`grok_analysis`) is stored whole in the verification record, so when the document number joins the
   schema it is stripped from the stored analysis and only the HMAC is kept; the number is never logged. This is what
   makes one ID, one account true. Expect support load: a passport number changes on renewal. Older accounts hold a
   hash of whatever the user typed; they cannot use document-number matching and take the support path.
3. **Small-order and identity keys** are refused at registration and rotation (all eight small-order encodings
   and the non-canonical ones, with vectors).
4. A recovery KYC session is refused at registration, and a registration session is refused at recovery.

## (b) Replace my key

Routes (bearer token required): `POST /api/identity/rotate/begin` returns `{challenge}` (one-time, 5 minutes,
filed by digest, bound to the account and the purpose); `POST /api/identity/rotate` with
`{new_public_key, challenge, old_signature, new_signature, revoke}`. The "old" signature may be made by the account's
current key.

Both signatures cover (fields are hex or a UUID only, so none can contain a newline):

```
eustress-key-rotation-v1
<origin>
<account id>
<old public key>
<new public key>
<challenge>
```

`<origin>` is the origin the request reached and must be on `LOGIN_ORIGINS`.

Checks, in the Object: the challenge exists, is unused, unexpired and was issued for this account for rotation;
the old signature verifies under the account's **current** key (a stolen token alone cannot take
the account); the new signature verifies under the new key; the new key is 64 hex, not a small-order or identity
point, and neither any account's current key nor in any retired list; no dispute is open. Then, in one step: the key
changes, the old key goes to `retired`, the floor moves past every token issued, the value freeze starts, the audit
entry is written and the notice is queued. The reply is `{token, user}` with the token stamped at the floor.

- **Freeze:** **7 days** when the account holds a Bliss or Delta balance, a payout connection or a published
  listing, **72 hours** otherwise. An open dispute extends it. The payout pin is 30 days.
- **Credentials:** the request lists the account's API keys, webhook endpoints and Discord link (the reply to
  `begin` carries the list) and `revoke` names the ones to revoke in the same step. The notice lists what is left.
- **Old copies:** the prompt and the rotation notice say that files already issued still hold the birthday and an
  unsalted ID hash and will keep holding them, and ask the owner to delete old copies, **including the emails that
  carried them**.
- **Existing accounts, the one-time prompt:** `GET /api/auth/me` gains `key_epoch` (0 until an account replaces its
  key) and `key_prompt_seen_at`. On the first sign-in after the deploy the site shows one modal, in this order: verify
  the account email; replace the key (plain download plus email by default; the lock once Studio can read it); and
  and say what happens if the key is lost (recovery by live capture, or support). Either choice, including "Later",
  sets `key_prompt_seen_at`. Settings keeps a permanent "Replace my key".
- **Site flow** (`settings.rs`, new `api/identity_keys.rs`): load the current file (to sign with the old key); the
  browser generates the new pair; begin, sign twice, rotate; build the new file in the new shape; download it;
  say plainly "your old file no longer signs in to Eustress; copy the new file to every computer you use".

## Ways to replace a key, in order of preference

1. **(b) With the current key:** instant, needs the old key.
2. **Recovery by live capture (c):** below.
3. **Support:** an admin-run identity check, for everything else (accounts without a retained original ID image, an account
   holding money that cannot meet its extra checks, a denied claim that is appealed, a tripped circuit breaker, the first
   weeks while the capture runs in shadow mode).

A recovery key and a face baseline were designed and are **not part of the default**; they are written up under "Options
kept for later".

## (c) Recovery of a lost key by live capture

For a user who has lost the private key and cannot sign with the old one. **In normal operation, approved or denied in
seconds, with no hold and no human in the approval.** (A circuit breaker, below, can switch every recovery to the human
path, and the first weeks run in shadow mode, below.)

### The anchor: the original registration portrait

The live face must be tied to **the victim's** face, and the only trustworthy anchor is the portrait on the **original
registration ID image**, the one that passed KYC when the account was made. The ID uploaded during a recovery is a *new*
image: an attacker with the victim's name, birth date and ID number (all in breach dumps) can forge a card with those
fields and the **attacker's own face**, type the number, and do a genuine live capture of their own face. Every document
gate would pass, and the face would match the forged card. So:

- The face from the capture that passed the live checks must match **the stored original portrait**, on that same capture.
- The newly uploaded ID is used only for the document fields and the number. **Its portrait is not an anchor.**
- **An account with no retained original ID image, or none with a usable portrait, cannot use the instant path.**

### The steps

1. **Start.** `POST /api/identity/recovery/start` with `{username, new_public_key}`, no token. The browser has generated the
   new pair. The answer is the same, **in the same time and with the same work** (an unknown account gets a dummy Object
   call), whether the account exists, has a verification or has an original image. The reply gives **a secret to that
   browser only**; the capture and every extra check work only in the browser session that called `start`. A phone handoff
   uses a **single-use token derived from that secret and shown only on that browser's screen**. A start takes no
   per-account slot; limits at start are per address.
2. **Consent and context.** Before anything is captured the site shows the consent screen: a written release naming the
   purposes (verifying identity, and **investigating fraud**), who receives the data (xAI for document reading; nobody else,
   because we run the capture ourselves), the retention schedule (14 months), no sale, and where the policy is published; its
   acceptance is stored with its time and the policy version. **Every screen of the capture, and the email code's mail,
   says plainly: "You are replacing the key of account X. Requested at TIME from COUNTRY on BROWSER. If you did not start
   this, stop."** This blunts the consent-phishing relay (an attacker starts a recovery with their own new key and gets the
   real owner to do the capture on the real site); it lowers that risk and does not remove it.
3. **Extra checks when the account holds money.** "Holds money" means any Bliss or Delta balance above zero, or a connected
   payout account. **Only** for such an account, three extra checks are required, and an account that cannot meet one cannot
   use the instant path (support instead; it is never waved through):
   - **A one-time code** emailed to a verified address at least 30 days old;
   - **the last four digits of the connected payout account**, when it has one;
   - **the capture made on a phone**, through the QR handoff from the desktop (this removes the desktop virtual-camera route).
   An account that holds no money takes the capture and the document check alone.
4. **Capture.** The user does the live capture with our capture page ("The capture we build", below) and uploads the ID. The
   capture session is created with the **recovery id as its reference, single use, 10 minutes**; the Worker binds the result
   to the recovery, stores its digest, and **the new key's final signature covers the capture session id and that digest**, so
   only the session that finished can complete. A capture service that is unreachable or returns something ambiguous means
   deny or support, never approve.
5. **Decision, in code, in seconds. Approve only if every gate passes:** the document fields (name, date of birth, sex when
   both have it, jurisdiction) equal the stored ones; the document number read from the image equals the number typed and its
   HMAC equals the stored `id_hash` (when the account has one); every live-capture check passed (the challenge responses, the
   flash-reflection check, the injection and fake detectors, below); **the face matches the original registration portrait**
   at its threshold; every extra check passed (when the account holds money); risk context is acceptable; the circuit breaker
   is closed; the new key is unused anywhere. **Anything else is a denial.**
6. **Denial and limits.** A denial is generic: one message and the same capture tips every time; the reason codes stay in the
   audit only. **Only an attempt that passed the document and number gates counts against an account's budget** (3 a day, 5 a
   month): the attacker in scope has the name, birth date and number, so they pass those gates and fail the face step on
   purpose, and a budget that counted all attempts would lock the real owner out. **Exhausting the budget alerts every address
   and routes the owner to the human appeal, never a silent denial.** Attempt notices are capped at one a day per address,
   batched. A denied person can **appeal to a human review**, a slow path separate from the instant decision (EU law gives
   people a right to human review of a fully automated decision, and a system that denies on low confidence will refuse some
   genuine people with old IDs and bad cameras).
7. **The circuit breaker and the kill switch.** An instant system lets a working attack scale at once, and attackers hold
   breach lists. Before launch: a global cap on instant approvals per hour and per day (a few, at this size); per-device,
   per-address and per-ASN caps; an automatic switch to hold mode when a pattern shows (several approvals from one device,
   address or ASN; an approval followed within minutes by value-route attempts; a rate above the cap); and a **kill switch
   only McKale can flip**, which sends every recovery to the human path within a minute. In hold mode a recovery becomes a
   support case. The counters and the switch live in **one singleton Durable Object**, not KV (KV reads lag about a minute
   and a read-then-write counter is not atomic). Accounts registered with forged IDs (there is no liveness at registration)
   could burn the cap; that fails safe, to the hold path.
8. **Approval takes effect at once.** The key swaps as in (b); the floor moves; **every API key and webhook endpoint is
   disabled (a flag, never deletion) and the Discord link is unlinked**, to be re-enabled one at a time by the owner; the
   notice rule's recipients are told, each with a **"This wasn't me" link** valid for 90 days; every live session shows a
   banner; the reply is `{token, user}`. **There is no value freeze, probation or payout lock after a recovery by default**
   (McKale's decision; they are written up under "Options kept for later"). That is the largest single reduction in what
   limits a fake that gets through, and "What McKale should be told" says so. **One configured switch, off by default:** the
   first payout after a recovery is delayed 24 hours (see "The first-payout delay switch"); recovery itself stays instant.

**"An address that existed before the recovery"** means the **registration address**, or an address that was **verified and
at least 30 days old when the recovery started**, never a young or pending one; the "This wasn't me" notices and the
one-time code use it.

**Risk context** (soft, from the audit, never a pass on its own): the account's age and whether it holds money, attempts in
the last 30 days, the device and country against the account's history, a recovery within seven days of a rotation (flagged).
Elevated risk adds the extra checks of step 3 to an account that holds none, or denies.

### The capture we build

McKale chose to build the capture ourselves. This section is the design; the spikes that prove each part are in "The spike
plan". **The `LivenessProvider` interface stays**, so the part below is one implementation (`OwnCapture`) and a certified
provider could be added later, as a replacement or as a second opinion.

**Principles.** (1) **The client is hostile.** All code in the browser is under the attacker's control, so every signal that
counts is derived from the *pixels* and from the *server's own clock*, never from anything the page reports. (2) **A live
stream is not what makes a capture live.** A virtual camera feeds a stream exactly as a webcam does, over WebRTC or anything
else. A capture counts as live only because the **server issues stimuli the client could not have predicted and measures the
face's physical response to them**. (3) **A model can lower trust but never raise it past the gates:** a fake detector's
score can deny; approval comes only from every deterministic gate. (4) **No browser can prove its camera is real.** There is no
device attestation for a web page; we cannot promise injection-attack resistance beyond what the challenge-response and
the pixel analysis measure, and we say so.

**The media path (what changed from the earlier WebRTC idea).** Cloudflare's Realtime SFU terminates WebRTC, but its
WebSocket adapter (beta) forwards audio and JPEG video at about **one frame a second** (higher only on an Enterprise
contract), which cannot measure a screen-flash reflection; its recorder analyses a session only afterwards; Workers AI has no
face matcher or liveness model. And the transport is not the protection. So the design does **not** use WebRTC for the
analysis. The capture page sends frames over **one authenticated secure WebSocket**:

- The browser gets the camera with `getUserMedia` (front camera, 640 by 480 or better, 15 frames a second), draws each frame
  to a canvas, encodes it as **JPEG at about 0.7 quality** (25 to 40 KB, so 0.4 to 0.6 MB a second, which a phone uplink
  carries), and sends it with a sequence number and the browser's frame time. Audio is not captured.
- A **Worker upgrades the socket and hands it to a Durable Object** (one per capture session, hibernation API), which holds
  the server clock, releases the challenge steps, timestamps every frame on arrival, and relays frames to the analysis
  service.
- The **analysis service is a container** (Cloudflare Containers, CPU only; generally available since April 2026, with inbound
  TCP since August 2026) running the face and fake models through ONNX Runtime. Frames are analysed **as they arrive**, so the
  decision follows the end of the capture by about a second or two, not by a batch job.
- **Hosting is Cloudflare only** (McKale, 2026-10-01): no fallback server that we run. If spike S0 shows Containers cannot
  meet the rate or the latency, that result **goes back to McKale as a decision**; nothing is switched on its own. **The owner's
  workstation is not used**: biometric data on a home machine is a legal risk and a single point of failure.
- WebRTC through the SFU with a post-session recording remains possible later for a different purpose (a recorded human
  review), not for the live analysis.

**Server-issued challenges.** One capture is a sequence the server builds at random for that session, releasing each step
only after the last response, inside a **15 second budget**, with every release and every arriving frame stamped by the
server's clock:

1. **Colour flashes (active illumination):** 10 to 12 full-screen flashes of random hue, 150 to 400 ms each, released one at a
   time over the socket (a client cannot prefetch the sequence). A real face under a bright screen takes on a measurable
   colour cast within a frame or two; the server correlates the face region's chroma change with the flash sequence at the
   expected delay. A replayed or pre-generated video has no matching response.
2. **Pose prompts:** 3 to 4 from a pool (turn left, turn right, look up, look down, open the mouth, blink twice, move closer),
   in random order and amount. The server measures the **latency** of the response (people take about 300 to 900 ms; a
   generation pipeline adds lag and jitter) and checks that the head's **3D structure is consistent** across the turn
   (landmark reprojection residual), which a flat replay and many 2D face-swap pipelines fail.
3. **Consistency of the face with its surroundings under the flash:** the response of the face region is compared with the
   response of the neck, ears, hair and background in the same frames. A face swapped onto a real head is a good place for the
   lighting to disagree. This is the signal most likely to catch a real-time swap and the one that needs the most
   validation (spike S2).

**Injection and virtual-camera detection.** Signals in two classes, treated differently:

- **Strong** (tied to secrets the client learned a moment earlier, measured in pixels and server time): the flash correlation,
  the pose-response latency and 3D consistency, the face-versus-surroundings lighting agreement, frame-timestamp continuity
  (injected video often shows irregular timing), and compression history (double compression and re-encoding artefacts; the
  frames are re-encoded by the page anyway, so this signal is weaker than it sounds and is validated in S2).
- **Soft** (reported by the page, so forgeable; they only raise risk and can only deny): the camera track's label and settings
  (for example "OBS Virtual Camera"), the device list, user agent, frame-rate stability, canvas and codec behaviour.

**What "strong" means.** Signals tied to secrets the client has just learned are strong against a **replay**. They are **not**
strong against a hostile client that has been told the stimulus: a real-time pipeline can add a **relighting layer synced to the
flash command**, and a human's reaction time plus about 100 ms of pipeline lag sits inside the 300 to 900 ms we treat as human.
**This repository is source-available, so the detector will be public**; the design assumes an attacker who has read it, and the
red team is run that way.

**The face match.** The container (a) takes the original registration ID image from the KYC bucket and finds the portrait
(card and portrait detection; the larger, sharper face if the card carries a ghost image; a minimum size and detection
confidence, otherwise no instant path); (b) from the capture, picks the best frontal, sharp frames (pose, blur and exposure
scores) from the steps where the challenges passed; (c) computes face embeddings for the portrait and for those frames with a
commercially usable model (candidates: YuNet for detection and SFace for recognition from the OpenCV model zoo, or another
model whose **weights' licence is verified for commercial use**; **not** InsightFace's published weights, which are for
non-commercial research); (d) compares the portrait's embedding with the aggregate of the live frames' embeddings by cosine
similarity against a threshold. **No face template is stored by us**: the portrait's embedding is computed at recovery time
and discarded with the frames. An ID portrait is small, old and often glared, so the false-reject rate is expected to be
material (the appeal path exists for this); the threshold is set on data (spike S1), not by guess.

**The fake detector, and how it is trained, evaluated and audited.** This is the "AI that spots fakes". It has three layers:

1. **Deterministic active-challenge analysis** (above): the strong signals, computed by signal processing, not learned.
   They are auditable and need little data.
2. **A learned presentation-and-injection classifier** on features: the active-challenge features plus texture and frequency
   cues (moiré and screen-pixel energy, colour gamut, specular behaviour) and temporal features, in a small, interpretable
   model (gradient-boosted trees to start; a small convolutional network on face crops only if the data supports it).
3. **A vision-language "spotter"** (Grok) reading a few frames for visual artefacts, with its reasoning logged. It is a
   **soft signal that can only deny or raise risk**, never approve (its outputs are not calibrated and drift between model
   versions, and an image can carry prompt injection, so its text is treated as data).

*Data.* We have no data and 16 users, so the data is **collected by us from consenting people** (staff and testers, with a
consent form, a retention limit and a deletion path, the same standard as the product): at least 50 people and 200 genuine
sessions across phones, lighting and ages, and **attack sessions we make ourselves**: a printed photo, a photo and a video on
a phone and on a monitor, a video through a virtual camera (OBS), a **real-time face swap through a virtual camera** (open
tools such as Deep-Live-Cam or DeepFaceLive, built from a teammate's public video), camera injection on an Android emulator,
and paper or silicone masks if we can. Public research datasets are used only where the licence allows commercial use and the
provenance is clean; otherwise they are used for evaluation ideas, not for training. Every dataset has a manifest (hash,
source, consent, licence) kept with the model.

*Evaluation.* The standard metrics: **APCER** (the share of attacks accepted, per attack type, never averaged away) and
**BPCER** (the share of genuine captures rejected), in the style of ISO/IEC 30107-3, with **95% confidence intervals** (the
exact binomial). Train, validation and test splits are **by person and by device**, so the model is never tested on someone it
trained on. The threshold is chosen on validation to hit a target BPCER (about 5 to 10%, because the appeal path exists) and
the test set reports the APCER that results. **The statistics limit what can be claimed:** with zero accepted attacks in 20
tries the 95% upper bound on the true rate is still about 14%; in 100 tries about 3%; in 300 tries about 1%. A pass bar is
therefore written as "at most k accepted in n tries", with n set in advance, never as "none got through".

*Audit.* Every model has a **model card and a registry entry**: weights hash, training-data manifest hash, evaluation
report hash, thresholds, the date and who approved it. **Every decision's audit record stores the model hashes and the
thresholds it used** (the hash-chained audit above). Any model or threshold change re-runs the whole red-team regression suite
and needs Security's sign-off. After launch, genuine reject rate, appeal outcomes and any dispute that proves a fake are fed
back as evaluation data (with consent), and drift is watched.

**Shadow mode, in counts.** A real comparison with human decisions cannot reach a bar with 16 users, and a human is **not ground
truth for a deepfake** (people cannot reliably tell one). So before any instant approval, **every real recovery goes to the human
path while the capture runs and records its verdict**, and the evidence of readiness is counted, not sampled: **staged recoveries
by staff on test accounts, with written minimums** (for example, at least 50 genuine staged recoveries across phones, lighting and
ID ages, with a genuine-reject rate within the bar), and **attack ground truth from the red team** (every attack there is known).
Instant approval then switches on under the circuit breaker's caps. Shadow mode is a launch gate, not a product rule.

### The scripted client, and the container as the root of trust

**A scripted client defeats the session binding and the phone-only rule.** A client with no camera and no browser (rendering frames
server-side from one portrait photo) is not stopped by a secret returned to a browser or by a phone handoff; the user agent is soft.
We say so. The future strong path for accounts that hold money is **capture in a native app with platform attestation** (Play
Integrity on Android, App Attest on iOS); we have no mobile app, so this is later work.

**The analysis container is the root of trust and it parses attacker-supplied images.** Whoever controls it can approve anything.
So: it has **no secrets and no R2, KV or Stripe access**; its **egress is only to the capture-session Durable Object**; it runs
with minimal privileges; its image and image decoders are **pinned and patched**; its **approval rate is monitored against the
expected rate** and an unusual one trips the circuit breaker; and there is a **global concurrency cap and a spend alarm**, so an
attacker cannot use it as a free compute or a denial of service.

### Revoking a fraudulent recovery

The real holder may have lost their key (which makes a fake's claim plausible) or may still hold it.

1. **The one-click link.** In every notice, valid 90 days. It needs **no sign-in** and does one thing: pauses **value
   movement and the re-enabling of keys and endpoints** (a pause destroys nothing, so a click by anyone does no harm beyond a
   pause). It opens a dispute. A link-only pause does not suspend sessions or the key unless it is corroborated (by a
   retired-key signature or a live capture that passes below).
2. **The retired key.** A key retired by a recovery or a rotation can sign a dispute over a server nonce for **30 days**. That
   pauses and escalates; it **never restores the account by itself**, because a thief holding a stolen identity file looks
   exactly the same as the real holder holding the old one.
3. **A pause from a retired key or a link lapses by itself after 48 hours** unless the disputer passes a **live capture matched
   to the stored original portrait**, which a thief cannot. **At most one dispute per recovery per claimant.**
4. **The frozen state** (a corroborated dispute): the new key, API keys and sessions are suspended; payouts, withdrawals and
   Bliss transfers stop, and Stripe Connect payouts are paused through Stripe's API; listings are paused; **both claimants** get
   a read-only dispute page where they submit evidence.
5. **Human adjudication,** for two claimants who both pass or both fail. Each does a fresh live capture and a **video call
   showing the physical ID**. The admin compares both captures against the stored original portrait, the name on the Stripe
   payout account, device and address history, and the account's activity; the reasoning is written into the audit. **With one
   admin there is no separation of duties; the policy says so.**
6. **Outcome.** The proven holder gets a fresh key (as in (b)); the other capture is kept under a **legal hold** as evidence.
   Evidence is disclosed only on a subpoena, a warrant, or consent.

### The audit and the evidence

An **append-only, hash-chained audit**: each record holds the inputs by reference, the model hashes, the prompt hashes, the
scores, the thresholds, the outcome and the previous record's hash; the Object keeps the head. Every capture has its SHA-256 in
the audit, and every read of a capture is itself logged. A legal-hold flag suspends deletion for that case.

**The evidence store is the highest-value target in the system.** It holds ID images, live footage and numbers for every
recovery, and the legal exposure scales with it (Illinois damages are per person). With no provider holding the captures, **we
hold them**, so:

- **A separate Cloudflare account holds a tiny evidence-vault Worker with its own R2 bucket.** The vault has a **put endpoint**
  the capture service calls with a service secret, and a **read endpoint behind McKale's hardware-key step-up** (the
  adjudication path). R2 API tokens, as far as we know, have no write-only class, so "a token the recovery Worker can write to
  but not read" is not something to rely on; the vault Worker makes it true by construction. **The master key lives outside the
  Worker**, and the read log is shipped to an append-only sink that the Worker does not control.
- What is kept as evidence: the best few face stills and a short clip of the capture, the decision record and the hashes; not
  every frame.
- The container processes frames in memory and writes nothing to disk; it holds no keys to the vault's read side.
- The **original registration ID images**, which anchor the face match, get the same protection and are kept as long as the
  account lives and for a stated period after closure (counsel names it).
- The policy says plainly that **with one admin there is no separation of duties.**

### Retention (a proposal for counsel; each class configurable)

Each class has its own deletion prefix, and counsel can change a number without a rebuild.

| Data | Kept |
|---|---|
| The original registration ID images (the KYC record and the face anchor) | While the account lives, and for the period counsel names after closure. McKale: keep on file. |
| Recovery photos and video (approved or denied) | **14 months** (McKale's decision, 2026-10-01), then deleted; the hashes and scores stay. **Longer only under a legal hold for a dispute**, and in any case no longer than **3 years after that person's last interaction** unless there is legal process. Counsel confirms. |
| Training and test data from consenting volunteers | Per the consent form, with a stated end date and a deletion path |
| Face templates | **None held by us**: the container computes embeddings in memory and discards them |

**For counsel:** 14 months is inside Illinois' outer limit (3 years after the last interaction) but longer than the 90 to
120 days an outside review proposed, and Texas requires destruction no later than one year after the purpose for collecting
expires, so the **purpose must be written to include fraud investigation across the whole retention period**, which the
consent screen and the published policy must state. Counsel decides whether that wording holds.

**Words we do not use:** "deepfake-proof", "impossible to impersonate", "100% accurate", "instant" as a promise, "certified"
(we are not), "we never store your face". We say "designed to detect" and "reduces the risk".

### Law, flagged and not blocking

Counsel reviews this before launch; the consent screen and the public retention schedule must exist before the first real
capture. The statutes an outside review checked:

- **Illinois BIPA (740 ILCS 14/15):** a public written policy with a retention schedule; destruction when the initial purpose is
  satisfied or **within 3 years of the person's last interaction, whichever comes first** (only a warrant or subpoena
  overrides); informed written release before capture naming the purpose and the period; no sale or profit; disclosure only
  with consent or legal process; since the 2024 amendment an electronic signature counts as written release and damages count
  once per person. Face geometry taken from photos has been held covered, so plan as if the face match is.
- **Texas (Business and Commerce Code 503.001):** notice and consent before capture; no sale; reasonable care; destroy within a
  reasonable time and no later than **one year after the purpose expires**; the Attorney General can seek up to $25,000 per
  violation.
- **GDPR Article 9:** explicit consent, a data protection impact assessment, purpose limitation, erasure rights, and Article 22
  safeguards (the human-review appeal above).
- Also for counsel: Washington, Colorado, California sensitive data; **the licence and the training-data provenance of every
  face model we use** (the models may have been trained on faces collected without consent); whether sending ID images to xAI is
  a disclosure that needs consent and a processing agreement; the volunteers' consent and the data collected for training and
  testing; the impostor's footage is biometric data too, so the policy and the consent screen name fraud investigation as a
  purpose.

### What McKale should be told plainly

- **Building the capture is a research-and-engineering project of months, not weeks, and the result cannot be certified.** An
  independent standard (CEN/TS 18099 for injection attacks, ISO/IEC 30107-3 for presentation attacks) is what a certified
  provider is tested against; we will report our own numbers on our own tests, with wide confidence intervals, and that is a
  weaker claim.
- **A skilled attacker with the victim's ID images and public video can still pass.** A real-time face swap with the attacker
  in front of the screen is the attack that no homegrown check reliably stops. The extra checks (an emailed code, the last four
  digits of the payout bank, a phone-only capture) apply only to accounts that hold money.
- **Without a post-recovery value freeze, a fake that gets through can move money at once.** The freeze, the probation and the
  bank lock were designed and McKale chose not to include them; Security recommended them. The notices and the one-click pause
  are what remain, and they depend on the real holder seeing an email in time. Under "Options kept for later" the freeze can be
  switched on without redesign.
- **The first-payout delay covers the USD payout only unless McKale widens it:** a thief can still move Bliss or Delta balance to
  another account, withdraw inside Eustress or buy another user's listing at once.
- **A consent-phishing relay is a residual risk**: an owner who can be talked into doing the capture for an attacker's key is
  helped only by the on-screen warning.
- Genuine users will sometimes be denied, and the appeal to a human is how they get through.
- The first weeks run in **shadow mode**: humans decide while the machine's verdict is recorded, so the first real recoveries
  are not instant.

## Options kept for later

Designed, reviewed by Security, and **not** in the default. Each can be switched on without redesigning the rest.

1. **A recovery key.** A second Ed25519 key the holder keeps offline (printed), enrolled while they hold the first, which signs
   the rotation message in place of the old key: a lost key is replaced instantly with no biometrics. Enrolling one is itself
   sensitive: it waits **7 days**, sends a notice with a cancel link to every address in the notice rule, and has a **30 day
   youth period** (not accepted on an account that holds money inside it); a rotation lists the enrolled recovery keys with a
   revoke choice; **every recovery clears every recovery key**.
2. **An opt-in face baseline.** A live capture enrolled while the holder still holds their key, with its own consent, accepted
   only if it matches the original portrait, and used as a second reference in the face match; cleared by every recovery.
3. **A freeze after recovery.** Value frozen until the owner confirms from a pre-recovery address or 30 days pass (not a flat 7
   days), a **probation** on the irreversible acts (publishing, hosting a listing, transfers, withdrawals, payout changes,
   re-enabling keys and endpoints) for the same period (not on signing in, playing or reading), and the **payout bank pinned 30
   days**. The Object already has `value_frozen_until`, `payout_pinned_until` and `probation_until`; the route checks that
   honour the first two are built for (b) and for disputes, so switching this on is a policy change in the approval step.

## Abuse cases and what stops each

| Attack | Stopped by |
|---|---|
| **A forged ID with the attacker's own face and the victim's name, birth date and number** (no deepfake) | The face must match the **original registration portrait**, not the new upload; no usable original means no instant path |
| A leaked ID and a **real-time deepfake of the victim** from public video | The flash and face-versus-surroundings analysis, the pose-response timing and 3D check, the learned detector, the circuit breaker; for accounts that hold money the emailed code, the payout last-four and the phone-only capture. **Not reliably stopped** (a homegrown check cannot promise it), and with no post-recovery freeze the damage is not limited |
| **A replay** (print, photo, screen video, virtual-camera video) | Random flash and pose challenges the recording cannot answer; the flash correlation; timing |
| **A working attack run at scale** | The circuit breaker: global, per-device, per-address and per-ASN caps; an automatic switch to hold mode; McKale's kill switch (all in one singleton Durable Object) |
| **A consent-phishing relay** (the attacker starts, the real owner does the capture) | Capture and checks only in the starting browser session; the single-use phone token shown only on that screen; "you are replacing the key of account X, requested at TIME from COUNTRY" on every screen and in the code email. **Residual** |
| **Lockout by exhausting the attempt budget** | Only attempts that passed the document and number gates count; exhaustion alerts every address and routes to the human appeal; attempt notices capped at one a day per address |
| **Pause griefing** by a thief with a retired key, or anyone with a forwarded notice | A link-only pause touches value and re-enabling, not sessions, unless corroborated; a pause from a retired key or a link lapses after 48 hours unless the disputer passes a live capture matched to the original portrait; one dispute per recovery per claimant |
| **Evidence-store breach** | A separate Cloudflare account and vault Worker, reads behind McKale's step-up, the key outside the Worker, the read log in a sink the Worker cannot change, deletion by schedule; the container keeps nothing |
| A model "fooled" by an instruction in an image | The fake detector's spotter is soft and can only deny; its text is escaped and never decides or emails; approval only from the deterministic gates |
| A biased or drifting face model | Thresholds set on our own data, genuine reject rate recorded by group where the data allows, the appeal to a human, a model registry with hashes in every audit record, regression suite on every change |
| A leaked identity file also leaking the ID number | The file no longer carries birthday, type or hash; the stored value is an HMAC with a server secret |
| Replacing a leaked file with another emailed plaintext file | Not stopped by default (McKale chose plain download plus email as the default): the optional passphrase lock |
| A thief holding the old key rotates first | The owner uses the live capture against the original portrait; the (b) rotation freeze; the notices |
| A fake recovery succeeds | The "This wasn't me" link and the retired-key dispute; human adjudication with fresh captures and the physical ID; the legal-hold evidence. **No value freeze by default** (an option) |
| Credentials outliving the session (API keys, webhooks, Discord) | Listed and revocable at rotation; disabled at recovery; owner re-enables one at a time |
| The notice going to an address the owner does not hold | One notice rule: all addresses, all live sessions, and the replaced address for 30 days |
| Phishing through the one-time prompt | Exists only inside the signed-in site; no email carries a link to it; the wording says so |
| Replaying or crossing recovery sessions, or completing with another capture | Server-made, bound to one recovery and one new key; the capture session carries the recovery id, is single use for 10 minutes, and the new key's final signature covers its id and result digest |
| Account enumeration through recovery | One answer, in one time, with the same work, for every case; limits per address |
| Squatting an account's recovery | A start takes no slot |
| Racing two rotations or a rotation and a recovery | One Durable Object per account; sign-in requires the current key |
| A stolen token alone rotating the key | Rotation needs a signature by the current key |
| A retired key coming back | The retired list is permanent and is checked at registration, rotation and sign-in; the Object is authoritative |
| A failed mirror write locking the owner out | The outbox retries until confirmed; an admin repair route |
| A stale write of the user record restoring an old key | Every auth path reads the Object; `public_key` leaves the user record after migration |
| Registering someone else's public key (squatting) | Registration proves possession of the key |
| Insider or adjudicator abuse | Every read logged to a sink the Worker cannot change, a hash-chained audit, a policy that states the one-admin limit |
| A route added later that skips the fresh check | Deny by default and the route-classification test |

## The spike plan

Each part is proved by its own spike, with a pass or fail written before it starts. A spike produces numbers and a short
report; nothing here is built for production until its spike passes. The estimate is **about 8 to 12 weeks of work to a
shadow-mode launch**, most of it S3.

| # | Proves | Pass means |
|---|---|---|
| **S0** Capture transport | Browser to Worker to Durable Object to container over one secure WebSocket; mobile Safari and Chrome | 15 frames a second at 640 by 480 sustained on a mid-range phone on 4G; frame loss under 2%; server-stamped latency under 300 ms median; the container receives and analyses a frame within 100 ms; the container's cold start measured and under 10 s; cost per capture measured. If Containers fail, S0 stops and the result goes back to McKale as a decision (hosting is Cloudflare only). |
| **S1** Face match | The detector, the recognizer and the card-portrait extraction on CPU | The models' weights **and their training-data terms** are read for commercial use (not assumed); inference under 50 ms a frame; on at least 50 consenting people with their real ID against a live capture, a false-accept and false-reject curve is measured **per demographic group where the data allows** and a threshold with BPCER under 10% found; the portrait is found on at least 90% of our retained originals (the anchor inventory) |
| **S2** Active challenges | Flash correlation, pose-response timing, 3D consistency, face-versus-surroundings lighting | Against genuine captures: separation from screen replays, a virtual-camera replay and an injected-camera replay (zero accepted in at least 100 tries each). **Three adaptive attacks, built by someone who has read this design, are tried and their results reported with intervals, whatever they are:** (1) a real-time swap **with relighting** synced to the flash; (2) a **fully scripted client** with no camera and no browser, which renders frames server-side from one portrait photo with a reenactment model and answers each prompt (**the one that scales**); (3) a **uniform tint with falloff** against the face-versus-surroundings check |
| **S3** Fake detector | The learned classifier on the S2 features plus texture, and the vision-language spotter | Data collected as above; APCER per attack type and BPCER with intervals on a held-out set split by person and device, **plus a leave-one-attack-type-out run and a held-out tool** (a generator the training never saw); the model card, registry entry and audit record exist; the result is compared with S2 alone, and the classifier ships only if it improves it. **The spotter sends live face frames to xAI, so it ships only if S3 shows it helps at a fixed BPCER, and only with the consent screen and counsel's answer on that disclosure** |
| **S4** Decision, audit and breaker | The policy, the hash-chained audit, the singleton breaker and kill switch, with a scripted evaluator | Every gate failing alone denies; identical denials; the breaker trips at its caps and on a pattern; a flipped kill switch takes effect within a minute; the audit chain verifies and detects tampering |
| **S5** Red team and staged recoveries | Everything together, against a pass bar McKale sets in advance | The bar below is met against **adaptive** attackers (the three named in S2); the staged-recovery counts below are met |

**The pass bar, for McKale to set before S5.** Set it from **tolerable damage, not rate alone.** Takeovers a day equal the attempts
the circuit breaker allows times the adaptive attacker's pass rate (and attempts are **not independent**: a working attack is
repeated). So a 95% upper bound of 10% on an account that holds money means up to one attempt in ten could pass. Recommended:
**at most 0 accepted in at least 100 tries each** for print, photo, screen replay and virtual-camera replay; for **each adaptive
attack** the measured rate and its 95% upper bound are written down; **for accounts that hold money, instant approval is
switched on only if the 95% upper bound against the adaptive attacks is at or below 1%** (about 300 tries with none accepted,
which is cheap when the attacker is scripted), **otherwise those accounts stay on the human path**; genuine reject rate at most 10%.
**Launch order:** instant recovery for accounts that hold **no money** first; accounts that hold money follow only when the money
bar is met.

## The first-payout delay switch

McKale's decision of 2026-10-01: **a 24 hour delay on only the first payout after a recovery** is switched on **if the red team
shows face swaps beating the check.** Recovery itself stays instant, and no other payout is delayed.

- It is a **configured switch** (`FIRST_PAYOUT_DELAY_HOURS`, 0 when off, 24 when on) read by the payout run, not a code change.
- A recovery approval sets `first_payout_hold` on the account when the switch is on; the **first payout after it** (the next
  scheduled one or a requested one) is held for the delay, the owner is told on every channel in the notice rule with the
  "This wasn't me" link, and a corroborated dispute cancels the payout; after that payout the flag clears.
- **Tied to the red-team result:** the spike plan's pass bar has two numbers for the real-time-swap result. At or below the
  first, the switch stays off. Above it, the switch is **on** before launch for accounts that hold money. McKale may also set a
  higher second number above which those accounts use the human path instead (recommended first number 1%, second 10%, both
  on the 95% upper confidence bound).
- The scheduled payout run and the internal Wallet, Treasury and CommerceHub paths honour it, and a test proves it.
- **Scope, stated plainly:** "payout" here means the **USD payout**. A 24 hour delay on it does **not** delay Bliss or Delta transfers
  to another account, withdrawals inside Eustress, or the purchase of another user's listing, so a thief can still move balance at
  once. If that is the intent, so be it; if not, **apply the same one-time delay to the first outbound value move of any kind after a
  recovery** (it costs a genuine owner one day, once). McKale decides (decision 7).
- **A payout bank changed after the recovery** is named in the owner's notice and **restarts the delay clock** (a thief's first move
  is to change the bank through the hosted link and wait 24 hours). This depends on the answer to the Stripe question about
  changing the bank account.

## What changes where

**Worker** (`infrastructure/cloudflare/api`): new `src/account_auth.mjs` (the account Object, the route handlers, the outbox,
the audit chain, the dispute lapse); new `src/liveness.mjs` (the `LivenessProvider` interface, the decision policy, the
circuit breaker's Object, the kill switch); new `src/capture.mjs` (the capture-session Durable Object that holds the server
clock, releases the challenges and relays frames); `wrangler.toml` (bindings `ACCOUNT_AUTH`, `CAPTURE_SESSION`, `BREAKER`;
migration `v6-account-auth`; the container binding; the vault's service secret; the kill-switch variable; production and
staging; the vault itself is a separate account); `index.js` (routes; sign-in asks the Object; `requireFreshAuth` and its route
table; `endSessions` goes through the Object; `validIdentityFile` for the new shape and the envelope; registration proof of
possession and the server-computed `id_hash`; the document number in the KYC schema; the email verification routes; the
retention deletion job in `scheduled()`; `me` gains `key_epoch`); `commerce.mjs` (list, disable and re-enable keys and
endpoints; the freeze check on value routes for (b) and disputes); a notices module.

**The analysis container** (new, `infrastructure/cloudflare/capture-analysis`): the face and fake models through ONNX Runtime,
the flash and pose analysis, the portrait extraction; its image, its model registry and its evaluation harness.

**The evidence vault** (new, in the second account): the put and read endpoints, the log shipping.

**Pepper:** a new secret `ID_HASH_PEPPER`.

**Website** (`eustress/crates/web`): `pages/settings.rs` (replace my key), a recovery wizard from `pages/login.rs` ("Lost your
key?"), the consent screen, the **capture page** (camera, canvas, JPEG frames over the socket, the flash overlay, the prompts
and the context line), the phone handoff, the dispute page, the new identity-file shape, the registration signature,
`api/identity_keys.rs`, message builders in `api/auth.rs`, the one-time prompt, an admin entry for adjudication and the kill
switch in `pages/admin.rs`.

**Studio and Player** (Multiplayer, Client): after `key_retired`, Studio stops trying that file on every start, marks that
identity retired, keeps the file, and says "this identity file was replaced on DATE; use your new file or recover your
account"; `session_revoked` means sign in again with no retry loop; `account_frozen` and `recovery_completed` are banners with
the link to the dispute page. The Player holds no key, so the floor and the codes are enough. Studio passphrase support is
Phase 1b.

**Open, outside the Worker:** the engine's Bliss node and the identity crate derive a wallet address from the public key.
Rotation changes the key, so confirm nothing outside the Worker keys a balance or an address on it.

**Shared vectors:** `docs/architecture/key-rotation-vectors.json` for the rotation and registration messages.

## Tests

`tests/account_auth.test.mjs`, the account Object under the Worker harness. Rotation succeeds with both signatures; each of
these fails: the old signature missing, wrong, or for another account, origin or purpose; the new signature missing or for
another key; a reused, expired or other-purpose challenge; a retired key; another account's current key; every small-order and
non-canonical encoding; two rotations in parallel (exactly one wins and the loser's key never signs in); the old key is refused
at sign-in at once; tokens end and a sign-in right after is not caught by the floor. The (b) freeze is 7 days with value and
72 hours without.

`tests/liveness.test.mjs`, the policy and the interface with a scripted capture service. **Every gate failing alone denies**
(document field, document number, each live-capture check, the face match against the original portrait, each extra check for
an account that holds money, a used or foreign key, elevated risk, an open circuit breaker). **A forged ID with the attacker's
face and the victim's data is denied** (live face matches the new card and not the original portrait). **An account with no
usable original portrait never takes the instant path.** An account that holds money cannot take the instant path without the
three extra checks, and an account that holds none is not asked for them. A missing, unparseable or unreachable capture result
denies. The final signature must cover the capture session id and the result digest. The capture and every extra check fail
from any browser session but the one that called `start`; a phone token works once and only from that session. The response
bodies and timing of every denial are identical; an unknown account does the same work and gives the same answer. **Only attempts
that passed the document and number gates count against the budget**; exhausting it alerts every address and routes to the
appeal; attempt notices are capped at one a day per address. The circuit breaker: the global, per-device, per-address and
per-ASN caps, the automatic switch to hold mode on a pattern, and the kill switch (a recovery during hold mode becomes a
support case), all in the singleton Object. An approval swaps the key, moves the floor, disables credentials and notifies every
recipient, and **starts no freeze, probation or pin**. A recovery session is refused at registration and the reverse; no capture
is accepted before consent is recorded.

**The first-payout delay:** with the switch off a recovery sets no hold; with it on, the first payout after a recovery waits
the configured hours on every payout path, the next one does not, a corroborated dispute cancels the held payout, and a recovery
by an account with no value sets nothing that matters.

`tests/capture.test.mjs`, the capture-session Object: the server releases each step only after the last response; every frame is
stamped by the server's clock; a client that sends frames faster than allowed, out of order, or after the budget is cut off; a
step released early or twice is impossible; the session ends at the budget.

The analysis container's own tests (Python or Rust, run offline): the flash correlation on recorded genuine, replay and swap
clips; the pose and 3D checks; the portrait extraction; the evaluation harness reproduces every number in a model card from the
manifest.

Disputes: the one-click link pauses value and re-enabling without a sign-in and not sessions unless corroborated; the retired
key's signature pauses and escalates and never restores; **a pause from either lapses after 48 hours unless the disputer passes a
live capture matched to the original portrait; one dispute per recovery per claimant**; the frozen state suspends the new key,
sessions, API keys and Connect payouts; resolution needs an admin; a legal hold stops deletion. The audit chain: each record's
previous hash matches, a tampered record is detected, every capture read is logged, the model hashes and thresholds are in every
decision record. Retention: the deletion job removes by prefix at each class's schedule (14 months for recovery captures) and
skips a held prefix.

Elsewhere: the outbox retries a failed mirror write and the account never locks out; the notice rule's recipient set contains
the verified, the young or pending, the on-file and the replaced address and the live sessions; the email rules (the first link
only to the address on file, a change needs the current key's signature and waits 7 days with a cancel link, a 29 day old
address is unverified and a 30 day old one verified); the scheduled payout run and the internal Wallet, Treasury and
CommerceHub paths skip a frozen account; the stored analysis never holds the raw document number and nothing logs it; the hash is
versioned; lazy seeding; fail-closed sign-in when the Object is unavailable; the route-classification test; the identity-file
validator accepts the old and new shapes and the envelope and refuses extra keys. Mutation checks as for every earlier security
set.

## Ordering

1. **Phase 1 (staging first), now, independent of recovery:** the account Object with outbox and lazy seeding, rotation (b),
   verified email, the one-time prompt (verify the email, replace the key), the credentials list and revoke, sign-in checks,
   notices, the distinct client errors, deny-by-default fresh auth, the hash-chained audit. Worker and site only.
2. **Phase 1b:** Studio passphrase support (Multiplayer); the optional lock for sign-up and rotation.
3. **Phase 2a:** the dispute machinery (the one-click pause, the retired-key dispute, the lapse, the frozen state,
   adjudication, the legal hold), the circuit breaker and kill switch, registration proof of possession and the server-computed
   `id_hash`, the evidence vault. Until live-capture recovery exists, a lost key is **support only**.
4. **Before Phase 2a, inventory the anchor:** how many existing accounts have a **retained, readable original portrait** a face
   matcher can use, and confirm that **no lifecycle rule in the KYC bucket deletes objects**. **Checked 2026-10-01:** neither the
   production bucket (`eustress-kyc-documents`) nor the staging bucket has any deletion rule (only the default rule that aborts
   incomplete multipart uploads after 7 days, listed through the API so a dashboard rule would show); staging holds 7 KYC records
   for 1 account. **Still to do:** the production count, which reads production KYC records and waits for McKale's go. It is
   written so its output is **only counts and opaque ids** (per account, keyed by a hash of the account id: whether
   `kyc-{id}-front` exists, whether it carries an explicit document approval, whether its image object exists with a plausible
   size and image type, and a pass or fail from a local face detector); it never prints a name, birth date, number, sex or image,
   copies no image anywhere, runs on McKale's machine or in a throwaway Worker, deletes its output once the count is recorded,
   and is proved on staging first. **The script and its staging output go to Security before the production run.** Later, use the same capture at registration, so a new anchor is a verified live face matched to
   the ID.
5. **Phase 2b: the spikes S0 to S4** (the spike plan), then **Phase 2c: S5, the red team and shadow mode**, then **Phase 2d:
   instant recovery**, after counsel's review of the consent and the retention schedule and McKale's go.
6. **Later:** a certified provider as a second opinion or a replacement; the options above.

## Gates before instant recovery launches in production

1. The spikes S0 to S4 pass their written criteria.
2. **The red team** (including the forged card with the attacker's face, the relay, and the attack at scale) meets the **pass bar
   McKale sets in advance**, with the genuine-user reject rate recorded.
3. **Shadow mode, in counts:** the staged-recovery minimums are met and the red team's attack ground truth is within the bar (a human's view of a real recovery is not ground truth for a deepfake).
4. **The lawyer review, the consent screen and the published retention schedule exist before the first real capture, which is the first volunteer capture in S1 and S3** (not only a member's recovery).
5. **The anchor inventory** is done and the accounts without a usable anchor are known (they take the support path).
6. **The circuit breaker and the kill switch** are in place and tested.

## Decisions for McKale

Decided 2026-10-01: the file carries only the account, the keys and `verified`; the passphrase lock is optional with plain
download plus email as the default; recovery is a live selfie check with an instant approve or deny, the footage kept, the real
holder able to revoke; **the capture is built by us (not bought); the extra safeguards are only the three extra checks for
accounts that hold money (the recovery key, the face baseline and the post-recovery freeze are out of the default); recovery
photos and video are kept 14 months, longer only under a legal hold; the appeal to a human, the email rules, the circuit
breaker with his kill switch, the lawyer review and Security's launch gates stay. Also decided, later the same day: the capture
transport is a secure WebSocket (not WebRTC); shadow mode comes first; hosting is Cloudflare only.**

Still open:

1. **The pass bar: decided.** For accounts that hold money, instant approval only if the 95% upper bound against the adaptive attacks is
   at or below **1%**, otherwise those accounts go to a person; 0 in 100 for print, screen replay and virtual-camera replay; genuine
   reject at most 10%; instant recovery for accounts with no money launches first (McKale chose Security's bar, 2026-10-01).
2. **Shadow mode first: decided yes.** Real recoveries go to humans while the machine's verdict is recorded; readiness is measured in counts (staged staff recoveries on test accounts with written minimums, and the red team's known attacks), because a human cannot tell a deepfake.
3. **Where the analysis runs: decided, Cloudflare only.** **Counsel and the consent form come before spike S1** (McKale, 2026-10-01). If a spike shows Containers cannot do it, that comes back to you as a decision.
4. **Time and cost:** about 8 to 12 weeks to a shadow-mode launch, mostly collecting data and building the fake detector;
   cheaper and faster to launch is the certified-provider route, which you declined; the interface keeps it open.
5. **Without a post-recovery freeze,** a fake that gets through can move money at once (Security recommended the freeze). Confirm
   you accept that, or choose to switch on option 3 for accounts that hold money only.
6. **Counsel** on the consent, the 14 month schedule, the volunteers' data, the face models' licences and the xAI disclosure:
   flagged, not blocking Phase 1 or 2a.

7. **The scope of the first-payout delay:** the USD payout only (as written), or the first outbound value move of any kind after a
   recovery (recommended, it costs a genuine owner one day, once). Either way, a payout bank changed after the recovery restarts the
   clock.

8. **Delta spending is not paused by a key replacement: decided 2026-10-06 (McKale: "Audit trail Yes.").** `POST /api/delta/charge`
   stays a fresh route (the session must be current) and is not a value route: it spends the same account's balance on Eustress's
   own tools and moves nothing to anyone else, and the audit trail can revert the damage if a breach ever happens. The code already
   works this way (`route_policy.mjs`, pinned in `tests/route_policy.test.mjs`) and nothing on the site says otherwise. This
   answers Delta spending only: items 5 and 7 above are still for Security to put to McKale.

## Security's statement of residual risk, 2026-10-01

Recorded verbatim, as Security gave it to the orchestrator on 2026-10-01 and to this session for the record. Not edited.

> Statement for the record, plain words (Security, 2026-10-01). Basis: instant recovery, a live check and face match built by us, no post-recovery money freeze or bank lock, no recovery key, no face baseline, extra checks only for accounts holding money, footage kept 14 months plus legal hold. I read the designs and the code; nothing is built or tested.
>
> 1. What this stops: an attacker who has only breach data (name, birth date, ID number). They also need a live face that matches the face on the account's original registration ID.
> 2. What it does not stop: an attacker who also has the victim's ID images and public video can feed a real-time face-swapped stream into the check through a virtual camera or a hooked browser camera. A check we build ourselves cannot reliably detect that; Website's own design document says so. We have no independent certification, so we do not know the real pass rate until the red team measures it, and with 16 users we cannot measure how often genuine owners are wrongly refused.
> 3. What a successful attacker gets: the account at once, with the owner's identity, listings and reputation, and the ability to publish and host as them, until the owner notices.
> 4. For an account holding money: also the money. With no freeze, nothing stops a withdrawal, transfer or payout change in the minutes after the takeover except the extra checks: an email code (only from an address verified for 30 days or more, so none exists for the first month), the payout account's last four digits, and a phone-only capture. For such an account the whole defence is "beat the live check AND hold the owner's mailbox or know their bank digits". Stolen mailboxes are common, so one mailbox plus one face swap is enough.
> 5. After the fact: notices and the "this wasn't me" link can stop further harm and expose the case; without a freeze they cannot bring back what already left. The owner may read the email after the money is gone.
> 6. Who bears the loss: the user for balances inside Eustress. For USD payouts, Eustress creates Stripe Connect accounts as `type: custom` (index.js, handleStripeConnectOnboard); as I understand Stripe's rules the platform is responsible for negative balances and fraud losses on Custom accounts. I have not verified that, nor how strictly Stripe matches a bank account's holder to the verified person (a partial backstop). Ask Stripe and counsel.
> 7. Because we build it, we, not a provider, hold and run the face recognition. Biometric-law exposure (written consent, a public retention schedule, per-person statutory damages in some states) sits wholly with us. The machine that decides is the root of trust for every recovery: whoever controls it can approve anything.
