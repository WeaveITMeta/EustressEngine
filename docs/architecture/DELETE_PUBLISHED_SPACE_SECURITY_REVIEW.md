# Delete Published Space: Security Review

Independent adversarial review of the server code for deleting a published Space, written 2026-11-12 on `staging/integration`
because the Security session was not running. Read-only on the live tree: nothing under `src/` or `tests/` was changed.
Paths are relative to `infrastructure/cloudflare/api/` unless they start with `eustress/`. Line numbers are as read on
2026-11-12; `src/index.js` and `src/moderation.mjs` are edited by other sessions, so search by the quoted text.

Code reviewed: `src/space_delete.mjs`, `src/space_delete_ops.mjs`, `src/space_delete_mark.mjs`, and the hooks in
`src/index.js` (write gate, dispatch, hourly sweep), `src/moderation.mjs` (`simWithLegalLock`, `legalLockedIds`,
`withLegalLockFrom`), `src/account_auth.mjs`, `src/email_ops.mjs`, `src/commerce.mjs`, `src/case_history.mjs`,
`src/route_policy.mjs`, `src/takedown.mjs`, `src/evidence_vault.mjs`, `src/purchases.mjs`, `src/live.mjs`, `wrangler.toml`.
Design and the builder's checklist: `docs/architecture/DELETE_PUBLISHED_SPACE.md` section 10.

## Verdict

**No blocker and no high finding.** The code path that a person can trigger (request, emailed code, confirm, soft delete,
restore) holds up: authorization, the code, the address, the rate limits, the session binding and the serving overlay all
behave as section 10 says, and the probes below could not break them.

**May `DELETE_CODE_EMAIL` be switched on once one code mail has been read in a real inbox? Yes, on one condition:** the hourly
removal job must not be able to run its destructive steps until M1 to M4 below are fixed. Every medium finding is in the
hourly job or in what happens after it, and the job first removes anything 90 days after the first deletion. The smallest way
to make that condition hold is a second setting that the job reads before anything else (for example
`SPACE_HARD_DELETE = "on"`, off in both environments), so switching on the code mail never arms the removal by itself. M2 must
also be fixed before anyone relies on switching `DELETE_CODE_EMAIL` back off. M5 is a decision for McKale and counsel.

## Evidence (what was run)

Scratch copy: `C:\Users\miksu\AppData\Local\Temp\claude\f4e4f9229cad81e7\4f7eb5ac-1847-4466-9ea5-53fc20c31261\scratchpad\space_delete_probes`

