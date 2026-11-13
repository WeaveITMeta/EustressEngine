# Delete Published Space

Status: design, not built. Read-only review of the tree as of 2026-11-12 (branch `staging/integration`). Nothing here is deployed.
Source proposal: section 6 of `Eustress_Technical_Document.md` (NOVA folder). This document replaces its open items with what the
code says.

Line numbers are as read on 2026-11-12. `infrastructure/cloudflare/api/src/index.js` is changing in the shared tree, so search by
the function name given beside each number. Paths below are relative to `infrastructure/cloudflare/api/` unless they start with
`eustress/`.

## 0. Summary

What "a published Space" is in the code: a **listing**, the KV record `SOCIAL:sim:{id}` made by `handlePublishSimulation`
(src/index.js:5308). One listing is one published Universe; it holds one or more Spaces (`sim.space_names`, `sim.spaces`). This
feature deletes the whole listing. Removing one Space from a multi-Space listing is a republish without that Space, not this
feature.

The design in one paragraph: the owner types the listing's name, proves the request with a second factor held by the account's
Durable Object (`AccountAuth`), and the listing is **soft deleted**: hidden from everyone at once through a strongly consistent R2
mark that every serving path reads, its bytes untouched, restorable for 30 days. An hourly job then **hard deletes** it in bounded,
resumable batches, but only after re-checking every legal hold, takedown, evidence seal, report debt and open review; any one of
them defers the hard delete indefinitely, silently to the author. Money records, case records, evidence and audit chains are never
deleted by this feature.

Four findings change the proposal:

1. `fresh_auth.mjs` does not check how recently a session signed in. `freshGate` (src/fresh_auth.mjs:30) asks the account's
   object whether the session is still above the floor; tokens last 72 hours (src/index.js:2877 `createJwt`). "Fresh-auth" as a
   recent-sign-in step-up does not exist. The step-up has to come from the second factor itself.
2. A KV deletion marker alone is not safe. At least six writers rewrite `sim:{id}` whole (moderation `applyToSim`
   src/moderation.mjs:6222, `markSimQuietly` 8337, the upload routes, world commit src/world.mjs:525, tags, cover, archive). A write
   that read the record before the deletion would put it back. The truth must be an R2 object, the pattern the legal lock already
   uses (src/moderation.mjs:5273-5301).
3. The email binding can mail arbitrary addresses as far as config and public DNS show (section 5), but delivery to McKale on
   2026-10-21 failed for a reason nobody has found. Recommended factor: the identity key's signature, or a passkey assertion for a
   passkey session. Email carries the notice, not the gate.
4. Archive does not stop the website manifest today: `handleGetWebsiteManifest` (src/index.js:6302) withholds only on moderation
   status, and serves when the listing record is missing (`listing` null, line 6318-6319). Deletion must close both.

## 1. Inventory: every place a published listing lives

Legend for the two right columns. **Soft**: what the soft delete does. **Hard**: what the hard delete does. "Keep" means never
touched by this feature. "Overlay" means the bytes stay and every reader sees the listing as deleted through the R2 mark (section
4.6).

### 1.1 KV `SOCIAL`