- `api\` holds `src\` byte-identical to the live tree (SHA-256 compared for `space_delete.mjs`, `space_delete_ops.mjs`,
  `space_delete_mark.mjs`, `index.js`, `moderation.mjs`, `purchases.mjs`), the builder's `tests\space_delete.test.mjs`, and the
  probes in `tests\zz_security_probes.test.mjs`. `node_modules` is a junction to the live folder's.
- `api_fixed\` is the same with the candidate fixes applied; `fixes.diff` is the diff between the two `src` folders.
- Control: the builder's 35 tests pass on the unchanged copy.
- Unchanged code: `node --test tests/zz_security_probes.test.mjs tests/space_delete.test.mjs` gives 45 tests, 35 pass,
  **10 fail: every probe fails**, each for the reason its message names (read back, not inferred: for example M1 reports
  `{"due":23,"deferred":{"never_delete":23},"budget_spent":true}` three runs in a row, M4 reports the location result
  `not_found`, L2 reports `402 Insufficient balance` against `404 Simulation not found`).
- With the fixes: 45 tests, **43 pass**; the builder's 35 still pass, and 8 probes pass. M4 and M5 still fail because no fix
  was applied for them (M4 is a takedown-side change, M5 is a decision).
- No deploy, no production call, no email, no cargo.

## Findings

| ID | Severity | One line |
|----|----------|----------|
| M1 | Medium | Listings held for good sit at the head of the due index and spend the whole run budget every hour; about 22 of them stop every later removal, silently |
| M2 | Medium | With `DELETE_CODE_EMAIL` off nobody can restore, yet the job still removes for good on schedule |
| M3 | Medium | Evidence kept in place under a TAKE IT DOWN reference (`never_delete`, `copied: false`) is not a hold; only the moderation case's reference is read |
| M4 | Medium | After the removal, a TAKE IT DOWN naming the listing finds nothing and blocks none of its files; the tombstone hashes are written and never read |
| M5 | Medium (decision) | A retention-only flag left by a released hold makes a normally served Space undeletable, and the status read tells the author something is held |
| L1 | Low | The job can run 10 minutes on one listing under a 6-minute lease it never extends, and keeps deleting after another run has taken the lease |
| L2 | Low | `POST /api/ledger/spend` tells a soft-deleted listing from a missing one, and stamps the deleted listing's name on a receipt |
| L3 | Low | The create-if-absent conflict branch deletes the winning deletion's due key when both fall in the same hour |
| L4 | Low | The free-text listing name goes verbatim into the Security Notice that carries the code, and into the deleted and restored notices |
| L5 | Low | The job's counts (deferred, failed, budget spent) are discarded; nothing reaches the night record or staff |
| L6 | Low (verify) | The tombstone's per-file SHA-256 is unproven: it is read from `list()` checksums, which the test fake never returns and R2 may not either |

### M1. Held listings starve every later removal (Medium)

Where: `src/space_delete.mjs:871-882` (due keys gathered and sorted oldest first), `:884` (stop when the budget is spent),
`:896` (a deferred listing keeps its due key where it was), `:928-937` (holds re-read under the lease), `:76` (400 calls a run).

A deferred listing is checked again every hour at its original due hour, so it stays at the head of the oldest-first list
forever. Each one costs about 18 metered calls (mark read, lease, mark read again, `holdsOf`, money), so about 22 permanently
held listings (a released possible-CSAM case with `never_delete`, a staff hold nobody releases, a report debt that never
clears) spend the whole 400-call budget before any newer due listing is reached. From then on, no deletion anyone asks for is
ever completed: the bytes stay, the Projects card reads "Removal is in progress" forever, and because `scheduled()` discards
the job's return value (L5), nobody is told.

Scenario: 30 listings deleted by their authors carry `legal_release.never_delete`; a 31st, ordinary deletion is five days past
due. Three hourly runs each defer 23 held listings, spend the budget, and never reach the ordinary one.

Probe: `PROBE M1` fails (`state soft`; every run `budget_spent: true`).

Smallest fix (in `fixes.diff`): when a listing is deferred, re-file its due key at the next hour (`dueKey(now + HOUR, id)`) and
delete the old one, so every older due listing goes first next run. This keeps the hourly re-check the builder's tests expect
(all 35 still pass). Re-filing at a day instead also works but changes the builder's legal-lock test, which resumes an hour
later.

### M2. Removal continues while restore is impossible (Medium)

Where: `src/space_delete.mjs:550` (every owner route, restore included, answers 503 unless the setting is exactly `"on"`),
`:904-946` (the job never reads the setting).

Turning the setting off is the documented response to a mail problem (design section 10, Open). While it is off, an owner whose
account was used to delete their Space (a stolen session plus access to their mailbox, the threat the 90 days exist for) cannot
restore it, and the job removes it for good on schedule anyway. The same gap opens whenever the owner cannot receive a code:
their address was changed and is not yet 30 days verified, or kycGate now refuses the account (D10).

Probe: `PROBE M2` fails (the package is removed at day 91 after a refused restore at day 80).

Smallest fix (in `fixes.diff`): in `hardDeleteOne`, before a removal starts (`mark.state === 'soft'`), add the reason
`restore_unavailable` when `!deleteCodeEmailOn(env)`; it defers like any hold and staff see the reason on the mark. Consider the
same for an owner with no aged address, or extend `hard_delete_after` by the time restore was unavailable.

### M3. Evidence under a takedown's reference is not a hold (Medium)

Where: `src/space_delete.mjs:250-266` reads sealed evidence indexes only under `caseReferenceOf(env, 'moderation', id)` (`:252`).
A possible-CSAM TAKE IT DOWN seals the listing's files under the TAKEDOWN's own reference (`src/takedown.mjs:495-500`,
`caseKind: 'takedown'`), and the vault writes `copied: false` for an object over 512 MB and `never_delete: true` for category
`csam` (`src/evidence_vault.mjs:209-216`). For such an item the listing's own bytes are the evidence.

Today the moderation case usually carries `never_delete` too (`src/takedown.mjs:755`, copied into `legal_release` on an
invalid finding at `:783`), which hides the gap. It is not durable: a later, unrelated quarantine writes a fresh `legal_hold`
without `never_delete` (`src/moderation.mjs:7233`) and its release overwrites `legal_release` (`:7280`). After that, and once
`preserve_until` has passed and the takedown is `invalid` or `closed`, nothing in `holdsOf` sees the vault's `never_delete`
item, and the job deletes the bytes it points at. This contradicts McKale's default "never delete evidence".

Probe: `PROBE M3` fails (a closed takedown with an in-place `never_delete` index under its reference; the package is removed).

Smallest fix (in `fixes.diff`): in `holdsOf`, also collect `caseReferenceOf(env, 'takedown', tdId)` for every
`tdlisting:{id}:{td}` key, and scan each reference's `evidence/{ref}/*.json` the same way as the moderation one. Any other
case kind that seals a listing's files (none found besides moderation and takedown) belongs in the same list.

### M4. The tombstone does not let a later takedown name the files (Medium)

Where: `src/space_delete.mjs:963-979` and `:1039-1040` write the tombstone and the per-page file records; the comment at
`:32-33` says "a later takedown can still name the files". Nothing reads them: TAKE IT DOWN intake resolves a location through
`readSim` (`src/takedown.mjs:381-385`) and records a removed listing as `not_found` ("no listing at this location", `:1660-1664`);
the `hashes` step (`:479-486`) also reads only `sim:{id}`. Decision D6 kept the hashes so that "a later TAKE IT DOWN request can
still name the deleted listing and block re-uploads of the same files".

Scenario: an author deletes a Space holding someone's intimate images before anyone reports it; 91 days later the person files
a TAKE IT DOWN with the old link. It is `not_found`, no hash goes on the blocklist, and an identical re-upload is accepted.

Probe: `PROBE M4` fails (the tombstone has `pak_sha256`; after the request, `blockedHash` finds nothing).

Smallest fix: in takedown intake, when `sim:{id}` is missing and `simgone:{id}` exists, record the target as removed (no lock
or notice to write) and run the hashes step from the tombstone: `pak_sha256`, `world_manifest_sha256`, and every `sha256` in
`tombstones/{id}/files-*.json`. Not applied in `api_fixed`; it is a change in `src/takedown.mjs`, owned by the takedown work.

### M5. Retention-only flags refuse the deletion and tell the author (Medium, decision)

Where: `holdsOf` (`src/space_delete.mjs:226-229`, `:234-237`) counts `preserve_until` and `never_delete` on a RELEASED hold, a
report debt, a stuck report and a pending seal; `startRefusal` (`:423`) and the status read (`:746`) answer `in_review`.

Once a person releases a hold, the listing serves and takes every write again, so nothing tells its author a hold was ever
placed. The deletion status read (`GET /api/account/spaces/{id}/deletion`, no rate limit) then says "This listing is being
reviewed and cannot be changed right now" for a listing that is plainly public and editable. That contradiction tells the
author a legal retention flag exists, and polling it shows when it clears (for example when a report debt to an authority is
paid). For a possible-CSAM case that is a tip-off the takedown flow otherwise avoids (`authorVisible`, `src/takedown.mjs:386`).

Probe: `PROBE M5` fails (the listing serves to a stranger and archives with 200; the status read answers `in_review`).

Fix (decision for McKale and counsel): the design's own section 2.3 already had the answer: for the retention-only reasons
(`preserve_until`, `never_delete`, `report_owed`, `report_stuck`, `vault_pending`, `evidence_in_place`) allow the soft delete,
which only hides, and defer the removal; keep refusing the start for the reasons that already block writes (legal lock,
quarantine, case hold, open review, open takedown, workspace, staff hold). This changes "refuse under any hold" to "refuse or
defer", which McKale's wording allows but did not choose explicitly.

### L1. The lease can run out under the job (Low)

Where: `src/space_delete.mjs:77` (`SWEEP_WALL_MS` 10 minutes), `:914` (the lease is taken once per listing and never
extended), `src/moderation.mjs:353` (`lease_seconds: 360`).

One listing can occupy the whole run. If R2 is slow, the lease expires after 6 minutes, a review run or a takedown's case step
takes it over (`acquireLease` takes an expired lease, `src/moderation.mjs:5221-5223`), and the job keeps deleting pages and
rewriting the mark without noticing. The legal lock is still read before every delete call, so evidence under a new lock is
safe; what is lost is the mutual exclusion the comments rely on (the case stamp at `:984-988` is written with a plain put, not
`saveCase`).

Probe: `PROBE L1` fails (the clock is moved 7 minutes during the first page delete; another holder takes the lease; the cover
is still deleted afterwards).

Smallest fix (in `fixes.diff`): stop the listing, saving its place, once `lease_seconds` minus a minute has passed since the
lease was taken (a `leaseUntil` on the meter, checked by `spent`).

### L2. The ledger spend route is an existence oracle for deleted listings (Low)

Where: `src/purchases.mjs:156-157` reads `sim:{id}` raw in `resolveAttribution`, called by `POST /api/ledger/spend`
(`src/index.js:6978`). A signed-in account with no balance gets 402 for a soft-deleted listing and 404 for a missing one; with a
balance, the deleted listing's name is stamped on its receipt. It works the same for any private or unapproved listing today,
so it is not new in kind, but a deleted listing should read as missing everywhere outside its author's view. Bliss is burned
here, not paid to the creator, so no money reaches a deleted Space.

Probe: `PROBE L2` fails (402 against 404). Smallest fix (in `fixes.diff`): treat a listing with a deletion mark as not found
(`deletionMarkHead`), or read it through `simWithLegalLock` and require `isListable` unless the caller is the author.

### L3. The conflict branch can drop the winner's due key (Low)

Where: `src/space_delete.mjs:482-488`. When the create-if-absent mark write loses to another request's mark, the branch
deletes `dueKey(hardAfter, id)`. Both due keys are named by the hour of their `hard_delete_after`, so when the two deletions
fall in the same hour the key deleted is the winner's, and the job never removes that listing. Reaching the branch needs two
confirmed requests racing past the lease, which the lease and the one-entry-per-listing object make very unlikely; the effect
is a listing kept forever, not a listing lost.

Probe: `PROBE L3` fails. Smallest fix (in `fixes.diff`): delete our due key only when it differs from the winner's.

### L4. Author-chosen text in the code mail (Low)

Where: `src/space_delete.mjs:325` and `:333` (the code mail), `:350-365` (the deleted and restored notices). HTML is escaped
(`src/email_shell.mjs:65-68`), but the text part carries the name verbatim and mail clients turn a bare address into a link.
Someone holding a stolen session can rename a listing (it goes back to review first) and then ask for codes, so the owner's
genuine, signed "Security Notice" carries a lure such as "Account locked visit https://...". At most five a day.

Probe: `PROBE L4` fails. Smallest fix (in `fixes.diff`): leave the name out of the code mail ("a Space on your Eustress
account"); the page that asked shows which Space. For the notices, keep the name but cut it to a short length and break any
`://` so no client links it.

### L5. The job's outcome is invisible (Low)

Where: `src/index.js:700` discards the return of `sweepSpaceDeletions`; the night record (`:709-720`) has no field for it,
though design section 4.8 says its counts are added there. A deferred, failing or starved job (M1) is seen by nobody.

Probe: `PROBE L5` fails. Smallest fix (in `fixes.diff`): keep the result and write it as `space_deletions` in
`cronrun:{day}`; alert staff when `budget_spent` is true several runs in a row or a listing has been deferred past a set age.

### L6. The tombstone's SHA-256 per file is unproven (Low, verify on staging)

Where: `src/space_delete.mjs:1039` takes `o.checksums?.sha256` from `SCENES.list()`. The builder's test counts the file records
(`tests/space_delete.test.mjs:756-758`) but never checks that any has a SHA-256, and its fake `list()` returns no checksums, so
in the test every one is null. Whether R2's `list()` returns `checksums.sha256` for objects stored with one is not something
the code or the docs read here establish. Check on staging. If it does not, take the hashes from what is already known for
free: `pak_sha256`, `spaces[*].sha256`, the hash in each `manifests/{sha256}.json` and `chunks/{hash}.echk` key, and a `head()`
for the few images.

## Hardening notes (safe today, one layer deep)

No failing probe is possible for these because another check stops the attack; each is one refactor away from a hole.

- **Live admit trusts `approved` alone** (`src/live.mjs:1074-1080`): the overlay sets `is_public: false` and leaves
  `moderation.status` as `approved`, so this check alone would admit joiners to a deleted listing. It is safe only because
  `spaceDeleteWriteGate` refuses every POST under the listing first. Add `|| served.deletion` to the refusal.
- **Website manifest serves when `sim:` is missing** (`src/index.js:6357-6360`, `!listing` gives `withheld: false`). Safe
  because the job deletes the manifest object (r2 phase) before `sim:` (kv phase). Withhold when the listing is missing.
- **A product can be put back on sale in a deleted listing** (`POST /api/commerce/products/{id}`,
  `src/commerce.mjs:1921-1938`, no `commerceBlockedReason`). No sale can happen (purchase creation refuses a deletion), and the
  job takes products off sale. Refuse the update for a listing with a deletion mark.
- **Lease probe on every request** (`leaseFree`, `src/space_delete.mjs:309-314`) takes and gives back the moderation lease on
  each delete or restore request, limited only by `AUTH_RATE_LIMITER`, since the object's 3-an-hour limit is counted after it.
  A held or reviewed listing is refused by `holdsOf` before the lease, so no review can be starved this way.
- **Dedicated-server credentials** (`esrv_`) keep reading the world for the 90 days (author-equivalent); only revoked at the
  removal (`:1013-1016`). The author can revoke one at any time (the gate lets `DELETE server-credential` through).

## What is safe, with the evidence

Authorization
- Owner only, one 404 for missing and not-yours: `ownedListing` (`src/space_delete.mjs:178-181`), checked before any other
  read (`:553-554`); the builder's test compares the two bodies byte for byte. The status read gives a removed listing's name
  only to the author named in the mark (`:719-725`). No co-author concept exists (searched the Worker for co-author,
  collaborator and editors fields: none).
- Sessions: only a JWT bearer authenticates (`src/index.js:2956-2971`); `verifyFreshAuth` checks the account object's floor
  (`src/fresh_auth.mjs:62-74`), and the routes are `fresh` in `src/route_policy.mjs:131-132`. No API key, server credential or
  MCP path reaches them: Studio's `http_request` tool attaches no session
  (`eustress/crates/tools/src/simulation_tools.rs:514-550`) and the MCP server's only token is the moderator's admin JWT
  (`eustress/crates/mcp-server/src/shared_registry.rs:318-335`). The only client code naming these routes is the website page
  `eustress/crates/web/src/pages/project_delete.rs`.
- Staff routes: under `/api/admin/` (`isAdminOnlyPath`, `src/index.js:3579`), `requireAdmin` refuses a passkey session
  (`:3567-3575`), a 20-character rationale, audited (`src/space_delete.mjs:804-818`).
- kycGate runs before dispatch (`src/index.js:772-773`), so an unverified account is refused (D10 as the code has it).
- The value freeze after a key replacement refuses delete-request and delete-confirm (`src/space_delete.mjs:562-565`).

The code
- Uniform six digits by rejection sampling (`:124-130`); request ids are 24 base62 characters, about 143 bits (`:134-140`).
- Stored only as HMAC-SHA-256 over account, listing, request and code (`:157-161`), under a key derived with its own label.
  The label is distinct from every other use of `JWT_SECRET`: JWT signatures (input always contains a `.`), the evidence vault
  (`evidence-vault-v1`), adult image URLs (`adult-image-url-v1`), forum evidence (`forum-evidence-v1`), identity tickets
  (`eustress-identity-ticket-v1`), guest join (`eustress-guest-join-v1`), unsubscribe tokens (input `unsub:{id}`) and address
  prints (input `eustress-address:{address}`). No route signs a caller-chosen whole string with the raw secret, so the derived
  key cannot be obtained, and without it a leaked digest cannot be searched offline.
- Brute force: the object runs one operation at a time; the attempt is saved before any comparison
  (`src/space_delete_ops.mjs:155-156`), the fifth wrong answer locks (`:159-163`), and parallel confirms cannot exceed five
  (builder's test). Replacing the request resets the count, but requests are limited to 3 per listing an hour and 5 per
  account a day (`:39-40`, `:111-122`), delete and restore together: at most 25 guesses a day against a million codes, each
  request mailing the owner a code. Over one 72-hour session that is a 1-in-13,000 chance; staying in for a year, under 1%.
- Constant-time comparison of two 64-hex digests (`:50-55`); a timing leak would reveal digest bytes, not the code.
- Replay: a confirmed request answers the same request, session and code as a replay that re-runs only idempotent steps and
  writes no second audit or notice (`:145-150`; `src/space_delete.mjs:623-631`, `:496-504`). A wrong code against a confirmed
  request is not counted, but it can only learn a code already spent.
- Cross-session and cross-account: the session digest (`src/space_delete.mjs:164-167`, compared at
  `src/space_delete_ops.mjs:144`) binds the confirm to the bearer token that asked; pending entries live in each account's own
  object and the digest names the account.
- Expiry: 15 minutes, enforced when the state is read (`:62`) and again after the attempt is counted (`:167-172`).
- Never logged, audited, answered or put in a subject (builder's test reads every log line, audit record and response).

Email
- The code goes only to the verified address once it is 30 days verified, decided in the object (`src/space_delete_ops.mjs:105-110`;
  `src/email_ops.mjs:88-108`). Changing that address needs the identity key's signature, waits 7 days with a cancel link sent
  to every address, starts unverified, and is usable 30 days after verification (`src/email_ops.mjs:227-325`). A pending,
  replaced or unverified address never receives a code (builder's test).
- Volume: at most 5 code mails a day per account, to its own address; each counts against the account's own `notice` bucket of
  30 (`src/mail_limits.mjs:41`), so they cannot crowd out a key-change notice.
- Switch off: every owner route answers 503 before any listing is read (`src/space_delete.mjs:550`); see M2 for the job.

Races
- Confirm against a new TAKE IT DOWN or lock: the legal lock is read again under the lease (`:639`). A lock landing after that
  read can coincide with a soft delete, which only hides; the removal re-reads every hold on each run and the legal lock
  before every delete call (`:928-937`, `:1035-1038`).
- Restore against the start of removal: both take the moderation lease and re-read the mark under it (`:681-689`, `:914-925`);
  restore is closed from the due time (`:651-657`), and the job only starts at or after it (`:913`).
- Two confirms: one holds the lease; the other is refused, or finds the mark and replays.
- KV whole-record writers: the soft delete writes nothing to `sim:{id}`; the R2 mark is the truth (builder's test rewrites the
  record with a fresh approval and the listing stays hidden). The mark is made create-if-absent (`:480`).
- Cron overlap: the run is capped at 10 minutes against an hourly schedule; see L1 for the lease inside one run.

Removal
- Only `universes/{id}/`, `thumbnails/{id}/` and `covers/{id}/`, with `{id}` a validated UUID (`:79`, `:92`), minus
  `universes/{id}/moderation/` (`:93`, `:1031`). `evidence/`, `evidence-views/`, `derived/`, `caserefs/`, `locks/legal/`,
  `locks/state/`, cases, references, legal reports, money and audit are outside every prefix (builder's byte-for-byte test).
- No key is shared between listings: ids are server-generated (`src/index.js:5361`), every content key is built from the
  listing's own id (`src/index.js:5495`, `src/world.mjs:77-78`, `src/changelogs.mjs:52`), and chunks are stored per listing
  (`src/world.mjs:23-24`). `derived/` (shared by content) is never touched.
- Resumption: the phase, prefix and cursor are saved after every page; the builder's test resumes from a failure, and the
  cursor is a key position, so deleted keys before it cannot shift the next page.
- The website namespace stays claimed by its owner; another author is refused by the existing owner check
  (`src/index.js:6557-6568`).

Money
- Refused while any product of the listing is active or any live sale is inside its 72-hour window (`moneyBlockOf`,
  `src/space_delete.mjs:274-306`), checked at request and again at confirm. The hub lists sales newest first
  (`salet:` keys by inverted time, `src/commerce.mjs:1273-1284`), filters by listing and mode (`:1414-1419`), and stores
  `created` in seconds, so the early stop at the first closed window is correct. A product put back on sale between request
  and confirm stops the confirm (builder's test). A sale cannot happen once the mark exists (`src/commerce.mjs:335`).
- Refunds run from the creator's hub without reading the listing, and no deletion can start inside a window, so no refund
  window is open at deletion. A buyer's pass or unspent item in a removed Space points at nothing: option A as decided.

Serving
- Every route that serves listing data, content or images to someone other than the author reads the listing through
  `simWithLegalLock`, `simForServing` or `withLegalLockFrom` before it decides, and then `isListable` or `canServe`
  (`src/moderation.mjs:6210-6224`). This sweep was delegated to a separate reader and spot-checked here on the download
  (`src/index.js:5960-6021`), the website manifest (`:6342-6379`), live admit (`src/live.mjs:1074-1080`) and the commerce
  product routes (`src/commerce.mjs:1882-1939`). Covered: gallery, search, tags, featured, makers, the live strip (account and
  guest), the listing page, play, download, world manifest, source and chunks, thumbnail, cover, previews, change logs (author
  only), website manifest by id and namespace, guest ticket and play, commerce catalog and purchase, projects, case history.
  No route serves an R2 object by a request-derived key without first loading the listing. The website crate renders no
  listing on the server: `sitemap.xml` lists static routes only and `/simulation/:id` is the client shell.
- What stays visible for a while, already stated in the modal copy: other isolates' 10-second lock list, the live strip's
  10-second body cache, the makers and tags `max-age=300`, thumbnails `max-age=3600`, covers and previews `max-age=300`,
  chunks `private, max-age=86400`, and the website manifest `max-age=300, stale-while-revalidate=86400` in any cache that
  honours it.

## The cost of the extra R2 reads

At R2's published prices as last known (Class B, a head or get, $0.36 a million; Class A, a list, $4.50 a million; not
re-checked today):

- Single-listing serving routes: one more head per request, in parallel with the legal-lock head. A million listing views
  cost about $0.36 more, and add no latency beyond the slower of two parallel heads.
- Mutating requests under `/api/simulations/{uuid}`: one head each, through the write gate.
- List routes: `legalLockedIds` now lists `locks/deleted/` as well as `locks/legal/`, once per isolate per 10 seconds while the
  gallery is busy: one more Class A call per refresh for each 1,000 marks. Marks are never deleted (a `hard_deleted` mark stays
  forever), so this grows without bound. With 20 busy isolates that is about 5.2 million list calls a month, roughly $23 a
  month for every 1,000 marks. Worth measuring on staging (section 10, Open). If it matters, move a mark to a second prefix
  once its removal is done and have the single-listing paths head both (a cheap Class B read) while the list reads only the
  live one.

## Open items

- Fix M1, M2, M3 and M4 before the job can first remove anything, or add the separate job switch described in the verdict.
- Decide M5 (McKale and counsel).
- Read one code mail in a real inbox (section 10, Open), and while there, check L6 on staging.
- An administrator's restore is still not built (section 10, Open). With M2 it is the only way back for an owner who has lost
  the mailbox the code goes to.
- D10 and D11 are not decided; nothing here changes them.

## How to run the probes

```
cd C:\Users\miksu\AppData\Local\Temp\claude\f4e4f9229cad81e7\4f7eb5ac-1847-4466-9ea5-53fc20c31261\scratchpad\space_delete_probes\api
node --test tests/zz_security_probes.test.mjs tests/space_delete.test.mjs
cd ..\api_fixed
node --test tests/zz_security_probes.test.mjs tests/space_delete.test.mjs
```

The first run is the code as reviewed (10 probes fail, 35 builder tests pass); the second has `fixes.diff` applied (M4 and M5
fail, everything else passes). The probe file copies its helpers from `tests/space_delete.test.mjs` so it runs on its own.