| # | Key | Written at | Soft | Hard |
|---|-----|-----------|------|------|
| 1 | `sim:{id}` the listing record | src/index.js:5380 `handlePublishSimulation`; rewritten by world.mjs:468/525, moderation.mjs:6233/8337, index.js upload routes 5461-5711, cover.mjs, tags | Set `deletion: {state:'soft', at, hard_after}`, `deleted_was_public`, `is_public:false` (the `handleArchiveSimulation` pattern, index.js:5393). Archive fields left as they are | Delete, after the tombstone (row 27) is written and after every R2 batch |
| 2 | `sim-author:{author}:{id}` | index.js:5381 | Keep | Delete (it fills the 100-key read window of Projects index.js:5727 and Cases case_history.mjs:202) |
| 3 | `simCount:{author}` | index.js:5384-5385 | Keep | Decrement, floor 0 |
| 4 | `plays:{id}` | index.js:3368 `handlePlay` | Keep | Delete |
| 5 | `playseen:{user}:{id}:{day}` | index.js:3363, TTL 2 days | Keep | Leave to TTL |
| 6 | `totalPlays:{author}` | index.js:3371, visits.mjs:105 | Keep | Keep (history of the author) |
| 7 | `visits:{id}` | visits.mjs:103 | Keep | Delete |
| 8 | `visit:{id}:{day}:{who}` | visits.mjs:95, TTL 25 h | Keep | Leave to TTL |
| 9 | `favorites:{user}` arrays naming the id | index.js:3320 `handleFavorite` | Keep | Keep; readers only count them (index.js:3185). Residue, documented |
| 10 | `modcase:{id}` the review case | moderation.mjs:5025, 5056 `saveCase` | Keep | Keep. Stamp `listing_deleted: {at, request_id}` under the moderation lease |
| 11 | `modq:{status}:{id}` queue index | moderation.mjs:5026, 5058 | Keep | Delete the index entry only (the sweeps stop reading a listing that is gone, moderation.mjs:5715-5720) |
| 12 | `wsq:{kind}:{id}` Review Workspace queue | moderation.mjs:3275, 3323 | Keep | `hold`, `quarantine`, `appeal`: defer (section 2.3). `audit`: delete |
| 13 | `modnotif:{id}:...` dedup | moderation.mjs:6372, TTL 90 d | Keep | Leave to TTL |
| 14 | `modmail:{id}:...` dedup | moderation.mjs:6393, TTL 180 d | Keep | Leave to TTL |
| 15 | `notif:{author}:{stamp}:{id}` author feed | moderation.mjs:6381, TTL 90 d | Keep | Leave to TTL |
| 16 | `modshadow:{day}:{id}:{run}` | moderation.mjs:3151/3165, TTL 400 d | Keep | Leave to TTL |
| 17 | `modcost:{id}` and `modcost:run:{day}:{run}` lines | review_costs.mjs:98-100 | Keep | Keep (Eustress's own cost books) |
| 18 | `modroot:{author}:{ident}:{size}` dedup by bytes | moderation.mjs:5028 | Keep | Keep (a decision about bytes, not about the listing) |
| 19 | `legalowed:moderation:{id}` report debt | moderation.mjs:6601 | Keep | **Never.** Its presence defers the hard delete |
| 20 | `legalcase:*:moderation:{id}`, `legalreport:*`, `legalq:*`, `legalalert:*`, `legalstuck:moderation:{id}:*`, `legalunconfirmed:*` | legal_reports.mjs:364-367, 447, 751 | Keep | **Never.** `legalstuck` defers |
| 21 | `caseref:{ref}`, `caseref-of:moderation:{id}` | case_reference.mjs:44-45, 100-101 | Keep | **Never** (the reference must keep resolving, case_history.mjs:233) |
| 22 | `tdlisting:{id}:{td}` and the takedown records naming it (`tdcase`, `tdagainst`, `tdactive`, `tdpending`, `tdq`, `tdmail:{td}:{id}:*`) | takedown.mjs:159-166, 1174 | Keep | **Never.** A takedown not `invalid` or `closed` defers |
| 23 | `ncii-hash:{sha256}` blocklist, `ncii-attempt:*` | takedown.mjs:169, 104 | Keep | **Never** |
| 24 | `vault-pending:moderation:{id}:{item}` (and `takedown:{td}:*`) | evidence_vault.mjs:86, 220-245 | Keep | **Never.** Presence defers |
| 25 | `website-ns:{namespace}` claim, `website-ns-of:{id}` | index.js:6587, 6607 | Keep; the manifest GET is withheld (section 4.6) | `website-ns-of`: delete. `website-ns`: keep as a reserved claim `{sim_id:null, owner_id, released_at}` (decision D8) |
| 26 | `srvcred-sim:{id}`, `srvcred:{hash}` dedicated-server credential | live.mjs:118-119, 177-185 | Keep (heartbeats are refused by the overlay) | Delete both |
| 27 | **new** `simgone:{id}` tombstone, `simgone-author:{author}:{id}` | this design | none | Write first (section 4.9) |
| 28 | **new** `spacedel:due:{YYYY-MM-DDTHH}:{id}` | this design | Write | Delete last |

USERS: nothing is keyed by the listing. The author's `notify-prefs`, `mailcap:*`, `email:{id}` are read for notices only.

### 1.2 KV `INVENTORY`, `PAYOUTS`, `AUDIT_LOG`

| # | Key | Written at | Soft | Hard |
|---|-----|-----------|------|------|
| 29 | `txn:{buyer}:{ms}:{pur}` and `txn:{seller}:{ms}:{pur}` naming `simulation_id` (and `_refund` lines) | commerce.mjs:697-751, TTL 3 years | Keep | **Never** (the Tickets books) |
| 30 | `purchase:{buyer}:{inverted ms}:{id}` receipts with simulation id and name | purchases.mjs:129, 259 `recordPurchase` | Keep | **Never.** The receipt page asks `sim:` only for a signed thumbnail and skips a missing one (purchases.mjs:418-433) |
| 31 | `vscore:{day}:{creator}:{ref}`, `vref:{ref}` value scores | bliss.mjs:36-40, 396-400, TTL 90 d | Keep | **Never** (keyed by sale, not listing) |
| 32 | Bliss ledger `entry:`, `bal:`, `cp:`, `spendref:` | bliss.mjs:13-22 (append-only) | Keep | **Never** (account-level) |
| 33 | `audit:{ts}-{rand}` naming the listing | index.js:3578 `auditLog`; evidence_vault.mjs:122-130 | Keep; add `SPACE_SOFT_DELETED` | Keep; add `SPACE_HARD_DELETED` |

### 1.3 R2 `SCENES`

| # | Object | Written at | Soft | Hard |
|---|--------|-----------|------|------|
| 34 | `universes/{id}/universe.pak` | index.js:5481 `handleUploadScene`; multipart 5515-5610 | Overlay | Delete |
| 35 | `universes/{id}/spaces/{name}.pak` | index.js:5611 `handleUploadSingleSpace` | Overlay | Delete |
| 36 | `universes/{id}/chunks/{hash}.echk` (up to 65,536) | world.mjs:77, 387 | Overlay | Delete |
| 37 | `universes/{id}/manifests/{sha256}.json` world and source | world.mjs:78, 455 | Overlay | Delete |
| 38 | `universes/{id}/website-manifest.json` | index.js:6075 `websiteManifestKey`, 6563 | Overlay plus a new withheld rule | Delete (a missing listing still serves it today, index.js:6317-6334) |
| 39 | `universes/{id}/changelogs/*.md` | changelogs.mjs:52-53 | Overlay | Delete |
| 40 | `universes/{id}/moderation/[e{N}/]dossier.json` | moderation.mjs:5033 | Keep | Delete unless a gate holds (D9) |
| 41 | `universes/{id}/moderation/[e{N}/]capture-{n}.{ext}` (previews) | moderation.mjs:5034; served by cover.mjs | Overlay | Delete unless a gate holds |
| 42 | `universes/{id}/moderation/[e{N}/]script-reader/{batch}.json` | moderation.mjs:3114, 8102 | Keep | Delete unless a gate holds |
| 43 | `universes/{id}/moderation/package_scripts/v{N}/{key}/...` | package_scripts.mjs:108 | Keep | Delete unless a gate holds |
| 44 | `thumbnails/{id}/thumb.{webp,png,jpg}` | index.js:5654 `handleUploadThumbnail` | Overlay | Delete |
| 45 | `covers/{id}/cover.{ext}` | cover.mjs:46 | Overlay | Delete |
| 46 | `locks/legal/{id}` | moderation.mjs:5278, 5635; takedown.mjs:173, 401 | Keep | **Never.** Presence refuses everything |
| 47 | `locks/approved/{id}.json` approval binding | moderation.mjs:5338, 5488 | Keep (restore needs it) | Delete |
| 48 | `locks/state/{id}.json` evidence epochs | moderation.mjs:5261 | Keep | Delete |
| 49 | `locks/moderation/{id}.json` the review lease | moderation.mjs:5168, 5204 | Taken and released by the job | Released by `releaseLease` (moderation.mjs:5240) |
| 50 | `caserefs/moderation/{id}` | case_reference.mjs:46, 91 | Keep | **Never** |
| 51 | `locks/legalcase/{kind}/moderation/{id}.json` | legal_reports.mjs:412 | Keep | **Never** |
| 52 | `evidence/{reference}/*`, `evidence-views/{reference}/*` | evidence_vault.mjs:83-87 | Keep | **Never** (keyed by reference, so a prefix delete of `universes/{id}/` cannot reach it) |
| 53 | `derived/verdicts/{ident}`, `derived/stripped/{sha}` | user_images.mjs:353-356; moderation.mjs:5358 | Keep | Keep: content-addressed, shared by every listing with the same bytes, "written once, never deleted" (user_images.mjs:356) |
| 54 | Unfinished multipart uploads of `universe.pak` | index.js:5515 `handleMultipartCreate` | Refuse new parts (section 4.6) | R2 aborts them after 7 days (Cloudflare R2 docs); nothing to do |
| 55 | **new** `locks/deleted/{id}.json` the deletion mark | this design | Create if absent | Keep forever, state `hard_deleted` |

### 1.4 Durable Objects

| # | Object and keys | Where | Soft | Hard |
|---|-----------------|-------|------|------|
| 56 | `LIVE_HOSTS` named `{id}` (account session) and `guest:{id}` | live.mjs:75-76, 414-892 | Call `stop` on both; later heartbeats get 404 with `end_all_guests` (live.mjs:984-986) | Call `stop` again; the object expires its own kept records (live.mjs:305-328) |
| 57 | `LIVE_INDEX` `all` and `guest`, entry `s:{id}` | live_index.mjs:80-115 | `tellLiveIndex(env,'stop',{sim_id})` on both (live_index.mjs:204); readers re-check listed (228) | Same |
| 58 | `COMMERCE_HUBS` (the author's): `prod:{pid}` with `sim_id`, `pnum:{id}:{n}`, `pseq:{id}`, `pcount:{id}`, `sale:{pur}`, events, `settle:*` | commerce.mjs:965-979, 1289-1304 | Keep; sales refused by the overlay and `commerceBlockedReason` (commerce.mjs:331) | New op `deactivate_sim_products`; records kept (section 3) |
| 59 | `WALLETS` (each buyer): `pur:{pur}`, `ent:{mode}:{id}:{pid}`, `pend:{mode}:{id}:{pur}`, `out:*`, `idem:*` | commerce.mjs:830-958 | Keep | **Never** (the buyer's money and what they own) |
| 60 | `GUEST_ALLOWANCE` per guest: `a.sims[]` | guest.mjs:288-312 | Keep (deletes itself after 25 h) | Keep |
| 61 | `ACCOUNT_AUTH` (the author's): new `space_delete` state and audit records | account_auth.mjs:427-450 | New ops (section 4.2) | Audit `space_hard_deleted` |
| 62 | `TREASURY`, `DELTA_ACCOUNTS`, `HOLDER_LEDGER`, `PASSKEY_INDEX` | wrangler.toml:199-292 | Not keyed by listing | Keep |

### 1.5 Indexes and caches that are computed, not stored

| # | What | Where | Effect |
|---|------|-------|--------|
| 63 | Gallery list, search (`?q=`, `?tag=`), featured shelf | index.js:5813 `handleListSimulations`, `galleryTextMatches` | Computed from a `sim:` scan through `withLegalLockFrom`; the overlay drops the listing |
| 64 | Popular tags | index.js:5876 `handlePopularTags` (browser cache 300 s) | Same scan |
| 65 | Makers | makers.mjs (body cache 10 s, browser 300 s) | Same scan |
| 66 | Live strip body cache | live_index.mjs:284 (10 s) | Reads `sim:` per entry |
| 67 | Isolate memory: `committedCache` (world.mjs:229), `LOCK_LIST_CACHE` (moderation.mjs:5305, 10 s) | | Stale for at most 10 s for the lock list; the manifest cache is content-addressed |
| 68 | HTTP caches the Worker cannot purge: thumbnails `public, max-age=3600` (index.js:6733), cover and previews 300 s (cover.mjs:276, 350), website manifest `max-age=300, stale-while-revalidate=86400` (index.js:6388), pinned manifest 3600 s (6387), chunks `private, max-age=86400, immutable` (world.mjs:620), commerce catalog 30 s (commerce.mjs:1788) | | Copy in the modal: "Pages and pictures already in someone's browser can show for up to a day." |
| 69 | Off platform: a Player's downloaded world, a Share Source copy someone opened in Studio (eustress/crates/engine/src/space/gallery_edit.rs), a website that baked the manifest (engine website/bake.rs), emails already sent | | Out of reach. Said plainly in the modal |
| 70 | Studio's own record: `.eustress/sync.toml` `remote.listing_id` | eustress/crates/engine/src/space/echk_publish.rs:339-363 | Section 6.2 |

Not tied to a listing, checked and left out: `release_notify.mjs` (products are the Player and Studio, line 47), `sponsor.mjs`
(engine sponsorships per account, lines 1-27, 134-161), `subscriptions.mjs` (the company's offering and holder units), Delta
plans (`plan:{account}`, delta.mjs:97), `WORKSHOP_CONTEXT` (`ctx:{user}:{project}`, Studio project ids), the sitemap (lists no
listing, eustress/crates/web/sitemap.xml).

## 2. What must not be deleted, and the refusals

### 2.1 Never deleted by this feature

| What | Why, from the code |
|------|--------------------|
| The legal lock `locks/legal/{id}` | "Nothing is deleted: the bytes are kept as they are, as evidence" (takedown.mjs:35-36). Lifted only by a person: `moderation_release` (moderation.mjs:7263) or a takedown found invalid (takedown.mjs:71-72, 774) |
| Bytes under a lock | The vault does not copy an object over 512 MB (evidence_vault.mjs:59) or a world's chunks (474-476): those objects ARE the evidence, "kept in place under the legal lock" (206) |
| The evidence vault `evidence/*`, `evidence-views/*` | Retained at least 366 days (evidence_vault.mjs:53), CSAM `never_delete` (55); 18 U.S.C. 2258A(h) as cited at line 13 |
| `vault-pending:*` | "Nothing is ever deleted, here or by a caller on a failure" (evidence_vault.mjs:263) |
| Legal reports and debts | "The sealed evidence is kept whatever the outcome" (legal_reports.mjs:83); a released quarantine keeps its CSAM debt, NOT DECIDED (counsel) (85) |
| TAKE IT DOWN records and the hash blocklist | takedown.mjs:93-109; `ncii-attempt` is "evidence; kept" (104) |
| `legal_hold.preserve_until`, `never_delete`, and `legal_release` | 366 days from the hold (takedown.mjs:157, 760; moderation.mjs:7226, 7243); possible CSAM `never_delete: true, preserve_until: null` (takedown.mjs:755); a release keeps the hold's record (moderation.mjs:7273, takedown.mjs:783) |
| Moderation cases and their references | The author and any reference holder still read them (case_history.mjs:191-270) |
| Audit chains | `AUDIT_LOG` 5-year TTL (index.js:3578-3587); the AccountAuth hash chain (account_auth.mjs:210-224) |
| Money | Wallet and CommerceHub records, `txn:` lines, receipts, Bliss entries, value scores, Treasury, payouts, Delta, holder ledger. None is the listing's to delete |
| `derived/*` | Shared by identity across listings (row 53) |

### 2.2 The refusal copy

Every author-facing refusal caused by a hold uses the existing body, byte for byte, so an author learns nothing about which hold
applies (moderation.mjs:5079-5084, `inReview`):

```
HTTP 409
{ "error": "This listing is being reviewed and cannot be changed right now", "code": "in_review" }
```

`delete-request`, `delete-confirm`, `restore-request` and `restore-confirm` answer this, and do nothing, when any of these hold:

1. `contentLockedFor(env, sim)` is true (moderation.mjs:5090): the marker says quarantined, the R2 legal lock exists, or the case
   carries a legal hold or a legal-lane hold (`legalLockKind`, 5280).
2. The moderation lease is held (`acquireLease` returns null, moderation.mjs:5204): a review, a takedown step
   (`holdModerationCase`, takedown.mjs:744) or a seal is writing the case right now.
3. The case is `classifying`, or has an appeal pending (`rec.appeal.status === 'pending'`).

This is a refusal, not a soft delete: a held listing is already blocked from serving, and changing its record while a person
decides is what `refuseWhileHeld` (index.js:5454) forbids for every other write.

### 2.3 Soft delete allowed, hard delete deferred

These allow the soft delete (the listing is hidden and the bytes stay) and defer the hard delete for as long as they last. The
author is never told which; past the due date the Projects card reads "Removal is in progress" (section 6.1), which is true for
every cause, including an ordinary batch delay.

| Gate | Check |
|------|-------|
| A lock appeared during the 30 days | `legalLockOf` (moderation.mjs:5287), checked again before every R2 batch |
| A hold appeared during the 30 days | `caseLocksContent(loadCase)` (moderation.mjs:5086) |
| Preservation still owed after a release | `rec.legal_release.preserve_until > now`, or `never_delete` on `legal_hold` or `legal_release`: hard delete never runs for `never_delete` (D7) |
| A report debt | `legalowed:moderation:{id}` exists, or any `legalstuck:moderation:{id}:*` |
| A seal still failing | any `vault-pending:moderation:{id}:*`, or `vault-pending:takedown:{td}:*` for a takedown naming the listing |
| Evidence kept in place | any sealed index under the case's reference (`evidence/{ref}/*.json`) with `copied: false` |
| A takedown not final | any `tdlisting:{id}:{td}` whose case status is not `invalid` or `closed` |
| A review not decided | case `pending`, `classifying` or `held` (D5); the sweep keeps reviewing a soft-deleted listing like any unlisted one |
| A person's queue | `wsq:hold:{id}`, `wsq:quarantine:{id}`, `wsq:appeal:{id}` |
| Staff hold | `deferred.reason === 'staff_hold'` on the deletion mark (section 4.11) |
| The lease is busy | skip this run, try next hour |

## 3. Commerce

### 3.1 What ties money to a listing

- Products live in the creator's CommerceHub with `sim_id` and a `space` that must be one of `publishedSpaces(sim)`
  (commerce.mjs:324, 1289-1304, 2121). A live purchase needs `isListable(sim)` (2169) and no `commerceBlockedReason` (331, 2183).
- A buyer's Wallet holds what they own in the listing: passes `ent:{mode}:{id}:{pid}` and unfulfilled consumables
  `pend:{mode}:{id}:{pur}` (commerce.mjs:833, 866). A refund is possible for 72 hours (`REFUND_WINDOW_MS`, 67) and runs from the
  creator's hub without reading the listing (1982-1994). A value score is filed one minute after the window (360).
- Fulfilling a pending consumable reads the listing (`loadSimulation`, 1967). After the hard delete it answers 400, so a
  consumable bought and not yet granted can never be granted. A pass stays in the wallet and points at nothing.
- Receipts (`purchase:*`) and `txn:` lines copy the listing's name and stay correct after deletion.
- Subscriptions and sponsorships: no listing link exists. `subscriptions.mjs` is the company's offering; Delta subscriptions are
  Studio plans; `sponsor.mjs` is an account-level gift to the engine. Deleting a listing changes none of them.

### 3.2 What the design does in every option

- Soft delete: live and test purchases are refused (`commerceBlockedReason` gains "This Space is deleted"); the public catalog
  answers 404 (commerce.mjs:1781 through the overlay); `/api/commerce/account` shows the listing with `can_sell: false`. Products
  are not touched, so a restore is exact.
- Hard delete: one call to a new hub op `deactivate_sim_products({sim_id})` sets `active: false` on every product of the listing,
  in DO writes of at most 128 keys, and publishes one `product.archived` event per product that was active (the creator's
  webhooks hear it). Product and sale records stay: sales name them.
- Refunds keep working through the creator, as today, for 72 hours after each sale.

### 3.3 Options for McKale

Each option, in plain words:

- **A. Wait for money to settle (recommended).** Deletion is refused while any product of the listing is on sale ("Take your
  products off sale first") or while any live sale is younger than 72 hours ("You can delete this Space after {time}, when the
  last refund window closes"). No money moves automatically. Buyers keep the same 72-hour refund path they have today. After
  that, passes and unused items in a deleted Space are gone, and the modal and the Terms say so.
- **B. Refund the recent ones.** Deletion is allowed at any time; at the soft delete, every live sale still inside its 72-hour
  window is refunded automatically through the existing refund path (`refund` wallet op, commerce.mjs:919; the creator's 70% is
  taken back and may leave the creator owing Tickets, `allow_negative` at 674). Older purchases are not refunded.
- **C. Delete freely, say so.** No refusal, no refunds. The modal and the Terms say that what buyers bought in a deleted Space is
  gone.
- **D. Spaces that sold can only be archived.** A listing that ever made a live sale cannot be deleted, only archived, so a
  buyer's pass always points at something that could come back.

A second, separate choice: **tell buyers.** When a listing that sold passes is soft deleted, each buyer with a live entitlement
gets one feed item ("A Space you bought in was deleted by its creator"), found from the hub's sales list
(`list_sales` with `sim_id`, commerce.mjs:1412). Off by default until McKale decides.

## 4. The API

### 4.1 Routes

| Route | Policy class (route_policy.mjs RULES) | What |
|-------|---------------------------------------|------|
| `GET /api/account/spaces/{id}/delete-preview` | read (not gated; handler uses `verifyFreshAuth`, fresh_auth.mjs:62) | What would be removed, what blocks it |
| `POST /api/account/spaces/{id}/delete-request` | `FRESH` | Checks, rate limit, opens the pending deletion, returns the factor's challenge |
| `POST /api/account/spaces/{id}/delete-confirm` | `VALUE` (refused during the value freeze after a key replacement, fresh_auth.mjs:49) or `FRESH`: decision D2 | Verifies the factor, soft deletes |
| `DELETE /api/account/spaces/{id}/delete-request` | `FRESH` | Cancels a pending request |
| `POST /api/account/spaces/{id}/restore-request` | `FRESH` | Opens a pending restore (D12) |
| `POST /api/account/spaces/{id}/restore-confirm` | `FRESH` | Verifies the factor, restores |
| `GET /api/admin/space-deletions?state=&cursor=` | admin read | Staff view |
| `POST /api/admin/space-deletions/{id}/restore`, `/hold`, `/release-hold` | `FRESH` via `'/api/admin/*'` (route_policy.mjs:121), identity-file admin session (index.js `isAdminOnlyPath`, `adminPasskeyRefusal`) | Staff actions, rationale of at least 20 characters, audited |

`tests/route_policy.test.mjs` fails until every new path is named in RULES (route_policy.mjs:19-22), which is the point.
`{id}` is `[a-f0-9-]+` like every listing route. Dispatch: a new `handleSpaceDeleteRoute` in a new module `src/space_delete.mjs`,
called from `fetch` before the `/api/simulations/` block (index.js:1100), after `freshGate` (748) and `kycGate` (about 771).

Every route answers one 404 for a listing that does not exist and one that is not the caller's, as archive does
(index.js:5398-5399, `handleArchiveSimulation`): `{ "error": "Simulation not found" }`.

### 4.2 Where the pending state lives

In the author's `AccountAuth` object, as new operations in a new file `src/space_delete_ops.mjs` exported as
`spaceDeleteOps(h)` and spread into `OPS` the way `emailOps` is (account_auth.mjs:639). Why there:

- The attempt counter must be atomic. KV allows one write per second per key and is eventually consistent between locations
  (wrangler.toml:111-114 says the same of a counter), so five attempts in KV could be fifty in parallel. The object runs one
  operation at a time (account_auth.mjs:243-253).
- It already has single-use challenges stored as digests (`newChallenge`/`takeChallenge`, account_auth.mjs:454-473, five
  minutes, five open), the Ed25519 check, passkey verification (`passkey_login`, 1086), the notice list (`notice_recipients`,
  email_ops.mjs:162) and the audit chain (`commit`, account_auth.mjs:427).

State, inside `state.space_delete`:

```
{
  pending: {
    "{sim_id}": {
      request_id: "sdr_" + 24 base62,        // randomToken, commerce.mjs:129
      action: "delete" | "restore",
      sim_id, factor: "key" | "passkey" | "email",
      name_sha256,                           // SHA-256 hex of the typed name, NFC, trimmed
      session_iat,                           // the requesting token's iat; confirm must carry the same token
      requested_at, expires_at,              // ms; 15 minutes
      attempts: 0, max_attempts: 5,
      status: "pending" | "locked" | "confirmed" | "expired" | "cancelled",
      code_digest: hex | null,               // email only
      confirmed_at: ms | null, applied_at: ms | null
    }
  },
  log: { "request:{sim_id}": [ms, ...], "request": [ms, ...] }
}
```

One pending entry per listing; a new request replaces the old one. Terminal entries are dropped 24 hours after they end; the audit
chain keeps the history. The record holds the listing, the factor's digest, the expiry, the attempts and the session, and nothing
else: never the code, never an address.

Operations:

| Op | Does | Audit record |
|----|------|--------------|
| `space_delete_begin {sim_id, action, factor, name_sha256, session_iat, origin}` | Rate check, replaces any pending entry, opens a challenge (`newChallenge(state, 'space_delete:{sim_id}')`) or makes the email code; returns `{request_id, expires_at, challenge?, code?, recipient?}`. The plain code goes back to the Worker once, for the mail, like `email_verify_start`'s token (email_ops.mjs:183-186) | `space_delete_requested {sim, action, factor, request}` |
| `space_delete_confirm {request_id, sim_id, name_sha256, session_iat, signature? / assertion? / code?, origin}` | Counts the attempt and saves it BEFORE verifying; checks status, expiry, listing, name, session; verifies; on success `confirmed`; on the fifth failure `locked` | `space_delete_confirmed` or `space_delete_locked` |
| `space_delete_applied {request_id, sim_id, hard_after}` | The Worker finished the soft delete or restore | `space_soft_deleted` or `space_restored` |
| `space_hard_deleted {sim_id, objects, bytes}` | Called by the job | `space_hard_deleted` |

### 4.3 Rate limits

Sliding windows kept in `state.space_delete.log`, the pattern of `allow()` (email_ops.mjs:110-116). `allow` is not exported;
export it (as `allowIn(log, kind, max, windowMs, now)`) rather than copy it.

- 3 requests per listing per hour (`request:{sim_id}`, 3, 1 h).
- 5 requests per account per day (`request`, 5, 24 h), delete and restore together.
- 5 confirm attempts per request, then `locked`.
- A burst guard in front, per account: `env.AUTH_RATE_LIMITER.limit({ key: 'space-delete:{account}' })` (30 a minute, the
  limiter index.js uses for the identity backup at its `identity-backup:` key).

Over a limit: `429 { "error": "Too many deletion requests. Try again later.", "code": "rate_limited", "retry_after": seconds }`
with `Retry-After`. With the email factor the worst case is 5 requests times 5 attempts a day against 1,000,000 codes: a
1 in 40,000 chance a day, and only for someone who already holds the session.

### 4.4 The factors

**Key (recommended).** The identity key signs, newline-joined with no trailing newline (the shape of `emailChangeMessage`,
email_ops.mjs:55):

```
eustress-space-delete-v1          (eustress-space-restore-v1 for a restore)
{origin}                          the API origin the request reached, one of LOGIN_ORIGINS
{account_id}
{public_key}                      the account's current key, from the object
{sim_id}
{name_sha256}
{request_id}
{challenge}                       64 hex, single use, five minutes
```

Verified in the object against `state.public_key`. `takeChallenge` spends the challenge on the first try, right or wrong
(account_auth.mjs:463-473), so a failed signature needs a new request. Shared test vectors go in
`docs/architecture/space-delete-vectors.json`, like `email-change-vectors.json`, and the website (Rust, `sign_challenge_ed25519`,
eustress/crates/web/src/pages/login.rs:1639) and Studio (eustress/crates/engine/src/auth.rs `IdentityKey`) are tested against
them.

**Passkey.** For a passkey session. `delete-request` returns WebAuthn request options: the 32 challenge bytes as base64url,
`rpId` from `PASSKEY_RP_ID`, `allowCredentials` from the account's passkeys, `userVerification: "required"`. The Worker checks the
client data, RP and flags with `checkAssertion` (webauthn.mjs:507); the object checks the signature against the stored key and
the sign count the way `passkey_login` does (account_auth.mjs:1086-1120), with the challenge bound to
`space_delete:{sim_id}` instead of a sign-in.

**Email code.** Only if D1 keeps it. Six digits, uniform: draw a `Uint32`, reject values of 4,294,000,000 and above, take it
modulo 1,000,000, pad to six. Stored as

```
K      = HMAC-SHA-256(JWT_SECRET, "eustress-space-delete-code-v1")   derived on first use, as vaultKeyOf does (evidence_vault.mjs:93)
digest = hex(HMAC-SHA-256(K, account_id + "|" + sim_id + "|" + request_id + "|" + code))
```

compared with `crypto.subtle.timingSafeEqual`. Sent only to a verified address that is at least 30 days verified
(`emailView(...).verified && aged`, email_ops.mjs:88-107), never to a pending, replaced or unverified one. No such address:
`409 { "error": "This account has no verified email address old enough to receive a code. Confirm with your identity file instead.", "code": "no_verified_email" }`.
A send that fails (`E_DAILY_LIMIT_EXCEEDED`, `E_RATE_LIMIT_EXCEEDED` and the rest, section 5) marks the request `cancelled` and
answers `503 { "error": "We could not send the code. Confirm with your identity file or passkey instead.", "code": "code_not_sent" }`.

The code is never written to a log, an audit record, a staff alert, an HTTP response or DO storage. No route logs a request body;
`sendNotice` logs only a scrubbed error (account_notices.mjs:290); the object logs the error, not the arguments
(account_auth.mjs:269).

### 4.5 Request and response shapes

`GET /api/account/spaces/{id}/delete-preview`

```json
{
  "sim_id": "…", "name": "The Super Station", "state": "listed",
  "removes": { "listing": true, "gallery_card": true, "play_and_join_links": true,
               "live_sessions": 1, "website_namespace": "superstation",
               "products": 3, "products_on_sale": 0, "spaces": ["Main", "Hangar"] },
  "keeps": ["Your Universe files in Studio on your computer", "Receipts and sales records", "Review and case records"],
  "outside_our_reach": ["Copies players already downloaded", "Copies opened with Share Source", "Websites that saved your manifest"],
  "soft_delete_days": 30,
  "factors": ["key"],
  "blocked": null
}
```

`blocked` is null or one of `{ "code": "in_review", "error": "<the exact copy in 2.2>" }`,
`{ "code": "products_on_sale", "error": "Take your products off sale first." }`,
`{ "code": "refund_window_open", "error": "You can delete this Space after {time}, when the last refund window closes.", "until": "ISO" }`
(option A), `{ "code": "already_deleted", ... }`.

`POST /api/account/spaces/{id}/delete-request`

```json
{ "typed_name": "The Super Station", "factor": "key" }
```

```json
200 { "request_id": "sdr_…", "factor": "key", "expires_at": "2026-11-12T20:21:00.000Z",
      "sign": { "version": "eustress-space-delete-v1", "origin": "https://api.eustress.dev", "account_id": "…",
                "public_key": "…", "sim_id": "…", "name_sha256": "…", "request_id": "sdr_…", "challenge": "…64 hex…",
                "challenge_expires_at": "…" } }
200 { "request_id": "sdr_…", "factor": "passkey", "expires_at": "…", "publicKey": { "challenge": "…b64url…", "rpId": "eustress.dev",
      "allowCredentials": [{ "type": "public-key", "id": "…" }], "userVerification": "required", "timeout": 300000 } }
200 { "request_id": "sdr_…", "factor": "email", "expires_at": "…", "sent_to": "o***@example.com", "attempts_left": 5 }
```

Errors: 400 `name_mismatch` ("The name you typed does not match this Space's name."), 400 `bad_factor`, 403 `kyc_required`
(kycGate, see D10), 404, 409 `in_review` (2.2), 409 `already_deleted`, 409 `products_on_sale` / `refund_window_open` (option
A), 409 `no_verified_email`, 423 `account_frozen` (only if D2 is VALUE), 429 `rate_limited`, 503 `account_unavailable` (the
object did not answer; never let through, fresh_auth.mjs:17), 503 `code_not_sent`.

The name check, at request and again at confirm: `typed.normalize('NFC').trim() === sim.name.normalize('NFC').trim()`, case
sensitive, and the trimmed name must not be empty. A listing renamed between request and confirm (a world commit,
world.mjs:316) fails confirm with 409 `name_changed`.

`POST /api/account/spaces/{id}/delete-confirm`

```json
{ "request_id": "sdr_…", "typed_name": "The Super Station",
  "signature": "…128 hex…" }                                     // key
{ "request_id": "sdr_…", "typed_name": "…", "credential": { … } }  // passkey: the navigator.credentials.get answer
{ "request_id": "sdr_…", "typed_name": "…", "code": "123456" }    // email
```

```json
200 { "ok": true, "state": "soft_deleted", "sim_id": "…",
      "hidden_at": "…", "hard_delete_after": "2026-12-12T20:06:00.000Z", "restore_until": "2026-12-12T20:06:00.000Z" }
200 { "ok": true, "replay": true, ... same body ... }                // the same request_id confirmed again
```

Errors: 400 `request_invalid` (unknown request, wrong listing, another session), 400 `challenge_invalid`, 401
`signature_invalid` (`attempts_left` in the body), 401 `code_invalid` (`attempts_left`), 409 `request_locked`, 410
`request_expired`, 409 `request_used` (a different answer after success), 409 `in_review`, 409 `name_changed`.

### 4.6 The soft delete

Steps, in this order, each one safe to run again:

1. **The mark.** `SCENES.put('locks/deleted/{id}.json', body, { onlyIf: { etagDoesNotMatch: '*' }, customMetadata: { state: 'soft' } })`.
   Body: `{ v: 1, sim_id, author_id, state: 'soft', request_id, factor, soft_deleted_at, hard_delete_after, was_public,
   was_archived, deferred: null, progress: null }`. A mark that already exists with the same `request_id` means a retry: go on.
   With another `request_id`: 409 `already_deleted`. From this write on, the listing is hidden everywhere (step 1 is the delete;
   the rest is bookkeeping).
2. **The record.** Rewrite `sim:{id}` with `deletion`, `deleted_was_public`, `is_public: false`, as archive does.
3. **The due index.** `spacedel:due:{hour of hard_delete_after}:{id}`.
4. **Hosting.** `stop` on the LiveHost objects `{id}` and `guest:{id}`, and on both live indexes. Best effort: the overlay refuses
   the next heartbeat anyway.
5. **Audit.** `space_delete_applied` in the object; `SPACE_SOFT_DELETED` in `AUDIT_LOG`.
6. **Notice.** `notice_recipients` (email_ops.mjs:162), then `sendNotice(env, recipients, rendered, { kind: 'notice', accountId })`
   (account_notices.mjs:273) with a new kind `space_deleted` added to `KEY_NOTICE_KINDS` (account_notices.mjs:36) and rendered in
   the shell. It says what was deleted, when it becomes permanent in the account's own time zone and 12-hour clock
   (`formatWhen`, email_shell.mjs:125), how to restore (sign in, Projects, Restore) and how to reach support. Like every key
   notice it carries no link that acts (account_notices.mjs:7-9).

If the Worker stops between steps, the confirm replay (same `request_id`) runs them again, and the hourly job re-writes the KV
marker for any mark whose record lacks it.

**The overlay.** One function, `deletionOf(env, id)` (a head of the mark), and a list form `deletedIds(env)` (one R2 list of
`locks/deleted/`, kept 10 s per isolate, as `legalLockedIds` does at moderation.mjs:5306-5319). Folded into
`simWithLegalLock` (5299) and `withLegalLockFrom` (5321), which every serving route already calls (directly, or through
`simForServing` 5527): the listing reads `{ ...sim, is_public: false, deletion: mark }`. That alone takes it out of the Gallery,
search, tags, makers, featured, the live strip, `GET /api/simulations/{id}` for anyone but the author, previews, guest play
(guest.mjs:595, 753) and the live catalog. Both heads run in parallel, so the cost is one more R2 class B read per serving
request.

Routes that read `sim:` without the overlay, or let the author through, each get an explicit check that answers
`409 { "error": "This Space is deleted. Restore it from Projects to change it.", "code": "listing_deleted", "restore_until": "…" }`
after the hold check (a hold answers `in_review` first):

| Route | Where |
|-------|-------|
| Archive, unarchive, tags, `.pak` upload, multipart create and complete, per-Space upload, thumbnail, website manifest PUT | `refuseWhileHeld` (index.js:5454) gains the deletion check; every one of these calls it (index.js:5400, 5433, 5470, 5523, 5573, 5619, 5662, 6432) |
| Multipart part | `handleMultipartPart` (index.js:5536) calls no hold check today (complete does); it needs `refuseWhileHeld` too |
| Website manifest GET | index.js:6319: `withheld` also when `listing?.deletion` or `!listing` |
| World begin, chunk, commit | `ownedListing` (world.mjs:264) |
| Cover PUT and DELETE | cover.mjs:142 |
| Change log POST | changelogs.mjs:116 |
| Dossier, captures, submit, appeal, rating questionnaire | `handleModerationRoute` (moderation.mjs:8411, check beside 8658) and `openEvidenceSlot` (5755) |
| Live heartbeat, stop, admit | `handleLive` (live.mjs:973): POST answers 404 with `END_ALL_GUESTS` |
| Server credential mint | `handleServerCredential` (live.mjs:152) |
| Guest chat check | guest.mjs:643 |
| Commerce products, purchases, test trigger | `commerceBlockedReason` (commerce.mjs:331) gains "This Space is deleted" |

The author keeps read access to their own world (download, manifest, chunks, source) during the 30 days through `canServe`
(moderation.mjs:6209), so they can keep a copy before it goes.

### 4.7 Restore

- Who: the owner, through `restore-request` and `restore-confirm`, with the same factor as a delete (D12); a staff member through
  `POST /api/admin/space-deletions/{id}/restore` with a rationale, audited. Nobody once the mark's state is `hard_deleting`.
- Refused with `in_review` (2.2) under any hold, with 410 `removal_started` once the hard delete has begun.
- Order: rewrite `sim:{id}` (`is_public` from `deleted_was_public`, `deletion` removed, archive fields as they were), delete the
  due index, delete the mark last. A failure part way leaves the mark in place, so the listing stays hidden: the safe direction.
- Moderation, the approval binding and the bytes were never touched, so an approved listing serves again exactly as before.
- Notice `space_restored` to the notice set; audit `space_restored` and `SPACE_RESTORED`.

### 4.8 The hard delete job

`sweepSpaceDeletions(env, { now, budget })` in `src/space_delete.mjs`, called from `scheduled()` on the hourly tick, after
`sweepModeration` (index.js:695), its counts added to the night's `cronrun:{day}` record (index.js:704-714). The hourly tick has
15 minutes of wall time and 15 minutes of CPU on Workers Paid (Cloudflare limits page); the job stops itself at 10 minutes.

For each `spacedel:due:` key whose hour is not later than now, oldest first:

1. Read the mark. Gone (restored): delete the due key, next. State `hard_deleted`: delete the due key, next.
2. `acquireLease(env, id, t, 'space_delete')`. Busy: next listing, this one waits an hour.
3. Run every gate in 2.3. One holds: write `deferred: { reason, since, checked_at }` on the mark, release the lease, next. Staff
   see the reason (4.11); the author sees "Removal is in progress".
4. Mark state `hard_deleting`, `progress: { phase: 'tombstone' }`.
5. **Tombstone first**, so no case is ever orphaned: `simgone:{id}` =
   `{ v: 1, id, author_id, name, soft_deleted_at, deleted_at, request_id, case_reference, was_public, format, version, space_names, hashes? }`
   (`hashes`: D6), and `simgone-author:{author}:{id}`. `case_reference` from `assignCaseReference(env, 'moderation', id)`
   (case_reference.mjs:83) when a case exists.
6. Stamp the case: `rec.listing_deleted = { at, request_id }`, saved with `saveCase` under the lease.
7. Commerce: `deactivate_sim_products`. Live: `stop` on both sessions and both indexes; delete `srvcred-sim:{id}` and its
   `srvcred:{hash}`.
8. **R2, in pages.** For each prefix `universes/{id}/`, `thumbnails/{id}/`, `covers/{id}/`: `SCENES.list({ prefix, limit: 1000, cursor })`
   then `SCENES.delete(keys)` (R2 takes up to 1,000 keys per delete call and lists at most 1,000 per page). **Before every delete
   call, `legalLockOf(env, id)` again**: a lock that appeared stops the job, writes `deferred: legal_lock`, and leaves what is
   left. Then delete `locks/approved/{id}.json` and `locks/state/{id}.json`. The cursor and phase are saved on the mark after
   each page, so a stopped run resumes there.
9. KV: `sim:{id}` (last of the listing's own keys, so a TAKE IT DOWN request filed during the job still finds the listing,
   takedown.mjs:1659), `sim-author:{author}:{id}`, `plays:{id}`, `visits:{id}`, `website-ns-of:{id}`, the namespace claim made
   reserved (D8), `modq:*:{id}` (the case's own status key), `wsq:audit:{id}`; `simCount:{author}` minus one.
10. Mark state `hard_deleted`, `hard_deleted_at`. The mark stays forever: it is what refuses any later write to that id.
11. Audit `space_hard_deleted` (object) and `SPACE_HARD_DELETED` (`AUDIT_LOG`) with the object count and bytes; release the lease;
    delete the due key. Optional notice `space_hard_deleted`.

Budget. The largest world is 65,536 chunks (world.mjs:53); with its manifests, change logs (at most 200, changelogs.mjs:44) and
review files it is under 70 pages: about 140 R2 calls. Add about 20 KV calls and 5 Durable Object calls: about 165 calls for the
worst listing. The repo budgets 1,000 subrequests per invocation (release_notify.mjs:50, 58) and 1,000 KV operations
(purchases.mjs:19); Cloudflare's limits page now gives 10,000 subrequests on Paid. The job keeps its own counter and stops at 400
calls a run, so it never starves the takedown, evidence and settlement sweeps that share the invocation (index.js:667-696).
Inside Durable Objects, `put` and `delete` take at most 128 keys a call (live.mjs:294), which the new hub op respects.

Idempotency. Every step tolerates what is already gone: an R2 delete of an absent key does nothing, the tombstone put is the
same bytes, the hub op deactivates only active products, the KV deletes are deletes, and the phase on the mark says where to
resume. Running the job twice on a finished listing does nothing but drop its due key.

### 4.9 Case history after the hard delete

`accountCases` (case_history.mjs:191-229) builds an author's review entries from `sim-author:` and `sim:` and skips a listing
whose record is gone (line 207). Without a change, the hard delete would erase the review, its decision and its appeal state
from "Your cases". The change: also list `simgone-author:{author}:` and build the entry from `modcase:{id}` and the tombstone
(`{ id, name }`), with `listing.deleted_at`. The status by reference (case_history.mjs:233-270) already copes with a missing
listing (line 263). Takedown entries already tolerate one (takedown.mjs:1932-1936).

### 4.10 Notices

| When | Kind (new, account_notices.mjs) | To |
|------|---------------------------------|----|
| Soft delete | `space_deleted` | every address in `noticeSet` (email_ops.mjs:78-85): verified, pending, on file, replaced in the last 30 days |
| Restore | `space_restored` | same |
| Hard delete | `space_hard_deleted` (optional) | same |
| Email code | `space_delete_code` | the verified, aged address only |

All of them count in the account's `notice` bucket, 30 a day (mail_limits.mjs:41, 65-79). Subjects never name the Space
("A Space on your Eustress account was deleted", "Your Eustress confirmation code"); the body names it once. Plain text and HTML
both, from `renderEmail` (email_shell.mjs). The code is in the body only, never the subject.

### 4.11 What staff see

- `GET /api/admin/space-deletions?state=soft|deferred|hard_deleting|hard_deleted`: one R2 list of `locks/deleted/` with each mark,
  including `deferred.reason`: `legal_lock`, `case_hold`, `preserve_until`, `never_delete`, `report_owed`, `report_stuck`,
  `vault_pending`, `evidence_in_place`, `takedown_open`, `review_open`, `workspace_open`, `staff_hold`.
- Restore, hold and release-hold actions (4.1), each audited with the administrator and the rationale.
- The Review Workspace case shows `listing_deleted` and the dates.
- One line in the staff digest (`sendStaffDigest`, takedown.mjs): deletions deferred, by reason. No alert for an ordinary
  deletion, and never a code.

## 5. Email

### 5.1 What Cloudflare's documentation says (fetched 2026-11-12)

- Email Service sends transactional email; Email Sending has been in public beta since 2026-04-16 and is "available on the
  Workers paid plan" (changelog, developers.cloudflare.com/changelog/product/email-service/).
- Recipients: "Before you onboard a sending domain, you can send emails only to verified destination addresses in your account.
  After you onboard a sending domain, you can send to any recipient immediately." (email-service/platform/limits/). "Sending to
  arbitrary recipients requires the Workers Paid plan." (email-service/platform/pricing/).
- Binding shape: `[[send_email]] name = "EMAIL"`, with optional `destination_address`, `allowed_destination_addresses`,
  `allowed_sender_addresses` restrictions (email-service/configuration/send-bindings/). The getting-started page adds
  `remote = true` for real sends from `wrangler dev`.
- API: `env.EMAIL.send({ from, to, subject, html, text, cc, bcc, replyTo, attachments, headers })` is the recommended form and
  returns `{ messageId }`; "The `EmailMessage` API remains supported for backward compatibility" (email-service/api/send-emails/workers-api/).
  Errors carry a `code`: `E_SENDER_NOT_VERIFIED`, `E_RECIPIENT_NOT_ALLOWED`, `E_RECIPIENT_SUPPRESSED`, `E_DAILY_LIMIT_EXCEEDED`,
  `E_RATE_LIMIT_EXCEEDED`, `E_DELIVERY_FAILED` and others.
- Domain: onboarding adds MX and SPF on `cf-bounce.{domain}`, DKIM at `cf-bounce._domainkey.{domain}`, and recommends DMARC;
  Cloudflare DNS is required (email-service/configuration/domains/).
- Limits: 50 recipients, 25 MiB, "New accounts start with a conservative daily quota and scale up over time"; 3,000 included a
  month on Paid, then $0.35 per 1,000.
- Headers: an allowlist; `Message-ID`, `Date`, `MIME-Version`, `Content-Type` are platform-controlled in the structured API, and
  "Email Service rejects the entire send request if it contains a disallowed header" (email-service/reference/headers/). The page
  does not say whether that applies to a raw-MIME `EmailMessage`.

### 5.2 What the repo and public DNS show

- `wrangler.toml:381-384` and `:608`: `[[send_email]] name = "EMAIL"` in both environments, no restriction attributes. The comment
  "Requires Email Routing enabled" is stale: the binding is now Email Service's.
- Public DNS on 2026-11-12: `cf-bounce.eustress.dev` has three Cloudflare MX records and `v=spf1 include:_spf.mx.cloudflare.net ~all`;
  `cf-bounce._domainkey.eustress.dev` has a DKIM key; `_dmarc.eustress.dev` is `v=DMARC1; p=reject;`. Those are the records the
  docs give as the sign that a domain is onboarded for sending.
- `[limits] cpu_ms = 300000` (wrangler.toml:373) is configurable only on Workers Paid.
- Staging `/health` answers `email: { configured: true }` (read-only GET, 2026-11-12).
- Every send today uses raw MIME `EmailMessage` (index.js:1453, 1685; account_notices.mjs:286; moderation.mjs:6410). The
  registration email's MIME has no `Date` and no `Message-ID` (index.js:592 `buildMimeWithAttachment`); the notice and moderation
  MIME set `Message-ID` (account_notices.mjs:237, moderation_email.mjs:129).

### 5.3 Verdict

The binding can carry a code to an arbitrary user address, as far as the config, the docs and public DNS show: the domain is
onboarded and the plan is Paid. What is not shown is that mail actually arrives. McKale's identity file did not arrive on
2026-10-21, and the registration path swallows a failed send into a log line (`signup auto-email failed`, index.js:1458). The
possible causes, none confirmed: onboarding finished after that date; the new account's daily quota; the address on a suppression
list; a raw-MIME send refused for its headers; spam filtering; or the client not sending `toml_content`. Until one test send is
read in an inbox, email is not a gate anyone can rely on.

### 5.4 The smallest correct path, if email is used

1. Read Workers Logs for `signup auto-email failed` around 2026-10-21 and the Email Sending Activity log in the dashboard.
2. Send one `space_delete_code` mail from staging to McKale's own verified address and read it, headers included (SPF, DKIM
   `d=eustress.dev` with selector `cf-bounce`, DMARC pass).
3. New mail uses `sendNotice` (one path, the per-recipient caps, the scrubbed logs); move `sendNotice` to the structured
   `env.EMAIL.send({...})` form if step 2 shows a raw-MIME header refusal.
4. Treat any send error as `code_not_sent` and offer the key or passkey.
5. Fix the wrangler.toml comment.

### 5.5 Recommendation

**The identity key's signature as the factor, and a passkey assertion for a passkey session. Email carries the notices, not the
code.** Reasons: the key is the account's root of trust and is already the proof for every key and address change
(email_ops.mjs:225-264, passkeys.mjs:12-16); Studio already holds it and signs with it (auth.rs:463); the website already asks
for it the same way (eustress/crates/web/src/pages/email_settings.rs:108-158); a signature over the listing, the name and a
single-use challenge is a recent, deliberate act, which is the step-up `fresh_auth.mjs` does not give; it does not depend on mail
that has not been shown to arrive, or on a verified address that must be 30 days old; and a mailbox is the account's recovery
channel, so a thief who holds the mailbox would hold both factors. The email code stays in the design behind a setting
(`SPACE_DELETE_EMAIL_CODE = "on"`) for when delivery is proven and McKale wants it (D1).

## 6. Website and Studio

### 6.1 Website (eustress/crates/web)

Where: the Projects card's action menu, "Delete…" as a danger item after Archive (eustress/crates/web/src/pages/projects.rs,
the Archive item and its confirm modal at 280, 405-464), and a "Delete this Space" section at the end of the card's settings
dialog.

1. **What happens.** The modal reads `delete-preview` and lists, in this order: what is removed now (the listing, its page, its
   Gallery card, play and join links, live sessions ended, the website manifest at `{namespace}`, products taken off sale); what is
   kept (your Universe files in Studio on your computer, receipts, review and case records); what is out of reach (copies players
   downloaded, Share Source copies, websites that saved your manifest, pages already in a browser for up to a day); and the
   timeline ("Hidden now. Permanently removed on {date}. You can restore it from Projects until then."). Dates in the account's
   time zone, 12-hour clock. A `blocked` answer replaces the form with its sentence and what to do.
2. **Type the name.** "Type **{name}** to confirm." The name is shown in a copyable block. Delete stays disabled until
   `typed.normalize('NFC').trim()` equals the name exactly, case included. Paste is allowed.
3. **Prove it.** An identity-file session: "Choose your identity file" (the file picker and `sign_challenge_ed25519` exactly as
   email_settings.rs does), sign the `sign` fields, confirm. A passkey session: "Confirm with your passkey"
   (`navigator.credentials.get` with the returned `publicKey`). Email, if on: a six-digit field with the masked address, the
   expiry and the attempts left.
4. **Done.** "{name} is deleted. It is hidden from everyone now and will be permanently removed on {date}. You can restore it
   from Projects until then." The card reads "Deleting" with a Restore button; after the due date, "Removal is in progress" with
   no button.
5. Errors map one to one from the codes in 4.5. A 503 says "Try again in a minute." and changes nothing.

`account_cases.rs` shows a deleted listing's review with "Deleted on {date}".

### 6.2 Studio (eustress/crates/engine)

Studio calls the same routes and holds no code logic and no deletion logic.

- Where: the Explorer context menu on the Universe node, and on a Space node, when `.eustress/sync.toml` names a listing
  (`known_listing`, echk_publish.rs:339): "Delete published listing…". The modal is Slint, with every colour and font size from the
  Display global, and the same four steps and copy as the website.
- The factor is always the key: Studio re-reads the identity file it signed in with and signs the `sign` fields. `auth.rs` gains
  one public function that builds the `eustress-space-delete-v1` message from the response and signs it (the private
  `sign_login`, auth.rs:463, is the model), tested against `space-delete-vectors.json`. If email is on and chosen, Studio shows a
  text field and posts what the person typed.
- Afterwards Studio keeps the listing id. A later publish into it gets 409 `listing_deleted`; the Publish dialog then offers
  "Restore the listing" or "Publish as a new listing" (which clears `remote.listing_id`, `remember_listing` echk_publish.rs:349).
- No MCP tool and no agent path deletes a listing. An MCP publish into a deleted listing gets `listing_deleted` like any client.

## 7. Tests and the Security review

### 7.1 Tests (node:test against the real Worker on in-memory storage, tests/helpers/load_worker.mjs, as tests/archive.test.mjs does)

Request and confirm:
1. Wrong name refuses (case differs, inner space differs, empty after trim); exact name after trim and NFC is accepted; a
   name changed between request and confirm gives `name_changed`.
2. Another account's listing and a missing listing give byte-identical 404s.
3. Each hold refuses with the exact `in_review` body: R2 legal lock with a stale KV case, case `legal_hold`, quarantine marker,
   legal-lane hold, lease held, `classifying`, appeal pending. Nothing is written in any of them.
4. Rate limits: the fourth request on one listing within an hour, and the sixth on the account within a day, give 429; a request
   at 60 minutes plus 1 ms is allowed (sliding window).
5. Key factor: a good signature soft deletes; a signature over another listing, name, origin, request or challenge gives 401;
   a used challenge and an expired one (5 minutes, clock shifted) give `challenge_invalid`.
6. Passkey factor: a good assertion soft deletes (tests/helpers/soft_authenticator.mjs); wrong RP, missing UV, a sign count that
   went backwards, an assertion made for a sign-in challenge, all refuse.
7. Email factor (if kept): the object's storage holds a 64-hex digest and no six-digit string anywhere; a wrong code counts;
   the fifth wrong code locks and a right code afterwards is refused; a code at 14:59.999 works and at 15:00 is `request_expired`
   (tests/helpers/clock_shift.mjs).
8. Replay: confirming twice with the same `request_id` returns `replay: true` and runs nothing again; a new request makes the old
   code and challenge useless; a confirm from another session token of the same account is `request_invalid`.
9. The code appears in no console output (capture `console`), no audit record, no staff alert, no HTTP response.

Soft delete:
10. Out of the Gallery, search, tags, makers, featured, live strip; 404 to a stranger for the listing, thumbnail, cover, previews,
    download, world manifest, chunks, source, website manifest (by id and by namespace); guest play refused; heartbeat 404 with
    `end_all_guests`; live and test purchases refused; catalog 404 to the public.
11. Every author write in the table of 4.6 answers `listing_deleted`.
12. A whole-record writer racing the marker (write the pre-deletion `sim:` record back after the soft delete, as `applyToSim`
    would): every route in test 10 still refuses, and the job restores the KV marker.
13. A TAKE IT DOWN request naming a soft-deleted listing still locks it, holds its case and hashes its files.

Restore:
14. The owner restores within 30 days and gets exactly the prior record (is_public, archive fields, moderation, version); an
    approved listing serves again through its binding.
15. Restore under a hold gives `in_review`; after the job started gives 410; by a stranger gives 404; a staff restore without a
    rationale is refused and with one is audited.

Hard delete:
16. A due listing: every R2 object under its three prefixes, `locks/approved`, `locks/state`, and the KV keys of 1.1 rows 1, 2, 4,
    7, 25 (reverse), 26 are gone; the tombstone exists; the case is kept and stamped; its reference still resolves; the author's
    case list still shows the review.
17. A listing not yet due is untouched.
18. Idempotency: run the job twice, and the second run changes nothing; fail the fake R2 after 1,500 deletes, and the next run
    finishes from the saved cursor; a 70,000-object world finishes within the call budget (count the fake's calls).
19. Every gate in 2.3 defers with its reason: a lock that appears between two R2 pages stops the job before the next page.
20. **Legal hold untouched**: a locked listing with sealed evidence, a report debt and a takedown; request, confirm, then the job
    with the clock shifted 400 days. Snapshot every R2 object, every KV key in every namespace and the hub and wallet records
    before and after: byte-identical.
21. **The test can fail**: remove the legal-lock check from the job, and test 20 must fail. Remove the per-page lock check, and
    test 19 must fail. Record both runs in the change's evidence.
22. Money untouched: `txn:`, `purchase:`, `vscore:`, Bliss entries, wallet `pur:`/`ent:`/`pend:`, hub `sale:` are identical
    before and after a hard delete; products are inactive and still readable.
23. `tests/route_policy.test.mjs` names every new route; `tests/email_shell.test.mjs` renders every new notice kind.
24. Studio and website: the message builders match `space-delete-vectors.json`; the website's Delete button is disabled for every
    near-miss name in test 1.

### 7.2 Security review checklist

- [ ] The R2 mark is the truth and every serving path reads it; no route serves a deleted listing to anyone but its author and
      an administrator.
- [ ] Every author write route refuses a deleted listing (the table in 4.6 matches the code).
- [ ] Hold refusals are the exact `in_review` body, and no answer, timing or status tells a held listing from another.
- [ ] The attempt counter and challenge live in the Durable Object; the attempt is saved before verification.
- [ ] Codes: uniform, HMAC digest only, constant-time compare, 15 minutes, 5 attempts, never logged, never in a subject.
- [ ] Signatures bind origin, account, key, listing, name, request and a single-use challenge; the vectors are shared.
- [ ] The passkey path requires user verification and checks the sign count.
- [ ] Rate limits hold under parallel requests (they are in the object, not KV).
- [ ] The confirm is bound to the requesting session; a second stolen session cannot finish it.
- [ ] D2 decided and the route class matches it.
- [ ] Nothing in 2.1 can be reached by the job: the prefixes are exactly `universes/{id}/`, `thumbnails/{id}/`, `covers/{id}/`,
      and `{id}` is validated as `[a-f0-9-]+` so it cannot be empty or contain `/`.
- [ ] The job checks the legal lock before every R2 delete call, under the moderation lease.
- [ ] The tombstone is written before anything is deleted; case history and references survive.
- [ ] The website namespace cannot be claimed by another author after a hard delete.
- [ ] No MCP or agent path reaches these routes.
- [ ] Notices go to the whole notice set; the code goes only to a verified, aged address.
- [ ] Staff routes need an identity-file admin session, a rationale, and are audited.
- [ ] The overlay's extra R2 read per request is measured on staging before production.

## 8. Decisions for McKale and counsel

Each option is defined in plain words; the recommendation is marked.

- **D1. The second factor.**
  (a) Identity key or passkey (recommended): the person proves the request with the file or passkey they sign in with.
  (b) Email code, as proposed: a six-digit code goes to the verified address and must be typed back.
  (c) Both: key or passkey, and the email code.
  (d) Either one, the person's choice.
- **D2. During the pause after a key replacement.**
  VALUE (recommended): deletion is refused for the 72 hours (7 days when the account holds value) after the identity key is
  replaced, the same pause spending and payouts get, so a thief who replaced the key cannot delete the owner's Spaces.
  FRESH: deletion is allowed during the pause.
- **D3. Commerce.** Options A to D in 3.3 (A recommended), and whether buyers are told.
- **D4. The soft-delete window.** 30 days (proposed, recommended), or another number of days. Counsel: say it in the Terms.
- **D5. A listing whose review never finished.**
  Wait (recommended): the review finishes before the bytes are destroyed, so content uploaded and deleted to dodge review is still
  reviewed.
  Do not wait: an unreviewed listing is destroyed on schedule; cheaper, and it may destroy illegal content nobody looked at.
- **D6. What the tombstone keeps** (counsel).
  With file hashes for 366 days (recommended): a later TAKE IT DOWN request can still name the deleted listing and block
  re-uploads of the same files.
  Without hashes: only the id, name, owner and dates.
  No tombstone: the case history change in 4.9 cannot work.
- **D7. Preservation after a hold was released** (counsel).
  Wait out `preserve_until` (recommended): a listing once under a legal hold keeps its bytes until 366 days after the hold, even
  after release; `never_delete` (possible CSAM) is never hard deleted.
  Delete on release: once a person lifts the hold, the listing deletes on its normal schedule.
- **D8. The website namespace after a hard delete.**
  Reserved to the same owner forever (recommended): nobody else can take it and serve their manifest to a site that names it.
  Reserved for 366 days, then free. Released at once (unsafe: another author can take over a live website's data).
- **D9. Moderation evidence of an ordinary case** (captures, dossier, script reports, counsel).
  Deleted with the listing (recommended): the case record keeps the decision and its reasons.
  Kept for a number of days after the hard delete.
- **D10. Accounts made before identity verification** (counsel: erasure rights).
  Allowed to delete their own listings (recommended): add the routes to `GATE_OPEN` (kyc_identity.mjs:457).
  Must verify first: kycGate answers `kyc_required` (kyc_identity.mjs:502-506).
- **D11. Account deletion.** `handleAdminDeleteUser` (index.js:4083) leaves the account's listings in place today. Option: run
  each listing through this soft and hard delete. Option: leave them, as now.
- **D12. Restore needs the factor.**
  Yes (recommended): restoring re-publishes content, so it is proven like a delete.
  No: a signed-in session is enough, like Unarchive.
- **D13. Email code, if D1 keeps it:** run the delivery check in 5.4 before switching `SPACE_DELETE_EMAIL_CODE` on.

## 9. What could not be verified

- The Cloudflare dashboard: when eustress.dev was onboarded for Email Sending, the account's plan (inferred Paid from
  `cpu_ms`), its daily sending quota, the suppression list, and the Activity log for 2026-10-21.
- Whether Email Service applies its header allowlist to raw-MIME `EmailMessage` sends; the docs do not say.
- Why McKale's identity email did not arrive. Workers Logs were not read (no production calls in this pass).
- The current KV-operations-per-invocation limit; the repo's figure (1,000) is used, and the platform's 10,000 subrequests on Paid
  is from the limits page.
- Durable Object batch limits (128 keys per call) are taken from the repo's own comment (live.mjs:294), not re-read in the docs.
- Nothing was run: no tests, no build. Every behaviour above is read from source.
- index.js moved by four lines while this was read (another session is editing it); every index.js line number is approximate,
  and the function name is the anchor.
- Found in passing, outside this feature: the Gallery list reads only the first 100 `sim:` keys with no cursor
  (index.js:5824), and Projects and Cases read only the first 100 `sim-author:` keys (index.js:5727, case_history.mjs:53). Soft
  deleted listings will take slots in those windows until the hard delete.

## 10. Security review checklist, as built

Built 2026-11-12 on `staging/integration`, not deployed, not committed. McKale's decisions of 2026-11-12 replace the
recommendations above where they differ: the second factor is a six-digit code emailed to the verified, aged address (not the
identity key); a Space with money attached is refused until every product is off sale and every refund window has closed
(option A); the soft-delete window is 90 days (not 30); the legal defaults are the most conservative ones (D5 wait, D6 tombstone
with file hashes, D7 wait out `preserve_until`, D8 namespace reserved to its owner, and D9 the review evidence under
`universes/{id}/moderation/` is KEPT, not deleted); restore needs the same emailed code (D12 yes). D2 is built as the
recommendation (deleting is refused during the value freeze after a key replacement, checked in the handler). D10 is left as
the code has it (kycGate applies). D11 (account deletion) is not built.

Line numbers are as written on 2026-11-12. `src/index.js` and `src/moderation.mjs` are being edited by other sessions; search by
the quoted text. Paths are relative to `infrastructure/cloudflare/api/`.

Files: `src/space_delete.mjs` (routes, gates, soft delete, restore, the hourly job, staff hold), `src/space_delete_ops.mjs` (the
AccountAuth operations), `src/space_delete_mark.mjs` (the R2 mark and the serving overlay; imports nothing). Hooks in shared
files: `src/account_auth.mjs:51,643`, `src/moderation.mjs:50,5305,5324,5331`, `src/index.js:63,700,777-782,5789-5791,6360`,
`src/commerce.mjs:335`, `src/case_history.mjs:59,236-246` (and the `prefix`/`max` parameters of `listingKeysOf`),
`src/route_policy.mjs:131-132,158`, `wrangler.toml:400,625`. Tests: `tests/space_delete.test.mjs`.

**The setting and the session**
- [x] `DELETE_CODE_EMAIL` must be exactly `"on"`: `src/space_delete.mjs:108`; every owner route answers 503
      `delete_unavailable` otherwise, before any listing is read (`:550`). Off in both environments (`wrangler.toml:400,625`).
- [x] A valid, current session on every owner route: `verifyFreshAuth` asks the account object for the floor
      (`src/space_delete.mjs:548`, status read `:714`); the routes are also `fresh` in the policy table
      (`src/route_policy.mjs:131-132`), so `freshGate` refuses a revoked session before dispatch.
- [x] The value freeze after a key replacement refuses a delete with 423 `account_frozen` (`src/space_delete.mjs:411`, called
      at `:563`).
- [x] One 404 for a listing that is missing and one that is not the caller's (`src/space_delete.mjs:554`, status `:717-728`).
- [x] A burst guard per account in front of the object (`AUTH_RATE_LIMITER`, key `space-delete:{account}`,
      `src/space_delete.mjs:556`).

**The code**
- [x] Six digits, uniform by rejection sampling (`src/space_delete.mjs:124`); request ids unbiased base62 (`:134`).
- [x] Stored only as HMAC-SHA-256 under a key derived from `JWT_SECRET` with the label `eustress-space-delete-code-v1`
      (`src/space_delete.mjs:143-161`). No separate secret exists for this; the derivation is the evidence vault's pattern. The
      digest covers account, listing, request and code, so one digest is good for one request of one listing of one account.
- [x] The object never sees a code: the Worker draws it, mails it, and passes only the digest (`src/space_delete.mjs:576-596`);
      the object compares digests in constant time (`src/space_delete_ops.mjs:50`).
- [x] 15 minutes (`src/space_delete_ops.mjs:35`), enforced twice (`:62` and `:167-172`).
- [x] Five attempts, counted and saved BEFORE any check (`src/space_delete_ops.mjs:155-156`); the fifth wrong answer locks
      (`:160`), and a locked request refuses the right code (`:151`). Every refusal after the count spends an attempt: wrong
      request or session (`:173`), renamed listing (`:174`), wrong code (`:175`).
- [x] Single use and replay-proof: a confirmed request answers the same request with the same code as a replay that re-runs
      only idempotent steps and writes no second audit or notice (`src/space_delete_ops.mjs:145-150`,
      `src/space_delete.mjs:623-630,496-505`); anything else is `request_used`. A new request replaces the old one, whose code
      is then useless (`src/space_delete_ops.mjs:123-131`).
- [x] Bound to the requesting session: the confirm must carry the same bearer token, kept as a SHA-256 digest
      (`src/space_delete.mjs:164`, compared at `src/space_delete_ops.mjs:144`).
- [x] Sent only to the verified address, and only once it is 30 days verified; never a pending, replaced or unverified one.
      The object decides, from its own email state (`src/space_delete_ops.mjs:106`), before any allowance is spent.
- [x] Never logged, audited, stored, answered or put in a subject: the subject is fixed (`src/space_delete.mjs:341`), the body
      carries it once (`:321-342`), the object's audit records hold the listing, action and request only
      (`src/space_delete_ops.mjs:131,162,179`), and every `console.error` in the module logs an error name only. A failed send
      cancels the request and answers 503 `code_not_sent` (`src/space_delete.mjs:588-591`); `sendNotice` logs a scrubbed error.
- [x] Rate limits in the object, where parallel requests cannot race them: 3 per listing an hour, 5 per account a day,
      delete and restore together, sliding windows (`src/space_delete_ops.mjs:39-40,111-122`).

**Holds (the author is never told which)**
- [x] Every hold refuses the start with the exact `in_review` body (`src/space_delete.mjs:422-429`, holds at `:423`): the legal lock in R2, the
      quarantine marker or case, a case legal hold or legal-lane hold, `preserve_until` still ahead, `never_delete` on the hold
      or its release or on any sealed evidence item, a report debt or a stuck one, a failing seal (the listing's or a
      takedown's naming it), evidence kept in place (`copied: false`), a TAKE IT DOWN request not `invalid` or `closed`, an
      open review (case pending, classifying, held, appeal pending; or a marker of pending, classifying, held or appealed on a
      listing with content), a Review Workspace hold, quarantine or appeal, a staff hold, an unreadable case
      (`holdsOf`, `src/space_delete.mjs:215-268`). A held moderation lease refuses too (`:309-313`).
- [x] Checked again at confirm (`src/space_delete.mjs:633`), then the moderation lease is taken (`:635`) and the R2 legal lock
      read once more under it (`:639`) before the code is checked.
- [x] Restore is refused under any hold, the same way (`src/space_delete.mjs:664,680,684`).
- [x] The status read shows the generic copy only (`src/space_delete.mjs:746`); the deferral reason lives on the mark for staff
      and never reaches the owner (the view, `src/space_delete_mark.mjs:32-43`, has no `deferred`).

**Money**
- [x] Refused in plain words while any product of the listing is active, or any live sale is inside its 72-hour window
      (`moneyBlockOf`, `src/space_delete.mjs:274-305`); checked at request and again at confirm. A hub that cannot be asked
      refuses (503). Test-mode sales are not counted: they move no money.
- [x] A deleted listing sells nothing (`src/commerce.mjs:335`, read through the overlay).
- [x] The job takes products off sale with the existing `update_product` (`src/space_delete.mjs:996-1004`); product and sale
      records, wallets, `txn:`, `purchase:`, payouts and Bliss are never touched (test: byte-for-byte snapshot).

**The mark and serving**
- [x] The mark is an R2 object made create-if-absent (`src/space_delete.mjs:480`); a mark another deletion wrote first is never
      overwritten (`:481-487`). The soft delete writes nothing to `sim:{id}`, so no KV whole-record rewrite can undo it, and a
      restore returns exactly the prior record.
- [x] Folded into both legal-lock read paths: `simWithLegalLock` (`src/moderation.mjs:5305`) and the list form through
      `legalLockedIds`/`withLegalLockFrom` (`src/moderation.mjs:5324,5331`; `src/space_delete_mark.mjs:84-111`). An unreadable
      mark still reads as deleted (`src/space_delete_mark.mjs:78`).
- [x] The website manifest is withheld for a deleted listing (`src/index.js:6360`), by id and by namespace.
- [x] Every author write under `/api/simulations/{id}` answers 409 `listing_deleted`, a hold answering `in_review` first;
      anyone else gets the 404 of a missing listing; a heartbeat also gets `end_all_guests` (`spaceDeleteWriteGate`,
      `src/space_delete.mjs:829-851`, wired at `src/index.js:777`). Ending a live session and revoking a server credential
      still pass (`:835`). Cost: one R2 head per mutating request under `/api/simulations/{uuid}`.
- [x] Hosting is stopped at the soft delete (both LiveHost objects and both live indexes, `src/space_delete.mjs:443-455`).

**The job**
- [x] Only the three prefixes, with `{id}` a validated UUID (`src/space_delete.mjs:92`, `SIM_ID` at `:79`), minus the review
      evidence (`:93`, filter at `:1031`). `evidence/`, `evidence-views/`, `derived/`, `caserefs/`, `locks/legal/`,
      `locks/state/`, cases, references, legal reports and audit keys are outside every prefix and every KV delete.
- [x] Not before the due time (`src/space_delete.mjs:913`); under the moderation lease (`:914-916`); the mark is read again
      under the lease so a restore that just finished is never undone (`:918-923`).
- [x] Every hold re-checked before any step (`src/space_delete.mjs:928-937`); the legal lock and the staff hold re-read before
      EVERY R2 delete call (`:1035-1038`).
- [x] Tombstone first (`src/space_delete.mjs:963-979`), with each removed object's key, size and SHA-256 where R2 holds one,
      written page by page before that page is deleted (`:1039-1041`); the case is kept and stamped (`:982-990`); the listing's
      own record is deleted last (`:1082`), so a takedown filed during the job still finds it.
- [x] Resumable and bounded: the phase and cursor are saved on the mark after every page; 400 storage calls and 10 minutes a
      run (`src/space_delete.mjs:76-77`); idempotent on a second run (test).
- [x] The namespace claim stays, reserved to its owner (`src/space_delete.mjs:1064-1071`); another author is refused by the
      existing claim check.
- [x] The author's case list still shows a removed listing's review, from its tombstone, bounded at 40 extra rows
      (`src/case_history.mjs:59,236-246`); the reference still resolves.

**Staff**
- [x] `GET /api/admin/space-deletions` and `POST /api/admin/space-deletions/{id}/hold|release-hold` need `requireAdmin`
      (identity-file session) and sit under `/api/admin/*` (fresh, passkey administrators refused); a hold or release needs a
      rationale of 20 characters and is written to `AUDIT_LOG` (`src/space_delete.mjs:777-818`).

**Evidence that the tests can fail.** Every line above has a test in `tests/space_delete.test.mjs`, and 27 deliberate
mutations were run on a scratch copy outside the tree; each one failed at least one test (the unchanged control passed). The
code's expiry is checked in two places, so only removing both fails the expiry test.

**Open**
- [ ] One code mail read in a real inbox before `DELETE_CODE_EMAIL` is switched on (section 5.4). Turning the setting off later
      also stops restores, since a restore needs the code.
- [ ] D10 (unverified accounts) and D11 (account deletion) are not decided; nothing here changes them.
- [ ] Measure the extra R2 head per serving request and per mutating listing request on staging.
- [ ] The website and Studio modals. `space-delete-vectors.json` is not needed (no signature).
- [ ] An administrator's restore (section 4.7) is not built; staff can hold, not restore.
